//! Final C-02/C-11 remediation (G06/G08/G09; SPEC §7.5, §13.2, §18.9).
//!
//! `c02-durable` qualifies durable ownership of accepted notification work
//! on the installed identity:
//! - A: forty responses accepted while no view is hydrated (beyond the old
//!   32-intent bound), handed to the login item and delivered in order;
//! - B: storage failures injected into the real intent store: no false
//!   acceptance, a failed replacement keeps its record, the record is
//!   retired only after the held commit lands, a failed consumption is
//!   reported and not re-applied, and exits after failures lose nothing;
//! - C: a notification Return in flight when the companion is killed or
//!   observation is stopped, and a normal Return.
//!
//! `c11-receipt` qualifies the notification Return's receipt deadline: a
//! Return queued behind a held one past its own deadline, one that waited
//! part of it, and a direct route.
//!
//! Every response is traced: acceptance, owner, store revision, writer,
//! claimant, deliveries, consumption and final disposition.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_contracts::control::{
    ControlRequestBody, ControlResponseBody, QualificationFault, StorageFaultStep,
    StorageFaultStore,
};
use threadspace_harness::evidence::Run;
use threadspace_harness::procs;
use threadspace_harness::service;
use threadspace_harness::terminal::Tab;

use crate::ctx::Ctx;
use crate::deadline::{exact, route_full};
use crate::handoff::{
    all_true, applications, applied_once, arm, cold_start, durable, freeze_ui, held, inject,
    precondition, request, thaw_ui, ts,
};
use crate::service_gates::kill_companion;
use crate::supervision::{
    Log, Sampler, attention_state, checkpoint, invariants, is_pid, login_item_owns, public_line,
    raise, record, restore, single_writer, wait_login_item_owner,
};
use crate::terminal_gates::{ClaudeTab, locked, raise_window, spawn_claude, window_selection};

const TRACED: &[&str] = &[
    "QUALIFY_NOTIFICATION_RESPONSE",
    "RESPONSE_RECORDED",
    "RESPONSE_RECORD_FAILED",
    "RESPONSE_LEFT_RECORDED",
    "NOTIFICATION_RESPONSE",
    "NOTIFICATION_RETURN_STARTED",
    "NOTIFICATION_RETURN",
    "INTENT_ACCEPTED",
    "INTENT_NOT_ACCEPTED",
    "NOTIFICATION_RESPONSE_NOT_ACCEPTED",
    "RESPONSE_RECORD_RETIRED",
    "RESPONSE_ALREADY_TAKEN",
    "INTENT_DELIVERED",
    "INTENT_CONSUMED",
    "CONSUMPTION_NOT_RECORDED",
];

fn mentions(line: &Value, id: &str) -> bool {
    ["requestId", "intentId", "notificationRequestId"]
        .iter()
        .any(|key| line[*key].as_str() == Some(id))
}

/// Every traced companion line about one response, in order, and where it
/// ended.
fn trace(log: &Log, id: &str, injected: Option<&str>) -> Value {
    let events: Vec<Value> = log
        .seen
        .iter()
        .filter(|line| {
            line["event"]
                .as_str()
                .is_some_and(|event| TRACED.contains(&event))
                && mentions(line, id)
        })
        .map(|line| {
            json!({
                "ts": line["ts"], "pid": line["pid"], "event": line["event"],
                "revision": line["revision"], "plan": line["plan"], "recovered": line["recovered"],
                "recorded": line["recorded"], "failure": line["failure"], "owner": line["owner"],
                "inFlight": line["inFlight"], "queuedMs": line["queuedMs"], "remainingMs": line["remainingMs"],
                "route": line["route"].get("reasonCode").map(|_| json!({ "reasonCode": line["route"]["reasonCode"], "focusPerformed": line["route"]["focusPerformed"], "latencyMs": line["route"]["latencyMs"] })),
            })
        })
        .collect();
    let has = |event: &str| events.iter().any(|e| e["event"] == event);
    let disposition = if has("INTENT_CONSUMED") {
        "CONSUMED"
    } else if has("INTENT_ACCEPTED") {
        "PENDING_IN_BACKLOG"
    } else if has("NOTIFICATION_RESPONSE_NOT_ACCEPTED") {
        "NOT_ACCEPTED"
    } else if has("RESPONSE_RECORDED") {
        "OWNED_BY_RECORD"
    } else {
        "UNKNOWN"
    };
    json!({ "requestId": id, "intentId": id, "injectedFault": injected, "events": events, "finalDisposition": disposition })
}

fn write_traces(
    run_dir: &Run,
    log: &mut Log,
    ids: &[(String, Option<&str>)],
) -> Result<(), String> {
    log.drain();
    for (id, fault) in ids {
        run_dir
            .append("traces.jsonl", &trace(log, id, *fault))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn storage_fault(ctx: &Ctx, store: StorageFaultStore, step: StorageFaultStep, count: u32) -> Value {
    request(
        ctx,
        ControlRequestBody::QualifyArmStorageFault { store, step, count },
    )
}

fn raise_for(ctx: &Ctx, label: &str, session: &str) -> Result<(String, String), String> {
    match ctx.companion().request(
        ControlRequestBody::QualifyRaiseAttention {
            label: label.to_owned(),
            session_id: Some(session.to_owned()),
        },
        Duration::from_secs(10),
    )? {
        ControlResponseBody::AttentionRaised {
            attention_id,
            notification_request_id,
            ..
        } => Ok((attention_id, notification_request_id)),
        other => Err(format!("unexpected {other:?}")),
    }
}

fn fresh(attention: &(String, String)) -> (String, String) {
    (attention.0.clone(), uuid::Uuid::new_v4().to_string())
}

fn has_id(list: &Value, id: &str) -> bool {
    list.as_array()
        .is_some_and(|items| items.iter().any(|item| item == id))
}

fn claimant_after(ctx: &Ctx, log: &mut Log) -> (i32, Option<Value>) {
    let _ = wait_login_item_owner(ctx, 45);
    let pid = ctx.companion().incarnation().map_or(0, |c| c.pid);
    let loaded = log.wait("PENDING_INTENTS_LOADED", &is_pid(pid), 15);
    (pid, loaded)
}

pub fn c02_durable(ctx: &Ctx, selection: &str) -> Result<Value, String> {
    let wanted: Vec<String> = selection
        .split(',')
        .map(|c| c.trim().to_uppercase())
        .collect();
    let all = wanted.iter().any(|c| c == "ALL");
    let want = |c: &str| all || wanted.iter().any(|w| w == c);
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/c02-durable-ownership",
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
    let root = PathBuf::from(format!("/private/tmp/ts-m0c-c02d-{tag}"));
    let mut windows: Vec<ClaudeTab> = Vec::new();
    let mut spares: Vec<Tab> = Vec::new();

    let outcome = (|| -> Result<(), String> {
        if want("A") {
            backlog_case(ctx, &run_dir, &mut log, &mut cases, since, tag)?;
        }
        if want("B") {
            storage_matrix(ctx, &run_dir, &mut log, &mut cases, since, tag)?;
        }
        if want("C") {
            return_ownership(
                ctx,
                &run_dir,
                &mut log,
                &mut cases,
                since,
                tag,
                &root,
                &mut windows,
                &mut spares,
            )?;
        }
        Ok(())
    })();
    let outcome_error = outcome.err();
    for ui in ctx.app().processes() {
        procs::signal(ui.pid, libc::SIGCONT);
    }
    let restored = restore(ctx, &mut log);
    run_dir
        .write_json("restore.json", &restored)
        .map_err(|e| e.to_string())?;
    locked(ctx, "c02d cleanup", || {
        for window in &windows {
            let _ = run_dir.append("cleanup.jsonl", &window.tab.close());
        }
        for spare in &spares {
            let _ = run_dir.append("cleanup.jsonl", &spare.close());
        }
    });
    let _ = crate::cleanup::resolve_qualification(ctx, "c02 durable qualification cleanup");
    let _ = crate::cleanup::clear_notifications(ctx);
    log.drain();
    let samples = sampler.finish();
    let f = invariants(&log, &samples);
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
    let lost: u64 = cases
        .iter()
        .map(|c| c["detail"]["applied"]["lost"].as_u64().unwrap_or(0))
        .sum();
    let duplicates: u64 = cases
        .iter()
        .map(|c| c["detail"]["applied"]["duplicates"].as_u64().unwrap_or(0))
        .sum();
    let summary = json!({
        "issue": "C-02",
        "gates": ["G06", "G09"],
        "selection": wanted,
        "pass": outcome_error.is_none() && cases.iter().all(|c| c["pass"] == true) && lost == 0 && duplicates == 0,
        "error": outcome_error,
        "lostAcceptedIntents": lost,
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

/// A: forty responses accepted with no view hydrated.
fn backlog_case(
    ctx: &Ctx,
    run_dir: &Run,
    log: &mut Log,
    cases: &mut Vec<Value>,
    since: i64,
    tag: &str,
) -> Result<(), String> {
    let (supervised, pre) = precondition(ctx, log, "A-start")?;
    let (w, cold) = cold_start(
        ctx,
        log,
        &supervised,
        "unregister",
        &format!("c02d-a0-{tag}"),
    )?;
    let pid = w.pid;
    let (ui, hold) = freeze_ui(ctx);
    let x = [
        raise(ctx, &format!("c02d-a1-{tag}"))?,
        raise(ctx, &format!("c02d-a2-{tag}"))?,
    ];
    let mut responses = Vec::new();
    for order in 0..40 {
        let response = fresh(&x[order % 2]);
        let sent = inject(ctx, &response);
        let accepted = log.wait(
            "INTENT_ACCEPTED",
            &|l| l["pid"] == pid && l["intentId"] == response.1.as_str(),
            10,
        );
        responses.push(json!({
            "order": order, "requestId": response.1, "attentionId": response.0, "sent": sent,
            "acceptance": accepted.as_ref().map_or(json!("NOT_ACCEPTED"), |_| json!("ACCEPTED")),
            "owner": accepted.as_ref().map(|_| "BACKLOG"),
            "revision": accepted.as_ref().map(|l| l["revision"].clone()),
            "pendingAfter": accepted.as_ref().map(|l| l["pending"].clone()),
            "writerPid": pid,
        }));
    }
    let ids: Vec<String> = responses
        .iter()
        .filter_map(|r| r["requestId"].as_str().map(str::to_owned))
        .collect();
    let accepted_store = durable(ctx);
    let registered = service::bootstrap(&ctx.id, "register");
    let released = log.wait("WRITER_RELEASED", &is_pid(pid), 30);
    let exited = procs::wait_exit(&w, Duration::from_secs(15));
    let (cpid, loaded) = claimant_after(ctx, log);
    let thawed = thaw_ui(&ui);
    let hydrated = log.wait("VIEW_HYDRATED", &is_pid(cpid), 90);
    let applied = applied_once(ctx, &ids, since, Duration::from_secs(90));
    log.drain();
    let max_in_flight = log
        .seen
        .iter()
        .filter(|l| l["pid"] == cpid && l["event"] == "INTENT_DELIVERED")
        .filter_map(|l| l["inFlight"].as_u64())
        .max();
    let end_store = durable(ctx);
    let point = checkpoint(ctx, "A-after");
    run_dir
        .append("checkpoints.jsonl", &point)
        .map_err(|e| e.to_string())?;
    let loaded_ids: Vec<String> = loaded
        .as_ref()
        .and_then(|l| l["intents"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let stored_ids: Vec<String> = accepted_store["pendingIntents"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let checks = json!({
        "beyondTheOldBound": ids.len() == 40,
        "everyResponseAcceptedDurably": responses.iter().all(|r| r["acceptance"] == "ACCEPTED"),
        "noneEvictedInAcceptanceOrder": stored_ids == ids,
        "oldWriterReleasedAll": released.as_ref().is_some_and(|l| l["pendingIntents"].as_array().is_some_and(|p| p.len() == 40)) && exited.is_some(),
        "claimantLoadedAllInOrder": loaded_ids == ids,
        "hydrationHeld": held(&hold),
        "deliveredExactlyOnceInOrder": hydrated.is_some() && applied["allExactlyOnce"] == true && applied["inAcceptanceOrder"] == true,
        "deliveryWindowBoundedAt32": max_in_flight == Some(32),
        "storeEmptyAfterConsumption": end_store["pendingIntents"].as_array().is_some_and(Vec::is_empty) && end_store["records"].as_array().is_some_and(Vec::is_empty),
        "singleWriter": single_writer(&point) && login_item_owns(&point),
    });
    write_traces(
        run_dir,
        log,
        &ids.iter().map(|id| (id.clone(), None)).collect::<Vec<_>>(),
    )?;
    record(
        run_dir,
        cases,
        "A-backlog-beyond-32",
        all_true(&checks),
        json!({
            "checks": checks, "precondition": pre, "coldStart": cold, "hydrationHold": hold, "thawed": thawed,
            "responses": responses, "acceptedCount": responses.iter().filter(|r| r["acceptance"] == "ACCEPTED").count(),
            "refusedCount": responses.iter().filter(|r| r["acceptance"] != "ACCEPTED").count(),
            "storeWhenAllAccepted": accepted_store, "register": registered["ok"], "released": released,
            "claimantPid": cpid, "loadedCount": loaded_ids.len(), "maxInFlight": max_in_flight,
            "applied": applied, "storeAtEnd": end_store,
        }),
    )
}

/// B: storage failures injected into the real intent store.
fn storage_matrix(
    ctx: &Ctx,
    run_dir: &Run,
    log: &mut Log,
    cases: &mut Vec<Value>,
    since: i64,
    tag: &str,
) -> Result<(), String> {
    use StorageFaultStep::Write;
    use StorageFaultStore::{Backlog, Record};

    // W1: B1 no false acceptance, B2 replacement failure, B3 retire after
    // commit, then B2's retry converges.
    let (supervised, pre) = precondition(ctx, log, "B-W1-start")?;
    let (w, cold) = cold_start(
        ctx,
        log,
        &supervised,
        "unregister",
        &format!("c02d-b0-{tag}"),
    )?;
    let pid = w.pid;
    let (ui, hold) = freeze_ui(ctx);
    let x = raise(ctx, &format!("c02d-b1-{tag}"))?;
    let by = |event: &'static str, id: String| {
        move |l: &Value| l["pid"] == pid && l["event"] == event && mentions(l, &id)
    };

    let r1 = fresh(&x);
    let armed1 = [
        storage_fault(ctx, Record, Write, 1),
        storage_fault(ctx, Backlog, Write, 1),
    ];
    let sent1 = inject(ctx, &r1);
    let record_failed1 = log.wait(
        "RESPONSE_RECORD_FAILED",
        &by("RESPONSE_RECORD_FAILED", r1.1.clone()),
        10,
    );
    let not_accepted1 = log.wait(
        "NOTIFICATION_RESPONSE_NOT_ACCEPTED",
        &by("NOTIFICATION_RESPONSE_NOT_ACCEPTED", r1.1.clone()),
        10,
    );
    let store1 = durable(ctx);

    let r2 = fresh(&x);
    let armed2 = storage_fault(ctx, Backlog, Write, 1);
    let sent2 = inject(ctx, &r2);
    let recorded2 = log.wait(
        "RESPONSE_RECORDED",
        &by("RESPONSE_RECORDED", r2.1.clone()),
        10,
    );
    let not_promoted2 = log.wait(
        "INTENT_NOT_ACCEPTED",
        &by("INTENT_NOT_ACCEPTED", r2.1.clone()),
        10,
    );
    let store2 = durable(ctx);

    let r3 = fresh(&x);
    let armed3 = arm(ctx, QualificationFault::HoldNextBacklogCommit);
    let sent3 = inject(ctx, &r3);
    let held3 = log.wait(
        "HANDOFF_BARRIER_REACHED",
        &|l| l["pid"] == pid && l["point"] == "BEFORE_BACKLOG_COMMIT",
        10,
    );
    let store3_held = durable(ctx);
    let release3 = request(ctx, ControlRequestBody::QualifyReleaseHandoffBarrier);
    let accepted3 = log.wait("INTENT_ACCEPTED", &by("INTENT_ACCEPTED", r3.1.clone()), 10);
    let retired3 = log.wait(
        "RESPONSE_RECORD_RETIRED",
        &by("RESPONSE_RECORD_RETIRED", r3.1.clone()),
        10,
    );
    let store3_after = durable(ctx);

    let thawed = thaw_ui(&ui);
    let hydrated = log.wait("VIEW_HYDRATED", &is_pid(pid), 60);
    let retried2 = log.wait("RESPONSE_RETRY", &is_pid(pid), 20);
    let accepted2 = log.wait("INTENT_ACCEPTED", &by("INTENT_ACCEPTED", r2.1.clone()), 20);
    let retired2 = log.wait(
        "RESPONSE_RECORD_RETIRED",
        &by("RESPONSE_RECORD_RETIRED", r2.1.clone()),
        20,
    );
    let applied = applied_once(
        ctx,
        &[r2.1.clone(), r3.1.clone()],
        since,
        Duration::from_secs(60),
    );
    log.drain();
    let r1_ever_accepted = log
        .seen
        .iter()
        .any(|l| l["event"] == "INTENT_ACCEPTED" && mentions(l, &r1.1));
    let r1_applied = applications(ctx, &r1.1, since).len();
    let store_end = durable(ctx);
    let checks = json!({
        "b1RecordWriteFailed": record_failed1.is_some(),
        "b1ReportedNotAccepted": not_accepted1.is_some() && !r1_ever_accepted && r1_applied == 0,
        "b1NothingDurableClaimed": !has_id(&store1["records"], &r1.1) && !has_id(&store1["pendingIntents"], &r1.1),
        "b2RecordedThenPromotionFailed": recorded2.is_some() && not_promoted2.as_ref().is_some_and(|l| l["owner"] == "RESPONSE_RECORD"),
        "b2RecordKeptBacklogUnchanged": has_id(&store2["records"], &r2.1) && !has_id(&store2["pendingIntents"], &r2.1),
        "b3HeldBeforeRename": held3.is_some() && has_id(&store3_held["records"], &r3.1) && !has_id(&store3_held["pendingIntents"], &r3.1) && store3_held["backlogTemporaries"].as_array().is_some_and(|t| !t.is_empty()),
        "b3RecordRetiredOnlyAfterCommit": accepted3.is_some() && retired3.is_some() && ts(&retired3) >= ts(&accepted3) && has_id(&store3_after["pendingIntents"], &r3.1) && !has_id(&store3_after["records"], &r3.1),
        "b2RetryConverged": retried2.is_some() && accepted2.is_some() && retired2.is_some() && !has_id(&store_end["records"], &r2.1),
        "hydrationHeld": held(&hold),
        "acceptedAppliedExactlyOnce": hydrated.is_some() && applied["allExactlyOnce"] == true,
    });
    write_traces(
        run_dir,
        log,
        &[
            (r1.1.clone(), Some("RECORD WRITE + BACKLOG WRITE")),
            (r2.1.clone(), Some("BACKLOG WRITE")),
            (r3.1.clone(), Some("HOLD BEFORE BACKLOG RENAME")),
        ],
    )?;
    record(
        run_dir,
        cases,
        "B1-B3-acceptance-replacement-ordering",
        all_true(&checks),
        json!({
            "checks": checks, "precondition": pre, "coldStart": cold, "writerPid": pid, "hydrationHold": hold, "thawed": thawed,
            "b1": { "requestId": r1.1, "armed": armed1, "sent": sent1, "recordFailed": record_failed1, "notAccepted": not_accepted1, "store": store1, "everAccepted": r1_ever_accepted, "applications": r1_applied },
            "b2": { "requestId": r2.1, "armed": armed2, "sent": sent2, "recorded": recorded2, "promotionFailed": not_promoted2, "store": store2, "retry": retried2, "acceptedOnRetry": accepted2, "retired": retired2 },
            "b3": { "requestId": r3.1, "armed": armed3, "sent": sent3, "held": held3, "storeWhileHeld": store3_held, "release": release3, "accepted": accepted3, "retired": retired3, "storeAfter": store3_after },
            "applied": applied, "storeAtEnd": store_end,
        }),
    )?;

    // W2 (B5): the writer exits while a record still owns a response whose
    // promotion failed; the claimant recovers it.
    let (supervised, pre) = precondition(ctx, log, "B5-start")?;
    let (w, cold) = cold_start(
        ctx,
        log,
        &supervised,
        "unregister",
        &format!("c02d-b5-{tag}"),
    )?;
    let pid = w.pid;
    let (ui, hold) = freeze_ui(ctx);
    let x = raise(ctx, &format!("c02d-b6-{tag}"))?;
    let r5 = fresh(&x);
    let armed5 = storage_fault(ctx, Backlog, Write, 1);
    let sent5 = inject(ctx, &r5);
    let not_promoted5 = log.wait(
        "INTENT_NOT_ACCEPTED",
        &|l| l["pid"] == pid && mentions(l, &r5.1),
        10,
    );
    let store5 = durable(ctx);
    let registered = service::bootstrap(&ctx.id, "register");
    let released5 = log.wait("WRITER_RELEASED", &is_pid(pid), 30);
    let exited5 = procs::wait_exit(&w, Duration::from_secs(15));
    let (cpid, loaded5) = claimant_after(ctx, log);
    let recovered5 = log.wait(
        "NOTIFICATION_RESPONSE",
        &|l| l["pid"] == cpid && l["recovered"] == true && mentions(l, &r5.1),
        15,
    );
    let accepted5 = log.wait(
        "INTENT_ACCEPTED",
        &|l| l["pid"] == cpid && mentions(l, &r5.1),
        15,
    );
    let thawed5 = thaw_ui(&ui);
    let applied5 = applied_once(
        ctx,
        std::slice::from_ref(&r5.1),
        since,
        Duration::from_secs(90),
    );
    let store5_end = durable(ctx);
    let checks5 = json!({
        "promotionFailedRecordKept": not_promoted5.is_some() && has_id(&store5["records"], &r5.1) && !has_id(&store5["pendingIntents"], &r5.1),
        "exitedWithTheRecordAsOwner": released5.as_ref().is_some_and(|l| has_id(&l["records"], &r5.1) && has_id(&l["unpromoted"], &r5.1)) && exited5.is_some(),
        "claimantRecoveredAsInspector": loaded5.as_ref().is_some_and(|l| has_id(&l["records"], &r5.1)) && recovered5.as_ref().is_some_and(|l| l["plan"] == "INSPECTOR") && accepted5.is_some(),
        "appliedExactlyOnce": applied5["allExactlyOnce"] == true,
        "recordRetired": !has_id(&store5_end["records"], &r5.1),
        "hydrationHeld": held(&hold),
    });
    write_traces(
        run_dir,
        log,
        &[(r5.1.clone(), Some("BACKLOG WRITE before Release"))],
    )?;
    record(
        run_dir,
        cases,
        "B5-release-after-storage-failure",
        all_true(&checks5),
        json!({
            "checks": checks5, "precondition": pre, "coldStart": cold, "writerPid": pid, "hydrationHold": hold, "thawed": thawed5,
            "requestId": r5.1, "armed": armed5, "sent": sent5, "promotionFailed": not_promoted5, "store": store5,
            "register": registered["ok"], "released": released5, "exitedAfterMs": exited5, "claimantPid": cpid,
            "loaded": loaded5, "recovered": recovered5, "accepted": accepted5, "applied": applied5, "storeAtEnd": store5_end,
        }),
    )?;

    // W3 (B4): consumption cannot be recorded; Release cannot record it
    // either; the claimant delivers the stale intent and the shell does not
    // apply it again.
    let (supervised, pre) = precondition(ctx, log, "B4-start")?;
    let (w, cold) = cold_start(
        ctx,
        log,
        &supervised,
        "unregister",
        &format!("c02d-b4-{tag}"),
    )?;
    let pid = w.pid;
    let (ui, hold) = freeze_ui(ctx);
    let x = raise(ctx, &format!("c02d-b7-{tag}"))?;
    let r4 = fresh(&x);
    let sent4 = inject(ctx, &r4);
    let accepted4 = log.wait(
        "INTENT_ACCEPTED",
        &|l| l["pid"] == pid && mentions(l, &r4.1),
        10,
    );
    let armed4 = storage_fault(ctx, Backlog, Write, 2);
    let mut desktop = ctx.app().desktop_log();
    let thawed4 = thaw_ui(&ui);
    let not_recorded4 = log.wait(
        "CONSUMPTION_NOT_RECORDED",
        &|l| l["pid"] == pid && mentions(l, &r4.1),
        60,
    );
    let store4 = durable(ctx);
    threadspace_harness::pause_ms(3000);
    log.drain();
    let delivered_by_old = log
        .seen
        .iter()
        .filter(|l| l["pid"] == pid && l["event"] == "INTENT_DELIVERED" && mentions(l, &r4.1))
        .count();
    let registered = service::bootstrap(&ctx.id, "register");
    let stale4 = log.wait("RELEASE_BACKLOG_STALE", &is_pid(pid), 30);
    let released4 = log.wait("WRITER_RELEASED", &is_pid(pid), 30);
    let exited4 = procs::wait_exit(&w, Duration::from_secs(15));
    let (cpid, loaded4) = claimant_after(ctx, log);
    let consumed4 = log.wait(
        "INTENT_CONSUMED",
        &|l| l["pid"] == cpid && mentions(l, &r4.1),
        90,
    );
    let mut lines = Vec::new();
    let skipped = desktop.wait_for(
        "INTENT_ALREADY_APPLIED",
        |l| l["detail"]["intentId"].as_str() == Some(r4.1.as_str()),
        Duration::from_secs(10),
        &mut lines,
    );
    threadspace_harness::pause_ms(3000);
    let applied4 = applications(ctx, &r4.1, since).len();
    let store4_end = durable(ctx);
    let checks4 = json!({
        "acceptedThenDelivered": accepted4.is_some() && delivered_by_old == 1,
        "consumptionFailureReported": not_recorded4.is_some(),
        "durableStateStillListsIt": has_id(&store4["pendingIntents"], &r4.1),
        "notDeliveredAgainByTheSameWriter": delivered_by_old == 1,
        "releaseRetriedAndReportedStale": stale4.is_some() && released4.as_ref().is_some_and(|l| l["backlogStale"] == true) && exited4.is_some(),
        "claimantLoadedTheStaleIntent": loaded4.as_ref().is_some_and(|l| has_id(&l["intents"], &r4.1)),
        "shellDidNotApplyAgain": skipped.is_some() && applied4 == 1,
        "removalConvergedOnTheClaimant": consumed4.is_some() && !has_id(&store4_end["pendingIntents"], &r4.1),
        "hydrationHeld": held(&hold),
    });
    write_traces(
        run_dir,
        log,
        &[(
            r4.1.clone(),
            Some("BACKLOG WRITE x2 at consumption and Release"),
        )],
    )?;
    record(
        run_dir,
        cases,
        "B4-consumption-removal-failure",
        all_true(&checks4),
        json!({
            "checks": checks4, "precondition": pre, "coldStart": cold, "writerPid": pid, "hydrationHold": hold, "thawed": thawed4,
            "requestId": r4.1, "sent": sent4, "accepted": accepted4, "armed": armed4, "consumptionNotRecorded": not_recorded4,
            "storeAfterFailure": store4, "deliveriesByFailingWriter": delivered_by_old, "register": registered["ok"],
            "releaseStale": stale4, "released": released4, "claimantPid": cpid, "loaded": loaded4, "consumedOnClaimant": consumed4,
            "shellSkipped": skipped, "applications": applied4, "storeAtEnd": store4_end,
            "applied": { "lost": u64::from(applied4 == 0), "duplicates": applied4.saturating_sub(1) },
        }),
    )
}

/// C: a notification Return in flight when the companion is killed or
/// observation is stopped, and a normal Return.
#[allow(clippy::too_many_arguments)]
fn return_ownership(
    ctx: &Ctx,
    run_dir: &Run,
    log: &mut Log,
    cases: &mut Vec<Value>,
    since: i64,
    tag: &str,
    root: &std::path::Path,
    windows: &mut Vec<ClaudeTab>,
    spares: &mut Vec<Tab>,
) -> Result<(), String> {
    let (_, pre) = precondition(ctx, log, "C-start")?;
    let a = spawn_claude(ctx, root.join("a"))?;
    let spare = locked(ctx, "c02d spare", || Tab::open_inert(root.join("spare")))?;
    locked(ctx, "c02d frames", || {
        a.tab.set_bounds(137, 151, 1001, 707);
        spare.set_bounds(211, 233, 1011, 733);
    });
    let session = a.session_id.clone();
    let spare_id = spare.window_id;
    windows.push(a);
    spares.push(spare);
    let to_spare = || {
        raise_window(spare_id);
        threadspace_harness::pause_ms(600);
    };
    let started_for = |log: &Log, pid: i32, id: &str| {
        log.seen
            .iter()
            .filter(|l| {
                l["pid"] == pid && l["event"] == "NOTIFICATION_RETURN_STARTED" && mentions(l, id)
            })
            .count()
    };

    for how in ["crash", "stop"] {
        let owner = ctx.companion().incarnation().ok_or("no companion")?;
        let x = raise_for(ctx, &format!("c02d-c-{how}-{tag}"), &session)?;
        let (before, armed, sent) = locked(ctx, "c02d held return", || {
            to_spare();
            let before = window_selection();
            let armed = arm(ctx, QualificationFault::HoldNextRouteBeforeFocus);
            let sent = inject(ctx, &x);
            (before, armed, sent)
        });
        let reached = log.wait(
            "ROUTE_BARRIER_REACHED",
            &|l| l["pid"] == owner.pid && l["point"] == "BEFORE_FOCUS",
            20,
        );
        let planned = log.wait(
            "NOTIFICATION_RESPONSE",
            &|l| l["pid"] == owner.pid && mentions(l, &x.1),
            5,
        );
        let store_held = durable(ctx);
        let interrupted = if how == "crash" {
            match kill_companion(ctx) {
                Ok((old, fresh, waited)) => {
                    json!({ "killed": old, "relaunched": fresh, "relaunchMs": waited })
                }
                Err(error) => json!({ "error": error }),
            }
        } else {
            let stopped = service::bootstrap(&ctx.id, "stop");
            let exited = procs::wait_exit(&owner, Duration::from_secs(20));
            let while_stopped =
                json!({ "companions": ctx.companion().processes(), "store": durable(ctx) });
            let enabled = service::bootstrap(&ctx.id, "enable");
            json!({ "stop": stopped["ok"], "ownerExitedAfterMs": exited, "whileStopped": while_stopped, "enable": enabled["ok"] })
        };
        let (cpid, loaded) = claimant_after(ctx, log);
        let recovered = log.wait(
            "NOTIFICATION_RESPONSE",
            &|l| l["pid"] == cpid && l["recovered"] == true && mentions(l, &x.1),
            15,
        );
        let accepted = log.wait(
            "INTENT_ACCEPTED",
            &|l| l["pid"] == cpid && mentions(l, &x.1),
            15,
        );
        let applied = applied_once(
            ctx,
            std::slice::from_ref(&x.1),
            since,
            Duration::from_secs(90),
        );
        log.drain();
        let replays = started_for(log, cpid, &x.1);
        let after = window_selection();
        let attention = attention_state(ctx, &x.0);
        let store_end = durable(ctx);
        let mut checks = json!({
            "returnPlannedAndHeld": planned.as_ref().is_some_and(|l| l["plan"] == "RETURN" && l["recorded"] == true) && reached.is_some(),
            "durableFallbackWhileInFlight": has_id(&store_held["records"], &x.1) && !has_id(&store_held["pendingIntents"], &x.1),
            "nextWriterFoundTheRecord": loaded.as_ref().is_some_and(|l| has_id(&l["records"], &x.1)),
            "recoveredAsInspector": recovered.as_ref().is_some_and(|l| l["plan"] == "INSPECTOR") && accepted.is_some(),
            "noFocusReplayed": replays == 0 && after["front"] == spare_id,
            "inspectorDeliveredOnce": applied["allExactlyOnce"] == true,
            "recordRetired": !has_id(&store_end["records"], &x.1),
            "attentionUnacknowledged": attention["acknowledgedAtMs"].is_null(),
        });
        if how == "stop" {
            checks["recordSurvivedWhileStopped"] = json!(
                interrupted["whileStopped"]["companions"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                    && has_id(&interrupted["whileStopped"]["store"]["records"], &x.1)
            );
        }
        let pass = all_true(&checks);
        write_traces(
            run_dir,
            log,
            &[(
                x.1.clone(),
                Some(if how == "crash" {
                    "SIGKILL during Return"
                } else {
                    "Stop Observation during Return"
                }),
            )],
        )?;
        record(
            run_dir,
            cases,
            &format!("C-return-in-flight-{how}"),
            pass,
            json!({
                "checks": checks, "requestId": x.1, "attentionId": x.0, "ownerPid": owner.pid, "armed": armed, "sent": sent,
                "selectionBefore": before, "planned": planned, "barrier": reached, "storeWhileHeld": store_held,
                "interrupted": interrupted, "claimantPid": cpid, "loaded": loaded, "recovered": recovered, "accepted": accepted,
                "returnStartsByNextWriter": replays, "selectionAfter": after, "applied": applied, "attention": attention, "storeAtEnd": store_end,
            }),
        )?;
    }

    // Normal Return: no stale fallback, no duplicate after a restart.
    let owner = ctx.companion().incarnation().ok_or("no companion")?;
    let x = raise_for(ctx, &format!("c02d-c-normal-{tag}"), &session)?;
    let sent = locked(ctx, "c02d normal return", || {
        to_spare();
        inject(ctx, &x)
    });
    let returned = log.wait(
        "NOTIFICATION_RETURN",
        &|l| l["pid"] == owner.pid && mentions(l, &x.1),
        20,
    );
    let accepted = log.wait(
        "INTENT_ACCEPTED",
        &|l| l["pid"] == owner.pid && mentions(l, &x.1),
        10,
    );
    let retired = log.wait(
        "RESPONSE_RECORD_RETIRED",
        &|l| l["pid"] == owner.pid && mentions(l, &x.1),
        10,
    );
    let consumed = log.wait(
        "INTENT_CONSUMED",
        &|l| l["pid"] == owner.pid && mentions(l, &x.1),
        60,
    );
    let store_done = durable(ctx);
    let restart = match kill_companion(ctx) {
        Ok((old, fresh, waited)) => {
            json!({ "killed": old, "relaunched": fresh, "relaunchMs": waited })
        }
        Err(error) => json!({ "error": error }),
    };
    let (cpid, loaded) = claimant_after(ctx, log);
    threadspace_harness::pause_ms(5000);
    let applied = applications(ctx, &x.1, since).len();
    let checks = json!({
        "returnExact": returned.as_ref().is_some_and(|l| l["exact"] == true),
        "resultCommittedThenRecordRetired": accepted.is_some() && retired.is_some() && ts(&retired) >= ts(&accepted),
        "consumed": consumed.is_some(),
        "noStaleFallback": !has_id(&store_done["records"], &x.1) && !has_id(&store_done["pendingIntents"], &x.1),
        "restartRecoversNothing": loaded.as_ref().is_some_and(|l| !has_id(&l["records"], &x.1) && !has_id(&l["intents"], &x.1)),
        "appliedOnceNoDuplicate": applied == 1,
    });
    write_traces(run_dir, log, &[(x.1.clone(), None)])?;
    record(
        run_dir,
        cases,
        "C-normal-return",
        all_true(&checks),
        json!({
            "checks": checks, "precondition": pre, "requestId": x.1, "sent": sent, "returned": returned, "accepted": accepted,
            "retired": retired, "consumed": consumed, "storeAfterConsumption": store_done, "restart": restart,
            "claimantPid": cpid, "loaded": loaded, "applications": applied,
            "applied": { "lost": u64::from(applied == 0), "duplicates": applied.saturating_sub(1) },
        }),
    )
}

pub fn c11_receipt(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/c11-notification-receipt-deadline",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let mut log = Log {
        cursor: ctx.companion().log(),
        seen: Vec::new(),
    };
    let mut cases: Vec<Value> = Vec::new();
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
    let root = PathBuf::from(format!("/private/tmp/ts-m0c-c11r-{tag}"));
    let (_, pre) = precondition(ctx, &mut log, "Q-start")?;
    let a = spawn_claude(ctx, root.join("a"))?;
    let spare = locked(ctx, "c11r spare", || Tab::open_inert(root.join("spare")))?;
    locked(ctx, "c11r frames", || {
        a.tab.set_bounds(137, 151, 1001, 707);
        spare.set_bounds(211, 233, 1011, 733);
    });
    let owner = ctx.companion().incarnation().ok_or("no companion")?;
    let pid = owner.pid;

    let outcome = (|| -> Result<(), String> {
        // (case, when the second is received after the first, how long the
        // second waits). The first is always released past its own
        // deadline, so it ends at once without focus.
        for (case, second_after_first_ms, wait_ms) in [
            ("Q1-queued-past-deadline", None, 2300_i64),
            ("Q2-queued-part-of-budget", Some(1100_i64), 1200),
        ] {
            let first = raise_for(ctx, &format!("c11r-{case}-a-{tag}"), &a.session_id)?;
            let second = raise_for(ctx, &format!("c11r-{case}-b-{tag}"), &a.session_id)?;
            let detail = locked(ctx, "c11r queued returns", || {
                raise_window(spare.window_id);
                threadspace_harness::pause_ms(600);
                let before = window_selection();
                let armed = arm(ctx, QualificationFault::HoldNextRouteBeforeFocus);
                let sent_first = inject(ctx, &first);
                let reached = log.wait(
                    "ROUTE_BARRIER_REACHED",
                    &|l| l["pid"] == pid && l["point"] == "BEFORE_FOCUS",
                    20,
                );
                let first_receipt = log.wait(
                    "QUALIFY_NOTIFICATION_RESPONSE",
                    &|l| l["pid"] == pid && l["requestId"] == first.1.as_str(),
                    10,
                );
                let first_received_ms = first_receipt
                    .as_ref()
                    .and_then(|l| l["receivedAtMs"].as_i64())
                    .unwrap_or(0);
                if let Some(after) = second_after_first_ms {
                    let wait = first_received_ms + after - threadspace_harness::now_ms();
                    if wait > 0 {
                        std::thread::sleep(Duration::from_millis(wait as u64));
                    }
                }
                let sent_second = inject(ctx, &second);
                let receipt = log.wait(
                    "QUALIFY_NOTIFICATION_RESPONSE",
                    &|l| l["pid"] == pid && l["requestId"] == second.1.as_str(),
                    10,
                );
                let received_ms = receipt
                    .as_ref()
                    .and_then(|l| l["receivedAtMs"].as_i64())
                    .unwrap_or(0);
                threadspace_harness::pause_ms(300);
                let store_queued = durable(ctx);
                let release_at = (received_ms + wait_ms).max(first_received_ms + 2300);
                let wait = release_at - threadspace_harness::now_ms();
                if wait > 0 {
                    std::thread::sleep(Duration::from_millis(wait as u64));
                }
                let release_ms = threadspace_harness::now_ms();
                let release = request(ctx, ControlRequestBody::QualifyReleaseRouteBarrier);
                let first_return = log.wait(
                    "NOTIFICATION_RETURN",
                    &|l| l["pid"] == pid && mentions(l, &first.1),
                    20,
                );
                let started = log.wait(
                    "NOTIFICATION_RETURN_STARTED",
                    &|l| l["pid"] == pid && mentions(l, &second.1),
                    20,
                );
                let second_return = log.wait(
                    "NOTIFICATION_RETURN",
                    &|l| l["pid"] == pid && mentions(l, &second.1),
                    20,
                );
                let accepted = log.wait(
                    "INTENT_ACCEPTED",
                    &|l| l["pid"] == pid && mentions(l, &second.1),
                    10,
                );
                let retired = log.wait(
                    "RESPONSE_RECORD_RETIRED",
                    &|l| l["pid"] == pid && mentions(l, &second.1),
                    10,
                );
                threadspace_harness::pause_ms(500);
                let after = window_selection();
                json!({
                    "selectionBefore": before, "armed": armed, "sentFirst": sent_first, "barrier": reached,
                    "firstReceivedAtMs": first_received_ms, "firstDeadlineMs": first_received_ms + 2000,
                    "sentSecond": sent_second, "secondReceivedAtMs": received_ms, "secondDeadlineMs": received_ms + 2000,
                    "storeWhileQueued": store_queued, "releaseSentMs": release_ms, "release": release,
                    "firstReturn": first_return, "secondStarted": started, "secondReturn": second_return,
                    "secondAccepted": accepted, "secondRecordRetired": retired, "selectionAfter": after,
                })
            });
            let applied = applied_once(
                ctx,
                std::slice::from_ref(&second.1),
                since,
                Duration::from_secs(60),
            );
            let attention = attention_state(ctx, &second.0);
            let started = &detail["secondStarted"];
            let route = &detail["secondReturn"]["route"];
            let queued = started["queuedMs"].as_i64().unwrap_or(0);
            let remaining = started["remainingMs"].as_i64().unwrap_or(i64::MAX);
            let latency = route["latencyMs"].as_i64().unwrap_or(0);
            let second_exact = detail["secondReturn"]["exact"] == true;
            let mut checks = json!({
                "secondQueuedBehindHeldFirst": detail["barrier"].is_object() && queued >= wait_ms - 400,
                "durableWhileQueued": has_id(&detail["storeWhileQueued"]["records"], &second.1),
                "noFreshBudget": remaining <= (2000 - queued).max(0) + 50,
                "noLateExact": !second_exact || queued + latency < 2000,
                "resolvedToOneInspectorIntent": detail["secondAccepted"].is_object() && detail["secondRecordRetired"].is_object() && applied["allExactlyOnce"] == true,
                "attentionUnacknowledged": attention["acknowledgedAtMs"].is_null(),
                "firstTimedOutWithoutFocus": detail["firstReturn"]["route"]["reasonCode"] == "TIMEOUT" && detail["firstReturn"]["route"]["focusPerformed"] == false,
            });
            if case.starts_with("Q1") {
                checks["expiredBeforeDequeue"] = json!(remaining == 0);
                checks["timedOutWithoutFocus"] = json!(
                    !second_exact
                        && route["reasonCode"] == "TIMEOUT"
                        && route["focusPerformed"] == false
                        && route["sessionVerification"] != "CURRENT_NATIVE_REVALIDATED"
                );
                checks["noFocusAtAll"] =
                    json!(detail["selectionAfter"]["front"] == spare.window_id);
            } else {
                checks["partialBudgetAtDequeue"] = json!(remaining > 0 && remaining < 1000);
            }
            let pass = all_true(&checks);
            record(
                &run_dir,
                &mut cases,
                case,
                pass,
                json!({
                    "checks": checks, "queuedMs": queued, "remainingMsAtDequeue": remaining, "secondRouteLatencyMs": latency,
                    "receiptToResultMs": queued + latency, "secondExact": second_exact, "observed": detail,
                    "applied": applied, "attention": attention, "firstRequestId": first.1, "secondRequestId": second.1,
                }),
            )?;
            write_traces(
                &run_dir,
                &mut log,
                &[
                    (first.1.clone(), Some("route held before focus")),
                    (second.1.clone(), None),
                ],
            )?;
        }

        // Direct control route: receipt before worker dispatch, unchanged.
        let direct = locked(ctx, "c11r direct", || {
            raise_window(spare.window_id);
            threadspace_harness::pause_ms(600);
            route_full(ctx, &a.session_id)
        });
        let result = &direct["result"];
        let checks = json!({
            "exact": exact(result) && result["reasonCode"] == "OK",
            "withinBudget": result["latencyMs"].as_i64().is_some_and(|l| l < 2000),
        });
        record(
            &run_dir,
            &mut cases,
            "direct-control-route",
            all_true(&checks),
            json!({ "checks": checks, "latencyMs": result["latencyMs"], "route": direct }),
        )
    })();
    let outcome_error = outcome.err();
    locked(ctx, "c11r cleanup", || {
        let _ = run_dir.append("cleanup.jsonl", &a.tab.close());
        let _ = run_dir.append("cleanup.jsonl", &spare.close());
    });
    let _ = crate::cleanup::resolve_qualification(ctx, "c11 receipt qualification cleanup");
    let _ = crate::cleanup::clear_notifications(ctx);
    log.drain();
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
        "issue": "C-11",
        "gate": "G08",
        "pass": outcome_error.is_none() && cases.iter().all(|c| c["pass"] == true),
        "error": outcome_error,
        "precondition": pre["checkpoint"]["diagnostics"],
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "companionSha256": environment["companionSha256"],
        "executableSha256": environment["executableSha256"],
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
