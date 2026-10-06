//! G09 independent companion, G10 live crash/receipt, G11 restart cycles, the
//! SPEC §19.5 maintenance ordering and the UI single-instance case, against
//! the installed identity and the system-started companion.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody, QualificationFault};
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::procs::{self, Incarnation};
use threadspace_harness::run::run;
use threadspace_harness::service;

use crate::bridge_gates::{companion_snapshot, ensure_ui};
use crate::ctx::Ctx;

const RELAUNCH_WAIT: Duration = Duration::from_secs(90);

fn raise(ctx: &Ctx, label: &str) -> Result<(String, String), String> {
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
        } => Ok((attention_id, cursor)),
        other => Err(format!("unexpected {other:?}")),
    }
}

fn writer_lock_free(ctx: &Ctx) -> Value {
    // Same probe as the bootstrap: a shared non-blocking lock on a read-only
    // descriptor, released at once.
    let path = ctx.id.agent.store_dir.join("writer.lock");
    let Ok(file) = std::fs::File::open(&path) else {
        return json!("missing");
    };
    use std::os::fd::AsRawFd;
    // SAFETY: `file` owns the descriptor for both calls.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
    if rc == 0 {
        // SAFETY: as above.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        json!(true)
    } else {
        json!(false)
    }
}

/// Facts that must survive any restart (SPEC §19.5, G11).
fn durable_facts(ctx: &Ctx) -> Result<Value, String> {
    let (cursor, snapshot) = companion_snapshot(ctx)?;
    let diagnostics = ctx.companion().diagnostics()?;
    let mut by_native: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for session in &snapshot.sessions {
        by_native
            .entry(format!(
                "{}:{}",
                session.provider, session.native_session_id
            ))
            .or_default()
            .push(session.session_id.clone());
    }
    let duplicates: Vec<&String> = by_native
        .iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(key, _)| key)
        .collect();
    Ok(json!({
        "cursor": cursor,
        "storeGeneration": diagnostics["storeGeneration"],
        "coreGeneration": diagnostics["coreGeneration"],
        "sessions": by_native.iter().map(|(native, ids)| (native.clone(), json!(ids))).collect::<BTreeMap<_, _>>(),
        "duplicateSessions": duplicates,
        "openAttention": snapshot.attention.iter().filter(|a| a.resolved_at_ms.is_none()).map(|a| a.attention_id.clone()).collect::<BTreeSet<_>>(),
        "bindings": snapshot.sessions.iter().filter_map(|s| s.binding.as_ref().map(|b| (s.session_id.clone(), json!({ "bindingId": b.binding_id, "revision": b.revision })))).collect::<BTreeMap<_, _>>(),
        "fixtureSession": snapshot.sessions.iter().find(|s| s.native_session_id == "m0a-fixture-session-1").map(|s| s.session_id.clone()),
        "sqlite": diagnostics["sqlite"],
    }))
}

fn kill_companion(ctx: &Ctx) -> Result<(Incarnation, Incarnation, u64), String> {
    let old = ctx
        .companion()
        .incarnation()
        .ok_or("companion not running")?;
    procs::signal(old.pid, libc::SIGKILL);
    procs::wait_exit(&old, Duration::from_secs(10)).ok_or("companion did not die")?;
    let (fresh, waited) = ctx
        .companion()
        .wait_new_incarnation(Some(&old), RELAUNCH_WAIT)
        .ok_or("ServiceManagement did not relaunch the companion")?;
    Ok((old, fresh, waited))
}

/// G09: UI failure leaves capture running; a companion crash with no UI and no
/// provider hook is relaunched by the system; one writer; capture resumes.
pub fn companion_independence(ctx: &Ctx, crashes: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g09-companion", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let app = ctx.app();
    let mut cases = Vec::new();
    for (name, how) in [
        ("ui-quit-cmd-q", "cmd-q"),
        ("ui-sigterm", "term"),
        ("ui-sigkill", "kill"),
    ] {
        let ui = ensure_ui(ctx)?;
        let _gui = ctx.gui(&format!("g09 {name}"))?;
        let companion_before = ctx.companion().incarnation();
        let idle = if how == "cmd-q" {
            let waited = wait_for_idle(&ctx.native, 5.0, Duration::from_secs(600));
            ctx.native
                .ax_action(ui.pid as u32, "raise", Some("Threadspace"));
            ctx.native.json(&["key", "12", "cmd"]);
            Some(waited)
        } else {
            procs::signal(
                ui.pid,
                if how == "term" {
                    libc::SIGTERM
                } else {
                    libc::SIGKILL
                },
            );
            None
        };
        let exited = procs::wait_exit(&ui, Duration::from_secs(15));
        let mut cursor = ctx.companion().log();
        let capture = raise(ctx, &format!("g09-{name}"));
        let mut seen = Vec::new();
        let pass_seen = cursor.wait_for(
            "DISCOVERY_PASS",
            |_| true,
            Duration::from_secs(12),
            &mut seen,
        );
        let companion_after = ctx.companion().incarnation();
        let record = json!({
            "case": name,
            "idleGate": idle,
            "uiExitedMs": exited,
            "companionUnchanged": companion_before.is_some() && companion_before == companion_after,
            "captureWhileUiAbsent": capture.as_ref().map(|(id, c)| json!({ "attentionId": id, "cursor": c })).unwrap_or_else(|e| json!(e)),
            "discoveryPassAfterUiDeath": pass_seen.is_some(),
            "pass": exited.is_some() && companion_before == companion_after && capture.is_ok() && pass_seen.is_some(),
        });
        run_dir
            .append("cases.jsonl", &record)
            .map_err(|e| e.to_string())?;
        cases.push(record);
    }
    app.stop_all();
    for index in 1..=crashes {
        let facts_before = durable_facts(ctx)?;
        let mut cursor = ctx.companion().log();
        let killed = kill_companion(ctx);
        let record = match killed {
            Ok((old, fresh, waited)) => {
                let processes = ctx.companion().processes();
                let lock_free = writer_lock_free(ctx);
                let capture = raise(ctx, &format!("g09-after-relaunch-{index}"));
                let mut seen = Vec::new();
                let discovery = cursor.wait_for(
                    "DISCOVERY_PASS",
                    |_| true,
                    Duration::from_secs(15),
                    &mut seen,
                );
                let facts_after = durable_facts(ctx)?;
                json!({
                    "case": format!("companion-sigkill-ui-closed-{index:02}"),
                    "uiProcesses": app.processes().len(),
                    "killed": old,
                    "relaunched": fresh,
                    "relaunchWaitMs": waited,
                    "relaunchedBy": if fresh.ppid == 1 { "launchd" } else { "other" },
                    "companionProcesses": processes.len(),
                    "writerLockHeld": lock_free == json!(false),
                    "captureResumed": capture.is_ok(),
                    "discoveryResumed": discovery.is_some(),
                    "storeGenerationKept": facts_before["storeGeneration"] == facts_after["storeGeneration"],
                    "coreGenerationChanged": facts_before["coreGeneration"] != facts_after["coreGeneration"],
                    "pass": processes.len() == 1 && lock_free == json!(false) && capture.is_ok() && discovery.is_some()
                        && facts_before["storeGeneration"] == facts_after["storeGeneration"]
                        && facts_before["coreGeneration"] != facts_after["coreGeneration"],
                })
            }
            Err(error) => {
                json!({ "case": format!("companion-sigkill-ui-closed-{index:02}"), "pass": false, "error": error })
            }
        };
        run_dir
            .append("cases.jsonl", &record)
            .map_err(|e| e.to_string())?;
        cases.push(record);
    }
    let passed = cases.iter().filter(|c| c["pass"] == true).count();
    let summary = json!({ "gate": "G09", "pass": passed == cases.len(), "passed": passed, "total": cases.len(), "service": service::status(&ctx.id) });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

/// G10 live: second writer refused; every acknowledged record survives a
/// companion crash; a retried owner command is idempotent across restart.
pub fn sqlite_live(ctx: &Ctx, rounds: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g10-sqlite-live", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let incumbent = ctx
        .companion()
        .incarnation()
        .ok_or("companion not running")?;
    let second = run(
        &ctx.id.companion_executable.display().to_string(),
        &[],
        Duration::from_secs(40),
    );
    let second_writer = json!({
        "exitStatus": second.status,
        "elapsedMs": second.elapsed_ms,
        "incumbentUnchanged": ctx.companion().incarnation() == Some(incumbent.clone()),
        "pass": second.status == Some(75) && ctx.companion().incarnation() == Some(incumbent),
    });
    run_dir
        .write_json("second-writer.json", &second_writer)
        .map_err(|e| e.to_string())?;

    let mut rounds_out = Vec::new();
    for round in 1..=rounds {
        let acknowledged =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String, i64)>::new()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = {
            let acknowledged = std::sync::Arc::clone(&acknowledged);
            let stop = std::sync::Arc::clone(&stop);
            let id = ctx.id.clone();
            std::thread::spawn(move || {
                let companion = threadspace_harness::companion::Companion::new(&id);
                let mut index = 0;
                while !stop.load(std::sync::atomic::Ordering::Acquire) {
                    index += 1;
                    // A durable fixture record per request: no attention item
                    // and no notification. A reply arrives only after COMMIT.
                    let observation_id = uuid::Uuid::new_v4().to_string();
                    let captured_wall_ms = threadspace_harness::now_ms();
                    if let Ok(ControlResponseBody::Admitted { cursor, .. }) = companion.request(
                        ControlRequestBody::QualifyAdmit {
                            observation_id: observation_id.clone(),
                            captured_wall_ms,
                        },
                        Duration::from_secs(2),
                    ) && let Ok(mut list) = acknowledged.lock()
                    {
                        list.push((observation_id, cursor, captured_wall_ms));
                    }
                }
                let _ = index;
            })
        };
        threadspace_harness::pause_ms(1500 + u64::from(round) * 137);
        let killed = kill_companion(ctx);
        stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = worker.join();
        let acked = acknowledged
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default();
        // Presence by re-admission: every acknowledged UUID, retried with its
        // original content, must answer ALREADY_COMMITTED at the cursor its
        // first receipt named.
        let mut client = ctx.companion().client(Duration::from_secs(10))?;
        let lost: Vec<&(String, String, i64)> = acked
            .iter()
            .filter(|(id, cursor, captured_wall_ms)| {
                !matches!(
                    client.request(ControlRequestBody::QualifyAdmit { observation_id: id.clone(), captured_wall_ms: *captured_wall_ms }),
                    Ok(ControlResponseBody::Admitted { status: threadspace_contracts::ui::ReceiptStatus::AlreadyCommitted, cursor: again, .. })
                        if &again == cursor
                )
            })
            .collect();
        let record = json!({
            "round": round,
            "acknowledged": acked.len(),
            "lostAcknowledged": lost,
            "relaunch": killed.as_ref().map(|(_, fresh, waited)| json!({ "incarnation": fresh, "waitedMs": waited })).unwrap_or_else(|e| json!(e)),
            "pass": killed.is_ok() && lost.is_empty() && !acked.is_empty(),
        });
        run_dir
            .append("crash-rounds.jsonl", &record)
            .map_err(|e| e.to_string())?;
        rounds_out.push(record);
    }
    // Idempotent owner command across a restart.
    let (attention_id, _) = raise(ctx, "g10-idempotent")?;
    let command_id = uuid::Uuid::new_v4().to_string();
    let ack = |ctx: &Ctx| {
        ctx.companion().request(
            ControlRequestBody::AcknowledgeAttention {
                command_id: command_id.clone(),
                attention_id: attention_id.clone(),
                expected_revision: None,
            },
            Duration::from_secs(10),
        )
    };
    let first = ack(ctx);
    let _ = kill_companion(ctx);
    let retry = ack(ctx);
    let receipt = |r: &Result<ControlResponseBody, String>| match r {
        Ok(ControlResponseBody::CommandReceipt { receipt }) => {
            json!({ "status": receipt.status, "cursor": receipt.cursor })
        }
        other => json!(format!("{other:?}")),
    };
    let idempotent = json!({ "first": receipt(&first), "retryAfterRestart": receipt(&retry) });
    let idempotent_ok = idempotent["first"]["status"] == "COMMITTED"
        && idempotent["retryAfterRestart"]["status"] == "ALREADY_COMMITTED"
        && idempotent["first"]["cursor"] == idempotent["retryAfterRestart"]["cursor"];
    let diagnostics = ctx.companion().diagnostics()?;
    let engine = json!({ "sqlite": diagnostics["sqlite"]["version"], "sourceId": diagnostics["sqlite"]["sourceId"], "journalMode": diagnostics["sqlite"]["journalMode"], "synchronous": diagnostics["sqlite"]["synchronous"], "fullfsync": diagnostics["sqlite"]["fullfsync"] });
    let lost_total: usize = rounds_out
        .iter()
        .map(|r| r["lostAcknowledged"].as_array().map_or(0, Vec::len))
        .sum();
    let acked_total: u64 = rounds_out
        .iter()
        .filter_map(|r| r["acknowledged"].as_u64())
        .sum();
    let summary = json!({
        "gate": "G10",
        "portion": "live companion",
        "pass": second_writer["pass"] == true && lost_total == 0 && rounds_out.iter().all(|r| r["pass"] == true) && idempotent_ok
            && engine["journalMode"] == "wal" && engine["synchronous"] == 2,
        "secondWriter": second_writer,
        "crashRounds": rounds,
        "acknowledgedTotal": acked_total,
        "lostAcknowledgedTotal": lost_total,
        "idempotentRetry": idempotent,
        "engine": engine,
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

/// G11: ten restart cycles mixing UI, helper and both, background capture
/// with the UI absent and same-label view replacement.
pub fn restarts(ctx: &Ctx, cycles: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g11-restarts", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let app = ctx.app();
    ensure_ui(ctx)?;
    // An owner command fixture whose receipt must replay unchanged.
    let (fixture_attention, _) = raise(ctx, "g11-owner-command-fixture")?;
    let command_id = uuid::Uuid::new_v4().to_string();
    let ack = || {
        ctx.companion().request(
            ControlRequestBody::AcknowledgeAttention {
                command_id: command_id.clone(),
                attention_id: fixture_attention.clone(),
                expected_revision: None,
            },
            Duration::from_secs(10),
        )
    };
    let original = match ack()? {
        ControlResponseBody::CommandReceipt { receipt } => receipt,
        other => return Err(format!("unexpected {other:?}")),
    };
    let kinds = [
        "ui",
        "helper",
        "both",
        "capture-with-ui-absent",
        "same-label-view",
    ];
    let mut cycles_out = Vec::new();
    for cycle in 1..=cycles {
        let kind = kinds[(cycle as usize - 1) % kinds.len()];
        let _gui = ctx.gui(&format!("g11 cycle {cycle} {kind}"))?;
        let before = durable_facts(ctx)?;
        let started = Instant::now();
        let mut captured = None;
        let action: Result<Value, String> = (|| {
            match kind {
                "ui" => {
                    app.stop_all();
                    ensure_ui(ctx)?;
                }
                "helper" => {
                    kill_companion(ctx)?;
                }
                "both" => {
                    app.stop_all();
                    kill_companion(ctx)?;
                    ensure_ui(ctx)?;
                }
                "capture-with-ui-absent" => {
                    app.stop_all();
                    captured = Some(raise(ctx, &format!("g11-capture-{cycle}"))?.0);
                    kill_companion(ctx)?;
                    ensure_ui(ctx)?;
                }
                _ => {
                    ensure_ui(ctx)?;
                    let mut desktop = app.desktop_log();
                    app.view_command_nowait("reload", json!({}))?;
                    let mut lines = Vec::new();
                    desktop
                        .wait_for(
                            "OFFICE_VIEW_RECOVERED",
                            |_| true,
                            Duration::from_secs(60),
                            &mut lines,
                        )
                        .ok_or("view was not recreated")?;
                }
            }
            Ok(json!(kind))
        })();
        threadspace_harness::pause_ms(2000);
        let after = durable_facts(ctx)?;
        let replay = ack();
        let replay_ok = matches!(&replay, Ok(ControlResponseBody::CommandReceipt { receipt })
            if receipt.status == threadspace_contracts::ui::ReceiptStatus::AlreadyCommitted && receipt.cursor == original.cursor);
        let sessions_kept = before["sessions"].as_object().is_some_and(|map| {
            map.iter()
                .all(|(native, ids)| after["sessions"][native] == *ids)
        });
        let open_before: BTreeSet<String> =
            serde_json::from_value(before["openAttention"].clone()).unwrap_or_default();
        let open_after: BTreeSet<String> =
            serde_json::from_value(after["openAttention"].clone()).unwrap_or_default();
        let attention_kept = open_before.is_subset(&open_after);
        let captured_kept = captured.as_ref().is_none_or(|id| open_after.contains(id));
        let helper_restarted = matches!(kind, "helper" | "both" | "capture-with-ui-absent");
        let generations_ok = before["storeGeneration"] == after["storeGeneration"]
            && (before["coreGeneration"] != after["coreGeneration"]) == helper_restarted;
        let bindings_kept = before["bindings"].as_object().is_some_and(|map| {
            map.iter().all(|(session, binding)| {
                after["bindings"]
                    .get(session)
                    .is_none_or(|now| now == binding || now["revision"] != binding["revision"])
            })
        });
        let pass = action.is_ok()
            && replay_ok
            && sessions_kept
            && attention_kept
            && captured_kept
            && generations_ok
            && after["duplicateSessions"]
                .as_array()
                .is_some_and(Vec::is_empty);
        let record = json!({
            "cycle": cycle,
            "kind": kind,
            "action": action.unwrap_or_else(|e| json!({ "error": e })),
            "elapsedMs": started.elapsed().as_millis() as u64,
            "ownerCommandReplayAlreadyCommitted": replay_ok,
            "sessionsKept": sessions_kept,
            "unresolvedAttentionKept": attention_kept,
            "capturedWhileUiAbsentKept": captured_kept,
            "storeGenerationKept": before["storeGeneration"] == after["storeGeneration"],
            "coreGenerationChangedIffHelperRestarted": generations_ok,
            "bindingsKeptOrReproven": bindings_kept,
            "duplicateSessions": after["duplicateSessions"],
            "fixtureSessionKept": before["fixtureSession"] == after["fixtureSession"],
            "pass": pass,
        });
        run_dir
            .append("cycles.jsonl", &record)
            .map_err(|e| e.to_string())?;
        cycles_out.push(record);
    }
    let passed = cycles_out.iter().filter(|c| c["pass"] == true).count();
    let summary = json!({ "gate": "G11", "pass": passed == cycles_out.len(), "passed": passed, "total": cycles_out.len(), "ownerCommandReceipt": { "commandId": original.command_id, "cursor": original.cursor } });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

/// SPEC §19.5 ordering: a failed preparation never unregisters; a successful
/// stop prepares while supervised, unregisters only after PREPARED, then the
/// old incarnation exits and the writer lock is free; enable restores it.
/// While stopped, a second UI launch forwards to the incumbent UI without
/// starting the companion.
pub fn maintenance(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "maintenance", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let app = ctx.app();
    app.stop_all();
    let before = ctx
        .companion()
        .incarnation()
        .ok_or("companion not running")?;

    // 1. Preparation failure.
    ctx.companion().request(
        ControlRequestBody::QualifyArmFault {
            fault: QualificationFault::FailNextMaintenanceBackup,
        },
        Duration::from_secs(10),
    )?;
    let failed = service::bootstrap(&ctx.id, "stop");
    let diagnostics = ctx.companion().diagnostics().unwrap_or_default();
    let failure = json!({
        "bootstrap": failed,
        "companionUnchanged": ctx.companion().incarnation() == Some(before.clone()),
        "serviceStatus": service::status(&ctx.id),
        "observationEnabled": diagnostics["observationEnabled"],
        "maintenancePhase": diagnostics["maintenancePhase"],
    });
    let failure_ok = failed["ok"] == false
        && failed["detail"]["unregistered"] == false
        && failure["companionUnchanged"] == true
        && failure["serviceStatus"] == "ENABLED"
        && failure["observationEnabled"] == true
        && failure["maintenancePhase"] == "NONE";
    run_dir
        .write_json("01-preparation-failure.json", &failure)
        .map_err(|e| e.to_string())?;

    // 2. Ordered stop.
    let mut cursor = ctx.companion().log();
    let stopped = service::bootstrap(&ctx.id, "stop");
    let mut lines = cursor.read_new();
    lines.retain(|l| {
        [
            "OBSERVATION_PREFERENCE",
            "MAINTENANCE_PREPARING",
            "MAINTENANCE_PREPARED",
            "MAINTENANCE_PREPARE_FAILED",
        ]
        .contains(&l["event"].as_str().unwrap_or(""))
    });
    let order: Vec<&str> = lines.iter().filter_map(|l| l["event"].as_str()).collect();
    let backups = std::fs::read_dir(ctx.id.agent.store_dir.join("backups"))
        .map(|dir| {
            dir.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let stop = json!({
        "bootstrap": stopped,
        "companionLogOrder": order,
        "backupsPresent": backups,
        "companionProcessesAfter": ctx.companion().processes().len(),
        "serviceStatus": service::status(&ctx.id),
    });
    let steps: Vec<&str> = stopped["detail"]["steps"]
        .as_array()
        .map(|s| s.iter().filter_map(|x| x["step"].as_str()).collect())
        .unwrap_or_default();
    let stop_ok = stopped["ok"] == true
        && steps
            == [
                "connect",
                "record-observation-disabled",
                "prepare-maintenance",
                "unregister",
                "verify-old-incarnation-exited",
                "verify-writer-lock-released",
            ]
        && order
            == [
                "OBSERVATION_PREFERENCE",
                "MAINTENANCE_PREPARING",
                "MAINTENANCE_PREPARED",
            ]
        && stop["companionProcessesAfter"] == 0
        && stop["serviceStatus"] == "NOT_REGISTERED";
    run_dir
        .write_json("02-ordered-stop.json", &stop)
        .map_err(|e| e.to_string())?;

    // 3. Second UI launch while observation is stopped: it forwards to the
    //    incumbent UI and nothing starts the companion.
    let _gui = ctx.gui("maintenance: second UI launch")?;
    let first = app.launch_packaged(&[])?;
    threadspace_harness::pause_ms(4000);
    let second = run(
        &ctx.id.executable.display().to_string(),
        &[],
        Duration::from_secs(30),
    );
    threadspace_harness::pause_ms(2000);
    let instance = json!({
        "incumbent": first.ui,
        "secondLaunchExit": second.status,
        "secondLaunchElapsedMs": second.elapsed_ms,
        "uiProcesses": app.processes(),
        "companionProcesses": ctx.companion().processes().len(),
    });
    let instance_ok = second.status == Some(0)
        && app.processes().len() == 1
        && app.processes().first() == Some(&first.ui)
        && ctx.companion().processes().is_empty();
    app.stop(&first.ui, false);
    run_dir
        .write_json("03-second-ui-launch-observation-stopped.json", &instance)
        .map_err(|e| e.to_string())?;

    // 4. Enable again.
    let enabled = service::bootstrap(&ctx.id, "enable");
    let diagnostics = ctx.companion().diagnostics().unwrap_or_default();
    let enable = json!({
        "bootstrap": enabled,
        "companion": ctx.companion().incarnation(),
        "observationEnabled": diagnostics["observationEnabled"],
        "maintenancePhase": diagnostics["maintenancePhase"],
        "serviceStatus": service::status(&ctx.id),
    });
    let enable_ok = enabled["ok"] == true
        && enable["observationEnabled"] == true
        && enable["maintenancePhase"] == "NONE"
        && enable["serviceStatus"] == "ENABLED";
    run_dir
        .write_json("04-enable.json", &enable)
        .map_err(|e| e.to_string())?;
    let summary = json!({
        "area": "maintenance-ordering",
        "pass": failure_ok && stop_ok && instance_ok && enable_ok,
        "preparationFailureDidNotUnregister": failure_ok,
        "orderedStop": stop_ok,
        "secondUiLaunchForwardedWithoutCompanion": instance_ok,
        "enable": enable_ok,
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
