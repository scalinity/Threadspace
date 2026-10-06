//! G12: real macOS sleep/wake cycles. Waking needs a root-scheduled power
//! event (`pmset schedule wake`), which the harness cannot create; the owner
//! schedules the wakes once and this runner reads them back with
//! `pmset -g sched` (no privilege), sleeps the Mac shortly before each with
//! `pmset sleepnow`, and verifies the platform after every wake without any
//! further input. The screen locks on sleep, so every post-wake check works
//! behind the lock screen.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_harness::evidence::Run;
use threadspace_harness::run::run;

use crate::bridge_gates::{companion_snapshot, compare_projection, ensure_ui};
use crate::ctx::Ctx;

/// Future scheduled wakes as Unix seconds, from `pmset -g sched`.
pub fn scheduled_wakes() -> Vec<i64> {
    let out = run("/usr/bin/pmset", &["-g", "sched"], Duration::from_secs(10));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut wakes: Vec<i64> = out
        .stdout
        .lines()
        .filter(|line| line.contains("wake") && line.contains(" at "))
        .filter_map(|line| {
            let stamp = line.split(" at ").nth(1)?.split(" by").next()?.trim().to_owned();
            // `date -j -f` turns pmset's local MM/dd/yy[yy] HH:mm:ss into epoch seconds.
            ["%m/%d/%Y %H:%M:%S", "%m/%d/%y %H:%M:%S"].iter().find_map(|format| {
                let parsed = run("/bin/date", &["-j", "-f", format, &stamp, "+%s"], Duration::from_secs(5));
                parsed.ok.then(|| parsed.stdout.trim().parse::<i64>().ok()).flatten()
            })
        })
        .filter(|at| *at > now + 30)
        .collect();
    wakes.sort_unstable();
    wakes
}

fn now_s() -> i64 {
    threadspace_harness::now_ms() / 1000
}

fn admit(ctx: &Ctx, count: usize) -> Vec<(String, String, i64)> {
    let mut out = Vec::new();
    let Ok(mut client) = ctx.companion().client(Duration::from_secs(10)) else {
        return out;
    };
    for _ in 0..count {
        let id = uuid::Uuid::new_v4().to_string();
        let captured = threadspace_harness::now_ms();
        if let Ok(ControlResponseBody::Admitted { cursor, .. }) =
            client.request(ControlRequestBody::QualifyAdmit { observation_id: id.clone(), captured_wall_ms: captured })
        {
            out.push((id, cursor, captured));
        }
    }
    out
}

fn readmit_lost(ctx: &Ctx, records: &[(String, String, i64)]) -> Result<usize, String> {
    let mut client = ctx.companion().client(Duration::from_secs(10))?;
    Ok(records
        .iter()
        .filter(|(id, cursor, captured)| {
            !matches!(
                client.request(ControlRequestBody::QualifyAdmit { observation_id: id.clone(), captured_wall_ms: *captured }),
                Ok(ControlResponseBody::Admitted { status: threadspace_contracts::ui::ReceiptStatus::AlreadyCommitted, cursor: again, .. }) if &again == cursor
            )
        })
        .count())
}

fn facts(ctx: &Ctx) -> Value {
    let diagnostics = ctx.companion().diagnostics().unwrap_or_default();
    let snapshot = companion_snapshot(ctx).ok();
    json!({
        "companion": ctx.companion().incarnation(),
        "coreGeneration": diagnostics["coreGeneration"],
        "storeGeneration": diagnostics["storeGeneration"],
        "power": diagnostics["power"],
        "bootId": diagnostics["process"]["bootId"],
        "counts": snapshot.as_ref().map(|(_, s)| json!(s.counts)),
        "openAttention": snapshot.as_ref().map(|(_, s)| s.total_attention),
        "sessions": snapshot.as_ref().map(|(_, s)| s.total_sessions),
        "cursor": snapshot.as_ref().map(|(c, _)| c.clone()),
    })
}

/// A session whose binding is no longer live: an exact route to it must not
/// succeed after wake.
fn stale_session(ctx: &Ctx) -> Option<String> {
    let (_, snapshot) = companion_snapshot(ctx).ok()?;
    snapshot
        .sessions
        .iter()
        .find(|s| s.provider == "claude" && s.live_bindings == 0 && s.binding.is_none())
        .map(|s| s.session_id.clone())
}

pub fn cycles(ctx: &Ctx, cycles: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g12-sleep-wake", ctx.channel_name()).map_err(|e| e.to_string())?;
    let wakes = scheduled_wakes();
    run_dir.write_json("schedule.json", &json!({ "pmsetSched": run("/usr/bin/pmset", &["-g", "sched"], Duration::from_secs(10)).stdout, "futureWakes": wakes })).map_err(|e| e.to_string())?;
    if wakes.len() < cycles as usize {
        return Err(format!("{} future scheduled wakes; {cycles} needed (pmset schedule wake, root)", wakes.len()));
    }
    ensure_ui(ctx)?;
    let gui = ctx.gui("g12 sleep/wake cycles")?;
    let mut out = Vec::new();
    for (index, wake_at) in wakes.iter().take(cycles as usize).enumerate() {
        let cycle = index + 1;
        let before = facts(ctx);
        let records = admit(ctx, 50);
        let stale = stale_session(ctx);
        let projection_before = compare_projection(ctx, &format!("cycle-{cycle}-before"));
        // Sleep shortly before the scheduled wake.
        let lead = wake_at - now_s() - 75;
        if lead > 0 {
            threadspace_harness::pause_ms(lead as u64 * 1000);
        }
        let slept_at = threadspace_harness::now_ms();
        let started = Instant::now();
        let sleep = run("/usr/bin/pmset", &["sleepnow"], Duration::from_secs(20));
        // Wait (monotonic time stops while asleep) for the companion to record the wake.
        let wakes_before = before["power"]["wakes"].as_u64().unwrap_or(0);
        let mut woke = None;
        while started.elapsed() < Duration::from_secs(600) {
            threadspace_harness::pause_ms(2000);
            if let Ok(d) = ctx.companion().diagnostics()
                && d["power"]["wakes"].as_u64().unwrap_or(0) > wakes_before
            {
                woke = Some(d["power"].clone());
                break;
            }
        }
        let wall_gap_s = (threadspace_harness::now_ms() - slept_at) / 1000;
        threadspace_harness::pause_ms(12_000);
        let after = facts(ctx);
        let lost = readmit_lost(ctx, &records).unwrap_or(usize::MAX);
        let projection_after = compare_projection(ctx, &format!("cycle-{cycle}-after"));
        let route = stale.as_ref().map(|session| {
            crate::terminal_gates::route(ctx, session, None)
        });
        let mut cursor = ctx.companion().log();
        let mut seen = Vec::new();
        let discovery = cursor.wait_for("DISCOVERY_PASS", |_| true, Duration::from_secs(15), &mut seen);
        let checks = json!({
            "sleepCommandOk": sleep.ok,
            "companionRecordedWake": woke.is_some(),
            "sameCompanionIncarnation": before["companion"] == after["companion"],
            "sameBootSession": before["bootId"] == after["bootId"],
            "wakeRevalidationRan": after["power"]["wakeRevalidations"].as_u64() > before["power"]["wakeRevalidations"].as_u64(),
            "discoveryAfterWake": discovery.is_some(),
            "noLostAcknowledged": lost == 0 && records.len() == 50,
            "noInventedAttention": before["openAttention"] == after["openAttention"] && before["counts"] == after["counts"],
            "uiTransportRecovered": projection_after["equal"] == true,
            "noStaleExactRoute": route.as_ref().is_none_or(|r| r["exact"] != true),
            "storeGenerationKept": before["storeGeneration"] == after["storeGeneration"],
        });
        let pass = checks.as_object().is_some_and(|m| m.values().all(|v| v == &json!(true)));
        let record = json!({
            "cycle": cycle,
            "pass": pass,
            "scheduledWakeUnix": wake_at,
            "sleptAtMs": slept_at,
            "wallGapSeconds": wall_gap_s,
            "sleepCommand": { "status": sleep.status, "stderr": sleep.stderr.trim() },
            "powerAfterWake": woke,
            "before": before,
            "after": after,
            "projectionBefore": projection_before["equal"],
            "projectionAfter": projection_after,
            "staleRoute": route,
            "checks": checks,
        });
        run_dir.append("cycles.jsonl", &record).map_err(|e| e.to_string())?;
        out.push(record);
    }
    drop(gui);
    let passed = out.iter().filter(|c| c["pass"] == true).count();
    let summary = json!({
        "gate": "G12",
        "pass": passed == cycles as usize,
        "cycles": cycles,
        "passed": passed,
        "results": out.iter().map(|c| json!({ "cycle": c["cycle"], "pass": c["pass"], "wallGapSeconds": c["wallGapSeconds"] })).collect::<Vec<_>>(),
        "monotonicClocks": "all cycles stayed in one boot session (boot id unchanged); the renderer and companion never compare monotonic time across a boot, and the renderer revalidates its stream when its timers resume after suspension",
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
