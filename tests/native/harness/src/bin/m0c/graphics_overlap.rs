//! H-11 (G15 remediation): the two overlaps the sustained run did not prove,
//! each held at a qualification-only barrier in the real code path.
//!
//! - Pending init: the renderer's WebGPU adapter request (the first await in
//!   the pinned backend's `init()`) is issued and its result withheld; the
//!   window is minimized and that generation retired while its init is
//!   pending; the late result must be discarded, the renderer disposed and no
//!   hidden frame scheduled; on return a fresh generation attests WebGPU and
//!   the canvas changes.
//! - Pending resource: the office view's same-scheme `tauri://` response for
//!   the scene's bundled texture is held in the shell's protocol handler; the
//!   document is reloaded, so the shell retires and recreates the view while
//!   the request is outstanding; the late response is released to a retired
//!   view, the new view hydrates and renders, and nothing late is consumed.
//!
//! Every overlap is proven by ordered timestamps from the barrier, the
//! lifecycle's own reports and the shell's native log.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::procs::Incarnation;

use crate::bridge_gates::{compare_projection, ensure_ui};
use crate::ctx::Ctx;
use crate::graphics::{Captures, canvas_crop, changed, command, keep_centre_animated, state};

const TITLE: &str = "Threadspace";

fn peek(ctx: &Ctx) -> Value {
    ctx.app()
        .view_command("renderer:peek-state", json!({}), Duration::from_secs(20))
        .map(|r| r["result"].clone())
        .unwrap_or_else(|e| json!({ "error": e }))
}

fn hold(ctx: &Ctx, op: &str) -> Value {
    ctx.app()
        .view_command(
            "resource-hold",
            json!({ "op": op, "pathContains": "floor-grain" }),
            Duration::from_secs(20),
        )
        .map(|r| json!({ "ok": r["ok"], "result": r["result"], "error": r["error"] }))
        .unwrap_or_else(|e| json!({ "error": e }))
}

fn poll(
    timeout: Duration,
    read: impl Fn() -> Value,
    done: impl Fn(&Value) -> bool,
) -> (Value, bool, u64) {
    let started = Instant::now();
    loop {
        let value = read();
        if done(&value) {
            return (value, true, started.elapsed().as_millis() as u64);
        }
        if started.elapsed() >= timeout {
            return (value, false, started.elapsed().as_millis() as u64);
        }
        threadspace_harness::pause_ms(200);
    }
}

fn outstanding_assets(snapshot: &Value) -> i64 {
    let a = &snapshot["asset"];
    let n = |k: &str| a[k].as_i64().unwrap_or(0);
    n("started") - n("applied") - n("aborted") - n("failed") - n("discardedLate")
}

/// Lifecycle reports written since `since`, as `(event, atMs, detail)`.
fn lifecycle_events(ctx: &Ctx, since: i64) -> Vec<Value> {
    ctx.app()
        .reports("renderer-lifecycle", since)
        .into_iter()
        .map(|(_, r)| json!({ "event": r["report"]["event"], "atMs": r["report"]["atMs"], "detail": r["report"]["detail"] }))
        .collect()
}

fn first_event(events: &[Value], name: &str, matches: impl Fn(&Value) -> bool) -> Option<Value> {
    events
        .iter()
        .find(|e| e["event"] == name && matches(e))
        .cloned()
}

/// Outside interference in a case: owner keyboard/mouse input (HID idle time
/// shorter than the case; this runner posts no input events), or the office
/// left 3D without a harness command. Such a case is reported as
/// interfered, not as a renderer result.
fn interference(ctx: &Ctx, case_started: Instant, view: &Value) -> Value {
    let idle = ctx.native.idle_seconds();
    let elapsed = case_started.elapsed().as_secs_f64();
    let owner_input = idle < elapsed;
    let not_3d = view["presentation"] != "3d";
    json!({ "ownerHidIdleS": idle, "caseElapsedS": elapsed, "ownerInputDuringCase": owner_input, "officeNot3dBeforeCapture": not_3d, "interfered": owner_input || not_3d })
}

/// Three canvas captures two seconds apart; the scene must change.
fn fresh_pixels(captures: &mut Captures<'_>, phase: &str) -> (Vec<Value>, usize) {
    captures.previous = None;
    let mut shots = Vec::new();
    let mut moving = 0;
    for _ in 0..3 {
        threadspace_harness::pause_ms(2000);
        let shot = captures.take(phase);
        if changed(&shot) {
            moving += 1;
        }
        shots.push(shot);
    }
    (shots, moving)
}

pub fn overlap(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/h11-graphics",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let app = ctx.app();
    app.stop_all();
    let gate = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    run_dir
        .write_json("idle-gate.json", &json!(gate))
        .map_err(|e| e.to_string())?;
    run_dir
        .write_json("environment.json", &ctx.environment())
        .map_err(|e| e.to_string())?;
    // The window must stay visible and unoccluded while frames are measured.
    let _gui = ctx.gui("h11 graphics overlap")?;
    let ui: Incarnation = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    let started = Instant::now();
    let mut timeline: Vec<Value> = Vec::new();
    let mut log = |event: &str, detail: Value| {
        let entry = json!({ "atS": started.elapsed().as_secs_f64(), "atMs": threadspace_harness::now_ms(), "event": event, "detail": detail });
        let _ = run_dir.append("timeline.jsonl", &entry);
        timeline.push(entry);
    };
    let mut captures = Captures {
        ctx,
        run: &run_dir,
        pid,
        previous: None,
        index: 0,
        crop: None,
    };
    let mut fixture: Vec<Value> = Vec::new();
    log("motion-fixture", keep_centre_animated(ctx, &mut fixture));
    let calibration = captures.take("calibration");
    let surface = state(ctx);
    captures.crop = calibration["stats"]["width"]
        .as_f64()
        .zip(calibration["stats"]["height"].as_f64())
        .and_then(|(w, h)| canvas_crop(&surface, w, h));
    if captures.crop.is_none() {
        return Err("the canvas rectangle could not be located in a window capture".into());
    }
    let (baseline_shots, baseline_moving) = fresh_pixels(&mut captures, "baseline");
    log(
        "baseline",
        json!({ "state": surface["state"], "generation": surface["generation"], "backend": surface["lastAttestation"]["backend"], "shots": baseline_shots, "moving": baseline_moving }),
    );
    let mut cases: Vec<Value> = Vec::new();
    let mut record = |name: &str, pass: bool, detail: Value| {
        let entry = json!({ "case": name, "pass": pass, "detail": detail });
        let _ = run_dir.append("cases.jsonl", &entry);
        cases.push(entry);
    };

    // ---------------------------------------------- H-11A: pending init
    // The gate must have seen the page's own first init, or it cannot hold one.
    let gate_seen = peek(ctx)["initBarrier"].clone();
    log("init-gate", gate_seen.clone());
    if gate_seen["installed"] != true || gate_seen["calls"].as_u64().unwrap_or(0) == 0 {
        return Err(format!(
            "the init gate does not observe renderer initialization: {gate_seen}"
        ));
    }
    let case_a_started = Instant::now();
    let before = state(ctx);
    let before_generation = before["generation"].as_i64().unwrap_or(0);
    log("enter-2d", command(ctx, "enter-2d"));
    let armed = command(ctx, "arm-init-barrier");
    log("arm-init-barrier", armed.clone());
    let since = threadspace_harness::now_ms();
    // Not awaited: this command settles only after the held init does.
    let rebuild = ctx.companion().view_command("renderer:exit-2d", json!({}));
    log("exit-2d-sent", json!(format!("{rebuild:?}")));
    let (pending, pending_ok, pending_ms) = poll(
        Duration::from_secs(15),
        || peek(ctx),
        |p| {
            p["initPending"]["generation"]
                .as_i64()
                .is_some_and(|g| g > before_generation)
                && p["initPending"]["retired"] == false
                && p["initBarrier"]["held"].is_object()
        },
    );
    log("init-pending", pending.clone());
    let held_generation = pending["initPending"]["generation"].as_i64();
    let minimize_requested_ms = threadspace_harness::now_ms();
    let minimize = ctx.native.ax_action(pid, "minimize", Some(TITLE));
    log("minimize", minimize.clone());
    let (retired, retired_ok, _) = poll(
        Duration::from_secs(15),
        || peek(ctx),
        |p| {
            p["initPending"]["generation"].as_i64() == held_generation
                && p["initPending"]["retired"] == true
                && p["initBarrier"]["held"].is_object()
        },
    );
    log("retired-while-pending", retired.clone());
    let released = command(ctx, "release-init-barrier");
    log("release-init-barrier", released.clone());
    let (hidden, hidden_ok, _) = poll(
        Duration::from_secs(20),
        || peek(ctx),
        |p| p["state"] == "hidden-disposed" && p["initPending"].is_null(),
    );
    log("hidden-disposed", hidden.clone());
    let quiet_a = peek(ctx);
    threadspace_harness::pause_ms(10_000);
    let quiet_b = peek(ctx);
    log(
        "hidden-quiet",
        json!({ "first": quiet_a["frames"], "afterTenSeconds": quiet_b["frames"] }),
    );
    let projection_hidden = compare_projection(ctx, "init-race-hidden");
    let events = lifecycle_events(ctx, since);
    let retired_event = first_event(&events, "generation-retired", |e| {
        e["detail"]["generation"].as_i64() == held_generation && e["detail"]["initialized"] == false
    });
    let late_event = first_event(&events, "late-init-discarded", |e| {
        e["detail"]["generation"].as_i64() == held_generation
    });
    let release_record = released["result"]["initBarrier"]["releases"]
        .as_array()
        .and_then(|r| r.last().cloned())
        .unwrap_or(Value::Null);
    let held_at = release_record["heldAtMs"].as_i64();
    let released_at = release_record["releasedAtMs"].as_i64();
    let retired_at = retired_event.as_ref().and_then(|e| e["atMs"].as_i64());
    let late_at = late_event.as_ref().and_then(|e| e["atMs"].as_i64());
    let ordered = matches!((held_at, retired_at, released_at, late_at), (Some(h), Some(r), Some(rel), Some(l)) if h < minimize_requested_ms && minimize_requested_ms < r && r < rel && rel <= l);
    ctx.native.ax_action(pid, "unminimize", Some(TITLE));
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    let (back, back_ok, _) = poll(
        Duration::from_secs(20),
        || state(ctx),
        |s| s["state"] == "live",
    );
    log("returned-visible", back.clone());
    log("motion-fixture", keep_centre_animated(ctx, &mut fixture));
    let interference_a = interference(ctx, case_a_started, &state(ctx));
    let (shots_a, moving_a) = fresh_pixels(&mut captures, "after-init-race");
    let init_checks = json!({
        "barrierInstalledAndArmed": armed["result"]["initBarrier"]["installed"] == true && armed["result"]["initBarrier"]["armed"] == true,
        "initPendingWitnessed": pending_ok && held_generation.is_some(),
        "minimized": minimize["after"]["minimized"] == true,
        "retiredWhileInitPending": retired_ok && retired_event.is_some(),
        "releasedAfterRetirement": released["ok"] == true && release_record["releasedBy"] == "COMMAND",
        "lateInitDiscarded": late_event.is_some() && hidden["counts"]["discardedLateInits"].as_i64() > before["counts"]["discardedLateInits"].as_i64(),
        "orderedOverlap": ordered,
        "noRevival": hidden_ok && hidden["liveGeneration"].is_null(),
        "noHiddenScheduling": quiet_b["frames"]["pending"] == 0 && quiet_b["frames"]["requestedSinceDisposal"] == 0
            && quiet_a["frames"]["requested"] == quiet_b["frames"]["requested"],
        "operationalWhileHidden": projection_hidden["equal"] == true,
        "freshGenerationAttested": back_ok && back["generation"].as_i64() > held_generation && back["liveGeneration"] == back["generation"]
            && back["lastAttestation"]["backend"] == "WEBGPU",
        "freshChangingPixels": moving_a >= 2,
    });
    let init_pass = init_checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true))
        && interference_a["interfered"] == false;
    record(
        "hide-while-renderer-init-pending",
        init_pass,
        json!({
            "checks": init_checks, "interference": interference_a, "heldGeneration": held_generation, "pendingWaitMs": pending_ms,
            "times": { "heldAtMs": held_at, "minimizeRequestedAtMs": minimize_requested_ms, "retiredAtMs": retired_at, "releasedAtMs": released_at, "lateInitDiscardedAtMs": late_at },
            "release": release_record, "pending": pending, "retired": retired, "hidden": hidden,
            "frames": { "first": quiet_a["frames"], "afterTenSeconds": quiet_b["frames"] },
            "projectionWhileHidden": projection_hidden["equal"], "returned": { "state": back["state"], "generation": back["generation"], "liveGeneration": back["liveGeneration"], "attestation": back["lastAttestation"] },
            "lifecycleEvents": events, "pixels": shots_a,
        }),
    );

    // ----------------------------------------- H-11B: pending resource
    let case_b_started = Instant::now();
    let shells_before = ctx.native.window_count(pid);
    // Outstanding requests before the held one: an asset a generation received
    // before its scene existed is disposed at retirement without a counter, so
    // the held request is measured as a change from this baseline.
    let outstanding_before = outstanding_assets(&peek(ctx));
    let mut desktop = app.desktop_log();
    let mut lines = Vec::new();
    let armed_hold = hold(ctx, "ARM");
    log("arm-resource-hold", armed_hold.clone());
    log("enter-2d", command(ctx, "enter-2d"));
    log("exit-2d", command(ctx, "exit-2d"));
    let held = desktop.wait_for(
        "QUALIFY_RESOURCE_HELD",
        |_| true,
        Duration::from_secs(20),
        &mut lines,
    );
    let pending_view = peek(ctx);
    let status = hold(ctx, "STATUS");
    log(
        "resource-held",
        json!({ "held": held, "view": { "generation": pending_view["generation"], "asset": pending_view["asset"] }, "status": status }),
    );
    let held_incarnation = held
        .as_ref()
        .map(|l| l["detail"]["incarnation"].clone())
        .unwrap_or(Value::Null);
    let reload_sent_ms = threadspace_harness::now_ms();
    log(
        "reload",
        app.view_command_nowait("reload", json!({}))
            .unwrap_or_else(|e| json!(e)),
    );
    let recovered = desktop.wait_for(
        "OFFICE_VIEW_RECOVERED",
        |_| true,
        Duration::from_secs(60),
        &mut lines,
    );
    log("view-recovered", json!(recovered));
    let (new_view, new_ok, _) = poll(
        Duration::from_secs(30),
        || state(ctx),
        |s| {
            s["state"] == "live"
                && outstanding_assets(s) == 0
                && s["asset"]["applied"].as_i64() >= Some(1)
        },
    );
    log(
        "new-view-live",
        json!({ "state": new_view["state"], "generation": new_view["generation"], "asset": new_view["asset"] }),
    );
    let released_hold = hold(ctx, "RELEASE");
    let released_line = desktop.wait_for(
        "QUALIFY_RESOURCE_RELEASED",
        |_| true,
        Duration::from_secs(15),
        &mut lines,
    );
    log(
        "resource-released",
        json!({ "reply": released_hold, "log": released_line }),
    );
    threadspace_harness::pause_ms(3000);
    let after_release = peek(ctx);
    let ui_after = app.processes();
    let shells_after = ctx.native.window_count(pid);
    let projection_new = compare_projection(ctx, "after-resource-reload");
    log("motion-fixture", keep_centre_animated(ctx, &mut fixture));
    let interference_b = interference(ctx, case_b_started, &state(ctx));
    let calibration_new = captures.take("calibration-new-view");
    let surface_new = state(ctx);
    captures.crop = calibration_new["stats"]["width"]
        .as_f64()
        .zip(calibration_new["stats"]["height"].as_f64())
        .and_then(|(w, h)| canvas_crop(&surface_new, w, h));
    let (shots_b, moving_b) = fresh_pixels(&mut captures, "after-resource-reload");
    let held_ms = held.as_ref().and_then(|l| l["detail"]["heldAtMs"].as_i64());
    let retired_ms = recovered.as_ref().and_then(|l| l["ms"].as_i64());
    let released_ms = released_line
        .as_ref()
        .and_then(|l| l["detail"]["releasedAtMs"].as_i64());
    let resource_ordered = matches!((held_ms, retired_ms, released_ms), (Some(h), Some(r), Some(rel)) if h < reload_sent_ms as i64 && reload_sent_ms <= r && r < rel);
    let resource_checks = json!({
        "holdArmed": armed_hold["ok"] == true && armed_hold["result"]["armed"] == "floor-grain",
        "sameSchemeRequestHeld": held.as_ref().is_some_and(|l| l["detail"]["path"].as_str().is_some_and(|p| p.contains("floor-grain"))),
        "pendingInView": outstanding_assets(&pending_view) - outstanding_before == 1,
        "viewRetiredWhilePending": recovered.as_ref().is_some_and(|l| l["detail"]["retiredIncarnation"] == held_incarnation),
        "orderedOverlap": resource_ordered,
        "lateResponseToRetiredView": released_line.as_ref().is_some_and(|l| l["detail"]["incarnationActiveAtRelease"] == false && l["detail"]["releasedBy"] == "COMMAND"),
        "newViewHydratedAndApplied": new_ok && projection_new["equal"] == true,
        "nothingLateConsumed": after_release["asset"] == new_view["asset"] && outstanding_assets(&after_release) == 0,
        "applicationAlive": ui_after.iter().any(|u| u.pid == ui.pid),
        "freshChangingPixels": moving_b >= 2,
    });
    let resource_pass = resource_checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true))
        && interference_b["interfered"] == false;
    record(
        "reload-while-resource-pending",
        resource_pass,
        json!({
            "checks": resource_checks, "interference": interference_b, "outstandingBeforeHold": outstanding_before,
            "times": { "heldAtMs": held_ms, "reloadSentMs": reload_sent_ms, "viewRetiredAtMs": retired_ms, "releasedAtMs": released_ms },
            "held": held, "pendingView": { "generation": pending_view["generation"], "asset": pending_view["asset"] },
            "recovered": recovered, "newView": { "state": new_view["state"], "generation": new_view["generation"], "asset": new_view["asset"], "attestation": new_view["lastAttestation"] },
            "released": released_line, "afterRelease": after_release["asset"],
            // Contract (SPEC §18.5, D-0006): the retired view's content and caches
            // go with its native removal; C-04's one retained shell per recovery is
            // the accepted M1 item, measured here, not closed.
            "nativeWindowShells": { "before": shells_before, "after": shells_after, "recoveries": 1 },
            "pixels": shots_b,
        }),
    );

    let _ = crate::cleanup::resolve_qualification(ctx, "h11 qualification cleanup");
    let summary = json!({
        "issue": "H-11",
        "gate": "G15",
        "pass": cases.iter().all(|c| c["pass"] == true) && cases.len() == 2 && baseline_moving >= 2,
        "baselineChangingPixels": baseline_moving,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "fixture": fixture,
        "injected": "none: both overlaps hold real work (the WebGPU adapter request, the tauri:// response) at qualification-only barriers",
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
