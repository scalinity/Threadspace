//! G13/G14 renderer attestation and G15 sustained packaged graphics, against
//! the installed production bundle. Presented pixels are captured from the
//! real window (`screencapture -l`) and compared by the harness; DOM/AX and
//! journal agreement come from the accessibility tree and the projection
//! digest; lifecycle facts come from the renderer's own diagnostics.
//! Injected device losses are labelled injected everywhere.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::evidence::Run;
use threadspace_harness::native::Native;
use threadspace_harness::procs::Incarnation;

use crate::bridge_gates::{compare_projection, ensure_ui};
use crate::ctx::Ctx;

fn state(ctx: &Ctx) -> Value {
    ctx.app()
        .view_command("renderer:report-state", json!({}), Duration::from_secs(30))
        .map(|r| r["result"].clone())
        .unwrap_or_else(|e| json!({ "error": e }))
}

fn command(ctx: &Ctx, name: &str) -> Value {
    ctx.app()
        .view_command(&format!("renderer:{name}"), json!({}), Duration::from_secs(60))
        .map(|r| json!({ "ok": r["ok"], "result": r["result"], "error": r["error"] }))
        .unwrap_or_else(|e| json!({ "error": e }))
}

fn ax_texts(ctx: &Ctx, pid: u32) -> Vec<String> {
    ctx.native.ax_tree(pid, 40)["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n["value"].as_str().or(n["label"].as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// G13/G14: packaged WebGPU attestation, and a forced WebGL2 run that is
/// visibly diagnostic and fails the primary-backend gate.
pub fn renderer(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g13-g14-renderer", ctx.channel_name()).map_err(|e| e.to_string())?;
    let app = ctx.app();
    let mut results = Vec::new();
    for (label, args) in [("webgpu", vec![]), ("forced-webgl2", vec!["--renderer=webgl2-compatibility"])] {
        app.stop_all();
        let _gui = ctx.gui(&format!("g13 {label}"))?;
        let since = threadspace_harness::now_ms();
        let mut cursor = ctx.companion().log();
        let launched = app.launch_packaged(&args)?;
        let hydrated = app.wait_hydrated(&mut cursor, Duration::from_secs(30));
        let attestation = app
            .wait_report("renderer-attestation", since, |_| true, Duration::from_secs(30))
            .map(|(_, r)| r["report"].clone());
        threadspace_harness::pause_ms(2500);
        let texts = ax_texts(ctx, launched.ui.pid as u32);
        let badge: Vec<&String> = texts.iter().filter(|t| t.contains("WebGPU") || t.contains("WEBGL2") || t.contains("diagnostic backend")).collect();
        let shot = ctx.native.main_window_id(launched.ui.pid as u32).map(|window| {
            let path = run_dir.path(&format!("{label}.png"));
            let ok = Native::capture_window(window, &path).ok;
            json!({ "file": format!("{label}.png"), "captured": ok })
        });
        let lifecycle = state(ctx);
        app.stop(&launched.ui, false);
        let pass = match label {
            "webgpu" => attestation.as_ref().is_some_and(|a| {
                a["backend"] == "WEBGPU" && a["webgpuGate"] == "PASS" && a["origin"] == "tauri://localhost"
                    && a["threeRevision"] == "186" && a["secureContext"] == true && a["navigatorGpu"] == true
            }),
            _ => attestation.as_ref().is_some_and(|a| a["backend"] == "WEBGL2_COMPATIBILITY" && a["webgpuGate"] == "FAIL" && a["forcedCompatibility"] == true)
                && badge.iter().any(|t| t.contains("does not pass the WebGPU gate")),
        };
        let record = json!({
            "run": label,
            "pass": pass,
            "hydrated": hydrated.is_some(),
            "attestation": attestation,
            "visibleBadge": badge,
            "capture": shot,
            "lifecycle": { "state": lifecycle["state"], "generation": lifecycle["generation"], "presentation": lifecycle["presentation"] },
        });
        run_dir.append("runs.jsonl", &record).map_err(|e| e.to_string())?;
        results.push(record);
    }
    let summary = json!({
        "gates": ["G13", "G14"],
        "pass": results.iter().all(|r| r["pass"] == true),
        "runs": results.iter().map(|r| json!({ "run": r["run"], "pass": r["pass"], "backend": r["attestation"]["backend"], "webgpuGate": r["attestation"]["webgpuGate"] })).collect::<Vec<_>>(),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

struct Captures<'a> {
    ctx: &'a Ctx,
    run: &'a Run,
    pid: u32,
    previous: Option<(String, Option<(f64, f64)>)>,
    index: u32,
}

impl Captures<'_> {
    /// Captures the window; compares with the previous capture of the same size.
    fn take(&mut self, phase: &str) -> Value {
        self.index += 1;
        let Some(window) = self.ctx.native.main_window_id(self.pid) else {
            return json!({ "phase": phase, "captured": false });
        };
        let name = format!("frames/{:04}-{phase}.png", self.index);
        let path = self.run.path(&name);
        let _ = std::fs::create_dir_all(self.run.path("frames"));
        if !Native::capture_window(window, &path).ok {
            return json!({ "phase": phase, "captured": false });
        }
        let stats = self.ctx.native.json(&["pixels-stats", &path.display().to_string()]);
        let size = stats["width"].as_f64().zip(stats["height"].as_f64());
        let diff = match &self.previous {
            Some((prev, prev_size)) if *prev_size == size => {
                Some(self.ctx.native.json(&["pixels-diff", prev, &path.display().to_string()]))
            }
            _ => None,
        };
        self.previous = Some((path.display().to_string(), size));
        json!({ "phase": phase, "captured": true, "file": name, "stats": stats, "diffFromPrevious": diff, "atMs": threadspace_harness::now_ms() })
    }
}

fn changed(capture: &Value) -> bool {
    capture["diffFromPrevious"]["changedFraction"].as_f64().is_some_and(|f| f > 0.0005)
        || capture["diffFromPrevious"]["meanAbsDiff"].as_f64().is_some_and(|d| d > 0.02)
}

/// G15: fifteen minutes of changing packaged output with the lifecycle
/// exercised: minimize, 2D, hide during initialization, injected visible
/// loss, repeated loss, loss while hidden, repeated hide/show and a document
/// reload with a resource request in flight.
pub fn sustained(ctx: &Ctx, minutes: u64) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g15-graphics", ctx.channel_name()).map_err(|e| e.to_string())?;
    let app = ctx.app();
    let reduce_motion = ctx.companion().diagnostics()?["accessibilityPreferences"]["reduceMotion"].clone();
    app.stop_all();
    // The whole run holds the GUI lock: the window must stay visible and
    // unoccluded so WebKit keeps presenting frames.
    let gui = ctx.gui("g15 sustained graphics (15 min)")?;
    let ui: Incarnation = ensure_ui(ctx)?;
    let pid = ui.pid as u32;
    ctx.native.ax_action(pid, "raise", Some("Threadspace"));
    let started = Instant::now();
    let mut captures = Captures { ctx, run: &run_dir, pid, previous: None, index: 0 };
    let mut timeline = Vec::new();
    let mut log = |event: &str, detail: Value| {
        let entry = json!({ "atS": started.elapsed().as_secs_f64(), "event": event, "detail": detail });
        let _ = run_dir.append("timeline.jsonl", &entry);
        timeline.push(entry);
    };
    log("start", json!({ "ui": ui, "reduceMotion": reduce_motion, "state": state(ctx) }));
    let mut live_pairs = 0;
    let mut live_changed = 0;
    let mut sample = |phase: &str, live: bool, captures: &mut Captures<'_>, log: &mut dyn FnMut(&str, Value)| {
        let shot = captures.take(phase);
        if live && shot["diffFromPrevious"].is_object() {
            live_pairs += 1;
            if changed(&shot) {
                live_changed += 1;
            }
        }
        log("capture", shot);
    };
    let wait_live = |seconds: u64, phase: &str, captures: &mut Captures<'_>, log: &mut dyn FnMut(&str, Value), sample: &mut dyn FnMut(&str, bool, &mut Captures<'_>, &mut dyn FnMut(&str, Value))| {
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            threadspace_harness::pause_ms(5000);
            sample(phase, true, captures, log);
        }
    };

    // 1. Steady visible animation.
    wait_live(50, "steady", &mut captures, &mut log, &mut sample);

    // 2. Minimize: the renderer is disposed and schedules no frames.
    log("minimize", ctx.native.ax_action(pid, "minimize", Some("Threadspace")));
    threadspace_harness::pause_ms(12_000);
    let hidden_a = state(ctx);
    threadspace_harness::pause_ms(10_000);
    let hidden_b = state(ctx);
    log("hidden-state", json!({ "first": hidden_a, "afterTenSeconds": hidden_b }));
    let hidden_ok = hidden_b["state"] == "hidden-disposed"
        && hidden_b["frames"]["pending"] == 0
        && hidden_b["frames"]["requestedSinceDisposal"] == 0
        && hidden_a["frames"]["requested"] == hidden_b["frames"]["requested"];
    log("unminimize", ctx.native.ax_action(pid, "unminimize", Some("Threadspace")));
    ctx.native.ax_action(pid, "raise", Some("Threadspace"));
    threadspace_harness::pause_ms(6000);
    let returned = state(ctx);
    let return_ok = returned["state"] == "live"
        && returned["generation"].as_u64() > hidden_b["generation"].as_u64()
        && returned["lastAttestation"]["backend"] == "WEBGPU";
    log("returned-visible", returned.clone());
    captures.previous = None;
    wait_live(30, "after-unminimize", &mut captures, &mut log, &mut sample);

    // 3. 2D mode: disposed, the DOM view operational; back to 3D.
    log("enter-2d", command(ctx, "enter-2d"));
    threadspace_harness::pause_ms(8000);
    let two_d = state(ctx);
    let projection_2d = compare_projection(ctx, "in-2d");
    log("2d-state", json!({ "state": two_d, "projectionEqual": projection_2d["equal"] }));
    let two_d_ok = two_d["state"] == "2d" && two_d["frames"]["pending"] == 0 && projection_2d["equal"] == true;
    sample("2d", false, &mut captures, &mut log);
    log("exit-2d", command(ctx, "exit-2d"));
    threadspace_harness::pause_ms(5000);
    captures.previous = None;
    wait_live(20, "after-2d", &mut captures, &mut log, &mut sample);

    // 4. Hide during renderer initialization: start a rebuild, minimize at once.
    log("enter-2d-for-init-race", command(ctx, "enter-2d"));
    threadspace_harness::pause_ms(3000);
    let rebuild = ctx.companion().view_command("renderer:exit-2d", json!({}));
    let racing_minimize = ctx.native.ax_action(pid, "minimize", Some("Threadspace"));
    log("hide-during-init", json!({ "rebuildIntent": format!("{rebuild:?}"), "minimize": racing_minimize }));
    threadspace_harness::pause_ms(15_000);
    let init_race = state(ctx);
    log("hide-during-init-state", init_race.clone());
    let init_race_ok = init_race["state"] == "hidden-disposed" && init_race["liveGeneration"].is_null() && init_race["frames"]["pending"] == 0;
    ctx.native.ax_action(pid, "unminimize", Some("Threadspace"));
    ctx.native.ax_action(pid, "raise", Some("Threadspace"));
    threadspace_harness::pause_ms(6000);
    let after_race = state(ctx);
    log("after-init-race", after_race.clone());
    captures.previous = None;
    wait_live(20, "after-init-race", &mut captures, &mut log, &mut sample);

    // 5. Injected visible device loss: one bounded rebuild.
    let before_loss = state(ctx);
    log("inject-visible-loss", command(ctx, "inject-device-loss"));
    threadspace_harness::pause_ms(8000);
    let after_loss = state(ctx);
    log("after-visible-loss", after_loss.clone());
    let loss_ok = after_loss["state"] == "live"
        && after_loss["lossCounts"]["injected"].as_u64() == before_loss["lossCounts"]["injected"].as_u64().map(|n| n + 1)
        && after_loss["counts"]["recoveryRebuilds"].as_u64() > before_loss["counts"]["recoveryRebuilds"].as_u64()
        && after_loss["deviceLosses"].as_array().is_some_and(|l| l.last().is_some_and(|x| x["injected"] == true));
    captures.previous = None;
    wait_live(20, "after-visible-loss", &mut captures, &mut log, &mut sample);

    // 6. Repeated loss: the recovery renderer is lost too — fall back to the
    //    operational DOM/2D view; journal and attention unaffected.
    log("inject-loss-1", command(ctx, "inject-device-loss"));
    threadspace_harness::pause_ms(1500);
    log("inject-loss-2", command(ctx, "inject-device-loss"));
    threadspace_harness::pause_ms(8000);
    let repeated = state(ctx);
    let projection_fallback = compare_projection(ctx, "after-repeated-loss");
    log("after-repeated-loss", json!({ "state": repeated, "projectionEqual": projection_fallback["equal"] }));
    let repeated_ok = (repeated["presentation"] == "fallback" || repeated["state"] == "failed-fallback")
        && projection_fallback["equal"] == true;
    log("recover-from-fallback", command(ctx, "exit-2d"));
    threadspace_harness::pause_ms(6000);
    let recovered = state(ctx);
    log("after-fallback-recovery", recovered.clone());
    captures.previous = None;
    wait_live(20, "after-fallback-recovery", &mut captures, &mut log, &mut sample);

    // 7. Loss while hidden: no live device exists once hidden; an injection
    //    finds nothing to lose and revives nothing.
    ctx.native.ax_action(pid, "minimize", Some("Threadspace"));
    threadspace_harness::pause_ms(10_000);
    let hidden_injection = command(ctx, "inject-device-loss");
    threadspace_harness::pause_ms(5000);
    let hidden_after = state(ctx);
    log("loss-while-hidden", json!({ "injection": hidden_injection, "state": hidden_after }));
    let hidden_loss_ok = hidden_after["state"] == "hidden-disposed" && hidden_after["liveGeneration"].is_null();
    ctx.native.ax_action(pid, "unminimize", Some("Threadspace"));
    ctx.native.ax_action(pid, "raise", Some("Threadspace"));
    threadspace_harness::pause_ms(6000);

    // 8. Repeated hide/show.
    let generation_before = state(ctx)["generation"].as_u64().unwrap_or(0);
    for _ in 0..5 {
        ctx.native.ax_action(pid, "minimize", Some("Threadspace"));
        threadspace_harness::pause_ms(4000);
        ctx.native.ax_action(pid, "unminimize", Some("Threadspace"));
        ctx.native.ax_action(pid, "raise", Some("Threadspace"));
        threadspace_harness::pause_ms(4000);
    }
    let toggled = state(ctx);
    log("after-repeated-hide-show", toggled.clone());
    let toggle_ok = toggled["state"] == "live" && toggled["generation"].as_u64().unwrap_or(0) >= generation_before + 5;
    captures.previous = None;
    wait_live(20, "after-toggles", &mut captures, &mut log, &mut sample);

    // 9. Document reload with the scene's asset request in flight.
    let mut desktop = app.desktop_log();
    log("reload", app.view_command_nowait("reload", json!({})).unwrap_or_else(|e| json!(e)));
    let mut lines = Vec::new();
    let recovered_view = desktop.wait_for("OFFICE_VIEW_RECOVERED", |_| true, Duration::from_secs(60), &mut lines);
    threadspace_harness::pause_ms(8000);
    let after_reload = state(ctx);
    log("after-reload", json!({ "recovered": recovered_view, "state": after_reload }));
    let reload_ok = recovered_view.is_some() && after_reload["state"] == "live";
    ctx.native.ax_action(pid, "raise", Some("Threadspace"));
    captures.previous = None;

    // 10. The rest of the run: steady animation with periodic state changes
    //     (synthetic sessions appear as workers) and DOM/journal agreement.
    let total = Duration::from_secs(minutes * 60);
    let mut agreement = Vec::new();
    let mut minute_mark = started.elapsed().as_secs() / 60;
    while started.elapsed() < total {
        threadspace_harness::pause_ms(5000);
        sample("sustained", true, &mut captures, &mut log);
        let minute = started.elapsed().as_secs() / 60;
        if minute > minute_mark {
            minute_mark = minute;
            let _ = ctx.companion().request(
                threadspace_contracts::control::ControlRequestBody::QualifySyntheticChanges { count: 4, duration_ms: 1000, sessions: 4 },
                Duration::from_secs(10),
            );
            threadspace_harness::pause_ms(3000);
            let projection = compare_projection(ctx, &format!("minute-{minute}"));
            let texts = ax_texts(ctx, pid);
            agreement.push(json!({ "minute": minute, "projectionEqual": projection["equal"], "axSessionsRow": texts.iter().filter(|t| t.contains("Stream ")).count(), "sessions": projection["sessions"] }));
        }
    }
    let final_state = state(ctx);
    log("final", final_state.clone());
    drop(gui);
    app.stop_all();

    let pixel_live_ratio = if live_pairs == 0 { 0.0 } else { f64::from(live_changed) / f64::from(live_pairs) };
    let agreement_ok = !agreement.is_empty() && agreement.iter().all(|a| a["projectionEqual"] == true);
    let checks = json!({
        "pixelsChangingWhileLive": { "pairs": live_pairs, "changed": live_changed, "ratio": pixel_live_ratio, "pass": live_pairs >= 60 && pixel_live_ratio >= 0.9 },
        "minimizeDisposesAndStopsFrames": hidden_ok,
        "returnRebuildsFreshAttestedRenderer": return_ok,
        "twoDModeDisposesAndDomStaysOperational": two_d_ok,
        "hideDuringInitDoesNotRevive": init_race_ok,
        "injectedVisibleLossOneBoundedRebuild": loss_ok,
        "repeatedLossFallsBackOperational": repeated_ok,
        "lossWhileHiddenRevivesNothing": hidden_loss_ok,
        "repeatedHideShowFreshGenerations": toggle_ok,
        "reloadWithResourceInFlight": reload_ok,
        "domJournalAgreementPerMinute": agreement_ok,
    });
    let pass = checks.as_object().is_some_and(|map| map.values().all(|v| v == &json!(true) || v["pass"] == true));
    let summary = json!({
        "gate": "G15",
        "pass": pass,
        "minutes": minutes,
        "elapsedS": started.elapsed().as_secs(),
        "reduceMotion": reduce_motion,
        "checks": checks,
        "agreement": agreement,
        "finalLifecycle": { "state": final_state["state"], "generation": final_state["generation"], "counts": final_state["counts"], "lossCounts": final_state["lossCounts"], "frames": final_state["frames"] },
        "injectedLossLabel": "every device loss in this run was qualification-injected; none is a natural GPU loss",
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
