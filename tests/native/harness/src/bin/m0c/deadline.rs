//! C-11 remediation (G08; SPEC §13.2, §20.2): one monotonic deadline covers a
//! whole Return attempt, and a result reached after it is never exact. Native
//! cases with a real Claude session in a disposable, harness-owned Terminal
//! window and qualification route holds that report the remaining budget:
//!
//! - ordinary: an exact route inside the budget, with its latency;
//! - focus-held: held before focus until the deadline passed; no focus;
//! - focus-in-flight: released just before the deadline, so the focus
//!   script runs into it; whatever macOS did is recorded, nothing is exact;
//! - decision-held: focus, readback and post-focus revalidation all prove the
//!   target, the final decision comes after the deadline;
//! - fullscreen: an exact Return into the target's own fullscreen Space;
//! - settle: expiry while macOS is still showing that Space, inside the focus
//!   script and around the readback, plus the bundled script run directly
//!   with no budget and with budget, showing its settle wait follows the
//!   budget it is given;
//! - attention: a notification Return that times out is not acknowledged; a
//!   later exact Return and the owner's acknowledgement behave normally.
//!
//! Holds are qualification timing only; ordinary and fullscreen latencies
//! are measured without one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody, QualificationFault};
use threadspace_contracts::route::RouteRequest;
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::terminal::{self, Tab};

use crate::ctx::Ctx;
use crate::supervision::attention_state;
use crate::terminal_gates::{
    ClaudeTab, locked, poll, raise_window, spawn_claude, terminal_pid, window_selection,
};

const BUDGET_MS: i64 = 2000;

/// One Return through the companion with its full result, and an independent
/// readback of Terminal's front window and selected tab right after.
pub(crate) fn route_full(ctx: &Ctx, session_id: &str) -> Value {
    let request = RouteRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_owned(),
        chosen_binding_id: None,
        expected_binding_revision: None,
    };
    let request_id = request.request_id.clone();
    let sent_ms = threadspace_harness::now_ms();
    let started = Instant::now();
    let outcome = ctx.companion().request(
        ControlRequestBody::ReturnToSession { route: request },
        Duration::from_secs(30),
    );
    let elapsed = started.elapsed().as_millis() as u64;
    let after = window_selection();
    match outcome {
        Ok(ControlResponseBody::Routed { result }) => json!({
            "requestId": request_id, "sentMs": sent_ms, "harnessElapsedMs": elapsed,
            "result": serde_json::to_value(&*result).unwrap_or(Value::Null),
            "selectionAfter": after,
        }),
        Ok(other) => json!({ "requestId": request_id, "error": format!("unexpected {other:?}") }),
        Err(error) => json!({ "requestId": request_id, "refused": error, "selectionAfter": after }),
    }
}

pub(crate) fn exact(result: &Value) -> bool {
    result["surfaceResult"] == "EXACT_NATIVE_SURFACE"
        && result["sessionVerification"] == "CURRENT_NATIVE_REVALIDATED"
}

fn timed_out(result: &Value) -> bool {
    result["reasonCode"] == "TIMEOUT"
        && result["surfaceResult"] == "UNAVAILABLE"
        && result["sessionVerification"] == "NATIVE_BOUND_LAST_KNOWN"
        && result["inputReadiness"] == "UNKNOWN"
        && result["validatedBindingRevision"].is_null()
}

/// Windows other than the harness's own whose selected tab changed.
fn unrelated_changes(before: &Value, after: &Value, owned: &[i64]) -> Vec<String> {
    before["selected"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(id, _)| !owned.iter().any(|o| o.to_string() == **id))
                .filter(|(id, tty)| after["selected"][id.as_str()] != **tty)
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// When to release a held route, from the barrier's report of the budget
/// left when it was reached.
#[derive(Clone, Copy)]
enum Release {
    /// This long after the attempt's deadline.
    AfterDeadline(i64),
    /// This long before it.
    BeforeDeadline(i64),
}

/// Arms one route hold, starts `start` (which leads to one Return), waits
/// for the route to reach the barrier, releases it as `release` says and
/// collects the companion's barrier and result lines.
fn held(
    ctx: &Ctx,
    fault: QualificationFault,
    point: &str,
    release: Release,
    start: impl FnOnce() -> Value + Send,
) -> Value {
    let mut log = ctx.companion().log();
    let mut seen = Vec::new();
    let armed = ctx.companion().request(
        ControlRequestBody::QualifyArmFault { fault },
        Duration::from_secs(5),
    );
    std::thread::scope(|scope| {
        let started = scope.spawn(start);
        let reached = log.wait_for(
            "ROUTE_BARRIER_REACHED",
            |l| l["point"] == point,
            Duration::from_secs(20),
            &mut seen,
        );
        let deadline_ms = reached
            .as_ref()
            .and_then(|r| Some(r["reachedAtMs"].as_i64()? + r["remainingMs"].as_i64()?));
        let target_ms = deadline_ms.map(|d| match release {
            Release::AfterDeadline(ms) => d + ms,
            Release::BeforeDeadline(ms) => d - ms,
        });
        if let Some(target) = target_ms {
            let wait = target - threadspace_harness::now_ms();
            if wait > 0 {
                std::thread::sleep(Duration::from_millis(wait as u64));
            }
        }
        let release_sent_ms = threadspace_harness::now_ms();
        let released_reply = ctx.companion().request(
            ControlRequestBody::QualifyReleaseRouteBarrier,
            Duration::from_secs(5),
        );
        let outcome = started
            .join()
            .unwrap_or_else(|_| json!({ "error": "route thread panicked" }));
        let released = log.wait_for(
            "ROUTE_BARRIER_RELEASED",
            |l| l["point"] == point,
            Duration::from_secs(10),
            &mut seen,
        );
        let result_line = log.wait_for(
            "ROUTE_RESULT",
            |l| {
                reached.as_ref().is_some()
                    && l["ts"].as_i64()
                        >= released.as_ref().and_then(|r| r["releasedAtMs"].as_i64())
            },
            Duration::from_secs(15),
            &mut seen,
        );
        json!({
            "armed": format!("{armed:?}"),
            "barrierReached": reached, "deadlineMs": deadline_ms,
            "releaseSentMs": release_sent_ms, "release": format!("{released_reply:?}"),
            "barrierReleased": released, "routeResultLogged": result_line,
            "outcome": outcome,
        })
    })
}

/// Reached inside the budget, released on the side of the deadline asked
/// for, and the result logged after the release.
fn ordered(detail: &Value, released_after_deadline: bool) -> bool {
    let reached = detail["barrierReached"]["reachedAtMs"].as_i64();
    let remaining_at_reach = detail["barrierReached"]["remainingMs"].as_i64();
    let deadline = detail["deadlineMs"].as_i64();
    let released = detail["barrierReleased"]["releasedAtMs"].as_i64();
    let remaining_at_release = detail["barrierReleased"]["remainingMs"].as_i64();
    let result = detail["routeResultLogged"]["ts"].as_i64();
    match (
        reached,
        remaining_at_reach,
        deadline,
        released,
        remaining_at_release,
        result,
    ) {
        (Some(r), Some(left), Some(d), Some(rel), Some(left_at_release), Some(res)) => {
            left > 0
                && r < d
                && rel <= res
                && if released_after_deadline {
                    rel > d && left_at_release == 0
                } else {
                    rel < d && left_at_release > 0
                }
        }
        _ => false,
    }
}

fn all_true(checks: &Value) -> bool {
    checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true))
}

/// The bundled focus script run directly, with `budget_ms` as its budget.
fn script_direct(ctx: &Ctx, tty: &str, budget_ms: u64) -> Value {
    let script = ctx
        .id
        .companion_executable
        .parent()
        .and_then(|macos| macos.parent())
        .map(|contents| contents.join("Resources/terminal-focus.applescript"))
        .unwrap_or_default();
    let started = Instant::now();
    let out = threadspace_harness::run::run(
        "/usr/bin/osascript",
        &[&script.display().to_string(), tty, &budget_ms.to_string()],
        Duration::from_secs(10),
    );
    let fields: Vec<String> = out.stdout.trim().split('\t').map(str::to_owned).collect();
    json!({
        "budgetMs": budget_ms, "elapsedMs": started.elapsed().as_millis() as u64,
        "ok": out.ok, "fields": fields, "stderr": out.stderr.trim(),
    })
}

pub fn c11(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/c11-route-deadline",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let root = PathBuf::from(format!(
        "/private/tmp/ts-m0c-c11-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let mut cases: Vec<Value> = Vec::new();
    let record =
        |cases: &mut Vec<Value>, name: &str, detail: Value, pass: bool| -> Result<(), String> {
            let entry = json!({ "case": name, "pass": pass, "detail": detail });
            run_dir
                .append("cases.jsonl", &entry)
                .map_err(|e| e.to_string())?;
            cases.push(entry);
            Ok(())
        };
    let gate = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    run_dir
        .write_json("idle-gate.json", &json!(gate))
        .map_err(|e| e.to_string())?;
    let environment = ctx.environment();
    run_dir
        .write_json("environment.json", &environment)
        .map_err(|e| e.to_string())?;
    let terminal_before = terminal::terminal_process();
    let ui_before = ctx.app().processes();

    let a: ClaudeTab = spawn_claude(ctx, root.join("a"))?;
    let spare = locked(ctx, "c11 open spare window", || {
        Tab::open_inert(root.join("spare"))
    })?;
    let tpid = terminal_pid()?;
    let owned = [a.tab.window_id, spare.window_id];
    locked(ctx, "c11 frames", || {
        a.tab.set_bounds(137, 151, 1001, 707);
        spare.set_bounds(211, 233, 1011, 733);
    });
    let window = |id: i64| {
        ctx.native
            .json(&["window-state", &tpid.to_string(), &id.to_string()])
    };
    let front_is = |selection: &Value, id: i64| selection["front"] == id;
    let to_spare = || {
        raise_window(spare.window_id);
        threadspace_harness::pause_ms(600);
    };

    // D. Ordinary exact route, no hold.
    let ordinary = locked(ctx, "c11 ordinary", || {
        to_spare();
        let before = window_selection();
        let routed = route_full(ctx, &a.session_id);
        json!({ "selectionBefore": before, "route": routed })
    });
    let r = &ordinary["route"]["result"];
    let checks = json!({
        "exact": exact(r) && r["reasonCode"] == "OK",
        "withinBudget": r["latencyMs"].as_i64().is_some_and(|l| l < BUDGET_MS),
        "currentSessionVerified": r["evidence"]["postFocusLookup"]["error"].is_null() && r["evidence"]["postFocusLookup"].is_object(),
        "readbackIsTarget": r["evidence"]["focus"]["readbackTty"] == a.tab.tty.as_str() && r["evidence"]["focus"]["frontWindowId"] == a.tab.window_id,
        "independentFrontIsTarget": front_is(&ordinary["route"]["selectionAfter"], a.tab.window_id)
            && ordinary["route"]["selectionAfter"]["selected"][a.tab.window_id.to_string()] == a.tab.tty.as_str(),
    });
    record(
        &mut cases,
        "D-ordinary-exact-route",
        json!({ "checks": checks, "latencyMs": r["latencyMs"], "phases": r["evidence"]["phases"], "observed": ordinary }),
        all_true(&checks),
    )?;

    // A1. Held at the focus barrier until the deadline passed: no focus.
    let focus_held = locked(ctx, "c11 focus held", || {
        to_spare();
        let before = window_selection();
        let detail = held(
            ctx,
            QualificationFault::HoldNextRouteBeforeFocus,
            "BEFORE_FOCUS",
            Release::AfterDeadline(300),
            || route_full(ctx, &a.session_id),
        );
        threadspace_harness::pause_ms(1500);
        json!({ "selectionBefore": before, "held": detail, "selectionLater": window_selection() })
    });
    let r = &focus_held["held"]["outcome"]["result"];
    let checks = json!({
        "orderedReachedBeforeDeadlineReleasedAfter": ordered(&focus_held["held"], true),
        "typedTimeout": timed_out(r),
        "noFocusIssued": r["focusPerformed"] == false && r["evidence"]["focus"].is_null(),
        "noSideEffect": front_is(&focus_held["selectionLater"], spare.window_id),
        "noUnrelatedSelectionChange": unrelated_changes(&focus_held["selectionBefore"], &focus_held["selectionLater"], &owned).is_empty(),
    });
    record(
        &mut cases,
        "A1-focus-held-past-deadline",
        json!({ "checks": checks, "observed": focus_held }),
        all_true(&checks),
    )?;

    // A2. Released 80 ms before the deadline: the focus script runs into it.
    let in_flight = locked(ctx, "c11 focus in flight", || {
        to_spare();
        let before = window_selection();
        let detail = held(
            ctx,
            QualificationFault::HoldNextRouteBeforeFocus,
            "BEFORE_FOCUS",
            Release::BeforeDeadline(80),
            || route_full(ctx, &a.session_id),
        );
        let (later, target_front, waited) = poll(Duration::from_secs(3), window_selection, |s| {
            s["front"] == a.tab.window_id
        });
        json!({ "selectionBefore": before, "held": detail, "selectionLater": later, "targetBecameFront": target_front, "targetFrontWaitMs": waited })
    });
    let r = &in_flight["held"]["outcome"]["result"];
    let started_with_budget = in_flight["held"]["barrierReleased"]["remainingMs"]
        .as_i64()
        .is_some_and(|ms| ms > 0);
    let checks = json!({
        "orderedReleasedInsideBudget": ordered(&in_flight["held"], false),
        "typedTimeout": timed_out(r),
        "notExact": !exact(r),
        "focusRecordedAsMayHaveActed": r["focusPerformed"] == started_with_budget,
        "pastDeadline": r["latencyMs"].as_i64().is_some_and(|l| l >= BUDGET_MS),
        "noWrongTarget": ([a.tab.window_id, spare.window_id].iter().any(|id| front_is(&in_flight["selectionLater"], *id))),
        "noUnrelatedSelectionChange": unrelated_changes(&in_flight["selectionBefore"], &in_flight["selectionLater"], &owned).is_empty(),
    });
    record(
        &mut cases,
        "A2-focus-script-in-flight-at-deadline",
        json!({ "checks": checks, "focusEvidence": r["evidence"]["focus"], "sideEffect": { "targetBecameFront": in_flight["targetBecameFront"], "afterMs": in_flight["targetFrontWaitMs"] }, "observed": in_flight }),
        all_true(&checks),
    )?;

    // C. Every proof holds; the final decision comes after the deadline.
    let decision = locked(ctx, "c11 decision held", || {
        to_spare();
        let before = window_selection();
        let detail = held(
            ctx,
            QualificationFault::HoldNextRouteBeforeDecision,
            "BEFORE_DECISION",
            Release::AfterDeadline(300),
            || route_full(ctx, &a.session_id),
        );
        json!({ "selectionBefore": before, "held": detail, "selectionLater": window_selection() })
    });
    let r = &decision["held"]["outcome"]["result"];
    let lookup = &r["evidence"]["postFocusLookup"];
    let checks = json!({
        "orderedReachedBeforeDeadlineReleasedAfter": ordered(&decision["held"], true),
        "focusAndReadbackProvedTarget": r["focusPerformed"] == true && r["evidence"]["focus"]["outcome"] == "FOCUSED" && r["evidence"]["focus"]["readbackTty"] == a.tab.tty.as_str(),
        "postFocusProofValid": lookup.is_object() && lookup["error"].is_null() && lookup.to_string().contains(&a.native_session_id),
        "bindingUnchanged": r["evidence"]["bindingRevisionAfterFocus"] == r["evidence"]["bindingRevisionLoaded"],
        "lateProofIsNotSuccess": timed_out(r) && !exact(r),
        "sideEffectRecorded": front_is(&decision["selectionLater"], a.tab.window_id),
        "noUnrelatedSelectionChange": unrelated_changes(&decision["selectionBefore"], &decision["selectionLater"], &owned).is_empty(),
    });
    record(
        &mut cases,
        "C-decision-held-past-deadline",
        json!({ "checks": checks, "observed": decision }),
        all_true(&checks),
    )?;

    // E and B: the target in its own fullscreen Space.
    let spaces = locked(ctx, "c11 fullscreen", || {
        let enter = ctx.native.json(&[
            "ax-action-number",
            &tpid.to_string(),
            &a.tab.window_id.to_string(),
            "fullscreen",
        ]);
        let (entered, entered_ok, _) = poll(
            Duration::from_secs(10),
            || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
            |v| {
                v["target"]["ax"]["fullScreen"] == true
                    && v["target"]["fullscreenFrame"] == true
                    && v["spare"]["onScreen"] == false
            },
        );
        let leave = || {
            raise_window(spare.window_id);
            poll(
                Duration::from_secs(10),
                || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
                |v| v["target"]["onScreen"] == false && v["spare"]["onScreen"] == true,
            )
        };
        let shown = || {
            poll(
                Duration::from_secs(5),
                || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
                |v| {
                    v["target"]["onScreen"] == true
                        && v["target"]["fullscreenFrame"] == true
                        && v["spare"]["onScreen"] == false
                },
            )
        };
        // E. Exact Return into the fullscreen Space.
        let away_e = leave();
        let fullscreen_route = route_full(ctx, &a.session_id);
        let returned_e = shown();
        // B1. Expiry around readback, after the script's settle completed.
        let away_b1 = leave();
        let before_b1 = window_selection();
        let readback_held = held(
            ctx,
            QualificationFault::HoldNextRouteBeforeReadback,
            "BEFORE_READBACK",
            Release::AfterDeadline(300),
            || route_full(ctx, &a.session_id),
        );
        let returned_b1 = shown();
        // B2. Expiry inside the focus script while macOS shows the Space: the
        //     script starts with too little budget left to see it settle.
        let mut settle_attempts = Vec::new();
        for left_ms in [250, 120] {
            let away = leave();
            let before = window_selection();
            let detail = held(
                ctx,
                QualificationFault::HoldNextRouteBeforeFocus,
                "BEFORE_FOCUS",
                Release::BeforeDeadline(left_ms),
                || route_full(ctx, &a.session_id),
            );
            let shown_later = shown();
            let after = window_selection();
            let r = &detail["outcome"]["result"];
            let expired_in_script = r["evidence"]["focus"]["error"]
                .as_str()
                .is_some_and(|e| e.contains("timed out true"));
            settle_attempts.push(json!({ "releasedWithMsLeft": left_ms, "away": away, "selectionBefore": before, "held": detail, "shownLater": shown_later, "selectionAfter": after, "expiredInScript": expired_in_script }));
            if expired_in_script {
                break;
            }
        }
        // B3. The bundled script itself: no budget, then budget.
        let away_s0 = leave();
        let no_budget = script_direct(ctx, &a.tab.tty, 0);
        let shown_s0 = shown();
        let away_s1 = leave();
        let with_budget = script_direct(ctx, &a.tab.tty, 1500);
        let exit = ctx.native.json(&[
            "ax-action-number",
            &tpid.to_string(),
            &a.tab.window_id.to_string(),
            "exit-fullscreen",
        ]);
        let (exited, exited_ok, _) = poll(
            Duration::from_secs(10),
            || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
            |v| {
                v["target"]["ax"]["fullScreen"] == false
                    && v["target"]["onScreen"] == true
                    && v["spare"]["onScreen"] == true
            },
        );
        let after_exit = route_full(ctx, &a.session_id);
        json!({
            "enter": enter, "entered": entered, "enteredWitnessed": entered_ok,
            "e": { "away": away_e, "route": fullscreen_route, "returned": returned_e },
            "b1": { "away": away_b1, "selectionBefore": before_b1, "held": readback_held, "returned": returned_b1, "selectionAfter": window_selection() },
            "b2": settle_attempts,
            "b3": { "awayNoBudget": away_s0, "noBudget": no_budget, "shownAfterNoBudget": shown_s0, "awayWithBudget": away_s1, "withBudget": with_budget },
            "exit": exit, "exited": exited, "exitWitnessed": exited_ok, "routeAfterExit": after_exit,
        })
    });
    let e = &spaces["e"]["route"]["result"];
    let checks = json!({
        "enteredFullscreenSpace": spaces["enteredWitnessed"] == true,
        "leftItsSpace": spaces["e"]["away"][1] == true,
        "exact": exact(e) && e["reasonCode"] == "OK",
        "withinBudget": e["latencyMs"].as_i64().is_some_and(|l| l < BUDGET_MS),
        "readbackIsTarget": e["evidence"]["focus"]["frontWindowId"] == a.tab.window_id && e["evidence"]["focus"]["readbackTty"] == a.tab.tty.as_str(),
        "spaceShown": spaces["e"]["returned"][1] == true,
        "independentFrontIsTarget": front_is(&spaces["e"]["route"]["selectionAfter"], a.tab.window_id),
        "exitedAndExactAfter": spaces["exitWitnessed"] == true && exact(&spaces["routeAfterExit"]["result"]),
    });
    record(
        &mut cases,
        "E-fullscreen-space-return",
        json!({ "checks": checks, "latencyMs": e["latencyMs"], "phases": e["evidence"]["phases"], "observed": { "enter": spaces["enter"], "entered": spaces["entered"], "e": spaces["e"], "exit": spaces["exit"], "exited": spaces["exited"], "routeAfterExit": spaces["routeAfterExit"] } }),
        all_true(&checks),
    )?;

    let b1 = &spaces["b1"];
    let r = &b1["held"]["outcome"]["result"];
    let b1_checks = json!({
        "leftItsSpace": b1["away"][1] == true,
        "orderedReachedBeforeDeadlineReleasedAfter": ordered(&b1["held"], true),
        "settledFocusRecorded": r["focusPerformed"] == true && r["evidence"]["focus"]["frontWindowId"] == a.tab.window_id,
        "exactRefused": timed_out(r) && !exact(r),
        "noUnrelatedSelectionChange": unrelated_changes(&b1["selectionBefore"], &b1["selectionAfter"], &owned).is_empty(),
    });
    let b2 = spaces["b2"].as_array().cloned().unwrap_or_default();
    let b2_last = b2.last().cloned().unwrap_or(Value::Null);
    let r2 = &b2_last["held"]["outcome"]["result"];
    let b2_checks = json!({
        "leftItsSpace": b2_last["away"][1] == true,
        "releasedInsideBudget": ordered(&b2_last["held"], false),
        "expiredInsideFocusScript": b2_last["expiredInScript"] == true,
        "exactRefused": timed_out(r2) && !exact(r2) && r2["focusPerformed"] == true,
        "noAttemptExactAfterDeadline": b2.iter().all(|t| !(exact(&t["held"]["outcome"]["result"]) && t["held"]["outcome"]["result"]["latencyMs"].as_i64().is_some_and(|l| l >= BUDGET_MS))),
        "noWrongTarget": b2.iter().all(|t| [a.tab.window_id, spare.window_id].iter().any(|id| t["selectionAfter"]["front"] == *id)),
        "noUnrelatedSelectionChange": b2.iter().all(|t| unrelated_changes(&t["selectionBefore"], &t["selectionAfter"], &owned).is_empty()),
    });
    let b3 = &spaces["b3"];
    let fields = |v: &Value, i: usize| v["fields"][i].as_str().and_then(|s| s.parse::<i64>().ok());
    let b3_checks = json!({
        "noBudgetRunsWithoutWaiting": b3["awayNoBudget"][1] == true && b3["noBudget"]["fields"][0] == "FOCUSED",
        "budgetWaitsForTheSpace": b3["awayWithBudget"][1] == true && b3["withBudget"]["fields"][0] == "FOCUSED" && fields(&b3["withBudget"], 3) == Some(a.tab.window_id),
    });
    // Informational: whether the no-budget readback caught macOS mid-switch.
    let b3_mid_switch = fields(&b3["noBudget"], 3) != Some(a.tab.window_id);
    let checks = json!({ "readbackHeld": all_true(&b1_checks), "insideScript": all_true(&b2_checks), "scriptBound": all_true(&b3_checks) });
    record(
        &mut cases,
        "B-settle-expiry",
        json!({ "checks": checks, "readbackHeld": { "checks": b1_checks, "observed": b1 }, "insideScript": { "checks": b2_checks, "attempts": b2 }, "scriptBound": { "checks": b3_checks, "noBudgetReadbackCaughtSpaceMidSwitch": b3_mid_switch, "observed": b3 } }),
        all_true(&checks),
    )?;

    // F. Attention: a timed-out notification Return is not acknowledged.
    let attention = locked(ctx, "c11 attention", || {
        to_spare();
        let label = format!("c11-f-{}", &uuid::Uuid::new_v4().to_string()[..6]);
        let raised = ctx.companion().request(
            ControlRequestBody::QualifyRaiseAttention {
                label: label.clone(),
                session_id: Some(a.session_id.clone()),
            },
            Duration::from_secs(10),
        );
        let Ok(ControlResponseBody::AttentionRaised {
            attention_id,
            notification_request_id,
            ..
        }) = raised
        else {
            return json!({ "error": format!("raise failed: {raised:?}") });
        };
        let mut log = ctx.companion().log();
        let mut seen = Vec::new();
        let detail = held(
            ctx,
            QualificationFault::HoldNextRouteBeforeReadback,
            "BEFORE_READBACK",
            Release::AfterDeadline(300),
            || {
                let reply = ctx.companion().request(
                    ControlRequestBody::QualifyNotificationResponse {
                        notification_request_id: notification_request_id.clone(),
                        attention_id: attention_id.clone(),
                    },
                    Duration::from_secs(5),
                );
                json!({ "injected": format!("{reply:?}") })
            },
        );
        let returned = log.wait_for(
            "NOTIFICATION_RETURN",
            |l| l["attentionId"] == attention_id.as_str(),
            Duration::from_secs(15),
            &mut seen,
        );
        let queued = log.wait_for(
            "INTENT_ACCEPTED",
            |l| l["intentId"] == notification_request_id.as_str(),
            Duration::from_secs(10),
            &mut seen,
        );
        threadspace_harness::pause_ms(1000);
        let after_timeout = attention_state(ctx, &attention_id);
        let retry = route_full(ctx, &a.session_id);
        threadspace_harness::pause_ms(500);
        let after_retry = attention_state(ctx, &attention_id);
        let acknowledged = ctx.companion().request(
            ControlRequestBody::AcknowledgeAttention {
                command_id: uuid::Uuid::new_v4().to_string(),
                attention_id: attention_id.clone(),
                expected_revision: None,
            },
            Duration::from_secs(10),
        );
        let after_ack = attention_state(ctx, &attention_id);
        json!({
            "attentionId": attention_id, "notificationRequestId": notification_request_id,
            "held": detail, "notificationReturn": returned, "intentQueued": queued,
            "attentionAfterTimeout": after_timeout, "retry": retry, "attentionAfterRetry": after_retry,
            "acknowledge": format!("{acknowledged:?}").chars().take(200).collect::<String>(), "attentionAfterAcknowledge": after_ack,
        })
    });
    let route_summary = &attention["notificationReturn"]["route"];
    let checks = json!({
        "orderedReachedBeforeDeadlineReleasedAfter": ordered(&attention["held"], true),
        "notificationReturnTimedOut": attention["notificationReturn"]["exact"] == false && route_summary["reasonCode"] == "TIMEOUT" && route_summary["focusPerformed"] == true,
        "intentKeptForTheInspector": attention["intentQueued"].is_object(),
        "notAcknowledgedAfterTimeout": attention["attentionAfterTimeout"]["present"] == true && attention["attentionAfterTimeout"]["acknowledgedAtMs"].is_null(),
        "laterRetryExact": exact(&attention["retry"]["result"]),
        "noAutomaticAcknowledgement": attention["attentionAfterRetry"]["acknowledgedAtMs"].is_null(),
        "ownerAcknowledgementApplies": attention["attentionAfterAcknowledge"]["acknowledgedAtMs"].is_number(),
    });
    record(
        &mut cases,
        "F-attention-not-acknowledged-on-timeout",
        json!({ "checks": checks, "note": "M0C acknowledges attention only by the owner's explicit command; the automatic rule of SPEC §7 (exact, current, foreground-compatible) is not implemented, and a timed-out result can never meet it", "observed": attention }),
        all_true(&checks),
    )?;

    locked(ctx, "c11 cleanup", || {
        for tab in [&a.tab, &spare] {
            let _ = run_dir.append("cleanup.jsonl", &tab.close());
        }
    });
    let _ = crate::cleanup::resolve_qualification(ctx, "c11 qualification cleanup");
    let _ = crate::cleanup::clear_notifications(ctx);
    if ui_before.is_empty() {
        ctx.app().stop_all();
    }
    let terminal_after = terminal::terminal_process();
    let wrong_targets = cases
        .iter()
        .filter(|c| c["detail"]["checks"]["noWrongTarget"] == false)
        .count();
    let summary = json!({
        "issue": "C-11",
        "gate": "G08",
        "pass": cases.iter().all(|c| c["pass"] == true) && wrong_targets == 0,
        "wrongTargets": wrong_targets,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "ordinaryLatencyMs": cases.iter().find(|c| c["case"] == "D-ordinary-exact-route").map(|c| c["detail"]["latencyMs"].clone()),
        "fullscreenLatencyMs": cases.iter().find(|c| c["case"] == "E-fullscreen-space-return").map(|c| c["detail"]["latencyMs"].clone()),
        "terminalIncarnationUnchanged": terminal_before == terminal_after,
        "companionSha256": environment["companionSha256"],
        "executableSha256": environment["executableSha256"],
        "disposableRoot": root.display().to_string(),
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
