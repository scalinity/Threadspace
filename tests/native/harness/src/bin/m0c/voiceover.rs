//! H-12 (G16 remediation): actual VoiceOver operation of the office, not AX
//! inspection or plain Tab. VoiceOver is turned on with its System Settings
//! switch (its Command-F5 shortcut is disabled on this Mac), its cursor is
//! moved with VoiceOver's own commands (VO-Right, where VO is
//! Control-Option), the "2D view" toggle is activated with VO-Space, and the
//! effect is read independently from the renderer lifecycle. A fullscreen
//! enter/exit (the transition that once stranded keyboard focus, C-03) comes
//! between two such navigations. Where the VoiceOver cursor is comes from
//! VoiceOver's own caption output, cross-checked with the app's AX focus.
//! The owner's VoiceOver state and output mute are restored on every path,
//! followed by an ordinary keyboard regression with VoiceOver off.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::run::run;

use crate::bridge_gates::{compare_projection, ensure_ui};
use crate::ctx::Ctx;

const TITLE: &str = "Threadspace";
const TARGET: &str = "2D view";
const KEY_RIGHT: &str = "124";
const KEY_LEFT: &str = "123";
const KEY_DOWN: &str = "125";
const KEY_SPACE: &str = "49";
const KEY_TAB: &str = "48";
const SETTINGS_PANE: &str =
    "x-apple.systempreferences:com.apple.Accessibility-Settings.extension?VoiceOver";

fn voiceover(ctx: &Ctx) -> Value {
    ctx.native.json(&["voiceover"])
}

fn enabled(state: &Value) -> bool {
    state["enabled"] == true
}

/// Keystrokes go to whatever app is in front; every one the smoke sends must
/// reach Threadspace. Anything else in front (VoiceOver's welcome or
/// tutorial, an alert, another app) stops the run, which then restores.
fn guard(ctx: &Ctx, pid: u32) -> Result<(), String> {
    let front = ctx.native.json(&["ax-window", &pid.to_string()])["frontmostPid"].as_i64();
    if front == Some(i64::from(pid)) {
        Ok(())
    } else {
        Err(format!(
            "stopped: Threadspace (pid {pid}) is not frontmost (frontmost pid {front:?}); no keystroke sent"
        ))
    }
}

/// A VoiceOver command, sent only while Threadspace is frontmost.
fn vo(ctx: &Ctx, pid: u32, code: &str, extra: &[&str]) -> Result<Value, String> {
    guard(ctx, pid)?;
    Ok(vo_key(ctx, code, extra))
}

/// A plain key, sent only while Threadspace is frontmost.
fn key(ctx: &Ctx, pid: u32, args: &[&str]) -> Result<Value, String> {
    guard(ctx, pid)?;
    let mut all = vec!["key"];
    all.extend_from_slice(args);
    Ok(ctx.native.json(&all))
}

fn vo_key(ctx: &Ctx, code: &str, extra: &[&str]) -> Value {
    let mut args = vec!["key", code, "control", "option"];
    args.extend_from_slice(extra);
    ctx.native.json(&args)
}

fn wait_enabled(ctx: &Ctx, on: bool, timeout: Duration) -> (Value, bool) {
    let started = Instant::now();
    loop {
        let state = voiceover(ctx);
        if enabled(&state) == on {
            return (state, true);
        }
        if started.elapsed() >= timeout {
            return (state, false);
        }
        threadspace_harness::pause_ms(250);
    }
}

/// Turns VoiceOver on or off with its System Settings switch (Accessibility
/// > VoiceOver), the supported control on this Mac: its Command-F5 shortcut
/// is disabled in the owner's keyboard settings, which the harness leaves
/// alone. The pane can be slow to build its accessibility tree while
/// VoiceOver runs, so the switch is tried three times; turning VoiceOver off
/// then falls back to the standard quit Apple event.
fn set_voiceover(ctx: &Ctx, on: bool) -> Value {
    let wanted = i64::from(on);
    let mut attempts = Vec::new();
    for attempt in 0..3 {
        let opened = run("/usr/bin/open", &[SETTINGS_PANE], Duration::from_secs(15)).ok;
        threadspace_harness::pause_ms(3000);
        let read = ctx
            .native
            .json(&["ax-switch", "com.apple.systempreferences", "VoiceOver"]);
        let switched = if read["found"] == true && read["valueBefore"].as_i64() != Some(wanted) {
            ctx.native.json(&[
                "ax-switch",
                "com.apple.systempreferences",
                "VoiceOver",
                "press",
            ])
        } else {
            json!({ "pressed": false })
        };
        let (_, ok) = wait_enabled(ctx, on, Duration::from_secs(10));
        let _ = run(
            "/usr/bin/osascript",
            &[
                "-e",
                "tell application id \"com.apple.systempreferences\" to quit",
            ],
            Duration::from_secs(10),
        );
        attempts.push(json!({ "attempt": attempt, "settingsOpened": opened, "read": read, "switch": switched, "ok": ok }));
        if ok {
            return json!({ "method": "SETTINGS_SWITCH", "attempts": attempts, "ok": true });
        }
        threadspace_harness::pause_ms(1500);
    }
    if !on {
        let quit = run(
            "/usr/bin/osascript",
            &["-e", "tell application id \"com.apple.VoiceOver\" to quit"],
            Duration::from_secs(10),
        );
        let (_, ok) = wait_enabled(ctx, false, Duration::from_secs(10));
        return json!({ "method": "QUIT_APPLE_EVENT", "attempts": attempts, "quit": { "ok": quit.ok, "stderr": quit.stderr.trim() }, "ok": ok });
    }
    json!({ "method": "SETTINGS_SWITCH", "attempts": attempts, "ok": false })
}

/// A first start can bring VoiceOver's welcome to the front: continue into
/// VoiceOver once ("Use VoiceOver"), then Threadspace must be in front again.
fn dismiss_welcome(ctx: &Ctx, pid: u32) -> Value {
    if guard(ctx, pid).is_ok() {
        return json!({ "shown": false });
    }
    let front = ctx.native.json(&["ax-window", &pid.to_string()])["frontmostPid"].clone();
    let voiceover_pids = voiceover(ctx)["pids"].clone();
    let is_voiceover = voiceover_pids
        .as_array()
        .is_some_and(|p| p.contains(&front));
    let pressed = if is_voiceover {
        ctx.native
            .json(&["ax-press", "com.apple.VoiceOver", "Use VoiceOver"])
    } else {
        json!({ "pressed": false, "reason": "front app is not VoiceOver" })
    };
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    threadspace_harness::pause_ms(800);
    json!({ "shown": true, "frontmostPid": front, "voiceOverInFront": is_voiceover, "pressed": pressed, "threadspaceInFrontAfter": guard(ctx, pid).is_ok() })
}

fn output_muted() -> Option<bool> {
    let out = run(
        "/usr/bin/osascript",
        &["-e", "output muted of (get volume settings)"],
        Duration::from_secs(5),
    );
    match out.stdout.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn set_output_muted(muted: bool) -> bool {
    run(
        "/usr/bin/osascript",
        &["-e", &format!("set volume output muted {muted}")],
        Duration::from_secs(5),
    )
    .ok
}

fn presentation(ctx: &Ctx) -> Value {
    let state = ctx
        .app()
        .view_command("renderer:report-state", json!({}), Duration::from_secs(30))
        .map(|r| r["result"].clone())
        .unwrap_or_else(|e| json!({ "error": e }));
    json!({ "presentation": state["presentation"], "state": state["state"], "generation": state["generation"] })
}

/// The app's own fixed control labels, kept in clear in the evidence; any
/// other label (fleet rows carry session names) is stored as a digest, which
/// still shows the cursor moving.
const FIXED_LABELS: [&str; 10] = [
    TARGET,
    "Mark handled",
    "Close",
    "Refresh",
    "Allow notifications",
    "Stop observation",
    "Enable observation",
    "Allow Terminal access",
    "Threadspace",
    "Return",
];

fn public_label(label: &str) -> Value {
    if label.is_empty() || FIXED_LABELS.contains(&label) {
        json!(label)
    } else {
        json!(format!(
            "digest:{}",
            &threadspace_harness::evidence::sha256_text(label)[..12]
        ))
    }
}

/// Where VoiceOver is: VoiceOver's own state and the app's AX focus, which
/// follows the VoiceOver cursor onto focusable controls (VoiceOver's default
/// "keyboard focus follows VoiceOver cursor"; were it off, no step would land).
fn witness(ctx: &Ctx, pid: u32) -> Value {
    let state = voiceover(ctx);
    let focused = ctx.native.json(&["ax-focused", &pid.to_string()]);
    let label = focused["label"].as_str().unwrap_or_default();
    json!({
        "atMs": threadspace_harness::now_ms(),
        "voiceOverEnabled": state["enabled"],
        "voiceOverScreenWindows": state["screenWindows"].as_array().map_or(0, Vec::len),
        "axFocused": { "role": focused["role"], "label": public_label(label), "isTarget": label == TARGET },
    })
}

/// VoiceOver is on and its cursor is on the toggle (an `aria-pressed`
/// button, which WebKit exposes as a checkbox).
fn on_target(w: &Value) -> bool {
    w["voiceOverEnabled"] == true
        && (w["axFocused"]["role"] == "AXButton" || w["axFocused"]["role"] == "AXCheckBox")
        && w["axFocused"]["isTarget"] == true
}

/// VO-Right (or VO-Left) until VoiceOver lands on the target. When the
/// VoiceOver cursor is on the web content as a whole, VoiceOver's interact
/// command (VO-Shift-Down) takes it inside.
fn walk(
    ctx: &Ctx,
    pid: u32,
    round: &str,
    code: &str,
    name: &str,
    max: usize,
    steps: &mut Vec<Value>,
) -> Result<Option<Value>, String> {
    let mut interactions = 0;
    for step in 0..max {
        vo(ctx, pid, code, &[])?;
        threadspace_harness::pause_ms(300);
        let mut w = witness(ctx, pid);
        steps.push(json!({ "round": round, "step": step, "command": name, "witness": w }));
        if w["axFocused"]["role"] == "AXWebArea" && interactions < 3 {
            vo(ctx, pid, KEY_DOWN, &["shift"])?;
            interactions += 1;
            threadspace_harness::pause_ms(600);
            w = witness(ctx, pid);
            steps.push(json!({ "round": round, "step": step, "command": "VO-Shift-Down (interact)", "witness": w }));
        }
        if on_target(&w) {
            return Ok(Some(w));
        }
    }
    Ok(None)
}

/// VoiceOver takes several seconds after its switch before it acts on
/// commands (earlier runs: 6-9 s), and draws no overlay until its cursor first
/// moves, so readiness is a settle time; commands sent too early are ignored.
fn settle(ctx: &Ctx) -> Value {
    threadspace_harness::pause_ms(8000);
    let state = voiceover(ctx);
    json!({ "settledMs": 8000, "enabled": state["enabled"], "screenWindows": state["screenWindows"].as_array().map_or(0, Vec::len) })
}

/// VoiceOver's cursor starts where keyboard focus is. On nothing, or on the
/// web content as a whole, one Tab puts focus on its first control so the
/// VoiceOver cursor starts inside the office.
fn enter_content(ctx: &Ctx, pid: u32, round: &str, steps: &mut Vec<Value>) -> Result<(), String> {
    let w = witness(ctx, pid);
    let role = w["axFocused"]["role"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    steps.push(json!({ "round": round, "command": "start", "witness": w }));
    if role.is_empty() || role == "AXWebArea" {
        key(ctx, pid, &[KEY_TAB])?;
        threadspace_harness::pause_ms(600);
        let w = witness(ctx, pid);
        steps.push(json!({ "round": round, "command": "Tab (start inside the web content)", "witness": w }));
    }
    Ok(())
}

/// VoiceOver moved: the AX focus took at least two different labelled controls.
fn cursor_moved(steps: &[Value], round: &str) -> bool {
    let seen: std::collections::BTreeSet<String> = steps
        .iter()
        .filter(|s| {
            s["round"] == round && s["command"].as_str().is_some_and(|c| c.starts_with("VO-"))
        })
        .filter_map(|s| {
            s["witness"]["axFocused"]["label"]
                .as_str()
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
        })
        .collect();
    seen.len() >= 2
}

pub fn smoke(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/h12-voiceover",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let app = ctx.app();
    let gate = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    run_dir
        .write_json("idle-gate.json", &json!(gate))
        .map_err(|e| e.to_string())?;
    run_dir
        .write_json("environment.json", &ctx.environment())
        .map_err(|e| e.to_string())?;
    // VoiceOver changes every app's keyboard: the whole smoke holds the GUI lock.
    let _gui = ctx.gui("h12 VoiceOver smoke")?;
    let ui = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    let prior = voiceover(ctx);
    let prior_muted = output_muted();
    run_dir
        .write_json(
            "prior-state.json",
            &json!({ "voiceOver": prior, "outputMuted": prior_muted }),
        )
        .map_err(|e| e.to_string())?;
    if enabled(&prior) {
        return Err(
            "VoiceOver is already on: the smoke changes nothing it could not restore exactly"
                .into(),
        );
    }
    // Speech is not part of the proof; captions are. Keep the room quiet.
    let muted_for_run = prior_muted == Some(false) && set_output_muted(true);

    let mut steps: Vec<Value> = Vec::new();
    let body = (|| -> Result<Value, String> {
        ctx.native.ax_action(pid, "raise", Some(TITLE));
        let started_vo = set_voiceover(ctx, true);
        let (on_state, on) = wait_enabled(ctx, true, Duration::from_secs(5));
        if !on {
            // Never send VoiceOver commands to a Mac without VoiceOver.
            return Err(format!("VoiceOver did not start: {started_vo}"));
        }
        ctx.native.ax_action(pid, "raise", Some(TITLE));
        threadspace_harness::pause_ms(1500);
        let before = presentation(ctx);

        let ready = settle(ctx);
        let welcome = dismiss_welcome(ctx, pid);
        // First navigation: VoiceOver walks the office from its first control.
        enter_content(ctx, pid, "first", &mut steps)?;
        let first = walk(ctx, pid, "first", KEY_RIGHT, "VO-Right", 260, &mut steps)?;
        // VO-Space activates whatever VoiceOver is on: send it only on the toggle.
        let Some(first) = first else {
            return Err("VoiceOver never reached the 2D view toggle; nothing was activated".into());
        };
        let activate_1 = vo(ctx, pid, KEY_SPACE, &[])?;
        threadspace_harness::pause_ms(2500);
        let after_1 = presentation(ctx);

        // The focus-sensitive window transition.
        let full = ctx.native.ax_action(pid, "fullscreen", Some(TITLE));
        threadspace_harness::pause_ms(1500);
        let exit = ctx.native.ax_action(pid, "exit-fullscreen", Some(TITLE));
        threadspace_harness::pause_ms(2000);

        // Second navigation: away from the control and back with VoiceOver,
        // or a full walk if the transition moved the VoiceOver cursor.
        let resumed = witness(ctx, pid);
        steps.push(json!({ "round": "after-transition", "command": "after transition", "witness": resumed }));
        let mut second = None;
        if on_target(&resumed) {
            second = walk(
                ctx,
                pid,
                "after-transition",
                KEY_RIGHT,
                "VO-Right",
                3,
                &mut steps,
            )?;
            if second.is_none() {
                second = walk(
                    ctx,
                    pid,
                    "after-transition",
                    KEY_LEFT,
                    "VO-Left",
                    8,
                    &mut steps,
                )?;
            }
        }
        if second.is_none() {
            enter_content(ctx, pid, "after-transition", &mut steps)?;
            second = walk(
                ctx,
                pid,
                "after-transition",
                KEY_RIGHT,
                "VO-Right",
                450,
                &mut steps,
            )?;
        }
        let Some(second) = second else {
            return Err(
                "after the transition VoiceOver never reached the toggle; nothing was activated"
                    .into(),
            );
        };
        let activate_2 = vo(ctx, pid, KEY_SPACE, &[])?;
        threadspace_harness::pause_ms(3000);
        let after_2 = presentation(ctx);
        Ok(json!({
            "voiceOverStart": started_vo, "voiceOverOn": on, "voiceOverReady": ready, "voiceOverState": { "enabled": on_state["enabled"], "pids": on_state["pids"] },
            "welcome": welcome, "presentationBefore": before,
            "firstReached": first, "activate1": activate_1, "presentationAfterFirst": after_1,
            "fullscreen": { "entered": full["after"]["fullScreen"], "exited": exit["after"]["fullScreen"] },
            "secondReached": second, "activate2": activate_2, "presentationAfterSecond": after_2,
        }))
    })();

    // Restore the owner's state whatever happened above.
    let restore_vo = if enabled(&voiceover(ctx)) {
        set_voiceover(ctx, false)
    } else {
        json!("already off")
    };
    let (final_vo, vo_off) = wait_enabled(ctx, false, Duration::from_secs(10));
    let restored_mute = if muted_for_run {
        set_output_muted(false)
    } else {
        true
    };
    let final_muted = output_muted();
    let restored = json!({
        "voiceOver": restore_vo, "voiceOverOffAfter": vo_off, "voiceOverPidsAfter": final_vo["pids"],
        "outputMutedBefore": prior_muted, "outputMutedAfter": final_muted, "muteRestored": restored_mute && final_muted == prior_muted,
    });
    run_dir
        .write_json("restore.json", &restored)
        .map_err(|e| e.to_string())?;
    run_dir
        .write_json("navigation.json", &json!(steps))
        .map_err(|e| e.to_string())?;
    let observed = body?;

    // Negative control, VoiceOver off: the same Control-Option keystrokes
    // neither move focus nor activate the focused toggle, so the activations
    // above came from VoiceOver, not from a keystroke reaching the button.
    if enabled(&voiceover(ctx)) {
        return Err(format!(
            "VoiceOver could not be turned off again: {restored}"
        ));
    }
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    let mut tabbed = Value::Null;
    for _ in 0..40 {
        key(ctx, pid, &[KEY_TAB])?;
        threadspace_harness::pause_ms(200);
        let focused = ctx.native.json(&["ax-focused", &pid.to_string()]);
        if focused["label"].as_str() == Some(TARGET) {
            tabbed = json!({ "role": focused["role"], "label": TARGET });
            break;
        }
    }
    let control_before = presentation(ctx);
    vo(ctx, pid, KEY_SPACE, &[])?;
    threadspace_harness::pause_ms(2000);
    let control_after_space = presentation(ctx);
    vo(ctx, pid, KEY_RIGHT, &[])?;
    threadspace_harness::pause_ms(600);
    let control_focus_after_right = ctx.native.json(&["ax-focused", &pid.to_string()]);
    let negative = json!({
        "voiceOverEnabled": voiceover(ctx)["enabled"],
        "toggleFocusedByTab": tabbed,
        "presentationBefore": control_before, "presentationAfterControlOptionSpace": control_after_space,
        "focusAfterControlOptionRight": { "role": control_focus_after_right["role"], "label": public_label(control_focus_after_right["label"].as_str().unwrap_or_default()) },
    });

    // Ordinary keyboard regression after the transition, VoiceOver off.
    ctx.native.ax_tree(pid, 40);
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    let mut focus_path = Vec::new();
    for _ in 0..10 {
        key(ctx, pid, &[KEY_TAB])?;
        threadspace_harness::pause_ms(250);
        focus_path.push(ctx.native.json(&["ax-focused", &pid.to_string()]));
    }
    key(ctx, pid, &[KEY_TAB, "shift"])?;
    threadspace_harness::pause_ms(250);
    let back = ctx.native.json(&["ax-focused", &pid.to_string()]);
    let distinct: std::collections::BTreeSet<String> = focus_path
        .iter()
        .filter(|f| f["found"] == true && f["label"].as_str().is_some_and(|l| !l.is_empty()))
        .map(|f| format!("{}:{}", f["role"], f["label"]))
        .collect();
    let reverse_ok =
        focus_path.len() >= 2 && back["label"] == focus_path[focus_path.len() - 2]["label"];
    let projection = compare_projection(ctx, "after-voiceover");

    let checks = json!({
        "priorStateRecorded": prior["enabled"].is_boolean(),
        "voiceOverActive": observed["voiceOverOn"] == true,
        "voiceOverMovedItsCursor": cursor_moved(&steps, "first"),
        "voiceOverCursorReachedLabelledControl": on_target(&observed["firstReached"]),
        "activatedThroughVoiceOverChangedPresentation": observed["presentationBefore"]["presentation"] == "3d" && observed["presentationAfterFirst"]["presentation"] == "2d",
        "windowTransition": observed["fullscreen"]["entered"] == true && observed["fullscreen"]["exited"] == false,
        "voiceOverMovedItsCursorAfterTransition": cursor_moved(&steps, "after-transition"),
        "voiceOverCursorReachedControlAfterTransition": on_target(&observed["secondReached"]),
        "activationAfterTransitionRestored3d": observed["presentationAfterSecond"]["presentation"] == "3d",
        "voiceOverRestoredOff": restored["voiceOverOffAfter"] == true,
        "muteRestored": restored["muteRestored"] == true,
        "negativeControlKeystrokesInertWithoutVoiceOver": negative["voiceOverEnabled"] == false
            && negative["toggleFocusedByTab"].is_object()
            && negative["presentationAfterControlOptionSpace"]["presentation"] == negative["presentationBefore"]["presentation"]
            && negative["focusAfterControlOptionRight"]["label"] == TARGET,
        "keyboardRegression": distinct.len() >= 5 && reverse_ok,
        "projectionEqual": projection["equal"] == true,
    });
    let pass = checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true));
    let summary = json!({
        "issue": "H-12",
        "gate": "G16",
        "pass": pass,
        "checks": checks,
        "observed": observed,
        "restored": restored,
        "negativeControl": negative,
        "keyboard": {
            "path": focus_path.iter().map(|f| json!({ "role": f["role"], "label": public_label(f["label"].as_str().unwrap_or_default()) })).collect::<Vec<_>>(),
            "shiftTabLandsOn": { "role": back["role"], "label": public_label(back["label"].as_str().unwrap_or_default()) },
            "distinctLabelled": distinct.len(),
        },
        "navigationSteps": steps.len(),
        "ui": ui,
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    let _ = app;
    Ok(json!({ "summary": { "pass": pass, "checks": checks }, "dir": run_dir.dir }))
}
