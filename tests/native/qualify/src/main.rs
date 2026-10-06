//! Qualification client: talks to a running companion through the same
//! verifying relay client the desktop uses, as the `QUALIFICATION` role that
//! only qualification builds accept. Output is JSON on stdout.
//!
//!   threadspace-qualify <dev|prod> diagnostics
//!   threadspace-qualify <dev|prod> integration
//!   threadspace-qualify <dev|prod> snapshot
//!   threadspace-qualify <dev|prod> raise-attention <label>
//!   threadspace-qualify <dev|prod> request-notifications
//!   threadspace-qualify <dev|prod> request-terminal
//!   threadspace-qualify <dev|prod> refresh
//!   threadspace-qualify <dev|prod> return <session-id> [binding-id]
//!   threadspace-qualify <dev|prod> export <after-cursor> [limit]
//!   threadspace-qualify <dev|prod> route-loop <count> <out.jsonl> <session-id>...
//!   threadspace-qualify <dev|prod> independent-check <native-session-id>
//!
//! `route-loop` asks the companion to Return to each session in turn and then
//! checks the outcome through channels independent of the companion: its own
//! provider inventory lookup for the expected session, its own kernel sample
//! of that session's process, its own Terminal readback of the selected tab
//! (sent from this harness's Terminal lineage), `stat(st_rdev)` of that tab,
//! and `lsappinfo` for the frontmost application. A route counts as a wrong
//! target when the companion reports an exact surface but the independently
//! read selected tab is not the expected session's controlling device.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ClientRole, ControlRequestBody, ControlResponseBody};
use threadspace_contracts::route::RouteRequest;
use threadspace_provider_claude::inventory::{ClaudeCli, ClaudeInstall, Inventory};
use threadspace_relay::client::{BlockingClient, connect};
use threadspace_relay::paths::{AgentPaths, agent_identifier_for, home_dir};
use threadspace_surfaces_macos::exec::{BoundedCommand, run_bounded};
use threadspace_surfaces_macos::{process, tty};

fn usage() -> ExitCode {
    eprintln!(
        "usage: threadspace-qualify <dev|prod> <diagnostics|integration|snapshot|raise-attention LABEL|request-notifications|request-terminal|refresh|return SESSION [BINDING]|export CURSOR [LIMIT]|route-loop COUNT OUT SESSION...|independent-check NATIVE_SESSION>"
    );
    ExitCode::from(64)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Independent readback: the selected tab of Terminal's front window, read by
/// this harness itself (its responsible application is Terminal).
fn harness_selected_tty() -> Option<String> {
    let output = run_bounded(
        &BoundedCommand::new("/usr/bin/osascript", Duration::from_secs(3), 4096)
            .arg("-e")
            .arg("tell application \"Terminal\" to return tty of selected tab of front window"),
    )
    .ok()?;
    output
        .succeeded()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Independent frontmost application from LaunchServices (no Apple events).
fn harness_frontmost() -> Option<String> {
    let front = run_bounded(
        &BoundedCommand::new("/usr/bin/lsappinfo", Duration::from_secs(2), 4096).arg("front"),
    )
    .ok()?;
    let asn = String::from_utf8_lossy(&front.stdout).trim().to_owned();
    let info = run_bounded(
        &BoundedCommand::new("/usr/bin/lsappinfo", Duration::from_secs(2), 4096)
            .arg("info")
            .arg("-only")
            .arg("bundleid")
            .arg(&asn),
    )
    .ok()?;
    let text = String::from_utf8_lossy(&info.stdout).into_owned();
    text.split("bundleID=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(str::to_owned)
}

/// The harness's own provider lookup and kernel sample for a native session.
fn independent_process(native_session_id: &str) -> Value {
    let Some(home) = home_dir() else {
        return json!({ "error": "no home" });
    };
    let Some(install) = ClaudeInstall::resolve(&home.join(".local/bin/claude")) else {
        return json!({ "error": "claude not installed" });
    };
    let cli = ClaudeCli {
        binary: install.binary.clone(),
        home,
        timeout: Duration::from_secs(5),
        now_ms,
    };
    let snapshot = match cli.fetch() {
        Ok(snapshot) => snapshot,
        Err(error) => return json!({ "error": error.to_string() }),
    };
    let rows = snapshot.rows_for_session(native_session_id);
    let pids: Vec<i64> = rows.iter().filter_map(|row| row.pid).collect();
    let samples: Vec<Value> = pids
        .iter()
        .map(|pid| match process::sample_incarnation(*pid as i32) {
            Ok(incarnation) => json!({
                "pid": pid,
                "startSeconds": incarnation.sample.start_seconds.to_string(),
                "startMicroseconds": incarnation.sample.start_microseconds,
                "controllingDevice": incarnation.sample.controlling_device,
                "executable": incarnation.executable.canonical(),
                "foreground": incarnation.sample.is_terminal_foreground(),
                "status": incarnation.sample.status,
            }),
            Err(error) => json!({ "pid": pid, "error": error.code() }),
        })
        .collect();
    json!({
        "lookupStartedMs": snapshot.request_started_ms,
        "lookupEndedMs": snapshot.request_ended_ms,
        "rows": rows.len(),
        "processes": samples,
    })
}

fn independent_check(native_session_id: &str) -> Value {
    let process = independent_process(native_session_id);
    let selected = harness_selected_tty();
    let selected_rdev = selected
        .as_deref()
        .and_then(|path| tty::character_device(path).ok());
    let devices: Vec<u64> = process["processes"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|p| p["controllingDevice"].as_u64())
                .collect()
        })
        .unwrap_or_default();
    let selected_is_expected = selected_rdev.is_some_and(|rdev| devices.contains(&u64::from(rdev)));
    json!({
        "nativeSessionId": native_session_id,
        "provider": process,
        "selectedTty": selected,
        "selectedRdev": selected_rdev,
        "frontmost": harness_frontmost(),
        "selectedIsExpectedSession": selected_is_expected,
    })
}

fn snapshot_bindings(client: &mut BlockingClient) -> Result<Value, String> {
    let subscription_id = uuid::Uuid::new_v4().to_string();
    let outcome = client.request(ControlRequestBody::AttachView {
        subscription_id: subscription_id.clone(),
    });
    let _ = client.request(ControlRequestBody::DetachView { subscription_id });
    match outcome {
        Ok(ControlResponseBody::ViewAttached { snapshot, .. }) => Ok(Value::Array(
            snapshot
                .sessions
                .iter()
                .map(|session| {
                    json!({
                        "sessionId": session.session_id,
                        "nativeSessionId": session.native_session_id,
                        "activation": session.activation,
                        "bindingId": session.binding.as_ref().map(|b| b.binding_id.clone()),
                        "bindingRevision": session.binding.as_ref().map(|b| b.revision.clone()),
                        "locator": session.binding.as_ref().map(|b| b.locator.clone()),
                        "pid": session.process.as_ref().map(|p| p.pid),
                        "liveBindings": session.live_bindings,
                    })
                })
                .collect(),
        )),
        Ok(_) => Err("unexpected snapshot reply".into()),
        Err(error) => Err(error.to_string()),
    }
}

fn percentile(sorted: &[u32], fraction: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((fraction * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn route_loop(client: &mut BlockingClient, count: u32, out: &Path, sessions: &[String]) -> Value {
    let _ = client.set_read_timeout(Duration::from_secs(30));
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
    {
        Ok(file) => file,
        Err(error) => return json!({ "ok": false, "error": error.to_string() }),
    };
    let baseline = snapshot_bindings(client).unwrap_or(Value::Null);
    let tracked = |snapshot: &Value| -> Vec<Value> {
        snapshot
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|s| {
                        sessions
                            .iter()
                            .any(|id| Some(id.as_str()) == s["sessionId"].as_str())
                    })
                    .map(|s| {
                        json!([
                            s["sessionId"],
                            s["activation"],
                            s["bindingId"],
                            s["bindingRevision"],
                            s["pid"]
                        ])
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let baseline_tracked = tracked(&baseline);
    let mut latencies = Vec::new();
    let mut exact = 0u32;
    let mut wrong = 0u32;
    let mut failures = Vec::new();
    let mut unrelated_changes = 0u32;
    let mut per_session: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
    for round in 0..count {
        for session_id in sessions {
            let native = baseline
                .as_array()
                .and_then(|list| list.iter().find(|s| s["sessionId"] == session_id.as_str()))
                .and_then(|s| s["nativeSessionId"].as_str())
                .unwrap_or_default()
                .to_owned();
            let request_id = uuid::Uuid::new_v4().to_string();
            let sent = Instant::now();
            let outcome = client.request(ControlRequestBody::ReturnToSession {
                route: RouteRequest {
                    request_id: request_id.clone(),
                    session_id: session_id.clone(),
                    chosen_binding_id: None,
                    expected_binding_revision: None,
                },
            });
            let round_trip_ms = sent.elapsed().as_millis() as u32;
            let check = independent_check(&native);
            let after = snapshot_bindings(client).unwrap_or(Value::Null);
            let unchanged = tracked(&after) == baseline_tracked;
            if !unchanged {
                unrelated_changes += 1;
            }
            let entry = match outcome {
                Ok(ControlResponseBody::Routed { result }) => {
                    let exact_route = result.reason_code == "OK"
                        && result.surface_result
                            == threadspace_contracts::route::SurfaceResult::ExactNativeSurface
                        && result.session_verification
                            == threadspace_contracts::route::SessionVerification::CurrentNativeRevalidated;
                    let independent_ok = check["selectedIsExpectedSession"] == true
                        && check["frontmost"] == "com.apple.Terminal";
                    let readback = result
                        .evidence
                        .focus
                        .as_ref()
                        .and_then(|focus| focus.readback_tty.clone());
                    let agrees =
                        readback.is_some() && readback.as_deref() == check["selectedTty"].as_str();
                    if result.focus_performed
                        && !check["selectedIsExpectedSession"]
                            .as_bool()
                            .unwrap_or(false)
                    {
                        wrong += 1;
                    }
                    let tally = per_session.entry(session_id.clone()).or_default();
                    if exact_route && independent_ok && agrees {
                        exact += 1;
                        tally.0 += 1;
                        latencies.push(result.latency_ms);
                    } else {
                        tally.1 += 1;
                        failures.push(json!({ "round": round, "sessionId": session_id, "reason": result.reason_code }));
                    }
                    json!({
                        "round": round,
                        "requestId": request_id,
                        "sessionId": session_id,
                        "nativeSessionId": native,
                        "roundTripMs": round_trip_ms,
                        "exactRoute": exact_route,
                        "independentOk": independent_ok,
                        "companionReadbackAgrees": agrees,
                        "unrelatedBindingsUnchanged": unchanged,
                        "result": result,
                        "independent": check,
                    })
                }
                Ok(other) => {
                    json!({ "round": round, "sessionId": session_id, "error": format!("unexpected {other:?}") })
                }
                Err(error) => {
                    failures.push(json!({ "round": round, "sessionId": session_id, "error": error.to_string() }));
                    json!({ "round": round, "sessionId": session_id, "error": error.to_string(), "independent": check })
                }
            };
            let _ = writeln!(file, "{entry}");
            std::thread::sleep(Duration::from_millis(400));
        }
    }
    latencies.sort_unstable();
    let median = percentile(&latencies, 0.5);
    json!({
        "ok": wrong == 0 && failures.is_empty() && unrelated_changes == 0,
        "sessions": sessions.len(),
        "routesPerSession": count,
        "attempted": count as usize * sessions.len(),
        "exactRoutes": exact,
        "wrongTargets": wrong,
        "failures": failures,
        "unrelatedBindingChanges": unrelated_changes,
        "perSession": per_session.iter().map(|(id, (ok, bad))| json!({ "sessionId": id, "exact": ok, "failed": bad })).collect::<Vec<_>>(),
        "latencyMs": {
            "median": median,
            "p95": percentile(&latencies, 0.95),
            "max": latencies.last().copied().unwrap_or(0),
            "samples": latencies.len(),
        },
        "baseline": baseline,
        "out": out.display().to_string(),
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let app_identifier = match args.first().map(String::as_str) {
        Some("dev") => "ai.scalinity.threadspace.dev",
        Some("prod") => "ai.scalinity.threadspace",
        _ => return usage(),
    };
    let Some(command) = args.get(1) else {
        return usage();
    };
    if command == "independent-check" {
        let Some(native) = args.get(2) else {
            return usage();
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&independent_check(native)).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }
    let Some(paths) = AgentPaths::for_agent(&agent_identifier_for(app_identifier)) else {
        return usage();
    };

    let connection = match connect(
        &paths.locator,
        ClientRole::Qualification,
        Duration::from_secs(3),
    ) {
        Ok(connection) => connection,
        Err(error) => {
            println!(
                "{}",
                json!({ "ok": false, "stage": "connect", "error": error.to_string() })
            );
            return ExitCode::from(2);
        }
    };
    let peer = json!({
        "pid": connection.peer.pid,
        "coreGeneration": connection.hello.core_generation,
        "storeGeneration": connection.hello.store_generation,
        "companion": connection.hello.companion,
    });
    let mut client = BlockingClient::new(connection);

    if command == "route-loop" {
        let (Some(count), Some(out)) = (
            args.get(2).and_then(|count| count.parse::<u32>().ok()),
            args.get(3),
        ) else {
            return usage();
        };
        let sessions: Vec<String> = args.iter().skip(4).cloned().collect();
        if sessions.is_empty() {
            return usage();
        }
        let summary = route_loop(&mut client, count, Path::new(out), &sessions);
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "peer": peer, "summary": summary }))
                .unwrap_or_default()
        );
        return if summary["ok"] == true {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }

    let request = match command.as_str() {
        "diagnostics" => ControlRequestBody::Diagnostics,
        "integration" => ControlRequestBody::IntegrationStatus,
        "snapshot" => ControlRequestBody::AttachView {
            subscription_id: uuid::Uuid::new_v4().to_string(),
        },
        "raise-attention" => ControlRequestBody::QualifyRaiseAttention {
            label: args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "qualification".into()),
        },
        "request-notifications" => ControlRequestBody::RequestNotificationAuthorization,
        "request-terminal" => ControlRequestBody::RequestTerminalAutomation,
        "refresh" => ControlRequestBody::RefreshEvidence,
        "return" => {
            let Some(session_id) = args.get(2) else {
                return usage();
            };
            ControlRequestBody::ReturnToSession {
                route: RouteRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.clone(),
                    chosen_binding_id: args.get(3).cloned(),
                    expected_binding_revision: None,
                },
            }
        }
        "export" => ControlRequestBody::QualifyExportObservations {
            after_cursor: args.get(2).cloned().unwrap_or_else(|| "0".into()),
            limit: args
                .get(3)
                .and_then(|limit| limit.parse().ok())
                .unwrap_or(50),
        },
        _ => return usage(),
    };
    if matches!(
        request,
        ControlRequestBody::RequestNotificationAuthorization
            | ControlRequestBody::RequestTerminalAutomation
    ) {
        // Waits for the owner to answer a native prompt.
        let _ = client.set_read_timeout(Duration::from_secs(200));
    } else if matches!(
        request,
        ControlRequestBody::RefreshEvidence | ControlRequestBody::ReturnToSession { .. }
    ) {
        let _ = client.set_read_timeout(Duration::from_secs(40));
    }
    let detach = match &request {
        ControlRequestBody::AttachView { subscription_id } => Some(subscription_id.clone()),
        _ => None,
    };
    let independent = matches!(&request, ControlRequestBody::ReturnToSession { .. });
    let outcome = client.request(request);
    if let Some(subscription_id) = detach {
        let _ = client.request(ControlRequestBody::DetachView { subscription_id });
    }
    match outcome {
        Ok(body) => {
            let check = match (&body, independent) {
                (ControlResponseBody::Routed { result }, true) => result
                    .evidence
                    .native_session_id
                    .as_deref()
                    .map(independent_check),
                _ => None,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({ "ok": true, "peer": peer, "response": body, "independent": check })
                )
                .unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!(
                "{}",
                json!({ "ok": false, "peer": peer, "error": error.to_string() })
            );
            ExitCode::from(1)
        }
    }
}
