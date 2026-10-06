//! G02: ten packaged and ten development launches. Each launch records the
//! UI incarnation, time to process, to companion-observed hydration and to an
//! attested renderer (backend, origin), a window capture with pixel
//! statistics, and a verified stop. Packaged runs also prove independence
//! from the dev server (nothing listens on 1420; origin is tauri://localhost)
//! and from the checkout (the executable does not embed the repository path).

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::app::DevSession;
use threadspace_harness::evidence::Run;
use threadspace_harness::native::Native;
use threadspace_harness::procs::{self, Incarnation};
use threadspace_harness::run::run;

use crate::ctx::Ctx;

fn port_1420_listeners() -> usize {
    run(
        "/usr/sbin/lsof",
        &["-nP", "-iTCP:1420", "-sTCP:LISTEN", "-t"],
        Duration::from_secs(5),
    )
    .stdout
    .split_whitespace()
    .count()
}

fn capture(ctx: &Ctx, run_dir: &Run, pid: u32, name: &str) -> Value {
    let Some(window) = ctx.native.main_window_id(pid) else {
        return json!({ "captured": false, "reason": "no on-screen window" });
    };
    let path = run_dir.path(name);
    let shot = Native::capture_window(window, &path);
    if !shot.ok {
        return json!({ "captured": false, "reason": shot.stderr.trim() });
    }
    json!({ "captured": true, "file": name, "windowId": window, "stats": ctx.native.json(&["pixels-stats", &path.display().to_string()]) })
}

fn percentile(values: &mut [u64], fraction: f64) -> u64 {
    values.sort_unstable();
    if values.is_empty() {
        return 0;
    }
    let rank = ((fraction * values.len() as f64).ceil() as usize).clamp(1, values.len());
    values[rank - 1]
}

pub fn packaged(ctx: &Ctx, count: u32) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "g02-launches",
        &format!("packaged-{}", ctx.channel_name()),
    )
    .map_err(|e| e.to_string())?;
    let app = ctx.app();
    app.stop_all();
    let repo_marker = ctx.repo.display().to_string();
    let strings = run(
        "/usr/bin/strings",
        &[&ctx.id.executable.display().to_string()],
        Duration::from_secs(60),
    );
    let embeds_checkout = strings
        .stdout
        .lines()
        .any(|line| line.contains(&repo_marker));
    let mut launches = Vec::new();
    let mut hydrate_ms = Vec::new();
    for index in 1..=count {
        let mut cursor = ctx.companion().log();
        let since = threadspace_harness::now_ms();
        let started = Instant::now();
        let record = match app.launch_packaged(&[]) {
            Ok(launched) => {
                let hydrated = app.wait_hydrated(&mut cursor, Duration::from_secs(30));
                let hydrated_ms = hydrated
                    .as_ref()
                    .and_then(|(_, h)| h["ts"].as_i64())
                    .map(|ts| ts - since);
                let attestation = app
                    .wait_report(
                        "renderer-attestation",
                        since,
                        |_| true,
                        Duration::from_secs(30),
                    )
                    .map(|(_, report)| report["report"].clone());
                let listeners = port_1420_listeners();
                threadspace_harness::pause_ms(1500);
                let shot = capture(
                    ctx,
                    &run_dir,
                    launched.ui.pid as u32,
                    &format!("launch-{index:02}.png"),
                );
                let stopped = app.stop(&launched.ui, false);
                if let Some(ms) = hydrated_ms {
                    hydrate_ms.push(ms.max(0) as u64);
                }
                let ok = hydrated.is_some()
                    && attestation.as_ref().is_some_and(|a| {
                        a["backend"] == "WEBGPU" && a["origin"] == "tauri://localhost"
                    })
                    && listeners == 0
                    && stopped.is_some();
                json!({
                    "index": index,
                    "ok": ok,
                    "ui": launched.ui,
                    "processAppearedMs": launched.process_appeared_ms,
                    "hydratedMs": hydrated_ms,
                    "subscriptionId": hydrated.as_ref().map(|(a, _)| a["subscriptionId"].clone()),
                    "attestation": attestation.as_ref().map(|a| json!({ "backend": a["backend"], "webgpuGate": a["webgpuGate"], "origin": a["origin"], "threeRevision": a["threeRevision"], "initDurationMs": a["initDurationMs"] })),
                    "port1420Listeners": listeners,
                    "capture": shot,
                    "stoppedAfterMs": stopped,
                    "elapsedMs": started.elapsed().as_millis() as u64,
                })
            }
            Err(error) => json!({ "index": index, "ok": false, "error": error }),
        };
        run_dir
            .append("launches.jsonl", &record)
            .map_err(|e| e.to_string())?;
        launches.push(record);
        threadspace_harness::pause_ms(1000);
    }
    let passed = launches.iter().filter(|l| l["ok"] == true).count();
    let summary = json!({
        "gate": "G02",
        "context": "packaged",
        "channel": ctx.channel_name(),
        "count": count,
        "passed": passed,
        "pass": passed as u32 == count && !embeds_checkout,
        "executableEmbedsCheckoutPath": embeds_checkout,
        "hydrateMs": { "p50": percentile(&mut hydrate_ms.clone(), 0.5), "p95": percentile(&mut hydrate_ms.clone(), 0.95), "max": hydrate_ms.iter().max() },
        "environment": ctx.environment(),
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

/// The debug UI binary `tauri dev` runs.
fn dev_ui(ctx: &Ctx) -> Vec<Incarnation> {
    procs::with_executable(
        &ctx.repo
            .join("target/aarch64-apple-darwin/debug/threadspace-desktop"),
    )
}

pub fn dev(ctx: &Ctx, count: u32) -> Result<Value, String> {
    if ctx.id.channel != threadspace_harness::identity::Channel::Dev {
        return Err("dev launches use the dev identity".into());
    }
    let run_dir =
        Run::create(&ctx.evidence_root(), "g02-launches", "dev").map_err(|e| e.to_string())?;
    // The packaged Dev UI shares the identifier and its UI lock.
    ctx.app().stop_all();
    let companion_before = ctx.companion().incarnation();
    let mut launches = Vec::new();
    for index in 1..=count {
        let mut cursor = ctx.companion().log();
        let since = threadspace_harness::now_ms();
        let log = run_dir.path(&format!("tauri-dev-{index:02}.log"));
        let session = DevSession::start(&ctx.repo, &log, &[])?;
        // The first launch may compile the debug shell.
        let timeout = if index == 1 {
            Duration::from_secs(600)
        } else {
            Duration::from_secs(180)
        };
        let hydrated = ctx.app().wait_hydrated(&mut cursor, timeout);
        let attestation = ctx
            .app()
            .wait_report(
                "renderer-attestation",
                since,
                |r| r["report"]["origin"] == "http://localhost:1420",
                Duration::from_secs(60),
            )
            .map(|(_, report)| report["report"].clone());
        let ui = dev_ui(ctx).into_iter().next();
        threadspace_harness::pause_ms(1500);
        let shot = ui.as_ref().map(|ui| {
            capture(
                ctx,
                &run_dir,
                ui.pid as u32,
                &format!("dev-launch-{index:02}.png"),
            )
        });
        let stopped = session.stop();
        let ui_gone = ui
            .as_ref()
            .is_none_or(|ui| procs::wait_exit(ui, Duration::from_secs(10)).is_some());
        let companion_same = ctx.companion().incarnation() == companion_before;
        let ok = hydrated.is_some()
            && attestation
                .as_ref()
                .is_some_and(|a| a["backend"] == "WEBGPU")
            && stopped["port1420Listeners"] == 0
            && ui_gone
            && companion_same;
        let record = json!({
            "index": index,
            "ok": ok,
            "ui": ui,
            "hydratedMs": hydrated.as_ref().and_then(|(_, h)| h["ts"].as_i64()).map(|ts| ts - since),
            "attestation": attestation.as_ref().map(|a| json!({ "backend": a["backend"], "webgpuGate": a["webgpuGate"], "origin": a["origin"], "threeRevision": a["threeRevision"] })),
            "capture": shot,
            "stop": stopped,
            "uiExited": ui_gone,
            "companionIncarnationUnchanged": companion_same,
        });
        run_dir
            .append("launches.jsonl", &record)
            .map_err(|e| e.to_string())?;
        launches.push(record);
    }
    let passed = launches.iter().filter(|l| l["ok"] == true).count();
    let summary = json!({
        "gate": "G02",
        "context": "dev",
        "count": count,
        "passed": passed,
        "pass": passed as u32 == count,
        "companionIncarnationBefore": companion_before,
        "companionIncarnationAfter": ctx.companion().incarnation(),
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
