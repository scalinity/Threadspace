//! G05/G06 notification lifecycle on the installed production identity. Each
//! interaction raises a qualification attention item, waits for the
//! companion's `UNUserNotificationCenter` submission, then presses the
//! presented banner through the Accessibility press action (the path
//! VoiceOver uses) — a real interaction with the system notification UI, not
//! a direct delegate call. It verifies the OS request identity, the
//! delegate's response, the plan (verified Return or inspector), the view's
//! applied intent and that nothing was acknowledged automatically.

use std::time::Duration;

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_harness::companion::LogCursor;
use threadspace_harness::evidence::Run;
use threadspace_harness::procs;
use threadspace_harness::service;

use crate::bridge_gates::{companion_snapshot, ensure_ui};
use crate::ctx::Ctx;
use crate::terminal_gates::spawn_claude;

struct Raised {
    label: String,
    attention_id: String,
    request_id: String,
}

fn raise(ctx: &Ctx, label: &str, session_id: Option<&str>) -> Result<Raised, String> {
    match ctx.companion().request(
        ControlRequestBody::QualifyRaiseAttention {
            label: label.to_owned(),
            session_id: session_id.map(str::to_owned),
        },
        Duration::from_secs(10),
    )? {
        ControlResponseBody::AttentionRaised {
            attention_id,
            notification_request_id,
            ..
        } => Ok(Raised {
            label: label.to_owned(),
            attention_id,
            request_id: notification_request_id,
        }),
        other => Err(format!("unexpected {other:?}")),
    }
}

fn wait_line(
    cursor: &mut LogCursor,
    event: &str,
    attention_id: &str,
    timeout_s: u64,
) -> Option<Value> {
    let mut seen = Vec::new();
    cursor.wait_for(
        event,
        |line| line["attentionId"].as_str() == Some(attention_id),
        Duration::from_secs(timeout_s),
        &mut seen,
    )
}

fn attention_state(ctx: &Ctx, attention_id: &str) -> Value {
    match companion_snapshot(ctx) {
        Ok((_, snapshot)) => snapshot
            .attention
            .iter()
            .find(|a| a.attention_id == attention_id)
            .map(|a| json!({ "present": true, "acknowledgedAtMs": a.acknowledged_at_ms, "resolvedAtMs": a.resolved_at_ms, "notificationState": a.notification_state }))
            .unwrap_or_else(|| json!({ "present": false })),
        Err(error) => json!({ "error": error }),
    }
}

fn press(ctx: &Ctx, raised: &Raised, timeout_s: u64) -> Value {
    let needle = raised.label.clone();
    ctx.native
        .json(&["notification", "press", &needle, &timeout_s.to_string()])
}

/// One interaction. `before_click` changes the world between submission and
/// the press; `expect` names the plan the companion must choose.
#[allow(clippy::too_many_arguments)]
fn interaction(
    ctx: &Ctx,
    run_dir: &Run,
    case: &str,
    session_id: Option<&str>,
    before_click: &dyn Fn(&Raised) -> Value,
    expect_plan: &str,
    expect_ui_cold_start: bool,
) -> Result<Value, String> {
    let label = format!("{case}-{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let mut cursor = ctx.companion().log();
    let raised = raise(ctx, &label, session_id)?;
    let submission = wait_line(
        &mut cursor,
        "NOTIFICATION_SUBMISSION",
        &raised.attention_id,
        20,
    );
    let setup = before_click(&raised);
    let _gui = ctx.gui(&format!("g06 {case}"))?;
    let ui_before = ctx.app().processes();
    let since = threadspace_harness::now_ms();
    let pressed = press(ctx, &raised, 20);
    let response = wait_line(
        &mut cursor,
        "NOTIFICATION_RESPONSE",
        &raised.attention_id,
        45,
    );
    let outcome = if expect_plan == "RETURN" {
        wait_line(&mut cursor, "NOTIFICATION_RETURN", &raised.attention_id, 30)
    } else {
        wait_line(
            &mut cursor,
            "NOTIFICATION_INSPECTOR",
            &raised.attention_id,
            30,
        )
    };
    let intent = ctx
        .app()
        .wait_report(
            "notification-intent",
            since,
            |r| r["report"]["attentionId"].as_str() == Some(raised.attention_id.as_str()),
            Duration::from_secs(60),
        )
        .map(|(_, r)| r["report"].clone());
    threadspace_harness::pause_ms(1500);
    let state = attention_state(ctx, &raised.attention_id);
    let ui_after = ctx.app().processes();
    let identity_ok = response
        .as_ref()
        .is_some_and(|r| r["notificationRequestId"].as_str() == Some(raised.request_id.as_str()));
    let plan_ok = response.as_ref().is_some_and(|r| r["plan"] == expect_plan);
    let cold_ok = !expect_ui_cold_start || (ui_before.is_empty() && !ui_after.is_empty());
    let no_auto_ack = state["acknowledgedAtMs"].is_null();
    let intent_ok = intent
        .as_ref()
        .is_some_and(|i| i["appliedAfterHydration"] == true);
    let pass = submission
        .as_ref()
        .is_some_and(|s| s["state"] == "SUBMITTED")
        && pressed["pressed"] == true
        && identity_ok
        && plan_ok
        && outcome.is_some()
        && intent_ok
        && cold_ok
        && no_auto_ack;
    let record = json!({
        "case": case,
        "pass": pass,
        "attentionId": raised.attention_id,
        "notificationRequestId": raised.request_id,
        "submission": submission,
        "setup": setup,
        "press": pressed,
        "response": response,
        "outcome": outcome,
        "viewIntent": intent,
        "uiBefore": ui_before,
        "uiAfter": ui_after,
        "attentionAfter": state,
        "checks": { "osIdentity": identity_ok, "plan": plan_ok, "coldStart": cold_ok, "noAutoAcknowledge": no_auto_ack, "intentAppliedAfterHydration": intent_ok },
    });
    run_dir
        .append("interactions.jsonl", &record)
        .map_err(|e| e.to_string())?;
    Ok(record)
}

pub fn lifecycle(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "g06-notifications",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let app = ctx.app();
    let nothing = |_: &Raised| json!(null);
    let mut records = Vec::new();

    // 1. UI open, fixture session (no routable surface): Return is attempted
    //    and refused, the inspector opens with the route result.
    ensure_ui(ctx)?;
    records.push(interaction(
        ctx, &run_dir, "ui-open", None, &nothing, "RETURN", false,
    )?);

    // 2. UI quit: cold UI start, intent applied only after hydration.
    app.stop_all();
    records.push(interaction(
        ctx,
        &run_dir,
        "ui-quit-cold-start",
        None,
        &nothing,
        "RETURN",
        true,
    )?);

    // 3. Helper restarted between submission and click.
    let restart = |_: &Raised| {
        let old = ctx.companion().incarnation();
        if let Some(old) = &old {
            procs::signal(old.pid, libc::SIGKILL);
        }
        let fresh = ctx
            .companion()
            .wait_new_incarnation(old.as_ref(), Duration::from_secs(90));
        json!({ "killed": old, "relaunched": fresh.map(|(i, w)| json!({ "incarnation": i, "waitedMs": w })) })
    };
    records.push(interaction(
        ctx,
        &run_dir,
        "helper-restarted",
        None,
        &restart,
        "RETURN",
        false,
    )?);

    // 4. Attention already resolved: the click opens the inspector, handled.
    let resolve = |raised: &Raised| {
        let reply = ctx.companion().request(
            ControlRequestBody::ResolveAttention {
                command_id: uuid::Uuid::new_v4().to_string(),
                attention_id: raised.attention_id.clone(),
                expected_revision: None,
                reason: "qualification: resolved before the click".into(),
            },
            Duration::from_secs(10),
        );
        json!(format!("{reply:?}").chars().take(200).collect::<String>())
    };
    records.push(interaction(
        ctx,
        &run_dir,
        "attention-already-resolved",
        None,
        &resolve,
        "INSPECTOR",
        false,
    )?);

    // 5./6. A real Claude session in a disposable window: an exact Return,
    //       then a target that changed (its window closed) before the click.
    let root = std::path::PathBuf::from(format!(
        "/private/tmp/ts-m0c-notify-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let live = spawn_claude(ctx, root.join("live"))?;
    let exact = interaction(
        ctx,
        &run_dir,
        "real-session-exact-return",
        Some(&live.session_id),
        &nothing,
        "RETURN",
        false,
    )?;
    let selected = threadspace_harness::terminal::selected_tty();
    let exact_ok =
        exact["outcome"]["exact"] == true && selected.as_deref() == Some(live.tab.tty.as_str());
    records.push(json!({ "case": exact["case"], "pass": exact["pass"] == true && exact_ok, "independentSelectedTty": selected, "detail": exact }));
    let changed = spawn_claude(ctx, root.join("changed"))?;
    let close_target = |_: &Raised| {
        let _gui = ctx.gui("g06 close target window");
        changed.tab.close()
    };
    let target_changed = interaction(
        ctx,
        &run_dir,
        "target-changed",
        Some(&changed.session_id),
        &close_target,
        "RETURN",
        false,
    )?;
    let refused = target_changed["outcome"]["exact"] == false;
    records.push(json!({ "case": "target-changed", "pass": target_changed["pass"] == true && refused, "detail": target_changed }));

    // 7. Response during companion recovery: the click lands while the killed
    //    helper is being relaunched.
    let crash_now = |_: &Raised| {
        let old = ctx.companion().incarnation();
        if let Some(old) = &old {
            procs::signal(old.pid, libc::SIGKILL);
        }
        json!({ "killed": old })
    };
    let during = interaction(
        ctx,
        &run_dir,
        "response-during-recovery",
        None,
        &crash_now,
        "RETURN",
        false,
    )?;
    let single_writer = ctx.companion().processes().len() == 1;
    records.push(json!({ "case": "response-during-recovery", "pass": during["pass"] == true && single_writer, "singleCompanionAfter": single_writer, "detail": during }));

    // 8. Maintenance prepared: admission closed, so the click opens the
    //    inspector and routes nothing.
    let prepare = |_: &Raised| service::bootstrap(&ctx.id, "prepare");
    let maintenance = interaction(
        ctx,
        &run_dir,
        "maintenance-prepared",
        None,
        &prepare,
        "INSPECTOR",
        false,
    )?;
    let cancelled = service::bootstrap(&ctx.id, "cancel");
    records.push(json!({ "case": "maintenance-prepared", "pass": maintenance["pass"] == true && maintenance["viewIntent"]["observationEnabled"] == false, "cancel": cancelled, "detail": maintenance }));

    // 9. Observation stopped (companion unregistered and gone): the click
    //    cold-starts the companion, which stays control-only, registers
    //    nothing and opens the inspector.
    app.stop_all();
    let stop = |_: &Raised| service::bootstrap(&ctx.id, "stop");
    let stopped = interaction(
        ctx,
        &run_dir,
        "observation-stopped-cold-start",
        None,
        &stop,
        "INSPECTOR",
        true,
    )?;
    let status_after = service::status(&ctx.id);
    records.push(json!({
        "case": "observation-stopped-cold-start",
        "pass": stopped["pass"] == true && status_after == "NOT_REGISTERED" && stopped["viewIntent"]["observationEnabled"] == false,
        "serviceStatusAfterClick": status_after,
        "detail": stopped,
    }));
    let enabled = service::bootstrap(&ctx.id, "enable");
    run_dir
        .write_json("enable-after-stopped.json", &enabled)
        .map_err(|e| e.to_string())?;

    // 10. UI open again after recovery, plain interaction.
    ensure_ui(ctx)?;
    records.push(interaction(
        ctx,
        &run_dir,
        "ui-open-after-recovery",
        None,
        &nothing,
        "RETURN",
        false,
    )?);

    {
        let _gui = ctx.gui("g06 cleanup");
        let _ = live.tab.close();
    }
    let passed = records.iter().filter(|r| r["pass"] == true).count();
    let interactions = records.len();
    let summary = json!({
        "gate": "G06",
        "pass": passed == interactions && interactions >= 10,
        "interactions": interactions,
        "passed": passed,
        "cases": records.iter().map(|r| json!({ "case": r["case"], "pass": r["pass"] })).collect::<Vec<_>>(),
        "grouped": "NOT_SUPPORTED in this phase: the companion submits one notification per attention item; grouping arrives with the attention policy (M5).",
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

/// G05 denied path: the development identity's notifications are denied, so
/// a submission fails while the attention item stays durable.
pub fn denied(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "g05-notifications",
        &format!("denied-{}", ctx.channel_name()),
    )
    .map_err(|e| e.to_string())?;
    let settings = ctx.companion().diagnostics()?["notificationSettings"].clone();
    let mut cursor = ctx.companion().log();
    let raised = raise(
        ctx,
        &format!("denied-{}", &uuid::Uuid::new_v4().to_string()[..6]),
        None,
    )?;
    let submission = wait_line(
        &mut cursor,
        "NOTIFICATION_SUBMISSION",
        &raised.attention_id,
        20,
    );
    let state = attention_state(ctx, &raised.attention_id);
    let summary = json!({
        "gate": "G05",
        "case": "notification denied",
        "authorizationStatus": settings["authorizationStatus"],
        "submission": submission,
        "attentionAfter": state,
        "pass": settings["authorizationStatus"] == "denied"
            && submission.as_ref().is_some_and(|s| s["state"] == "FAILED")
            && state["present"] == true,
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
