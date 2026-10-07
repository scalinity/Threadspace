//! C-02 final remediation (G06/G09; SPEC §7.5, §18.9): an accepted
//! notification intent always has an owner across a companion handoff.
//! Native scenarios against the installed identity, each starting from the
//! login item's companion owning the store with a hydrated UI:
//!
//! - AGB: a banner click cold-starts an unsupervised writer; the UI's
//!   hydration is then held (its main script is held by the shell, which
//!   releases it after 60 s), so every intent stays pending. Responses are
//!   accepted before the yield, after its reply (the post-drain
//!   counterexample), while the claimant waits for the lock, and after the
//!   writer stopped accepting. All four reach the UI exactly once, in order,
//!   from the login item's companion.
//! - CF: Stop, then a banner cold start, then the real Enable bootstrap while
//!   the yield is held for five seconds: the unsupervised writer refuses,
//!   stays and keeps accepting until the claimant takes the store.
//! - D: no claimant at all: with the UI gone the unsupervised writer reaches
//!   its idle exit with an intent pending; a login item registered later
//!   loads and delivers it.
//! - E: the yield reply is withheld (E1), and withheld while the writer's
//!   exit is held past the claimant's lock wait, so launchd relaunches the
//!   login item and its second claim converges (E2).
//!
//! Responses at exact phases are placed with `QualifyNotificationResponse`,
//! which takes the banner click's own acceptance path; the cold starts are
//! real banner clicks.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, QualificationFault};
use threadspace_harness::evidence::Run;
use threadspace_harness::procs::{self, Incarnation};
use threadspace_harness::service;

use crate::ctx::Ctx;
use crate::service_gates::writer_lock_free;
use crate::supervision::{
    Log, Sampler, checkpoint, diagnostics, invariants, is_pid, launch_witness, login_item_owns,
    press_held, public_line, raise, record, restore, single_writer, wait_login_item_owner,
    wait_new_companion,
};

/// The pending intents and spooled responses in the store, read directly.
fn durable(ctx: &Ctx) -> Value {
    let dir = &ctx.id.agent.store_dir;
    let pending: Vec<Value> = std::fs::read(dir.join("pending-intents.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
        .iter()
        .map(|intent| intent["intentId"].clone())
        .collect();
    let spooled: Vec<String> = std::fs::read_dir(dir.join("responses"))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    e.file_name()
                        .to_str()?
                        .strip_suffix(".json")
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "atMs": threadspace_harness::now_ms(), "pendingIntents": pending, "spooledResponses": spooled })
}

fn request(ctx: &Ctx, body: ControlRequestBody) -> Value {
    match ctx.companion().request(body, Duration::from_secs(10)) {
        Ok(reply) => {
            json!({ "ok": true, "reply": format!("{reply:?}").chars().take(160).collect::<String>() })
        }
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

fn arm(ctx: &Ctx, fault: QualificationFault) -> Value {
    request(ctx, ControlRequestBody::QualifyArmFault { fault })
}

/// A response accepted by the companion the locator names, as a banner
/// click would be.
fn inject(ctx: &Ctx, attention: &(String, String)) -> Value {
    let sent_ms = threadspace_harness::now_ms();
    let reply = request(
        ctx,
        ControlRequestBody::QualifyNotificationResponse {
            notification_request_id: attention.1.clone(),
            attention_id: attention.0.clone(),
        },
    );
    json!({ "sentMs": sent_ms, "intentId": attention.1, "attentionId": attention.0, "reply": reply })
}

/// The UI's hydration is held: the harness launches a fresh UI process and
/// suspends it before it connects, so no view is hydrated and every intent
/// stays pending until `thaw_ui`. The instance is the harness's own, proven
/// by executable path and birth.
fn freeze_ui(ctx: &Ctx) -> (Option<Incarnation>, Value) {
    let app = ctx.app();
    app.stop_all();
    let mut cursor = ctx.companion().log();
    let launched = match app.launch_packaged(&[]) {
        Ok(launched) => launched,
        Err(error) => return (None, json!({ "error": error })),
    };
    let stopped = procs::signal(launched.ui.pid, libc::SIGSTOP);
    let stopped_ms = threadspace_harness::now_ms();
    threadspace_harness::pause_ms(2000);
    let attached = cursor
        .read_new()
        .iter()
        .any(|l| l["event"] == "VIEW_ATTACHED" || l["event"] == "VIEW_HYDRATED");
    let sockets = threadspace_harness::run::run(
        "/usr/sbin/lsof",
        &["-a", "-U", "-p", &launched.ui.pid.to_string()],
        Duration::from_secs(5),
    );
    let detail = json!({
        "uiPid": launched.ui.pid, "launchStartedMs": launched.started_ms, "processAppearedMs": launched.process_appeared_ms,
        "stopped": stopped, "stoppedAtMs": stopped_ms, "viewAttachedWhileHeld": attached,
        "uiUnixSockets": sockets.stdout.lines().skip(1).count(),
    });
    (Some(launched.ui), detail)
}

fn thaw_ui(ui: &Option<Incarnation>) -> Value {
    let resumed = ui
        .as_ref()
        .is_some_and(|ui| procs::signal(ui.pid, libc::SIGCONT));
    json!({ "resumed": resumed, "atMs": threadspace_harness::now_ms() })
}

fn held(hold: &Value) -> bool {
    hold["stopped"] == true && hold["viewAttachedWhileHeld"] == false
}

/// UI reports of applying `intent_id` since `since_ms`.
fn applications(ctx: &Ctx, intent_id: &str, since_ms: i64) -> Vec<Value> {
    ctx.app()
        .reports("notification-intent", since_ms)
        .into_iter()
        .filter(|(_, r)| r["report"]["intentId"].as_str() == Some(intent_id))
        .map(|(_, r)| json!({ "recordedAtMs": r["recordedAtMs"], "streamSeq": r["report"]["streamSeq"] }))
        .collect()
}

/// Waits until every intent was applied, then a little longer, so a late
/// duplicate would be counted.
fn applied_once(ctx: &Ctx, ids: &[String], since_ms: i64, timeout: Duration) -> Value {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if ids
            .iter()
            .all(|id| !applications(ctx, id, since_ms).is_empty())
        {
            break;
        }
        threadspace_harness::pause_ms(500);
    }
    threadspace_harness::pause_ms(5000);
    let per: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "intentId": id, "applications": applications(ctx, id, since_ms) }))
        .collect();
    let counts: Vec<usize> = per
        .iter()
        .map(|p| p["applications"].as_array().map_or(0, Vec::len))
        .collect();
    let times: Vec<i64> = per
        .iter()
        .filter_map(|p| p["applications"][0]["recordedAtMs"].as_i64())
        .collect();
    json!({
        "perIntent": per,
        "allExactlyOnce": counts.iter().all(|c| *c == 1),
        "lost": counts.iter().filter(|c| **c == 0).count(),
        "duplicates": counts.iter().map(|c| c.saturating_sub(1)).sum::<usize>(),
        "inAcceptanceOrder": times.len() == ids.len() && times.windows(2).all(|w| w[0] <= w[1]),
    })
}

/// A hydrated UI on the current companion, launched if none runs.
fn ensure_ui(ctx: &Ctx) -> Value {
    let app = ctx.app();
    if !app.processes().is_empty()
        && ctx
            .app()
            .view_command("ping", json!({}), Duration::from_secs(10))
            .is_ok()
    {
        return json!({ "running": true });
    }
    app.stop_all();
    let mut cursor = ctx.companion().log();
    let launched = app.launch_packaged(&[]);
    let hydrated = app.wait_hydrated(&mut cursor, Duration::from_secs(60));
    json!({ "launched": launched.is_ok(), "hydrated": hydrated.is_some() })
}

/// The login item's companion owns the store, with a hydrated UI.
fn precondition(ctx: &Ctx, log: &mut Log, label: &str) -> Result<(Incarnation, Value), String> {
    let restored = restore(ctx, log);
    let ui = ensure_ui(ctx);
    let point = checkpoint(ctx, label);
    if !(single_writer(&point) && login_item_owns(&point)) {
        return Err(format!("precondition {label}: {point} {restored}"));
    }
    let owner = ctx.companion().incarnation().ok_or("no companion")?;
    Ok((
        owner,
        json!({ "restore": restored, "ui": ui, "checkpoint": point }),
    ))
}

/// The login item's companion goes away (`stop` or `unregister`) and a banner
/// click cold-starts an unsupervised writer, whose inspector intent the
/// hydrated UI applies.
fn cold_start(
    ctx: &Ctx,
    log: &mut Log,
    supervised: &Incarnation,
    how: &str,
    label: &str,
) -> Result<(Incarnation, Value), String> {
    // One GUI segment from the banner's submission to its press.
    let gui = ctx.gui("c02 handoff banner");
    let first = raise(ctx, label)?;
    let submitted = log.wait(
        "NOTIFICATION_SUBMISSION",
        &|l| l["attentionId"] == first.0.as_str() && l["state"] == "SUBMITTED",
        20,
    );
    let ended = service::bootstrap(&ctx.id, how);
    let exited = procs::wait_exit(supervised, Duration::from_secs(20));
    let pressed = press_held(ctx, label);
    drop(gui);
    // A banner can be gone by the time the login item has ended; then
    // LaunchServices starts the companion as the click would have.
    let (unsupervised, path) = match wait_new_companion(ctx, &[supervised.pid], 15) {
        Some(started) => (started, "BANNER_CLICK"),
        None => {
            let bundle = ctx
                .id
                .companion_executable
                .ancestors()
                .nth(3)
                .map(|app| app.display().to_string())
                .unwrap_or_default();
            threadspace_harness::run::run(
                "/usr/bin/open",
                &["-g", "-a", &bundle],
                Duration::from_secs(20),
            );
            let started = wait_new_companion(ctx, &[supervised.pid], 30)
                .ok_or("neither the banner click nor LaunchServices started a companion")?;
            (started, "LAUNCH_SERVICES_OPEN")
        }
    };
    let pid = unsupervised.pid;
    let ready = log.wait("CORE_READY", &is_pid(pid), 20);
    let inspector = (path == "BANNER_CLICK")
        .then(|| {
            log.wait(
                "NOTIFICATION_INSPECTOR",
                &|l| l["pid"] == pid && l["attentionId"] == first.0.as_str(),
                30,
            )
        })
        .flatten();
    let hydrated = log.wait("VIEW_HYDRATED", &is_pid(pid), 60);
    let state = log.wait("OBSERVATION_STATE", &is_pid(pid), 10);
    Ok((
        unsupervised,
        json!({
            "firstAttention": first.0, "firstIntent": first.1, "submitted": submitted.is_some(),
            "ended": { "how": how, "ok": ended["ok"], "statusAfter": ended["detail"]["statusAfter"] }, "supervisedExitedAfterMs": exited,
            "pressed": pressed["pressed"], "coldStartPath": path, "launchWitness": launch_witness(pid), "coreReady": ready.is_some(),
            "observationState": state, "inspector": inspector.is_some() || path != "BANNER_CLICK", "uiHydratedOnIt": hydrated.is_some(),
        }),
    ))
}

fn all_true(checks: &Value) -> bool {
    checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true))
}

fn ts(line: &Option<Value>) -> Option<i64> {
    line.as_ref().and_then(|l| l["ts"].as_i64())
}

pub fn c02_handoff(ctx: &Ctx, selection: &str) -> Result<Value, String> {
    let wanted: Vec<String> = selection
        .split(',')
        .map(|c| c.trim().to_uppercase())
        .collect();
    let all = wanted.iter().any(|c| c == "ALL");
    let want = |c: &str| all || wanted.iter().any(|w| w == c);
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/c02-intent-handoff",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let mut log = Log {
        cursor: ctx.companion().log(),
        seen: Vec::new(),
    };
    let mut cases: Vec<Value> = Vec::new();
    let sampler = Sampler::start(ctx.id.companion_executable.clone());
    let environment = ctx.environment();
    run_dir
        .write_json("environment.json", &environment)
        .map_err(|e| e.to_string())?;
    let gate =
        threadspace_harness::idle::wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    run_dir
        .write_json("idle-gate.json", &json!(gate))
        .map_err(|e| e.to_string())?;
    let since = threadspace_harness::now_ms();
    let tag = &uuid::Uuid::new_v4().to_string()[..6];

    let outcome = (|| -> Result<(), String> {
        if want("AGB") {
            let (supervised, pre) = precondition(ctx, &mut log, "AGB-start")?;
            let (w, cold) = cold_start(
                ctx,
                &mut log,
                &supervised,
                "unregister",
                &format!("c02h-agb0-{tag}"),
            )?;
            let pid = w.pid;
            let (ui, hold) = freeze_ui(ctx);
            let x: Vec<(String, String)> = (1..=4)
                .map(|i| raise(ctx, &format!("c02h-agb{i}-{tag}")))
                .collect::<Result<_, _>>()?;
            let d0 = durable(ctx);
            // G1: before the yield.
            let g1 = inject(ctx, &x[0]);
            let q1 = log.wait(
                "INTENT_QUEUED",
                &|l| l["pid"] == pid && l["intentId"] == x[0].1.as_str(),
                10,
            );
            let armed_reply = arm(ctx, QualificationFault::HoldNextYieldAfterReply);
            let registered = service::bootstrap(&ctx.id, "register");
            let reached = log.wait(
                "HANDOFF_BARRIER_REACHED",
                &|l| l["pid"] == pid && l["point"] == "AFTER_REPLY",
                30,
            );
            let yielded = log.wait("UNSUPERVISED_YIELD", &is_pid(pid), 5);
            let d1 = durable(ctx);
            // A / G2: accepted after the yield drained and replied.
            let g2 = inject(ctx, &x[1]);
            let q2 = log.wait(
                "INTENT_QUEUED",
                &|l| l["pid"] == pid && l["intentId"] == x[1].1.as_str(),
                10,
            );
            // G3: while the claimant waits for the lock.
            threadspace_harness::pause_ms(1500);
            let claimant_waiting = ctx
                .companion()
                .processes()
                .into_iter()
                .filter(|p| p.pid != pid)
                .collect::<Vec<_>>();
            let g3 = inject(ctx, &x[2]);
            let q3 = log.wait(
                "INTENT_QUEUED",
                &|l| l["pid"] == pid && l["intentId"] == x[2].1.as_str(),
                10,
            );
            let d2 = durable(ctx);
            let armed_exit = arm(ctx, QualificationFault::HoldNextYieldBeforeExit);
            let released_reply = request(ctx, ControlRequestBody::QualifyReleaseHandoffBarrier);
            let reached_exit = log.wait(
                "HANDOFF_BARRIER_REACHED",
                &|l| l["pid"] == pid && l["point"] == "BEFORE_EXIT",
                10,
            );
            // G4: after the writer stopped accepting, just before it exits.
            let g4 = inject(ctx, &x[3]);
            let spooled = log.wait(
                "RESPONSE_SPOOLED",
                &|l| l["pid"] == pid && l["requestId"] == x[3].1.as_str(),
                10,
            );
            let d3 = durable(ctx);
            threadspace_harness::pause_ms(1000);
            let released_exit = request(ctx, ControlRequestBody::QualifyReleaseHandoffBarrier);
            let released = log.wait("WRITER_RELEASED", &is_pid(pid), 10);
            let exited = procs::wait_exit(&w, Duration::from_secs(10));
            let owner_after = wait_login_item_owner(ctx, 30);
            let claimant = ctx.companion().incarnation();
            let cpid = claimant.as_ref().map_or(0, |c| c.pid);
            let claimed = log.wait("WRITER_CLAIMED", &is_pid(cpid), 10);
            let loaded = log.wait("PENDING_INTENTS_LOADED", &is_pid(cpid), 10);
            let recovered = log.wait(
                "NOTIFICATION_RESPONSE",
                &|l| l["pid"] == cpid && l["recovered"] == true,
                10,
            );
            let state = log.wait("OBSERVATION_STATE", &is_pid(cpid), 10);
            let unsupervised_diag_closed = log.seen.iter().any(|l| {
                l["pid"] == pid && l["event"] == "OBSERVATION_STATE" && l["admissionOpen"] == false
            });
            let thawed = thaw_ui(&ui);
            let hydrated = log.wait("VIEW_HYDRATED", &is_pid(cpid), 90);
            let ids: Vec<String> = x.iter().map(|a| a.1.clone()).collect();
            let applied = applied_once(ctx, &ids, since, Duration::from_secs(30));
            let first_once = cold["coldStartPath"] != "BANNER_CLICK"
                || applications(ctx, cold["firstIntent"].as_str().unwrap_or_default(), since).len()
                    == 1;
            let d4 = durable(ctx);
            let point = checkpoint(ctx, "AGB-after");
            run_dir
                .append("checkpoints.jsonl", &point)
                .map_err(|e| e.to_string())?;
            let reply_reported: Vec<String> = yielded
                .as_ref()
                .and_then(|l| l["pendingIntents"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            let checks = json!({
                "unsupervisedColdStart": cold["launchWitness"]["xpcServiceName"].as_str().is_some_and(|n| n.starts_with("application.")) && cold["inspector"] == true,
                "hydrationHeld": held(&hold),
                "acceptedBeforeYieldPersisted": q1.is_some() && d1["pendingIntents"].as_array().is_some_and(|p| p.iter().any(|i| i == x[0].1.as_str())),
                "yieldReplied": reached.is_some() && reply_reported == vec![x[0].1.clone()],
                "postDrainAcceptedByOldWriter": q2.as_ref().zip(reached.as_ref()).is_some_and(|(q, r)| q["ts"].as_i64() > r["reachedAtMs"].as_i64()) && !reply_reported.contains(&x[1].1),
                "acceptedWhileClaimantWaits": q3.is_some() && !claimant_waiting.is_empty(),
                "persistedBeforeRelease": d2["pendingIntents"].as_array().is_some_and(|p| p.len() == 3),
                "lateResponseSpooled": spooled.is_some() && d3["spooledResponses"].as_array().is_some_and(|s| s.iter().any(|r| r == x[3].1.as_str())),
                "oldWriterExitedOnlyAfterRelease": ts(&released).is_some() && exited.is_some() && released.as_ref().is_some_and(|l| l["pendingIntents"].as_array().is_some_and(|p| p.len() == 3)),
                "claimedAfterRelease": ts(&claimed) > ts(&released) && owner_after.is_some(),
                "claimantLoadedAll": loaded.as_ref().is_some_and(|l| l["intents"].as_array().is_some_and(|i| i.len() == 3) && l["spooledResponses"].as_array().is_some_and(|s| s.len() == 1)) && recovered.is_some(),
                "hydratedOnClaimant": hydrated.is_some(),
                "allFourExactlyOnceInOrder": applied["allExactlyOnce"] == true && applied["inAcceptanceOrder"] == true,
                "coldStartIntentNotRepeated": first_once,
                "storeEmptyAfterDelivery": d4["pendingIntents"].as_array().is_some_and(Vec::is_empty) && d4["spooledResponses"].as_array().is_some_and(Vec::is_empty),
                "noUnsupervisedAdmission": unsupervised_diag_closed,
                "claimantObserves": state.as_ref().is_some_and(|l| l["admissionOpen"] == true),
                "singleWriter": single_writer(&point) && login_item_owns(&point),
            });
            record(
                &run_dir,
                &mut cases,
                "AGB-post-drain-delayed-hydration-four-phases",
                all_true(&checks),
                json!({
                    "checks": checks, "precondition": pre, "coldStart": cold, "hydrationHold": hold, "thawed": thawed,
                    "phases": { "beforeYield": g1, "afterReply": g2, "claimantWaiting": g3, "afterAcceptanceClosed": g4 },
                    "queued": [q1, q2, q3], "spooled": spooled, "armed": [armed_reply, armed_exit], "register": registered["ok"],
                    "barrierAfterReply": reached, "yield": yielded, "claimantWaitingProcesses": claimant_waiting, "releaseReply": released_reply,
                    "barrierBeforeExit": reached_exit, "releaseExit": released_exit, "writerReleased": released, "oldWriterExitedAfterMs": exited,
                    "claimant": claimant, "claim": claimed, "loaded": loaded, "recoveredResponse": recovered, "claimantState": state, "hydratedOnClaimant": hydrated.map(|l| l["ts"].clone()),
                    "applied": applied, "durable": [d0, d1, d2, d3, d4],
                }),
            )?;
        }

        if want("CF") {
            let (supervised, pre) = precondition(ctx, &mut log, "CF-start")?;
            // Raised while writes are open: Stop leaves the store prepared
            // for maintenance, which the cold-started writer inherits.
            let x6 = raise(ctx, &format!("c02h-cf6-{tag}"))?;
            let x7 = raise(ctx, &format!("c02h-cf7-{tag}"))?;
            let (w, cold) = cold_start(
                ctx,
                &mut log,
                &supervised,
                "stop",
                &format!("c02h-cf0-{tag}"),
            )?;
            let pid = w.pid;
            let (ui, hold) = freeze_ui(ctx);
            let g6 = inject(ctx, &x6);
            let q6 = log.wait(
                "INTENT_QUEUED",
                &|l| l["pid"] == pid && l["intentId"] == x6.1.as_str(),
                10,
            );
            let before = diagnostics(ctx);
            let armed = arm(ctx, QualificationFault::HoldNextYieldAfterReply);
            let (enable, detail) = std::thread::scope(|scope| {
                let enabling = scope.spawn(|| service::bootstrap(&ctx.id, "enable"));
                let refused = log.wait("UNSUPERVISED_ENABLE_REFUSED", &is_pid(pid), 30);
                let reached = log.wait(
                    "HANDOFF_BARRIER_REACHED",
                    &|l| l["pid"] == pid && l["point"] == "AFTER_REPLY",
                    30,
                );
                // The claimant is held off for five seconds; the old writer stays.
                threadspace_harness::pause_ms(5000);
                let alive_after_wait = w.alive();
                let during = diagnostics(ctx);
                let g7 = inject(ctx, &x7);
                let q7 = log.wait(
                    "INTENT_QUEUED",
                    &|l| l["pid"] == pid && l["intentId"] == x7.1.as_str(),
                    10,
                );
                let d = durable(ctx);
                let released_reply = request(ctx, ControlRequestBody::QualifyReleaseHandoffBarrier);
                let released = log.wait("WRITER_RELEASED", &is_pid(pid), 10);
                let exited = procs::wait_exit(&w, Duration::from_secs(10));
                let enable = enabling
                    .join()
                    .unwrap_or_else(|_| json!({ "error": "enable thread panicked" }));
                (
                    enable,
                    json!({ "refused": refused, "barrier": reached, "aliveAfterFiveSeconds": alive_after_wait, "during": during, "afterReply": g7, "queued7": q7, "durable": d, "releaseReply": released_reply, "released": released, "exitedAfterMs": exited }),
                )
            });
            let claimant = ctx.companion().incarnation();
            let cpid = claimant.as_ref().map_or(0, |c| c.pid);
            let claimed = log.wait("WRITER_CLAIMED", &is_pid(cpid), 10);
            let loaded = log.wait("PENDING_INTENTS_LOADED", &is_pid(cpid), 10);
            let thawed = thaw_ui(&ui);
            let hydrated = log.wait("VIEW_HYDRATED", &is_pid(cpid), 90);
            let ids = vec![x6.1.clone(), x7.1.clone()];
            let applied = applied_once(ctx, &ids, since, Duration::from_secs(30));
            let after = diagnostics(ctx);
            let point = checkpoint(ctx, "CF-after");
            run_dir
                .append("checkpoints.jsonl", &point)
                .map_err(|e| e.to_string())?;
            let refused_ms = detail["refused"]["ts"].as_i64();
            let released_ms = detail["released"]["ts"].as_i64();
            let checks = json!({
                "hydrationHeld": held(&hold),
                "stopThenColdStart": cold["ended"]["ok"] == true && cold["inspector"] == true,
                "preferenceDisabledAndClosedOnOld": before["observationEnabled"] == false && before["admissionOpen"] == false && detail["during"]["observationEnabled"] == false && detail["during"]["admissionOpen"] == false,
                "enableRefusedByOld": refused_ms.is_some(),
                "oldStayedPastTheFormerTimer": detail["aliveAfterFiveSeconds"] == true && matches!((refused_ms, released_ms), (Some(r), Some(rel)) if rel - r > 5000),
                "pendingKeptAndLateAccepted": q6.is_some() && detail["queued7"].is_object() && detail["durable"]["pendingIntents"].as_array().is_some_and(|p| p.len() == 2),
                "oldExitedOnlyAfterClaimantYield": detail["barrier"].is_object() && released_ms.is_some() && detail["exitedAfterMs"].is_number(),
                "claimantLoadedBoth": claimed.is_some() && loaded.as_ref().is_some_and(|l| l["intents"].as_array().is_some_and(|i| i.len() == 2)),
                "enableSucceeded": enable["ok"] == true,
                "preferenceEnabledOnClaimant": after["observationEnabled"] == true && after["admissionOpen"] == true && after["launchProvenance"] == "LOGIN_ITEM",
                "bothExactlyOnce": hydrated.is_some() && applied["allExactlyOnce"] == true && applied["inAcceptanceOrder"] == true,
                "singleWriter": single_writer(&point) && login_item_owns(&point),
            });
            record(
                &run_dir,
                &mut cases,
                "CF-enable-refusal-delayed-claimant",
                all_true(&checks),
                json!({
                    "checks": checks, "precondition": pre, "coldStart": cold, "hydrationHold": hold, "thawed": thawed, "beforeYield": g6, "armed": armed,
                    "diagnosticsBefore": before, "enable": { "ok": enable["ok"], "detail": enable["detail"] }, "observed": detail,
                    "claimant": claimant, "claim": claimed, "loaded": loaded, "applied": applied, "diagnosticsAfter": after,
                }),
            )?;
        }

        if want("D") {
            let (supervised, pre) = precondition(ctx, &mut log, "D-start")?;
            let (w, cold) = cold_start(
                ctx,
                &mut log,
                &supervised,
                "unregister",
                &format!("c02h-d0-{tag}"),
            )?;
            let pid = w.pid;
            let (ui, hold) = freeze_ui(ctx);
            let x9 = raise(ctx, &format!("c02h-d9-{tag}"))?;
            let g9 = inject(ctx, &x9);
            let q9 = log.wait(
                "INTENT_QUEUED",
                &|l| l["pid"] == pid && l["intentId"] == x9.1.as_str(),
                10,
            );
            let closed = diagnostics(ctx);
            // No claimant, and nothing connected (the held UI never
            // connected): the idle lifetime runs out.
            let d_before = durable(ctx);
            let idle_started = Instant::now();
            let exited = procs::wait_exit(&w, Duration::from_secs(200));
            let idle_ms = idle_started.elapsed().as_millis() as u64;
            log.drain();
            let idle_exit = log.find("DISABLED_IDLE_EXIT", &is_pid(pid));
            let released = log.find("WRITER_RELEASED", &is_pid(pid));
            let d_after_exit = durable(ctx);
            let gap = json!({ "companions": ctx.companion().processes(), "lockFree": writer_lock_free(ctx), "serviceStatus": service::status(&ctx.id) });
            // Later the login item comes back and takes responsibility.
            let registered = service::bootstrap(&ctx.id, "register");
            let owner_after = wait_login_item_owner(ctx, 45);
            let claimant = ctx.companion().incarnation();
            let cpid = claimant.as_ref().map_or(0, |c| c.pid);
            let acquired = log.wait("WRITER_LOCK_ACQUIRED", &is_pid(cpid), 10);
            let loaded = log.wait("PENDING_INTENTS_LOADED", &is_pid(cpid), 10);
            let thawed = thaw_ui(&ui);
            let hydrated = log.wait("VIEW_HYDRATED", &is_pid(cpid), 60);
            let applied = applied_once(
                ctx,
                std::slice::from_ref(&x9.1),
                since,
                Duration::from_secs(60),
            );
            let point = checkpoint(ctx, "D-after");
            run_dir
                .append("checkpoints.jsonl", &point)
                .map_err(|e| e.to_string())?;
            let checks = json!({
                "pendingWhileUnsupervised": q9.is_some() && d_before["pendingIntents"].as_array().is_some_and(|p| p.iter().any(|i| i == x9.1.as_str())),
                "controlOnlyNoAdmission": closed["admissionOpen"] == false && closed["launchProvenance"] == "LAUNCH_SERVICES",
                "idleExitThroughWriter": exited.is_some() && idle_exit.is_some() && released.as_ref().is_some_and(|l| l["pendingIntents"].as_array().is_some_and(|p| p.iter().any(|i| i == x9.1.as_str()))),
                "intentDurablyRetained": d_after_exit["pendingIntents"].as_array().is_some_and(|p| p.iter().any(|i| i == x9.1.as_str())) && gap["lockFree"] == true,
                "laterClaimantLoaded": registered["ok"] == true && owner_after.is_some() && acquired.is_some() && loaded.as_ref().is_some_and(|l| l["intents"].as_array().is_some_and(|i| i.iter().any(|v| v == x9.1.as_str()))),
                "hydrationHeld": held(&hold),
                "exactlyOnce": applied["allExactlyOnce"] == true,
                "singleWriter": single_writer(&point) && login_item_owns(&point),
            });
            record(
                &run_dir,
                &mut cases,
                "D-claimant-unavailable-idle-exit",
                all_true(&checks),
                json!({
                    "checks": checks, "precondition": pre, "coldStart": cold, "hydrationHold": hold, "thawed": thawed, "accepted": g9, "queued": q9,
                    "diagnosticsWhileUnsupervised": closed, "durableBefore": d_before, "waitedForIdleExitMs": idle_ms, "exitedAfterMs": exited,
                    "idleExit": idle_exit, "released": released, "durableAfterExit": d_after_exit, "gap": gap,
                    "register": registered["ok"], "claimant": claimant, "acquired": acquired, "loaded": loaded, "hydratedOnClaimant": hydrated.map(|l| l["ts"].clone()), "applied": applied,
                }),
            )?;
        }

        if want("E") || want("E1") {
            // E1: the reply is withheld; the claimant waits for the lock anyway.
            let (supervised, pre) = precondition(ctx, &mut log, "E1-start")?;
            let (w, cold) = cold_start(
                ctx,
                &mut log,
                &supervised,
                "unregister",
                &format!("c02h-e0-{tag}"),
            )?;
            let pid = w.pid;
            let (ui, hold) = freeze_ui(ctx);
            let x11 = raise(ctx, &format!("c02h-e11-{tag}"))?;
            let g11 = inject(ctx, &x11);
            let armed = arm(ctx, QualificationFault::DropNextYieldReply);
            let registered = service::bootstrap(&ctx.id, "register");
            let dropped = log.wait("YIELD_REPLY_DROPPED", &is_pid(pid), 30);
            let released = log.wait("WRITER_RELEASED", &is_pid(pid), 10);
            let exited = procs::wait_exit(&w, Duration::from_secs(10));
            let owner_after = wait_login_item_owner(ctx, 30);
            let claimant = ctx.companion().incarnation();
            let cpid = claimant.as_ref().map_or(0, |c| c.pid);
            let unanswered = log.wait("WRITER_CLAIM_UNANSWERED", &is_pid(cpid), 10);
            let claimed = log.wait("WRITER_CLAIMED", &is_pid(cpid), 10);
            let loaded = log.wait("PENDING_INTENTS_LOADED", &is_pid(cpid), 10);
            let thawed = thaw_ui(&ui);
            let hydrated = log.wait("VIEW_HYDRATED", &is_pid(cpid), 90);
            let applied = applied_once(
                ctx,
                std::slice::from_ref(&x11.1),
                since,
                Duration::from_secs(30),
            );
            let point = checkpoint(ctx, "E1-after");
            run_dir
                .append("checkpoints.jsonl", &point)
                .map_err(|e| e.to_string())?;
            let checks = json!({
                "hydrationHeld": held(&hold),
                "replyWithheld": dropped.is_some(),
                "oldReleasedWithIntentInStore": released.as_ref().is_some_and(|l| l["pendingIntents"].as_array().is_some_and(|p| p.iter().any(|i| i == x11.1.as_str()))) && exited.is_some(),
                "claimantSawNoReplyAndConverged": unanswered.is_some() && claimed.is_some() && owner_after.is_some(),
                "claimantLoaded": loaded.as_ref().is_some_and(|l| l["intents"].as_array().is_some_and(|i| i.iter().any(|v| v == x11.1.as_str()))),
                "exactlyOnce": hydrated.is_some() && applied["allExactlyOnce"] == true,
                "singleWriter": single_writer(&point) && login_item_owns(&point),
            });
            record(
                &run_dir,
                &mut cases,
                "E1-yield-reply-withheld",
                all_true(&checks),
                json!({
                    "checks": checks, "precondition": pre, "coldStart": cold, "hydrationHold": hold, "thawed": thawed, "accepted": g11, "armed": armed,
                    "register": registered["ok"], "dropped": dropped, "released": released, "exitedAfterMs": exited,
                    "claimant": claimant, "unanswered": unanswered, "claim": claimed, "loaded": loaded, "applied": applied,
                }),
            )?;
        }
        if want("E") || want("E2") {
            // E2: the reply is withheld and the old writer's exit held past
            // the claimant's lock wait; launchd relaunches the login item.
            let (supervised, pre) = precondition(ctx, &mut log, "E2-start")?;
            let (w, cold) = cold_start(
                ctx,
                &mut log,
                &supervised,
                "unregister",
                &format!("c02h-e2-{tag}"),
            )?;
            let pid = w.pid;
            let (ui, hold) = freeze_ui(ctx);
            let x13 = raise(ctx, &format!("c02h-e13-{tag}"))?;
            let x14 = raise(ctx, &format!("c02h-e14-{tag}"))?;
            let g13 = inject(ctx, &x13);
            let armed = [
                arm(ctx, QualificationFault::DropNextYieldReply),
                arm(ctx, QualificationFault::HoldNextYieldBeforeExit),
            ];
            let registered = service::bootstrap(&ctx.id, "register");
            let reached = log.wait(
                "HANDOFF_BARRIER_REACHED",
                &|l| l["pid"] == pid && l["point"] == "BEFORE_EXIT",
                30,
            );
            let g14 = inject(ctx, &x14);
            let spooled = log.wait(
                "RESPONSE_SPOOLED",
                &|l| l["pid"] == pid && l["requestId"] == x14.1.as_str(),
                10,
            );
            let first_failed = log.wait("WRITER_CLAIM_LOCK_FAILED", &|l| l["pid"] != pid, 30);
            let first_pid = first_failed
                .as_ref()
                .and_then(|l| l["pid"].as_i64())
                .unwrap_or(0);
            let first_failed_ms = ts(&first_failed).unwrap_or(i64::MAX);
            // Only the relaunched claimant's own attempt, after the first gave up.
            let retry_unanswered = log.wait(
                "WRITER_CLAIM_UNANSWERED",
                &|l| {
                    l["pid"] != pid
                        && l["pid"].as_i64() != Some(first_pid)
                        && l["ts"].as_i64().is_some_and(|t| t > first_failed_ms)
                },
                90,
            );
            let released_exit = request(ctx, ControlRequestBody::QualifyReleaseHandoffBarrier);
            let released = log.wait("WRITER_RELEASED", &is_pid(pid), 10);
            let exited = procs::wait_exit(&w, Duration::from_secs(10));
            let owner_after = wait_login_item_owner(ctx, 30);
            let claimant = ctx.companion().incarnation();
            let cpid = claimant.as_ref().map_or(0, |c| c.pid);
            let claimed = log.wait("WRITER_CLAIMED", &is_pid(cpid), 10);
            let loaded = log.wait("PENDING_INTENTS_LOADED", &is_pid(cpid), 10);
            log.drain();
            let yields = log
                .seen
                .iter()
                .filter(|l| l["pid"] == pid && l["event"] == "UNSUPERVISED_YIELD")
                .count();
            let thawed = thaw_ui(&ui);
            let hydrated = log.wait("VIEW_HYDRATED", &is_pid(cpid), 90);
            let ids = vec![x13.1.clone(), x14.1.clone()];
            let applied = applied_once(ctx, &ids, since, Duration::from_secs(30));
            let point = checkpoint(ctx, "E2-after");
            run_dir
                .append("checkpoints.jsonl", &point)
                .map_err(|e| e.to_string())?;
            let checks = json!({
                "hydrationHeld": held(&hold),
                "firstClaimGaveUpWhileOldHeld": reached.is_some() && first_failed.is_some(),
                "launchdRetriedTheClaim": retry_unanswered.as_ref().is_some_and(|l| l["pid"] == cpid) && cpid as i64 != first_pid,
                "oldAnsweredTheRetryWhileDraining": yields == 2,
                "lateResponseSpooledThenRecovered": spooled.is_some() && loaded.as_ref().is_some_and(|l| l["spooledResponses"].as_array().is_some_and(|s| s.iter().any(|r| r == x14.1.as_str()))),
                "claimantLoadedPending": loaded.as_ref().is_some_and(|l| l["intents"].as_array().is_some_and(|i| i.iter().any(|v| v == x13.1.as_str()))),
                "convergedAfterRelease": released.is_some() && exited.is_some() && claimed.is_some() && owner_after.is_some() && ts(&claimed) > ts(&released),
                "bothExactlyOnce": hydrated.is_some() && applied["allExactlyOnce"] == true,
                "singleWriter": single_writer(&point) && login_item_owns(&point),
            });
            record(
                &run_dir,
                &mut cases,
                "E2-withheld-reply-claim-retried-by-launchd",
                all_true(&checks),
                json!({
                    "checks": checks, "precondition": pre, "coldStart": cold, "hydrationHold": hold, "thawed": thawed, "accepted": g13, "afterAcceptanceClosed": g14,
                    "armed": armed, "register": registered["ok"], "barrierBeforeExit": reached, "spooled": spooled,
                    "firstClaimFailed": first_failed, "retryUnanswered": retry_unanswered, "releaseExit": released_exit,
                    "released": released, "exitedAfterMs": exited, "claimant": claimant, "claim": claimed, "loaded": loaded,
                    "yieldsAnsweredByOld": yields, "applied": applied,
                }),
            )?;
        }
        Ok(())
    })();
    let outcome_error = outcome.err();

    // A held UI is always resumed, however the scenarios ended.
    for ui in ctx.app().processes() {
        procs::signal(ui.pid, libc::SIGCONT);
    }
    let restored = restore(ctx, &mut log);
    run_dir
        .write_json("restore.json", &restored)
        .map_err(|e| e.to_string())?;
    let _ = ensure_ui(ctx);
    let _ = crate::cleanup::resolve_qualification(ctx, "c02 handoff qualification cleanup");
    let _ = crate::cleanup::clear_notifications(ctx);
    log.drain();
    let samples = sampler.finish();
    let f = invariants(&log, &samples);
    let lost: u64 = cases
        .iter()
        .map(|c| c["detail"]["applied"]["lost"].as_u64().unwrap_or(0))
        .sum();
    let duplicates: u64 = cases
        .iter()
        .map(|c| c["detail"]["applied"]["duplicates"].as_u64().unwrap_or(0))
        .sum();
    record(
        &run_dir,
        &mut cases,
        "single-writer-and-observer",
        f["pass"] == true,
        json!({ "invariants": f }),
    )?;
    run_dir
        .write_text(
            "companion-log.jsonl",
            &log.seen
                .iter()
                .map(|l| public_line(l).to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .map_err(|e| e.to_string())?;
    let summary = json!({
        "issue": "C-02",
        "gates": ["G06", "G09"],
        "selection": wanted,
        "pass": outcome_error.is_none() && cases.iter().all(|c| c["pass"] == true) && lost == 0 && duplicates == 0,
        "error": outcome_error,
        "lostIntents": lost,
        "duplicateApplications": duplicates,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "companionSha256": environment["companionSha256"],
        "executableSha256": environment["executableSha256"],
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
