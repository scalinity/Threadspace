//! G16 native window, display and accessibility matrix on the installed
//! production bundle, through public Accessibility and input APIs. Every
//! step holds the shared GUI lock. System settings the run changes (Reduce
//! Motion) are restored to their original value before it ends.

use std::time::Duration;

use serde_json::{Value, json};
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::native::Native;
use threadspace_harness::run::run;

use crate::bridge_gates::{compare_projection, ensure_ui};
use crate::ctx::Ctx;

const TITLE: &str = "Threadspace";

fn frame(window: &Value) -> (f64, f64, f64, f64) {
    let f = &window["frame"];
    (
        f["x"].as_f64().unwrap_or(0.0),
        f["y"].as_f64().unwrap_or(0.0),
        f["width"].as_f64().unwrap_or(0.0),
        f["height"].as_f64().unwrap_or(0.0),
    )
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn set_frame(ctx: &Ctx, pid: u32, x: f64, y: f64, w: f64, h: f64) -> Value {
    ctx.native.json(&["ax-action", &pid.to_string(), "set-frame", &x.to_string(), &y.to_string(), &w.to_string(), &h.to_string(), TITLE])
}

fn reduce_motion(ctx: &Ctx) -> Option<bool> {
    ctx.companion().diagnostics().ok()?["accessibilityPreferences"]["reduceMotion"].as_bool()
}

fn open_settings(pane: &str) -> bool {
    run("/usr/bin/open", &[pane], Duration::from_secs(15)).ok
}

fn quit_settings() {
    let _ = run("/usr/bin/osascript", &["-e", "tell application id \"com.apple.systempreferences\" to quit"], Duration::from_secs(10));
}

pub fn matrix(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g16-window", ctx.channel_name()).map_err(|e| e.to_string())?;
    let app = ctx.app();
    let mut checks: Vec<Value> = Vec::new();
    let mut check = |name: &str, pass: bool, detail: Value| {
        let entry = json!({ "check": name, "pass": pass, "detail": detail });
        let _ = run_dir.append("checks.jsonl", &entry);
        checks.push(entry);
    };

    let gui = ctx.gui("g16 window matrix")?;
    app.stop_all();
    let ui = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    threadspace_harness::pause_ms(1500);

    // Traffic lights and focus.
    let window = ctx.native.ax_window(pid, Some(TITLE));
    let lights = &window["trafficLights"];
    check("traffic-lights", lights["close"] == true && lights["minimize"] == true && lights["zoom"] == true && lights["fullScreen"] == true, window.clone());
    check("focus-main-frontmost", window["main"] == true && window["frontmostPid"].as_u64() == Some(u64::from(pid)), json!({ "main": window["main"], "frontmostPid": window["frontmostPid"], "pid": pid }));

    // Titlebar drag region moves the window; content does not.
    set_frame(ctx, pid, 60.0, 60.0, 980.0, 600.0);
    threadspace_harness::pause_ms(800);
    let start = frame(&ctx.native.ax_window(pid, Some(TITLE)));
    ctx.native.json(&["drag", &(start.0 + 150.0).to_string(), &(start.1 + 560.0).to_string(), &(start.0 + 290.0).to_string(), &(start.1 + 650.0).to_string()]);
    let after_content = frame(&ctx.native.ax_window(pid, Some(TITLE)));
    check("content-drag-does-not-move", near(after_content.0, start.0, 1.0) && near(after_content.1, start.1, 1.0), json!({ "before": start, "after": after_content }));
    ctx.native.json(&["drag", &(after_content.0 + 470.0).to_string(), &(after_content.1 + 26.0).to_string(), &(after_content.0 + 610.0).to_string(), &(after_content.1 + 116.0).to_string()]);
    let after_drag = frame(&ctx.native.ax_window(pid, Some(TITLE)));
    let dx = after_drag.0 - after_content.0;
    let dy = after_drag.1 - after_content.1;
    check("titlebar-drag-moves", dx > 20.0 && dy > 20.0, json!({ "before": after_content, "after": after_drag, "delta": [dx, dy] }));

    // Minimize/restore through the native buttons.
    let minimized = ctx.native.ax_action(pid, "press-minimize", Some(TITLE));
    check("minimize-button", minimized["after"]["minimized"] == true, minimized.clone());
    let restored = ctx.native.ax_action(pid, "unminimize", Some(TITLE));
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    let after_restore = ctx.native.ax_window(pid, Some(TITLE));
    check("restore-focus", restored["after"]["minimized"] == false && after_restore["main"] == true, after_restore.clone());

    // Zoom toggles the frame and back.
    let before_zoom = frame(&after_restore);
    let zoomed = ctx.native.ax_action(pid, "press-zoom", Some(TITLE));
    let zoom_frame = frame(&zoomed["after"]);
    let unzoomed = ctx.native.ax_action(pid, "press-zoom", Some(TITLE));
    let unzoom_frame = frame(&unzoomed["after"]);
    check("zoom-toggles", zoom_frame != before_zoom && near(unzoom_frame.2, before_zoom.2, 2.0), json!({ "before": before_zoom, "zoomed": zoom_frame, "unzoomed": unzoom_frame }));

    // Fullscreen and back, with the renderer still live and the view correct.
    let full = ctx.native.ax_action(pid, "press-fullscreen", Some(TITLE));
    threadspace_harness::pause_ms(2000);
    let state_full = app.view_command("renderer:report-state", json!({}), Duration::from_secs(30)).map(|r| r["result"]["state"].clone()).unwrap_or_default();
    let exit = ctx.native.ax_action(pid, "exit-fullscreen", Some(TITLE));
    threadspace_harness::pause_ms(1500);
    let projection_after_fullscreen = compare_projection(ctx, "after-fullscreen");
    check("fullscreen-enter-exit", full["after"]["fullScreen"] == true && exit["after"]["fullScreen"] == false && state_full == "live" && projection_after_fullscreen["equal"] == true,
        json!({ "enter": full["after"]["fullScreen"], "rendererInFullscreen": state_full, "exit": exit["after"]["fullScreen"], "projectionEqual": projection_after_fullscreen["equal"] }));

    // Keyboard navigation: Tab moves focus through labelled controls. The
    // focused web element is reported only once WebKit has built its tree.
    ctx.native.ax_tree(pid, 40);
    ctx.native.ax_action(pid, "raise", Some(TITLE));
    let mut focus_path = Vec::new();
    for _ in 0..10 {
        ctx.native.json(&["key", "48"]);
        threadspace_harness::pause_ms(250);
        focus_path.push(ctx.native.json(&["ax-focused", &pid.to_string()]));
    }
    ctx.native.json(&["key", "48", "shift"]);
    threadspace_harness::pause_ms(250);
    let back = ctx.native.json(&["ax-focused", &pid.to_string()]);
    let labelled: Vec<&Value> = focus_path.iter().filter(|f| f["found"] == true && f["label"].as_str().is_some_and(|l| !l.is_empty())).collect();
    let distinct: std::collections::BTreeSet<String> = labelled.iter().map(|f| format!("{}:{}", f["role"], f["label"])).collect();
    let reverse_ok = focus_path.len() >= 2 && back["label"] == focus_path[focus_path.len() - 2]["label"];
    check("keyboard-navigation", distinct.len() >= 5 && reverse_ok, json!({ "path": focus_path, "shiftTabLandsOn": back, "distinctLabelled": distinct.len() }));

    // Accessibility tree: every button labelled, regions and headings present.
    let tree = ctx.native.ax_tree(pid, 40);
    let nodes = tree["nodes"].as_array().cloned().unwrap_or_default();
    let buttons: Vec<&Value> = nodes.iter().filter(|n| n["role"] == "AXButton").collect();
    // macOS names the window's own buttons from their subroles ("close
    // button"), so they carry no title or description of their own.
    let system_named = |n: &Value| matches!(n["subrole"].as_str(), Some("AXCloseButton" | "AXMinimizeButton" | "AXZoomButton" | "AXFullScreenButton"));
    let system_buttons = buttons.iter().filter(|n| system_named(n)).count();
    let unlabelled: Vec<&&Value> = buttons.iter().filter(|n| !system_named(n) && n["label"].as_str().is_none_or(str::is_empty)).collect();
    let regions: Vec<String> = nodes.iter().filter(|n| n["role"] == "AXGroup" || n["role"] == "AXLandmarkRegion").filter_map(|n| n["label"].as_str().map(str::to_owned)).collect();
    let headings = nodes.iter().filter(|n| n["role"] == "AXHeading").count();
    let _ = run_dir.write_json("ax-tree.json", &tree);
    check("accessibility-tree", !buttons.is_empty() && unlabelled.is_empty() && headings >= 3,
        json!({ "buttons": buttons.len(), "systemWindowButtons": system_buttons, "unlabelledButtons": unlabelled.len(), "regionLabels": regions, "headings": headings }));

    // Restored bounds across a relaunch.
    set_frame(ctx, pid, 150.0, 120.0, 1000.0, 650.0);
    threadspace_harness::pause_ms(1500);
    let saved = frame(&ctx.native.ax_window(pid, Some(TITLE)));
    app.stop_all();
    let ui = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    threadspace_harness::pause_ms(1500);
    let relaunched = frame(&ctx.native.ax_window(pid, Some(TITLE)));
    check("bounds-restored-across-relaunch", near(relaunched.0, saved.0, 3.0) && near(relaunched.1, saved.1, 3.0) && near(relaunched.2, saved.2, 3.0) && near(relaunched.3, saved.3, 3.0),
        json!({ "saved": saved, "relaunched": relaunched }));

    // Offscreen saved bounds are corrected onto the display.
    app.stop_all();
    if let Some(path) = ctx.id.bounds_file() {
        let _ = std::fs::write(&path, br#"{"x":-6000.0,"y":4000.0,"width":1000.0,"height":650.0}"#);
    }
    let ui = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    threadspace_harness::pause_ms(1500);
    let corrected = frame(&ctx.native.ax_window(pid, Some(TITLE)));
    let screen = ctx.native.json(&["displays"])["screens"][0]["visibleFrame"].clone();
    let (sw, sh) = (screen["width"].as_f64().unwrap_or(0.0), screen["height"].as_f64().unwrap_or(0.0) + 40.0);
    check("offscreen-bounds-corrected", corrected.0 >= -1.0 && corrected.1 >= -1.0 && corrected.0 + corrected.2 <= sw + 1.0 && corrected.1 + corrected.3 <= sh + 1.0,
        json!({ "written": [-6000, 4000, 1000, 650], "corrected": corrected, "visibleFrame": screen }));
    let window_id = ctx.native.main_window_id(pid);
    if let Some(id) = window_id {
        let _ = Native::capture_window(id, &run_dir.path("after-offscreen-correction.png"));
    }

    // Reduce Motion: turn it on through System Settings, observe the app
    // honour it (companion preference, renderer, no idle motion), restore.
    let original = reduce_motion(ctx);
    let mut motion = json!({ "original": original });
    if original == Some(false) {
        open_settings("x-apple.systempreferences:com.apple.Accessibility-Settings.extension?Display");
        threadspace_harness::pause_ms(2500);
        let on = ctx.native.json(&["ax-switch", "com.apple.systempreferences", "Reduce motion", "press"]);
        threadspace_harness::pause_ms(3000);
        let companion_sees = reduce_motion(ctx);
        let renderer = app.view_command("renderer:report-state", json!({}), Duration::from_secs(30)).map(|r| r["result"]["reducedMotion"].clone()).unwrap_or_default();
        ctx.native.ax_action(pid, "raise", Some(TITLE));
        threadspace_harness::pause_ms(1500);
        let shots: Vec<Value> = window_id
            .map(|id| {
                (0..2).map(|i| {
                    let path = run_dir.path(&format!("reduce-motion-{i}.png"));
                    let _ = Native::capture_window(id, &path);
                    threadspace_harness::pause_ms(3000);
                    json!(path.display().to_string())
                }).collect()
            })
            .unwrap_or_default();
        let still = if shots.len() == 2 {
            ctx.native.json(&["pixels-diff", shots[0].as_str().unwrap_or(""), shots[1].as_str().unwrap_or("")])
        } else {
            json!(null)
        };
        open_settings("x-apple.systempreferences:com.apple.Accessibility-Settings.extension?Display");
        threadspace_harness::pause_ms(2500);
        let off = ctx.native.json(&["ax-switch", "com.apple.systempreferences", "Reduce motion", "press"]);
        threadspace_harness::pause_ms(3000);
        quit_settings();
        let restored = reduce_motion(ctx);
        motion = json!({ "original": original, "toggleOn": on, "companionReadsReduceMotion": companion_sees, "rendererReducedMotion": renderer, "idleFrameDiff": still, "toggleOff": off, "restoredTo": restored });
        check("reduce-motion", companion_sees == Some(true) && renderer == true && still["changedFraction"].as_f64().is_some_and(|f| f < 0.0005) && restored == Some(false), motion.clone());
    } else {
        check("reduce-motion", false, json!({ "reason": "Reduce Motion was already on; the run does not change an owner setting it cannot restore", "original": original }));
    }
    let _ = run_dir.write_json("reduce-motion.json", &motion);

    // Device pixel ratio: the main display switches to its nearest 1x mode
    // for the helper process only (macOS restores the mode when the helper
    // exits, however it exits); the drawing buffer must follow 2x -> 1x -> 2x
    // with the scene live. Gated on owner idle: the whole screen changes.
    let idle = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    let displays_before = ctx.native.json(&["displays"]);
    let report = |label: &str| {
        app.view_command("renderer:report-state", json!({ "label": label }), Duration::from_secs(30))
            .map(|r| r["result"].clone())
            .unwrap_or_default()
    };
    let ratio = |state: &Value| {
        let canvas = &state["surface"]["canvas"];
        match (canvas["width"].as_f64(), canvas["clientWidth"].as_f64()) {
            (Some(width), Some(client)) if client > 0.0 => Some(width / client),
            _ => None,
        }
    };
    let follows = |state: &Value, dpr: f64| {
        state["state"] == "live"
            && state["surface"]["devicePixelRatio"].as_f64() == Some(dpr)
            && ratio(state).is_some_and(|r| near(r, dpr.min(1.5), 0.02))
    };
    let before = report("dpr-before");
    let mut hold = ctx.native.spawn(&["display-mode-hold", "90"]).map_err(|e| e.to_string())?;
    let mut line = String::new();
    if let Some(out) = hold.stdout.as_mut() {
        let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(out), &mut line);
    }
    let applied: Value = serde_json::from_str(line.trim()).unwrap_or(json!({ "applied": false, "raw": line.trim() }));
    let mut at_1x = Value::Null;
    for _ in 0..15 {
        threadspace_harness::pause_ms(1000);
        at_1x = report("dpr-1x");
        if follows(&at_1x, 1.0) {
            break;
        }
    }
    let live_at_1x = window_id.map(|id| {
        let a = run_dir.path("dpr-1x-0.png");
        let b = run_dir.path("dpr-1x-1.png");
        let _ = Native::capture_window(id, &a);
        threadspace_harness::pause_ms(1000);
        let _ = Native::capture_window(id, &b);
        ctx.native.json(&["pixels-diff", &a.display().to_string(), &b.display().to_string()])
    });
    let _ = hold.kill();
    let _ = hold.wait();
    let mut after = Value::Null;
    for _ in 0..15 {
        threadspace_harness::pause_ms(1000);
        after = report("dpr-restored");
        if follows(&after, 2.0) {
            break;
        }
    }
    let displays_after = ctx.native.json(&["displays"]);
    let projection_after_dpr = compare_projection(ctx, "after-dpr-change");
    let scene_changing = live_at_1x.as_ref().is_some_and(|d| d["changedFraction"].as_f64().is_some_and(|f| f > 0.0));
    check(
        "dpr-change",
        applied["applied"] == true
            && follows(&before, 2.0)
            && follows(&at_1x, 1.0)
            && scene_changing
            && follows(&after, 2.0)
            && displays_after == displays_before
            && projection_after_dpr["equal"] == true,
        json!({
            "idleGate": idle,
            "mode": applied,
            "before": { "surface": before["surface"], "appliedRatio": ratio(&before) },
            "at1x": { "surface": at_1x["surface"], "appliedRatio": ratio(&at_1x), "state": at_1x["state"], "pixelsChanging": live_at_1x },
            "restored": { "surface": after["surface"], "appliedRatio": ratio(&after), "state": after["state"] },
            "displaysRestored": displays_after == displays_before,
            "projectionEqual": projection_after_dpr["equal"],
        }),
    );
    check(
        "display-disconnect",
        false,
        json!({ "status": "MANUAL_EXTERNAL_REQUIRED", "reason": "the only display is the built-in panel, which cannot be disconnected; the case needs an external display physically attached and then removed", "displays": displays_after }),
    );

    drop(gui);
    app.stop_all();
    let required: Vec<&Value> = checks.iter().filter(|c| c["check"] != "display-disconnect").collect();
    let summary = json!({
        "gate": "G16",
        "pass": required.iter().all(|c| c["pass"] == true),
        "checks": checks.iter().map(|c| json!({ "check": c["check"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "manualExternalRequired": ["display-disconnect (needs an external display physically attached and removed)"],
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
