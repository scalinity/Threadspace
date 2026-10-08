//! G03 IPC, G04 Channel streaming and the SPEC §18.5 view-recovery runs,
//! driven through the live packaged office view: the harness sends
//! qualification commands as native intents and reads back the reports the
//! view records natively, the companion log and the desktop shell log.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_contracts::projection::FleetSnapshot;
use threadspace_harness::companion::LogCursor;
use threadspace_harness::evidence::{Run, sha256_text};
use threadspace_harness::procs;

use crate::ctx::Ctx;

/// A window count is settled once it has not changed for this long.
const SHELLS_STABLE: Duration = Duration::from_secs(5);
const SHELLS_SETTLE_CAP: Duration = Duration::from_secs(60);
/// The office window's title (`window::create_office`).
const OFFICE_TITLE: &str = "Threadspace";

/// Makes sure exactly one hydrated UI is running; returns its incarnation.
pub fn ensure_ui(ctx: &Ctx) -> Result<procs::Incarnation, String> {
    // Iterating under `tauri dev`: drive the attached development UI instead
    // of launching the packaged bundle (both share the dev identifier).
    if std::env::var_os("THREADSPACE_HARNESS_ATTACHED_DEV_UI").is_some() {
        return procs::with_executable(
            &ctx.repo
                .join("target/aarch64-apple-darwin/debug/threadspace-desktop"),
        )
        .into_iter()
        .next()
        .ok_or_else(|| "no attached tauri dev UI".to_owned());
    }
    let app = ctx.app();
    if let Some(ui) = app.processes().into_iter().next() {
        return Ok(ui);
    }
    let _gui = ctx.gui("launch office")?;
    let mut cursor = ctx.companion().log();
    let launched = app.launch_packaged(&[])?;
    app.wait_hydrated(&mut cursor, Duration::from_secs(30))
        .ok_or("UI did not hydrate")?;
    threadspace_harness::pause_ms(1500);
    Ok(launched.ui)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestSession<'a> {
    session_id: &'a str,
    revision: &'a str,
    display_name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestAttention<'a> {
    attention_id: &'a str,
    revision: &'a str,
    acknowledged_at_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestCounts {
    needs_attention: u32,
    awaiting_action: u32,
}

#[derive(Serialize)]
struct Digest<'a> {
    sessions: Vec<DigestSession<'a>>,
    attention: Vec<DigestAttention<'a>>,
    counts: DigestCounts,
}

/// The same canonical form `projectionDigest` computes in the renderer.
pub fn snapshot_digest(snapshot: &FleetSnapshot) -> (String, usize, usize) {
    let mut sessions: Vec<_> = snapshot
        .sessions
        .iter()
        .map(|row| DigestSession {
            session_id: &row.session_id,
            revision: &row.revision,
            display_name: &row.display_name,
        })
        .collect();
    sessions.sort_by(|a, b| a.session_id.cmp(b.session_id));
    let mut attention: Vec<_> = snapshot
        .attention
        .iter()
        .filter(|row| row.resolved_at_ms.is_none())
        .map(|row| DigestAttention {
            attention_id: &row.attention_id,
            revision: &row.revision,
            acknowledged_at_ms: row.acknowledged_at_ms,
        })
        .collect();
    attention.sort_by(|a, b| a.attention_id.cmp(b.attention_id));
    let (s, a) = (sessions.len(), attention.len());
    let digest = Digest {
        sessions,
        attention,
        counts: DigestCounts {
            needs_attention: snapshot.counts.needs_attention,
            awaiting_action: snapshot.counts.awaiting_action,
        },
    };
    (
        sha256_text(&serde_json::to_string(&digest).unwrap_or_default()),
        s,
        a,
    )
}

/// A consistent companion projection read through a short-lived view.
pub fn companion_snapshot(ctx: &Ctx) -> Result<(String, FleetSnapshot), String> {
    let mut client = ctx.companion().client(Duration::from_secs(10))?;
    let subscription_id = uuid::Uuid::new_v4().to_string();
    let reply = client
        .request(ControlRequestBody::AttachView {
            subscription_id: subscription_id.clone(),
        })
        .map_err(|e| e.to_string())?;
    let _ = client.request(ControlRequestBody::DetachView { subscription_id });
    match reply {
        ControlResponseBody::ViewAttached {
            cursor, snapshot, ..
        } => Ok((cursor, snapshot)),
        other => Err(format!("unexpected {other:?}")),
    }
}

/// Compares the view's applied projection with the companion's, retrying
/// briefly while the last patches are still being applied.
pub fn compare_projection(ctx: &Ctx, label: &str) -> Value {
    let app = ctx.app();
    let mut last = json!({});
    for _ in 0..10 {
        let view = app.view_command("projection-digest", json!({}), Duration::from_secs(20));
        let companion = companion_snapshot(ctx);
        let (Ok(view), Ok((cursor, snapshot))) = (view, companion) else {
            last = json!({ "label": label, "error": "digest unavailable" });
            threadspace_harness::pause_ms(1000);
            continue;
        };
        let (digest, sessions, attention) = snapshot_digest(&snapshot);
        let equal = view["result"]["digest"].as_str() == Some(digest.as_str());
        last = json!({
            "label": label,
            "equal": equal,
            "viewDigest": view["result"]["digest"],
            "companionDigest": digest,
            "viewCursor": view["result"]["cursor"],
            "companionCursor": cursor,
            "sessions": sessions,
            "attention": attention,
            "complete": snapshot.complete,
            "viewStream": view["result"]["stream"],
            "viewPaging": view["result"]["paging"],
        });
        if equal {
            return last;
        }
        threadspace_harness::pause_ms(1000);
    }
    last
}

fn raise(ctx: &Ctx, label: &str) -> Result<Value, String> {
    match ctx.companion().request(
        ControlRequestBody::QualifyRaiseAttention {
            label: label.to_owned(),
            session_id: None,
        },
        Duration::from_secs(10),
    )? {
        ControlResponseBody::AttentionRaised {
            attention_id,
            cursor,
            ..
        } => Ok(json!({ "attentionId": attention_id, "cursor": cursor })),
        other => Err(format!("unexpected {other:?}")),
    }
}

/// G03: the in-view suite (≥1,000 round trips and every negative case), the
/// unlisted-view ACL probe and the remote-origin probe.
pub fn ipc(ctx: &Ctx, rounds: u32) -> Result<Value, String> {
    let run = Run::create(&ctx.evidence_root(), "g03-ipc", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    ensure_ui(ctx)?;
    let raised = [raise(ctx, "ipc-suite-a")?, raise(ctx, "ipc-suite-b")?];
    threadspace_harness::pause_ms(1500);
    let suite = ctx.app().view_command(
        "ipc-suite",
        json!({ "rounds": rounds }),
        Duration::from_secs(900),
    )?;
    run.write_json("ipc-suite.json", &suite)
        .map_err(|e| e.to_string())?;
    let result = &suite["result"];
    let suite_ok = suite["ok"] == true
        && result["roundTripsOk"] == true
        && result["passed"] == result["total"]
        && result["rounds"].as_u64() >= Some(1000);

    // Unlisted WebView and remote origin: relaunch with the probe flags.
    let _gui = ctx.gui("g03 probe launch")?;
    let app = ctx.app();
    app.stop_all();
    let since = threadspace_harness::now_ms();
    let server = serve_origin_probe()?;
    let url = format!(
        "--qualify-origin-probe=http://127.0.0.1:{}/probe.html",
        server
    );
    let mut cursor = ctx.companion().log();
    let launched = app.launch_packaged(&["--qualify-acl-probe", &url])?;
    let _ = app.wait_hydrated(&mut cursor, Duration::from_secs(30));
    let acl = app
        .wait_report("acl-probe", since, |_| true, Duration::from_secs(30))
        .map(|(_, r)| r["report"].clone());
    let origin = app
        .wait_report("origin-probe", since, |_| true, Duration::from_secs(30))
        .map(|(_, r)| r["report"].clone());
    app.stop(&launched.ui, false);
    run.write_json("acl-probe.json", &json!(acl))
        .map_err(|e| e.to_string())?;
    run.write_json("origin-probe.json", &json!(origin))
        .map_err(|e| e.to_string())?;
    let acl_ok = acl.as_ref().is_some_and(|r| r["allRefused"] == true);
    let origin_ok = origin.as_ref().is_some_and(|r| r["allRefused"] == true);
    let summary = json!({
        "gate": "G03",
        "pass": suite_ok && acl_ok && origin_ok,
        "rounds": result["rounds"],
        "roundTripFailures": result["roundTripFailures"],
        "latencyMs": result["latencyMs"],
        "cases": { "passed": result["passed"], "total": result["total"] },
        "failedCases": result["results"].as_array().map(|cases| cases.iter().filter(|c| c["pass"] != true).cloned().collect::<Vec<_>>()),
        "aclProbeAllRefused": acl_ok,
        "originProbeAllRefused": origin_ok,
        "fixtureAttention": raised,
    });
    run.write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run.dir }))
}

const ORIGIN_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>probe</title><script>
(async () => {
  const results = [];
  const internals = window.__TAURI_INTERNALS__;
  results.push({ check: "bridge object in this origin", present: typeof internals === "object" && internals !== null });
  const context = null;
  for (const [command, args] of [
    ["ui_query", { request: { query: { kind: "ConnectionStatus" }, context } }],
    ["ui_connect", { request: { protocolVersion: 1, viewEpoch: crypto.randomUUID() }, events: { toJSON: () => "__CHANNEL__:0" } }],
    ["ui_action", { request: { action: { kind: "RefreshEvidence" }, expectedRevision: null, requestId: crypto.randomUUID(), context: { subscriptionId: crypto.randomUUID(), viewEpoch: crypto.randomUUID(), coreGeneration: "x", storeGeneration: "x" } } }],
  ]) {
    if (!internals || typeof internals.invoke !== "function") { results.push({ command, refused: true, detail: "no IPC bridge exposed to this origin" }); continue; }
    try { await internals.invoke(command, args); results.push({ command, refused: false, detail: "accepted" }); }
    catch (error) {
      const typed = typeof error === "object" && error !== null && "code" in error;
      results.push({ command, refused: !typed, detail: String(typeof error === "string" ? error : JSON.stringify(error)).slice(0, 200) });
    }
  }
  try {
    const response = await fetch("ipc://localhost/ui_query", { method: "POST", body: JSON.stringify({ request: { query: { kind: "ConnectionStatus" }, context: null } }), headers: { "Content-Type": "application/json" } });
    results.push({ command: "direct ipc:// fetch", refused: !response.ok, detail: (String(response.status) + " " + (await response.text())).slice(0, 200) });
  } catch (error) {
    results.push({ command: "direct ipc:// fetch", refused: true, detail: String(error).slice(0, 200) });
  }
  const commands = results.filter((r) => "command" in r);
  document.title = "ORIGIN-PROBE:" + JSON.stringify({ origin: location.origin, allRefused: commands.every((r) => r.refused), results });
})();
</script>"#;

/// Serves the origin-probe page on a loopback port for a minute.
fn serve_origin_probe() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let _ = listener.set_nonblocking(true);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        ORIGIN_PAGE.len(),
                        ORIGIN_PAGE
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });
    Ok(port)
}

fn wait_event(
    cursor: &mut LogCursor,
    event: &str,
    timeout: Duration,
    seen: &mut Vec<Value>,
) -> Option<Value> {
    cursor.wait_for(event, |_| true, timeout, seen)
}

/// G04: 10,000 committed synthetic changes over 60 s through the native
/// Channel with an exact final projection, then the stream fault suite.
pub fn stream(ctx: &Ctx, count: u32, duration_ms: u32) -> Result<Value, String> {
    let run = Run::create(&ctx.evidence_root(), "g04-stream", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    ensure_ui(ctx)?;
    let before = compare_projection(ctx, "before");
    run.write_json("projection-before.json", &before)
        .map_err(|e| e.to_string())?;
    let mut cursor = ctx.companion().log();
    let mut desktop = ctx.app().desktop_log();
    let started = ctx.companion().request(
        ControlRequestBody::QualifySyntheticChanges {
            count,
            duration_ms,
            sessions: 16,
        },
        Duration::from_secs(10),
    )?;
    let mut seen = Vec::new();
    let done = cursor.wait_for(
        "SYNTHETIC_RUN_DONE",
        |_| true,
        Duration::from_millis(u64::from(duration_ms) + 120_000),
        &mut seen,
    );
    let retired_backpressure = seen
        .iter()
        .filter(|l| l["event"] == "VIEW_RETIRED_BACKPRESSURE")
        .count();
    threadspace_harness::pause_ms(3000);
    let after = compare_projection(ctx, "after-synthetic-run");
    let recoveries = desktop
        .read_new()
        .into_iter()
        .filter(|l| l["event"] == "OFFICE_VIEW_RECOVERED")
        .count();
    let workload = json!({
        "started": format!("{started:?}"),
        "done": done,
        "committed": done.as_ref().and_then(|d| d["committed"].as_u64()),
        "failed": done.as_ref().and_then(|d| d["failed"].as_u64()),
        "elapsedMs": done.as_ref().and_then(|d| d["elapsedMs"].as_u64()),
        "patchBroadcasts": seen.iter().filter(|l| l["event"] == "PATCH_BROADCAST").count(),
        "viewRetiredBackpressure": retired_backpressure,
        "viewRecoveries": recoveries,
        "projection": after,
    });
    run.write_json("workload.json", &workload)
        .map_err(|e| e.to_string())?;
    let workload_ok = workload["committed"].as_u64() == Some(u64::from(count))
        && workload["failed"].as_u64() == Some(0)
        && workload["elapsedMs"]
            .as_u64()
            .is_some_and(|ms| ms >= u64::from(duration_ms) - 1000)
        && after["equal"] == true;

    let faults = stream_faults(ctx, &run)?;
    let faults_ok = faults.iter().all(|f| f["pass"] == true);
    let summary = json!({
        "gate": "G04",
        "pass": workload_ok && faults_ok,
        "workload": { "count": count, "durationMs": duration_ms, "committed": workload["committed"], "failed": workload["failed"], "elapsedMs": workload["elapsedMs"], "finalProjectionEqual": after["equal"], "viewRetiredBackpressure": retired_backpressure, "viewRecoveries": recoveries },
        "faults": faults,
    });
    run.write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run.dir }))
}

fn stream_faults(ctx: &Ctx, run: &Run) -> Result<Vec<Value>, String> {
    let app = ctx.app();
    let mut results = Vec::new();
    let mut fault = |name: &str,
                     setup: &dyn Fn() -> Result<Value, String>,
                     expect_recovery: bool,
                     changes: u32|
     -> Result<(), String> {
        let mut desktop = app.desktop_log();
        let mut companion = ctx.companion().log();
        let before = app.view_command("projection-digest", json!({}), Duration::from_secs(20))?;
        let connects_before = before["result"]["stream"]["connects"].as_u64().unwrap_or(0);
        let setup_result = setup()?;
        // Drive committed changes so a broken stream has something to miss.
        let burst = ctx.companion().request(
            ControlRequestBody::QualifySyntheticChanges {
                count: changes,
                duration_ms: 2000,
                sessions: 4,
            },
            Duration::from_secs(10),
        );
        let mut seen = Vec::new();
        let burst_done = wait_event(
            &mut companion,
            "SYNTHETIC_RUN_DONE",
            Duration::from_secs(30),
            &mut seen,
        );
        // Allow the independent watchdog (5 s stall + status check) to act,
        // then a settled steady state before the next fault.
        threadspace_harness::pause_ms(15_000);
        let projection = compare_projection(ctx, name);
        let connects_after = projection["viewStream"]["connects"].as_u64().unwrap_or(0);
        let desktop_lines = desktop.read_new();
        let recovered_view = desktop_lines
            .iter()
            .filter(|l| l["event"] == "OFFICE_VIEW_RECOVERED")
            .count();
        let reconnected = connects_after > connects_before || recovered_view > 0;
        let record = json!({
            "fault": name,
            "burstRequest": burst.as_ref().map(|reply| format!("{reply:?}")).unwrap_or_else(|e| format!("error: {e}")),
            "burstDone": burst_done,
            "setup": setup_result,
            "connectsBefore": connects_before,
            "connectsAfter": connects_after,
            "viewRecoveries": recovered_view,
            "reconnected": reconnected,
            "projectionEqual": projection["equal"],
            "pass": burst.is_ok()
                && burst_done.is_some()
                && projection["equal"] == true
                && (!expect_recovery || reconnected),
            "projection": projection,
            "desktopLog": desktop_lines,
        });
        run.append("faults.jsonl", &record)
            .map_err(|e| e.to_string())?;
        results.push(record);
        Ok(())
    };
    fault(
        "missing-frame-then-delivery",
        &|| {
            app.view_command(
                "drop-frames",
                json!({ "mode": "next" }),
                Duration::from_secs(20),
            )
        },
        true,
        20,
    )?;
    fault(
        "missing-frames-no-further-delivery",
        &|| {
            app.view_command(
                "drop-frames",
                json!({ "mode": "all" }),
                Duration::from_secs(20),
            )
        },
        true,
        20,
    )?;
    fault(
        "missing-acks-stalled-window",
        &|| {
            app.view_command(
                "ack-mode",
                json!({ "withhold": true }),
                Duration::from_secs(20),
            )
        },
        true,
        60,
    )?;
    fault(
        "delayed-acks",
        &|| {
            app.view_command(
                "ack-mode",
                json!({ "delayMs": 400 }),
                Duration::from_secs(20),
            )
        },
        false,
        20,
    )?;
    let _ = app.view_command("ack-mode", json!({}), Duration::from_secs(20));
    fault(
        "renderer-reconnect",
        &|| {
            app.view_command(
                "reconnect",
                json!({ "reason": "qualification" }),
                Duration::from_secs(20),
            )
        },
        true,
        20,
    )?;
    Ok(results)
}

/// The UI's layer-0 windows, each as [number, on screen, width, height, title].
fn layer0_windows(ctx: &Ctx, pid: u32) -> Vec<Value> {
    ctx.native.json(&["windows", &pid.to_string()])["windows"]
        .as_array()
        .map(|ws| {
            ws.iter()
                .filter(|w| w["layer"].as_i64() == Some(0))
                .map(|w| json!([w["id"], w["onScreen"], w["width"], w["height"], w["name"]]))
                .collect()
        })
        .unwrap_or_default()
}

/// Office windows (the live view and any retired shell, all titled
/// `OFFICE_TITLE`) and all layer-0 windows. AppKit also gives the process
/// windows of its own (menu-bar strips after a display-mode change, text
/// input), which are counted apart so they cannot pass for shells or hide one.
fn shell_counts(windows: &[Value]) -> (usize, usize) {
    let office = windows.iter().filter(|w| w[4] == OFFICE_TITLE).count();
    (office, windows.len())
}

/// The UI's window counts once they have held still for five seconds (at
/// most a minute), with every change seen on the way. At least one office
/// window (the live one) must be seen, or titles were unreadable and the
/// count proves nothing.
fn settle_shells(ctx: &Ctx, pid: u32) -> Value {
    let started = Instant::now();
    let mut counts = shell_counts(&layer0_windows(ctx, pid));
    let mut seen = vec![json!([counts.0, counts.1])];
    let mut stable_since = Instant::now();
    while stable_since.elapsed() < SHELLS_STABLE && started.elapsed() < SHELLS_SETTLE_CAP {
        threadspace_harness::pause_ms(500);
        let now = shell_counts(&layer0_windows(ctx, pid));
        if now != counts {
            counts = now;
            seen.push(json!([now.0, now.1]));
            stable_since = Instant::now();
        }
    }
    json!({
        "count": counts.0,
        "layer0": counts.1,
        "titlesReadable": counts.0 >= 1,
        "settled": stable_since.elapsed() >= SHELLS_STABLE,
        "waitedMs": started.elapsed().as_millis() as u64,
        "seen": seen,
    })
}

/// One sample of the UI's native resources: office windows (shells and the
/// live view), every layer-0 window, the WebContent processes serving it and
/// its physical footprint.
fn ui_resources(ctx: &Ctx, pid: u32) -> Value {
    let web_content = procs::web_content_of(pid as i32);
    let windows = layer0_windows(ctx, pid);
    let (office, layer0) = shell_counts(&windows);
    json!({
        "windowShells": office,
        "layer0Windows": layer0,
        "windows": windows,
        "webContentProcesses": web_content.as_ref().map(Vec::len),
        "webContentPids": web_content,
        "footprintBytes": procs::phys_footprint(pid as i32),
    })
}

/// SPEC §18.5 view recovery: document replacement, replacement with a native
/// request in flight, retirement with unconsumed cached data, repeated
/// recovery, and bootstrap with the companion unavailable.
pub fn recovery(ctx: &Ctx, repeats: u32) -> Result<Value, String> {
    let run = Run::create(&ctx.evidence_root(), "view-recovery", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let app = ctx.app();
    ensure_ui(ctx)?;
    let environment = ctx.environment();
    run.write_json("environment.json", &environment)
        .map_err(|e| e.to_string())?;
    let mut cases = Vec::new();
    let mut case = |name: &str,
                    trigger: &dyn Fn() -> Result<Value, String>,
                    expected: &str|
     -> Result<(), String> {
        let _gui = ctx.gui(&format!("view recovery: {name}"))?;
        let mut desktop = app.desktop_log();
        let mut companion = ctx.companion().log();
        let triggered = trigger()?;
        let mut lines = Vec::new();
        let recovered = desktop.wait_for(
            "OFFICE_VIEW_RECOVERED",
            |_| true,
            Duration::from_secs(60),
            &mut lines,
        );
        // The recreated view's own hydration: a subscription attached before
        // the recovery (a deliberately stalled one) never hydrates.
        let recovered_ms = recovered.as_ref().and_then(|r| r["ms"].as_i64()).unwrap_or(i64::MAX);
        let hydrated = companion.wait_for(
            "VIEW_HYDRATED",
            |line| line["ts"].as_i64().is_some_and(|ts| ts >= recovered_ms),
            Duration::from_secs(30),
            &mut Vec::new(),
        );
        threadspace_harness::pause_ms(2000);
        lines.extend(desktop.read_new());
        let stale_refusals: Vec<Value> = lines
            .iter()
            .filter(|l| l["event"] == "STALE_VIEW_REFUSED")
            .cloned()
            .collect();
        let created = lines
            .iter()
            .filter(|l| l["event"] == "OFFICE_VIEW_CREATED")
            .count();
        // The desktop's qualification-only account of retired native windows
        // (absent from builds that predate it).
        let native_windows = lines
            .iter()
            .rev()
            .find(|l| l["event"] == "OFFICE_NATIVE_WINDOWS")
            .map(|l| l["detail"].clone());
        let projection = compare_projection(ctx, name);
        let ok = recovered
            .as_ref()
            .is_some_and(|r| r["detail"]["reason"] == expected && r["detail"]["created"] == true)
            && hydrated.is_some()
            && projection["equal"] == true;
        let record = json!({
            "case": name,
            "pass": ok,
            "trigger": triggered,
            "recovered": recovered,
            "newViewHydrated": hydrated.is_some(),
            "hydratedSubscription": hydrated.as_ref().map(|h| h["subscriptionId"].clone()),
            "officeViewsCreated": created,
            "staleViewRefusals": stale_refusals,
            "projectionEqual": projection["equal"],
            "uiProcesses": app.processes(),
            "nativeWindows": native_windows,
        });
        run.append("cases.jsonl", &record)
            .map_err(|e| e.to_string())?;
        cases.push(record);
        Ok(())
    };
    case(
        "document-replacement",
        &|| app.view_command_nowait("reload", json!({})),
        "MAIN_DOCUMENT_REPLACED",
    )?;
    case(
        "replacement-with-request-in-flight",
        &|| app.view_command_nowait("request-then-reload", json!({})),
        "MAIN_DOCUMENT_REPLACED",
    )?;
    case(
        "retired-with-unconsumed-cached-frames",
        &|| {
            app.view_command(
                "ack-mode",
                json!({ "stallNext": true }),
                Duration::from_secs(20),
            )?;
            app.view_command_nowait(
                "reconnect",
                json!({ "reason": "qualification: stall next stream" }),
            )
        },
        "UNCONSUMED_DATA_ON_RETIREMENT",
    )?;
    // Native window shells (C-04): each recovery's retired window is
    // measured, not assumed gone (Tauri alpha.4 deregisters it; AppKit can
    // keep it). Shells are the office-titled layer-0 windows; every layer-0
    // window is recorded beside them. `before` is taken once the recoveries
    // above have settled, so one-time costs of a first recovery are not
    // counted as growth; `after` is read at once and again once the counts
    // hold still.
    let ui = app.processes().into_iter().next();
    let ui_pid = ui.as_ref().map(|ui| ui.pid as u32);
    let before = ui_pid.map(|pid| settle_shells(ctx, pid));
    let resources_before = ui_pid.map(|pid| ui_resources(ctx, pid));
    let mut growth = Vec::new();
    for index in 1..=repeats {
        let name = format!("repeated-replacement-{index:02}");
        case(
            &name,
            &|| app.view_command_nowait("reload", json!({})),
            "MAIN_DOCUMENT_REPLACED",
        )?;
        if let Some(pid) = ui_pid {
            let sample = ui_resources(ctx, pid);
            run.append("resources.jsonl", &json!({ "case": name, "sample": sample }))
                .map_err(|e| e.to_string())?;
            growth.push(sample);
        }
    }
    let immediate = ui_pid.map(|pid| shell_counts(&layer0_windows(ctx, pid)));
    let after = ui_pid.map(|pid| settle_shells(ctx, pid));
    let resources_after = ui_pid.map(|pid| ui_resources(ctx, pid));
    let same_ui = ui.as_ref().is_some_and(procs::Incarnation::alive);
    let measured = |s: &Value| s["settled"] == true && s["titlesReadable"] == true;
    let shells_pass = same_ui
        && matches!((&before, &after), (Some(b), Some(a))
            if measured(b) && measured(a) && a["count"] == b["count"]);
    // The desktop's own account after the last recovery, when it gives one.
    let retired_native = cases
        .iter()
        .rev()
        .find_map(|c| c["nativeWindows"]["retired"].as_array())
        .map(|retired| {
            let alive = |key: &str| retired.iter().filter(|r| r[key] == true).count();
            json!({
                "retired": retired.len(),
                "windowsAlive": alive("windowAlive"),
                "delegatesAlive": alive("delegateAlive"),
                "contentViewsAlive": alive("contentViewAlive"),
                "webviewsAlive": alive("webviewAlive"),
            })
        });
    let window_shells = json!({
        "uiPid": ui_pid,
        "retiredNative": retired_native,
        "sameUiIncarnation": same_ui,
        "recoveries": repeats,
        "before": before.as_ref().map(|b| b["count"].clone()),
        "afterImmediate": immediate.map(|(office, _)| office),
        "after": after.as_ref().map(|a| a["count"].clone()),
        "layer0": {
            "before": before.as_ref().map(|b| b["layer0"].clone()),
            "afterImmediate": immediate.map(|(_, layer0)| layer0),
            "after": after.as_ref().map(|a| a["layer0"].clone()),
        },
        "beforeSettle": before,
        "afterSettle": after,
        "pass": shells_pass,
    });
    let resources = json!({
        "before": resources_before,
        "after": resources_after,
        "perRecovery": growth,
    });
    let delays: Vec<Value> = cases
        .iter()
        .filter_map(|c| {
            c["recovered"]["detail"]["delayMs"]
                .as_u64()
                .map(|d| json!({ "case": c["case"], "delayMs": d }))
        })
        .collect();

    // Companion unavailable during UI bootstrap: freeze it, start a UI, thaw it.
    // (Packaged only: an attached dev UI shares the dev identifier's UI lock.)
    let _gui = ctx.gui("view recovery: bootstrap with companion frozen")?;
    if std::env::var_os("THREADSPACE_HARNESS_ATTACHED_DEV_UI").is_some() {
        let passed = cases.iter().filter(|c| c["pass"] == true).count();
        let summary = json!({ "area": "view-recovery", "mode": "attached dev UI (bootstrap case skipped)", "pass": passed == cases.len() && shells_pass, "passed": passed, "total": cases.len(), "recoveryDelays": delays, "nativeWindowShells": window_shells, "uiResources": resources });
        run.write_json("summary.json", &summary)
            .map_err(|e| e.to_string())?;
        return Ok(json!({ "summary": summary, "dir": run.dir }));
    }
    app.stop_all();
    let companion = ctx.companion().incarnation().ok_or("no companion")?;
    let mut cursor = ctx.companion().log();
    procs::signal(companion.pid, libc::SIGSTOP);
    let launched = app.launch_packaged(&[]);
    threadspace_harness::pause_ms(8000);
    let status_while_stopped = app
        .reports("renderer-attestation", threadspace_harness::now_ms() - 9000)
        .len();
    procs::signal(companion.pid, libc::SIGCONT);
    let hydrated = app.wait_hydrated(&mut cursor, Duration::from_secs(60));
    let bootstrap = json!({
        "case": "bootstrap-with-companion-unavailable",
        "launched": launched.as_ref().map(|l| json!(l.ui)).unwrap_or_else(|e| json!(e)),
        "frozenForMs": 8000,
        "rendererAttestedWhileFrozen": status_while_stopped > 0,
        "hydratedAfterThaw": hydrated.is_some(),
        "sameCompanion": ctx.companion().incarnation() == Some(companion.clone()),
        "pass": launched.is_ok() && hydrated.is_some(),
    });
    run.append("cases.jsonl", &bootstrap)
        .map_err(|e| e.to_string())?;
    cases.push(bootstrap);

    let passed = cases.iter().filter(|c| c["pass"] == true).count();
    let summary = json!({
        "area": "view-recovery",
        "pass": passed == cases.len() && shells_pass,
        "passed": passed,
        "total": cases.len(),
        "recoveryDelays": delays,
        "nativeWindowShells": window_shells,
        "uiResources": resources,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "executableSha256": environment["executableSha256"],
        "companionSha256": environment["companionSha256"],
    });
    run.write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run.dir }))
}
