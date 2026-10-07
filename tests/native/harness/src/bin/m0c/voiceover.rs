//! H-12 (G16 remediation): actual VoiceOver operation of the office, not AX
//! inspection or plain Tab. VoiceOver is turned on with its System Settings
//! switch (its Command-F5 shortcut is disabled on this Mac) and driven through
//! its own scripting interface, which the owner enables once in VoiceOver
//! Utility: `move` moves the VoiceOver cursor, `text under cursor` is
//! VoiceOver's own report of what its cursor is on, `perform action` is
//! VoiceOver activating that item, and `quit` turns VoiceOver off. The "2D
//! view" toggle is reached and activated this way, with the effect read
//! independently from the renderer lifecycle, before and after a fullscreen
//! enter/exit (the transition that once stranded keyboard focus, C-03). No
//! keystroke is sent to any app while VoiceOver runs. The owner's VoiceOver
//! state and output mute are restored on every path, followed by an ordinary
//! keyboard regression with VoiceOver off.
//!
//! A `tell application "VoiceOver"` starts VoiceOver if it is not running, so
//! every script is sent only while VoiceOver's process runs.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::run::run;

use crate::bridge_gates::{compare_projection, ensure_ui};
use crate::ctx::Ctx;

const TITLE: &str = "Threadspace";
const TARGET: &str = "2D view";
const KEY_TAB: &str = "48";
const SETTINGS_PANE: &str =
    "x-apple.systempreferences:com.apple.Accessibility-Settings.extension?VoiceOver";
const SCRIPTING_ENABLED: &str = "/private/var/db/Accessibility/.VoiceOverAppleScriptEnabled";

fn voiceover(ctx: &Ctx) -> Value {
    ctx.native.json(&["voiceover"])
}

fn enabled(state: &Value) -> bool {
    state["enabled"] == true
}

fn running(ctx: &Ctx) -> bool {
    voiceover(ctx)["pids"].as_array().is_some_and(|p| !p.is_empty())
}

/// One VoiceOver script, sent only while VoiceOver runs.
fn script(ctx: &Ctx, body: &str) -> Result<String, String> {
    if !running(ctx) {
        return Err("VoiceOver is not running; no script sent".into());
    }
    let out = run(
        "/usr/bin/osascript",
        &["-e", &format!("tell application \"VoiceOver\"\n{body}\nend tell")],
        Duration::from_secs(10),
    );
    if out.ok {
        Ok(out.stdout.trim().to_owned())
    } else {
        Err(out.stderr.trim().chars().take(200).collect())
    }
}

fn cursor_text(ctx: &Ctx) -> Result<String, String> {
    script(ctx, "return text under cursor of vo cursor")
}

fn wait_enabled(ctx: &Ctx, on: bool, timeout: Duration) -> (Value, bool) {
    let started = Instant::now();
    loop {
        let state = voiceover(ctx);
        let process_gone = state["pids"].as_array().is_some_and(Vec::is_empty);
        if enabled(&state) == on && (on || process_gone) {
            return (state, true);
        }
        if started.elapsed() >= timeout {
            return (state, false);
        }
        threadspace_harness::pause_ms(250);
    }
}

/// The System Settings switch (Accessibility > VoiceOver); the pane can be
/// slow to build its accessibility tree, so it is tried three times.
fn settings_switch(ctx: &Ctx, on: bool) -> Value {
    let wanted = i64::from(on);
    let mut attempts = Vec::new();
    for attempt in 0..3 {
        let opened = run("/usr/bin/open", &[SETTINGS_PANE], Duration::from_secs(15)).ok;
        threadspace_harness::pause_ms(3000);
        let read = ctx.native.json(&["ax-switch", "com.apple.systempreferences", "VoiceOver"]);
        let switched = if read["found"] == true && read["valueBefore"].as_i64() != Some(wanted) {
            ctx.native.json(&["ax-switch", "com.apple.systempreferences", "VoiceOver", "press"])
        } else {
            json!({ "pressed": false })
        };
        let (_, ok) = wait_enabled(ctx, on, Duration::from_secs(12));
        let _ = run("/usr/bin/osascript", &["-e", "tell application id \"com.apple.systempreferences\" to quit"], Duration::from_secs(10));
        attempts.push(json!({ "attempt": attempt, "settingsOpened": opened, "read": read, "switch": switched, "ok": ok }));
        if ok {
            return json!({ "method": "SETTINGS_SWITCH", "attempts": attempts, "ok": true });
        }
    }
    json!({ "method": "SETTINGS_SWITCH", "attempts": attempts, "ok": false })
}

/// Off: VoiceOver's own quit, then the settings switch if it is still on.
fn voiceover_off(ctx: &Ctx) -> Value {
    if !running(ctx) && !enabled(&voiceover(ctx)) {
        return json!({ "method": "ALREADY_OFF", "ok": true });
    }
    let quit = script(ctx, "quit");
    let (_, ok) = wait_enabled(ctx, false, Duration::from_secs(12));
    if ok {
        return json!({ "method": "VOICEOVER_QUIT", "quit": format!("{quit:?}"), "ok": true });
    }
    json!({ "method": "VOICEOVER_QUIT_THEN_SWITCH", "quit": format!("{quit:?}"), "switch": settings_switch(ctx, false) })
}

/// Each keystroke this runner sends, and each VoiceOver activation, requires
/// Threadspace in front.
fn guard(ctx: &Ctx, pid: u32) -> Result<(), String> {
    let front = ctx.native.json(&["ax-window", &pid.to_string()])["frontmostPid"].as_i64();
    if front == Some(i64::from(pid)) {
        Ok(())
    } else {
        Err(format!("stopped: Threadspace (pid {pid}) is not frontmost (frontmost pid {front:?})"))
    }
}

fn key(ctx: &Ctx, pid: u32, args: &[&str]) -> Result<Value, String> {
    guard(ctx, pid)?;
    let mut all = vec!["key"];
    all.extend_from_slice(args);
    Ok(ctx.native.json(&all))
}

fn output_muted() -> Option<bool> {
    let out = run("/usr/bin/osascript", &["-e", "output muted of (get volume settings)"], Duration::from_secs(5));
    match out.stdout.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn set_output_muted(muted: bool) -> bool {
    run("/usr/bin/osascript", &["-e", &format!("set volume output muted {muted}")], Duration::from_secs(5)).ok
}

fn presentation(ctx: &Ctx) -> Value {
    let state = ctx
        .app()
        .view_command("renderer:report-state", json!({}), Duration::from_secs(30))
        .map(|r| r["result"].clone())
        .unwrap_or_else(|e| json!({ "error": e }));
    json!({ "presentation": state["presentation"], "state": state["state"], "generation": state["generation"] })
}

/// Text kept in clear in the evidence: the toggle and the app's own fixed
/// controls. Anything else (fleet rows carry session names) is a digest,
/// which still shows the VoiceOver cursor moving.
fn public_text(text: &str) -> Value {
    const FIXED: [&str; 9] = [
        "Mark handled", "Close", "Refresh", "Allow notifications", "Stop observation",
        "Enable observation", "Allow Terminal access", "Attention", "Diagnostics",
    ];
    if text.contains(TARGET) || text.is_empty() || FIXED.iter().any(|f| text.starts_with(f)) {
        json!(text)
    } else {
        json!(format!("digest:{}", &threadspace_harness::evidence::sha256_text(text)[..12]))
    }
}

/// VoiceOver names the toggle itself, not a group around it.
fn is_toggle(text: &str) -> bool {
    let lower = text.to_lowercase();
    text.contains(TARGET) && (lower.contains("toggle") || lower.contains("button") || lower.contains("checkbox"))
}

/// The Office landmark region that holds the scene and its controls.
fn is_office_region(text: &str) -> bool {
    text.starts_with("Office") && text.to_lowercase().contains("region")
}

/// VoiceOver's cursor reached a window that is not Threadspace's.
fn left_app(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("window") && !text.contains("Threadspace")
}

fn step(ctx: &Ctx, pid: u32, round: &str, command: &str) -> Value {
    let text = cursor_text(ctx);
    let focused = ctx.native.json(&["ax-focused", &pid.to_string()]);
    json!({
        "round": round, "command": command, "atMs": threadspace_harness::now_ms(),
        "voiceOverCursor": text.as_deref().map(public_text).unwrap_or_else(|e| json!({ "error": e })),
        "onTarget": text.as_deref().is_ok_and(is_toggle),
        "axFocused": { "role": focused["role"], "label": public_text(focused["label"].as_str().unwrap_or_default()) },
    })
}

/// Raw VoiceOver cursor texts go only to the owner's private folder, outside
/// the repository, so a stalled walk can be diagnosed without publishing the
/// session names the fleet shows.
fn private_trace(text: &str) {
    if let Some(home) = threadspace_relay::paths::home_dir() {
        let path = home.join("Documents/Tools/Threadspace-private/h12-voiceover-cursor-trace.txt");
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            use std::io::Write;
            let _ = writeln!(file, "{} {text}", threadspace_harness::now_ms());
        }
    }
}

/// One VoiceOver `move`, recorded with VoiceOver's cursor text after it.
fn vo_move(ctx: &Ctx, pid: u32, round: &str, command: &str, steps: &mut Vec<Value>) -> Result<(Value, String), String> {
    script(ctx, &format!("tell vo cursor to move {command}"))?;
    threadspace_harness::pause_ms(250);
    let entry = step(ctx, pid, round, &format!("move {command}"));
    let text = cursor_text(ctx).unwrap_or_default();
    private_trace(&format!("{round} move {command} -> {text}"));
    steps.push(entry.clone());
    if left_app(&text) {
        return Err("stopped: VoiceOver's cursor left the Threadspace window; nothing was activated".into());
    }
    Ok((entry, text))
}

/// From the Office region: into it, to its last item (the status), then left
/// and into until VoiceOver is on the toggle. VoiceOver will not move right
/// past the scene canvas, so the toggle is reached from the region's end.
fn descend_office(ctx: &Ctx, pid: u32, round: &str, steps: &mut Vec<Value>) -> Result<Option<Value>, String> {
    let (entry, _) = vo_move(ctx, pid, round, "into item", steps)?;
    if entry["onTarget"] == true {
        return Ok(Some(entry));
    }
    let (entry, mut last) = vo_move(ctx, pid, round, "to last item", steps)?;
    if entry["onTarget"] == true {
        return Ok(Some(entry));
    }
    for _ in 0..8 {
        let (entry, text) = vo_move(ctx, pid, round, "left", steps)?;
        if entry["onTarget"] == true {
            return Ok(Some(entry));
        }
        let (entry, text) = if text == last || text.is_empty() || !text.contains(TARGET) {
            vo_move(ctx, pid, round, "into item", steps)?
        } else {
            (entry, text)
        };
        if entry["onTarget"] == true {
            return Ok(Some(entry));
        }
        last = text;
    }
    Ok(None)
}

/// VoiceOver to the toggle from wherever its cursor is: out to the web
/// view's scroll area, into the content, to its last item, then left back
/// through the short Inspector and Diagnostics panel to the Office region,
/// and down to the toggle. From the far end the route is short in both 3D
/// and 2D presentation (the fleet and 2D lists come before the region).
fn reach_toggle(ctx: &Ctx, pid: u32, round: &str, steps: &mut Vec<Value>) -> Result<Option<Value>, String> {
    let mut text = cursor_text(ctx).unwrap_or_default();
    for _ in 0..8 {
        if is_office_region(&text) {
            return descend_office(ctx, pid, round, steps);
        }
        if text == "scroll area" {
            break;
        }
        let (_, now) = vo_move(ctx, pid, round, "out of item", steps)?;
        if now == text {
            break;
        }
        text = now;
    }
    if text == "scroll area" {
        vo_move(ctx, pid, round, "into item", steps)?;
    }
    text = vo_move(ctx, pid, round, "to last item", steps)?.1;
    for _ in 0..200 {
        if is_toggle(&text) {
            return Ok(steps.last().cloned());
        }
        if is_office_region(&text) {
            return descend_office(ctx, pid, round, steps);
        }
        let (_, now) = vo_move(ctx, pid, round, "left", steps)?;
        // The start of a group: VoiceOver steps out of it and goes on.
        text = if now == text {
            vo_move(ctx, pid, round, "out of item", steps)?.1
        } else {
            now
        };
    }
    Ok(None)
}

/// VoiceOver activates the item under its cursor, only when VoiceOver itself
/// reports the toggle there and Threadspace is in front.
fn activate(ctx: &Ctx, pid: u32) -> Result<Value, String> {
    guard(ctx, pid)?;
    let text = cursor_text(ctx)?;
    if !is_toggle(&text) {
        return Err(format!("stopped: VoiceOver is not on the toggle ({})", public_text(&text)));
    }
    script(ctx, "tell vo cursor to perform action")?;
    Ok(json!({ "voiceOverCursor": text, "atMs": threadspace_harness::now_ms() }))
}

fn moved(steps: &[Value], round: &str) -> bool {
    let seen: std::collections::BTreeSet<String> = steps
        .iter()
        .filter(|s| s["round"] == round)
        .map(|s| s["voiceOverCursor"].to_string())
        .collect();
    seen.len() >= 2
}

pub fn smoke(ctx: &Ctx) -> Result<Value, String> {
    let scripting = std::path::Path::new(SCRIPTING_ENABLED).exists();
    if !scripting {
        return Err("VoiceOver scripting is off: enable \"Allow VoiceOver to be controlled with AppleScript\" in VoiceOver Utility first".into());
    }
    let run_dir = Run::create(&ctx.evidence_root(), "remediation/h12-voiceover", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let gate = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    run_dir.write_json("idle-gate.json", &json!(gate)).map_err(|e| e.to_string())?;
    run_dir.write_json("environment.json", &ctx.environment()).map_err(|e| e.to_string())?;
    let _gui = ctx.gui("h12 VoiceOver smoke")?;
    let ui = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    let prior = voiceover(ctx);
    let prior_muted = output_muted();
    run_dir.write_json("prior-state.json", &json!({ "voiceOver": { "enabled": prior["enabled"], "pids": prior["pids"] }, "outputMuted": prior_muted, "voiceOverScripting": scripting })).map_err(|e| e.to_string())?;
    if enabled(&prior) || running(ctx) {
        return Err("VoiceOver is already on: the smoke changes nothing it could not restore exactly".into());
    }
    // Speech is not part of the proof; VoiceOver's cursor text is.
    let muted_for_run = prior_muted == Some(false) && set_output_muted(true);

    let mut steps: Vec<Value> = Vec::new();
    let body = (|| -> Result<Value, String> {
        ctx.native.ax_action(pid, "raise", Some(TITLE));
        let started_vo = settings_switch(ctx, true);
        if started_vo["ok"] != true {
            return Err(format!("VoiceOver did not start: {started_vo}"));
        }
        // Ready when VoiceOver answers its own scripting interface (the first
        // query can wait on the owner's one-time Automation consent).
        let wait = Instant::now();
        let ready = loop {
            match cursor_text(ctx) {
                Ok(text) => break json!({ "ready": true, "afterMs": wait.elapsed().as_millis() as u64, "firstCursor": public_text(&text) }),
                Err(error) if wait.elapsed() >= Duration::from_secs(90) => return Err(format!("VoiceOver never answered: {error}")),
                Err(_) => threadspace_harness::pause_ms(500),
            }
        };
        ctx.native.ax_action(pid, "raise", Some(TITLE));
        threadspace_harness::pause_ms(1500);
        guard(ctx, pid)?;
        let before = presentation(ctx);

        steps.push(step(ctx, pid, "first", "start"));
        let first = reach_toggle(ctx, pid, "first", &mut steps)?
            .ok_or("VoiceOver never reached the 2D view toggle; nothing was activated")?;
        let activated_1 = activate(ctx, pid)?;
        threadspace_harness::pause_ms(2500);
        let after_1 = presentation(ctx);

        let full = ctx.native.ax_action(pid, "fullscreen", Some(TITLE));
        threadspace_harness::pause_ms(1500);
        let exit = ctx.native.ax_action(pid, "exit-fullscreen", Some(TITLE));
        threadspace_harness::pause_ms(2000);
        guard(ctx, pid)?;

        // After the transition: VoiceOver climbs out to the Office region and
        // back down to the toggle.
        steps.push(step(ctx, pid, "after-transition", "after transition"));
        let second = reach_toggle(ctx, pid, "after-transition", &mut steps)?
            .ok_or("after the transition VoiceOver never reached the toggle; nothing was activated")?;
        let activated_2 = activate(ctx, pid)?;
        threadspace_harness::pause_ms(3000);
        let after_2 = presentation(ctx);
        Ok(json!({
            "voiceOverStart": started_vo, "voiceOverReady": ready, "presentationBefore": before,
            "firstReached": first, "activate1": activated_1, "presentationAfterFirst": after_1,
            "fullscreen": { "entered": full["after"]["fullScreen"], "exited": exit["after"]["fullScreen"] },
            "secondReached": second, "activate2": activated_2, "presentationAfterSecond": after_2,
        }))
    })();

    // Restore the owner's state whatever happened above.
    let off = voiceover_off(ctx);
    let (final_vo, vo_off) = wait_enabled(ctx, false, Duration::from_secs(15));
    let restored_mute = if muted_for_run { set_output_muted(false) } else { true };
    let final_muted = output_muted();
    let restored = json!({
        "voiceOver": off, "voiceOverOffAfter": vo_off, "voiceOverPidsAfter": final_vo["pids"],
        "outputMutedBefore": prior_muted, "outputMutedAfter": final_muted, "muteRestored": restored_mute && final_muted == prior_muted,
    });
    run_dir.write_json("restore.json", &restored).map_err(|e| e.to_string())?;
    run_dir.write_json("navigation.json", &json!(steps)).map_err(|e| e.to_string())?;
    let observed = body?;
    if !vo_off {
        return Err(format!("VoiceOver could not be turned off again: {restored}"));
    }

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
    let reverse_ok = focus_path.len() >= 2 && back["label"] == focus_path[focus_path.len() - 2]["label"];
    let projection = compare_projection(ctx, "after-voiceover");

    let checks = json!({
        "priorStateRecordedOff": prior["enabled"] == false,
        "voiceOverActiveAndAnswering": observed["voiceOverReady"]["ready"] == true,
        "voiceOverMovedItsCursor": moved(&steps, "first"),
        "voiceOverCursorOnLabelledControl": observed["firstReached"]["onTarget"] == true,
        "activatedByVoiceOverChangedPresentation": observed["presentationBefore"]["presentation"] == "3d" && observed["presentationAfterFirst"]["presentation"] == "2d",
        "windowTransition": observed["fullscreen"]["entered"] == true && observed["fullscreen"]["exited"] == false,
        "voiceOverMovedItsCursorAfterTransition": moved(&steps, "after-transition"),
        "voiceOverCursorOnControlAfterTransition": observed["secondReached"]["onTarget"] == true,
        "activationAfterTransitionRestored3d": observed["presentationAfterSecond"]["presentation"] == "3d",
        "voiceOverRestoredOff": restored["voiceOverOffAfter"] == true,
        "muteRestored": restored["muteRestored"] == true,
        "keyboardRegression": distinct.len() >= 5 && reverse_ok,
        "projectionEqual": projection["equal"] == true,
    });
    let pass = checks.as_object().is_some_and(|m| m.values().all(|v| v == true));
    let summary = json!({
        "issue": "H-12",
        "gate": "G16",
        "pass": pass,
        "checks": checks,
        "observed": observed,
        "restored": restored,
        "keyboard": {
            "path": focus_path.iter().map(|f| json!({ "role": f["role"], "label": public_text(f["label"].as_str().unwrap_or_default()) })).collect::<Vec<_>>(),
            "shiftTabLandsOn": { "role": back["role"], "label": public_text(back["label"].as_str().unwrap_or_default()) },
            "distinctLabelled": distinct.len(),
        },
        "navigationSteps": steps.len(),
        "ui": ui,
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": { "pass": pass, "checks": checks }, "dir": run_dir.dir }))
}
