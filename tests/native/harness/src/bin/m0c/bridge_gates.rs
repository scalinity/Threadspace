//! G03 IPC, G04 Channel streaming and the SPEC §18.5 view-recovery runs,
//! driven through the live packaged office view: the harness sends
//! qualification commands as native intents and reads back the reports the
//! view records natively, the companion log and the desktop shell log.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_contracts::projection::FleetSnapshot;
use threadspace_harness::companion::LogCursor;
use threadspace_harness::evidence::{Run, sha256_text};
use threadspace_harness::procs;

use crate::ctx::Ctx;

/// A window count is settled once it has not changed for this long.
const SHELLS_STABLE: Duration = Duration::from_secs(5);
const SHELLS_SETTLE_CAP: Duration = Duration::from_secs(60);
/// The office window's title (`window::create_office`).
const OFFICE_TITLE: &str = "Threadspace";
/// The most the UI's footprint may trend upward per recovery, as the
/// least-squares slope across the second half of the per-recovery samples
/// (C-04). A leak costs every recovery alike and shows there in full (before
/// the D-0008 repair: 0.57 MiB per recovery); a one-time cost early in the
/// run does not, and on the M1 build one appeared as a single step that then
/// held flat for 46 recoveries.
const FOOTPRINT_SLOPE_MAX: f64 = 0.1 * 1024.0 * 1024.0;

/// Makes sure exactly one hydrated UI is running; returns its incarnation.
pub fn ensure_ui(ctx: &Ctx) -> Result<procs::Incarnation, String> {
    // Iterating under `tauri dev`: drive the attached development UI instead
    // of launching the packaged bundle (both share the dev identifier).
    if std::env::var_os("THREADSPACE_HARNESS_ATTACHED_DEV_UI").is_some() {
        return procs::with_executable(
            &ctx.repo
                .join("target/aarch64-apple-darwin/debug/threadspace-desktop"),
        )
        .into_iter()
        .next()
        .ok_or_else(|| "no attached tauri dev UI".to_owned());
    }
    let app = ctx.app();
    if let Some(ui) = app.processes().into_iter().next() {
        return Ok(ui);
    }
    let _gui = ctx.gui("launch office")?;
    let mut cursor = ctx.companion().log();
    let launched = app.launch_packaged(&[])?;
    app.wait_hydrated(&mut cursor, Duration::from_secs(30))
        .ok_or("UI did not hydrate")?;
    threadspace_harness::pause_ms(1500);
    Ok(launched.ui)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestSession<'a> {
    session_id: &'a str,
    revision: &'a str,
    display_name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestAttention<'a> {
    attention_id: &'a str,
    revision: &'a str,
    acknowledged_at_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestCounts {
    needs_attention: u32,
    awaiting_action: u32,
}

#[derive(Serialize)]
struct Digest<'a> {
    sessions: Vec<DigestSession<'a>>,
    attention: Vec<DigestAttention<'a>>,
    counts: DigestCounts,
}

/// The same canonical form `projectionDigest` computes in the renderer.
pub fn snapshot_digest(snapshot: &FleetSnapshot) -> (String, usize, usize) {
    let mut sessions: Vec<_> = snapshot
        .sessions
        .iter()
        .map(|row| DigestSession {
            session_id: &row.session_id,
            revision: &row.revision,
            display_name: &row.display_name,
        })
        .collect();
    sessions.sort_by(|a, b| a.session_id.cmp(b.session_id));
    let mut attention: Vec<_> = snapshot
        .attention
        .iter()
        .filter(|row| row.resolved_at_ms.is_none())
        .map(|row| DigestAttention {
            attention_id: &row.attention_id,
            revision: &row.revision,
            acknowledged_at_ms: row.acknowledged_at_ms,
        })
        .collect();
    attention.sort_by(|a, b| a.attention_id.cmp(b.attention_id));
    let (s, a) = (sessions.len(), attention.len());
    let digest = Digest {
        sessions,
        attention,
        counts: DigestCounts {
            needs_attention: snapshot.counts.needs_attention,
            awaiting_action: snapshot.counts.awaiting_action,
        },
    };
    (
        sha256_text(&serde_json::to_string(&digest).unwrap_or_default()),
        s,
        a,
    )
}

/// A consistent companion projection read through a short-lived view.
pub fn companion_snapshot(ctx: &Ctx) -> Result<(String, FleetSnapshot), String> {
    let mut client = ctx.companion().client(Duration::from_secs(10))?;
    let subscription_id = uuid::Uuid::new_v4().to_string();
    let reply = client
        .request(ControlRequestBody::AttachView {
            subscription_id: subscription_id.clone(),
        })
        .map_err(|e| e.to_string())?;
    let _ = client.request(ControlRequestBody::DetachView { subscription_id });
    match reply {
        ControlResponseBody::ViewAttached {
            cursor, snapshot, ..
        } => Ok((cursor, snapshot)),
        other => Err(format!("unexpected {other:?}")),
    }
}

/// Compares the view's applied projection with the companion's, retrying
/// briefly while the last patches are still being applied.
pub fn compare_projection(ctx: &Ctx, label: &str) -> Value {
    let app = ctx.app();
    let mut last = json!({});
    for _ in 0..10 {
        let view = app.view_command("projection-digest", json!({}), Duration::from_secs(20));
        let companion = companion_snapshot(ctx);
        let (Ok(view), Ok((cursor, snapshot))) = (view, companion) else {
            last = json!({ "label": label, "error": "digest unavailable" });
            threadspace_harness::pause_ms(1000);
            continue;
        };
        let (digest, sessions, attention) = snapshot_digest(&snapshot);
        let equal = view["result"]["digest"].as_str() == Some(digest.as_str());
        last = json!({
            "label": label,
            "equal": equal,
            "viewDigest": view["result"]["digest"],
            "companionDigest": digest,
            "viewCursor": view["result"]["cursor"],
            "companionCursor": cursor,
            "sessions": sessions,
            "attention": attention,
            "complete": snapshot.complete,
            "viewStream": view["result"]["stream"],
            "viewPaging": view["result"]["paging"],
        });
        if equal {
            return last;
        }
        threadspace_harness::pause_ms(1000);
    }
    last
}

fn raise(ctx: &Ctx, label: &str) -> Result<Value, String> {
    match ctx.companion().request(
        ControlRequestBody::QualifyRaiseAttention {
            label: label.to_owned(),
            session_id: None,
        },
        Duration::from_secs(10),
    )? {
        ControlResponseBody::AttentionRaised {
            attention_id,
            cursor,
            ..
        } => Ok(json!({ "attentionId": attention_id, "cursor": cursor })),
        other => Err(format!("unexpected {other:?}")),
    }
}

/// G03: the in-view suite (≥1,000 round trips and every negative case), the
/// unlisted-view ACL probe and the remote-origin probe.
pub fn ipc(ctx: &Ctx, rounds: u32) -> Result<Value, String> {
    let run = Run::create(&ctx.evidence_root(), "g03-ipc", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    ensure_ui(ctx)?;
    let raised = [raise(ctx, "ipc-suite-a")?, raise(ctx, "ipc-suite-b")?];
    threadspace_harness::pause_ms(1500);
    let suite = ctx.app().view_command(
        "ipc-suite",
        json!({ "rounds": rounds }),
        Duration::from_secs(900),
    )?;
    run.write_json("ipc-suite.json", &suite)
        .map_err(|e| e.to_string())?;
    let result = &suite["result"];
    let suite_ok = suite["ok"] == true
        && result["roundTripsOk"] == true
        && result["passed"] == result["total"]
        && result["rounds"].as_u64() >= Some(1000);

    // Unlisted WebView and remote origin: relaunch with the probe flags.
    let _gui = ctx.gui("g03 probe launch")?;
    let app = ctx.app();
    app.stop_all();
    let since = threadspace_harness::now_ms();
    let server = serve_origin_probe()?;
    let url = format!(
        "--qualify-origin-probe=http://127.0.0.1:{}/probe.html",
        server
    );
    let mut cursor = ctx.companion().log();
    let launched = app.launch_packaged(&["--qualify-acl-probe", &url])?;
    let _ = app.wait_hydrated(&mut cursor, Duration::from_secs(30));
    let acl = app
        .wait_report("acl-probe", since, |_| true, Duration::from_secs(30))
        .map(|(_, r)| r["report"].clone());
    let origin = app
        .wait_report("origin-probe", since, |_| true, Duration::from_secs(30))
        .map(|(_, r)| r["report"].clone());
    app.stop(&launched.ui, false);
    run.write_json("acl-probe.json", &json!(acl))
        .map_err(|e| e.to_string())?;
    run.write_json("origin-probe.json", &json!(origin))
        .map_err(|e| e.to_string())?;
    let acl_ok = acl.as_ref().is_some_and(|r| r["allRefused"] == true);
    let origin_ok = origin.as_ref().is_some_and(|r| r["allRefused"] == true);
    let summary = json!({
        "gate": "G03",
        "pass": suite_ok && acl_ok && origin_ok,
        "rounds": result["rounds"],
        "roundTripFailures": result["roundTripFailures"],
        "latencyMs": result["latencyMs"],
        "cases": { "passed": result["passed"], "total": result["total"] },
        "failedCases": result["results"].as_array().map(|cases| cases.iter().filter(|c| c["pass"] != true).cloned().collect::<Vec<_>>()),
        "aclProbeAllRefused": acl_ok,
        "originProbeAllRefused": origin_ok,
        "fixtureAttention": raised,
    });
    run.write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run.dir }))
}

const ORIGIN_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>probe</title><script>
(async () => {
  const results = [];
  const internals = window.__TAURI_INTERNALS__;
  results.push({ check: "bridge object in this origin", present: typeof internals === "object" && internals !== null });
  const context = null;
  for (const [command, args] of [
    ["ui_query", { request: { query: { kind: "ConnectionStatus" }, context } }],
    ["ui_connect", { request: { protocolVersion: 1, viewEpoch: crypto.randomUUID() }, events: { toJSON: () => "__CHANNEL__:0" } }],
    ["ui_action", { request: { action: { kind: "RefreshEvidence" }, expectedRevision: null, requestId: crypto.randomUUID(), context: { subscriptionId: crypto.randomUUID(), viewEpoch: crypto.randomUUID(), coreGeneration: "x", storeGeneration: "x" } } }],
  ]) {
    if (!internals || typeof internals.invoke !== "function") { results.push({ command, refused: true, detail: "no IPC bridge exposed to this origin" }); continue; }
    try { await internals.invoke(command, args); results.push({ command, refused: false, detail: "accepted" }); }
    catch (error) {
      const typed = typeof error === "object" && error !== null && "code" in error;
      results.push({ command, refused: !typed, detail: String(typeof error === "string" ? error : JSON.stringify(error)).slice(0, 200) });
    }
  }
  try {
    const response = await fetch("ipc://localhost/ui_query", { method: "POST", body: JSON.stringify({ request: { query: { kind: "ConnectionStatus" }, context: null } }), headers: { "Content-Type": "application/json" } });
    results.push({ command: "direct ipc:// fetch", refused: !response.ok, detail: (String(response.status) + " " + (await response.text())).slice(0, 200) });
  } catch (error) {
    results.push({ command: "direct ipc:// fetch", refused: true, detail: String(error).slice(0, 200) });
  }
  const commands = results.filter((r) => "command" in r);
  document.title = "ORIGIN-PROBE:" + JSON.stringify({ origin: location.origin, allRefused: commands.every((r) => r.refused), results });
})();
</script>"#;

/// Serves the origin-probe page on a loopback port for a minute.
fn serve_origin_probe() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        let _ = listener.set_nonblocking(true);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        ORIGIN_PAGE.len(),
                        ORIGIN_PAGE
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    });
    Ok(port)
}

fn wait_event(
    cursor: &mut LogCursor,
    event: &str,
    timeout: Duration,
    seen: &mut Vec<Value>,
) -> Option<Value> {
    cursor.wait_for(event, |_| true, timeout, seen)
}

/// G04: 10,000 committed synthetic changes over 60 s through the native
/// Channel with an exact final projection, then the stream fault suite.
pub fn stream(ctx: &Ctx, count: u32, duration_ms: u32) -> Result<Value, String> {
    let run = Run::create(&ctx.evidence_root(), "g04-stream", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    ensure_ui(ctx)?;
    let before = compare_projection(ctx, "before");
    run.write_json("projection-before.json", &before)
        .map_err(|e| e.to_string())?;
    let mut cursor = ctx.companion().log();
    let mut desktop = ctx.app().desktop_log();
    let started = ctx.companion().request(
        ControlRequestBody::QualifySyntheticChanges {
            count,
            duration_ms,
            sessions: 16,
        },
        Duration::from_secs(10),
    )?;
    let mut seen = Vec::new();
    let done = cursor.wait_for(
        "SYNTHETIC_RUN_DONE",
        |_| true,
        Duration::from_millis(u64::from(duration_ms) + 120_000),
        &mut seen,
    );
    let retired_backpressure = seen
        .iter()
        .filter(|l| l["event"] == "VIEW_RETIRED_BACKPRESSURE")
        .count();
    threadspace_harness::pause_ms(3000);
    let after = compare_projection(ctx, "after-synthetic-run");
    let recoveries = desktop
        .read_new()
        .into_iter()
        .filter(|l| l["event"] == "OFFICE_VIEW_RECOVERED")
        .count();
    let workload = json!({
        "started": format!("{started:?}"),
        "done": done,
        "committed": done.as_ref().and_then(|d| d["committed"].as_u64()),
        "failed": done.as_ref().and_then(|d| d["failed"].as_u64()),
        "elapsedMs": done.as_ref().and_then(|d| d["elapsedMs"].as_u64()),
        "patchBroadcasts": seen.iter().filter(|l| l["event"] == "PATCH_BROADCAST").count(),
        "viewRetiredBackpressure": retired_backpressure,
        "viewRecoveries": recoveries,
        "projection": after,
    });
    run.write_json("workload.json", &workload)
        .map_err(|e| e.to_string())?;
    let workload_ok = workload["committed"].as_u64() == Some(u64::from(count))
        && workload["failed"].as_u64() == Some(0)
        && workload["elapsedMs"]
            .as_u64()
            .is_some_and(|ms| ms >= u64::from(duration_ms) - 1000)
        && after["equal"] == true;

    let faults = stream_faults(ctx, &run)?;
    let faults_ok = faults.iter().all(|f| f["pass"] == true);
    let summary = json!({
        "gate": "G04",
        "pass": workload_ok && faults_ok,
        "workload": { "count": count, "durationMs": duration_ms, "committed": workload["committed"], "failed": workload["failed"], "elapsedMs": workload["elapsedMs"], "finalProjectionEqual": after["equal"], "viewRetiredBackpressure": retired_backpressure, "viewRecoveries": recoveries },
        "faults": faults,
    });
    run.write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run.dir }))
}

fn stream_faults(ctx: &Ctx, run: &Run) -> Result<Vec<Value>, String> {
    let app = ctx.app();
    let mut results = Vec::new();
    let mut fault = |name: &str,
                     setup: &dyn Fn() -> Result<Value, String>,
                     expect_recovery: bool,
                     changes: u32|
     -> Result<(), String> {
        let mut desktop = app.desktop_log();
        let mut companion = ctx.companion().log();
        let before = app.view_command("projection-digest", json!({}), Duration::from_secs(20))?;
        let connects_before = before["result"]["stream"]["connects"].as_u64().unwrap_or(0);
        let setup_result = setup()?;
        // Drive committed changes so a broken stream has something to miss.
        let burst = ctx.companion().request(
            ControlRequestBody::QualifySyntheticChanges {
                count: changes,
                duration_ms: 2000,
                sessions: 4,
            },
            Duration::from_secs(10),
        );
        let mut seen = Vec::new();
        let burst_done = wait_event(
            &mut companion,
            "SYNTHETIC_RUN_DONE",
            Duration::from_secs(30),
            &mut seen,
        );
        // Allow the independent watchdog (5 s stall + status check) to act,
        // then a settled steady state before the next fault.
        threadspace_harness::pause_ms(15_000);
        let projection = compare_projection(ctx, name);
        let connects_after = projection["viewStream"]["connects"].as_u64().unwrap_or(0);
        let desktop_lines = desktop.read_new();
        let recovered_view = desktop_lines
            .iter()
            .filter(|l| l["event"] == "OFFICE_VIEW_RECOVERED")
            .count();
        let reconnected = connects_after > connects_before || recovered_view > 0;
        let record = json!({
            "fault": name,
            "burstRequest": burst.as_ref().map(|reply| format!("{reply:?}")).unwrap_or_else(|e| format!("error: {e}")),
            "burstDone": burst_done,
            "setup": setup_result,
            "connectsBefore": connects_before,
            "connectsAfter": connects_after,
            "viewRecoveries": recovered_view,
            "reconnected": reconnected,
            "projectionEqual": projection["equal"],
            "pass": burst.is_ok()
                && burst_done.is_some()
                && projection["equal"] == true
                && (!expect_recovery || reconnected),
            "projection": projection,
            "desktopLog": desktop_lines,
        });
        run.append("faults.jsonl", &record)
            .map_err(|e| e.to_string())?;
        results.push(record);
        Ok(())
    };
    fault(
        "missing-frame-then-delivery",
        &|| {
            app.view_command(
                "drop-frames",
                json!({ "mode": "next" }),
                Duration::from_secs(20),
            )
        },
        true,
        20,
    )?;
    fault(
        "missing-frames-no-further-delivery",
        &|| {
            app.view_command(
                "drop-frames",
                json!({ "mode": "all" }),
                Duration::from_secs(20),
            )
        },
        true,
        20,
    )?;
    fault(
        "missing-acks-stalled-window",
        &|| {
            app.view_command(
                "ack-mode",
                json!({ "withhold": true }),
                Duration::from_secs(20),
            )
        },
        true,
        60,
    )?;
    fault(
        "delayed-acks",
        &|| {
            app.view_command(
                "ack-mode",
                json!({ "delayMs": 400 }),
                Duration::from_secs(20),
            )
        },
        false,
        20,
    )?;
    let _ = app.view_command("ack-mode", json!({}), Duration::from_secs(20));
    fault(
        "renderer-reconnect",
        &|| {
            app.view_command(
                "reconnect",
                json!({ "reason": "qualification" }),
                Duration::from_secs(20),
            )
        },
        true,
        20,
    )?;
    Ok(results)
}

/// The UI's layer-0 windows, each as [number, on screen, width, height, title].
fn layer0_windows(ctx: &Ctx, pid: u32) -> Vec<Value> {
    ctx.native.json(&["windows", &pid.to_string()])["windows"]
        .as_array()
        .map(|ws| {
            ws.iter()
                .filter(|w| w["layer"].as_i64() == Some(0))
                .map(|w| json!([w["id"], w["onScreen"], w["width"], w["height"], w["name"]]))
                .collect()
        })
        .unwrap_or_default()
}

/// Office windows (the live view and any retired shell, all titled
/// `OFFICE_TITLE`) and all layer-0 windows. AppKit also gives the process
/// windows of its own (menu-bar strips after a display-mode change, text
/// input), which are counted apart so they cannot pass for shells or hide one.
fn shell_counts(windows: &[Value]) -> (usize, usize) {
    let office = windows.iter().filter(|w| w[4] == OFFICE_TITLE).count();
    (office, windows.len())
}

/// The UI's window counts once they have held still for five seconds (at
/// most a minute), with every change seen on the way. At least one office
/// window (the live one) must be seen, or titles were unreadable and the
/// count proves nothing.
fn settle_shells(ctx: &Ctx, pid: u32) -> Value {
    let started = Instant::now();
    let mut counts = shell_counts(&layer0_windows(ctx, pid));
    let mut seen = vec![json!([counts.0, counts.1])];
    let mut stable_since = Instant::now();
    while stable_since.elapsed() < SHELLS_STABLE && started.elapsed() < SHELLS_SETTLE_CAP {
        threadspace_harness::pause_ms(500);
        let now = shell_counts(&layer0_windows(ctx, pid));
        if now != counts {
            counts = now;
            seen.push(json!([now.0, now.1]));
            stable_since = Instant::now();
        }
    }
    json!({
        "count": counts.0,
        "layer0": counts.1,
        "titlesReadable": counts.0 >= 1,
        "settled": stable_since.elapsed() >= SHELLS_STABLE,
        "waitedMs": started.elapsed().as_millis() as u64,
        "seen": seen,
    })
}

/// The live office window's frame (Accessibility) and whether an office
/// window is on screen.
fn office_window(ctx: &Ctx, pid: u32) -> Value {
    let frame = ctx.native.ax_window(pid, Some(OFFICE_TITLE))["frame"].clone();
    let visible = layer0_windows(ctx, pid)
        .iter()
        .any(|w| w[4] == OFFICE_TITLE && w[1] == true);
    json!({ "frame": frame, "visible": visible })
}

/// Frames equal within 3 points.
fn same_frame(a: &Value, b: &Value) -> bool {
    ["x", "y", "width", "height"].iter().all(|k| {
        a[k].as_f64()
            .zip(b[k].as_f64())
            .is_some_and(|(a, b)| (a - b).abs() <= 3.0)
    })
}

/// A resource sample with the office window in front for a second, so every
/// sample sees the same window state (a covered window's backing store can be
/// purged).
fn sample_resources(ctx: &Ctx, pid: u32) -> Value {
    ctx.native.ax_action(pid, "raise", Some(OFFICE_TITLE));
    threadspace_harness::pause_ms(1000);
    ui_resources(ctx, pid)
}

/// Least-squares slope of the UI footprint across samples, in bytes per
/// sample; `None` with fewer than two.
#[allow(clippy::cast_precision_loss)]
fn footprint_slope(samples: &[Value]) -> Option<f64> {
    let ys: Vec<f64> = samples.iter().filter_map(|s| s["footprintBytes"].as_u64()).map(|b| b as f64).collect();
    if ys.len() < 2 || ys.len() != samples.len() {
        return None;
    }
    let n = ys.len() as f64;
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = ys.iter().sum::<f64>() / n;
    let (num, den) = ys.iter().enumerate().fold((0.0, 0.0), |(num, den), (i, y)| {
        let dx = i as f64 - mean_x;
        (num + dx * (y - mean_y), den + dx * dx)
    });
    Some(num / den)
}

/// C-04's retired-native verdict recomputed from a retained run directory:
/// its recovery cases (`cases.jsonl`, bootstrap excluded) against the
/// recoveries its summary ran (the three single cases plus the repeats).
pub fn c04_verdict(dir: &str) -> Result<Value, String> {
    let dir = std::path::Path::new(dir);
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    let summary: Value = serde_json::from_str(&read("summary.json")?).map_err(|e| e.to_string())?;
    let cases: Vec<Value> = read("cases.jsonl")?
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|c: &Value| c["case"] != "bootstrap-with-companion-unavailable")
        .collect();
    let repeats = summary["nativeWindowShells"]["recoveries"]
        .as_u64()
        .ok_or("summary names no repeated recoveries")?;
    let verdict = retired_native_released(&cases, 3 + usize::try_from(repeats).map_err(|e| e.to_string())?);
    Ok(json!({ "run": dir.display().to_string(), "retiredNative": verdict }))
}

/// C-04's native oracle over a run's `recoveries` recovery cases (the
/// bootstrap case excluded). Each case names the office incarnation its
/// recovery retired (`OFFICE_VIEW_RECOVERED`), all distinct. The desktop's
/// qualification-only `OFFICE_NATIVE_WINDOWS` account read in the last case,
/// logged after that case's recovery, must list exactly those incarnations,
/// one entry each, with the window, its delegate, its content view and the
/// web view each reported and released (`false`). Absent or null data fails:
/// it is never read as no survivors.
fn retired_native_released(cases: &[Value], recoveries: usize) -> Value {
    const RELEASED: [&str; 4] = [
        "windowAlive",
        "delegateAlive",
        "contentViewAlive",
        "webviewAlive",
    ];
    let named: Vec<Option<&str>> = cases
        .iter()
        .map(|c| c["recovered"]["detail"]["retiredIncarnation"].as_str())
        .collect();
    let expected: BTreeSet<&str> = named.iter().flatten().copied().collect();
    let report = cases.last().map_or(&Value::Null, |c| &c["nativeWindows"]);
    let retired = report["retired"].as_array();
    let entries = retired.map_or(&[][..], Vec::as_slice);
    let observed: BTreeSet<&str> = entries
        .iter()
        .filter_map(|r| r["incarnation"].as_str())
        .collect();
    let unreleased: Vec<&Value> = entries
        .iter()
        .filter(|r| {
            !RELEASED
                .iter()
                .all(|flag| r[*flag].as_bool() == Some(false))
        })
        .map(|r| &r["incarnation"])
        .collect();
    let alive = |flag: &str| entries.iter().filter(|r| r[flag] == true).count();
    let pass = named.len() == recoveries
        && expected.len() == recoveries
        && retired.is_some()
        && observed.len() == entries.len()
        && observed == expected
        && unreleased.is_empty()
        && report["afterRecoveryOf"]
            .as_str()
            .is_some_and(|after| named.last() == Some(&Some(after)));
    json!({
        "recoveries": recoveries,
        "retiredIncarnations": named.iter().flatten().count(),
        "afterRecoveryOf": report["afterRecoveryOf"],
        "retired": retired.map(Vec::len),
        "missing": expected.difference(&observed).collect::<Vec<_>>(),
        "extra": observed.difference(&expected).collect::<Vec<_>>(),
        "unreleased": unreleased,
        "windowsAlive": alive("windowAlive"),
        "delegatesAlive": alive("delegateAlive"),
        "contentViewsAlive": alive("contentViewAlive"),
        "webviewsAlive": alive("webviewAlive"),
        "pass": pass,
    })
}

/// One sample of the UI's native resources: office windows (shells and the
/// live view), every layer-0 window, the WebContent processes serving it and
/// its physical footprint.
fn ui_resources(ctx: &Ctx, pid: u32) -> Value {
    let web_content = procs::web_content_of(pid as i32);
    let windows = layer0_windows(ctx, pid);
    let (office, layer0) = shell_counts(&windows);
    json!({
        "windowShells": office,
        "layer0Windows": layer0,
        "windows": windows,
        "webContentProcesses": web_content.as_ref().map(Vec::len),
        "webContentPids": web_content,
        "footprintBytes": procs::phys_footprint(pid as i32),
    })
}

/// SPEC §18.5 view recovery: document replacement, replacement with a native
/// request in flight, retirement with unconsumed cached data, repeated
/// recovery, and bootstrap with the companion unavailable.
pub fn recovery(ctx: &Ctx, repeats: u32) -> Result<Value, String> {
    let run = Run::create(&ctx.evidence_root(), "view-recovery", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let app = ctx.app();
    ensure_ui(ctx)?;
    let environment = ctx.environment();
    run.write_json("environment.json", &environment)
        .map_err(|e| e.to_string())?;
    let mut cases = Vec::new();
    let mut case = |name: &str,
                    trigger: &dyn Fn() -> Result<Value, String>,
                    expected: &str|
     -> Result<(), String> {
        let _gui = ctx.gui(&format!("view recovery: {name}"))?;
        // WebKit suspends a covered page's timers, and with them the view's
        // stall watchdog, so the office window is brought forward first.
        let raised = app
            .processes()
            .first()
            .map(|ui| ctx.native.ax_action(ui.pid as u32, "raise", Some(OFFICE_TITLE))["performed"].clone());
        let mut desktop = app.desktop_log();
        let mut companion = ctx.companion().log();
        let triggered = trigger()?;
        let mut lines = Vec::new();
        let recovered = desktop.wait_for(
            "OFFICE_VIEW_RECOVERED",
            |_| true,
            Duration::from_secs(60),
            &mut lines,
        );
        // The recreated view's own hydration: a subscription attached before
        // the recovery (a deliberately stalled one) never hydrates.
        let recovered_ms = recovered.as_ref().and_then(|r| r["ms"].as_i64()).unwrap_or(i64::MAX);
        let hydrated = companion.wait_for(
            "VIEW_HYDRATED",
            |line| line["ts"].as_i64().is_some_and(|ts| ts >= recovered_ms),
            Duration::from_secs(30),
            &mut Vec::new(),
        );
        threadspace_harness::pause_ms(2000);
        lines.extend(desktop.read_new());
        let stale_refusals: Vec<Value> = lines
            .iter()
            .filter(|l| l["event"] == "STALE_VIEW_REFUSED")
            .cloned()
            .collect();
        let created = lines
            .iter()
            .filter(|l| l["event"] == "OFFICE_VIEW_CREATED")
            .count();
        // The desktop's qualification-only account of retired native windows
        // (absent from builds that predate it).
        let native_windows = lines
            .iter()
            .rev()
            .find(|l| l["event"] == "OFFICE_NATIVE_WINDOWS")
            .map(|l| l["detail"].clone());
        let projection = compare_projection(ctx, name);
        let ok = recovered
            .as_ref()
            .is_some_and(|r| r["detail"]["reason"] == expected && r["detail"]["created"] == true)
            && hydrated.is_some()
            && projection["equal"] == true;
        let record = json!({
            "case": name,
            "pass": ok,
            "raised": raised,
            "trigger": triggered,
            "recovered": recovered,
            "newViewHydrated": hydrated.is_some(),
            "hydratedSubscription": hydrated.as_ref().map(|h| h["subscriptionId"].clone()),
            "officeViewsCreated": created,
            "staleViewRefusals": stale_refusals,
            "projectionEqual": projection["equal"],
            "uiProcesses": app.processes(),
            "nativeWindows": native_windows,
        });
        run.append("cases.jsonl", &record)
            .map_err(|e| e.to_string())?;
        cases.push(record);
        Ok(())
    };
    case(
        "document-replacement",
        &|| app.view_command_nowait("reload", json!({})),
        "MAIN_DOCUMENT_REPLACED",
    )?;
    case(
        "replacement-with-request-in-flight",
        &|| app.view_command_nowait("request-then-reload", json!({})),
        "MAIN_DOCUMENT_REPLACED",
    )?;
    case(
        "retired-with-unconsumed-cached-frames",
        &|| {
            app.view_command(
                "ack-mode",
                json!({ "stallNext": true }),
                Duration::from_secs(20),
            )?;
            app.view_command_nowait(
                "reconnect",
                json!({ "reason": "qualification: stall next stream" }),
            )
        },
        "UNCONSUMED_DATA_ON_RETIREMENT",
    )?;
    // Native window shells (C-04): each recovery's retired window is
    // measured, not assumed gone (Tauri alpha.4 deregisters it; AppKit can
    // keep it). Shells are the office-titled layer-0 windows; every layer-0
    // window is recorded beside them. `before` is taken once the recoveries
    // above have settled, so one-time costs of a first recovery are not
    // counted as growth; `after` is read at once and again once the counts
    // hold still.
    let ui = app.processes().into_iter().next();
    let ui_pid = ui.as_ref().map(|ui| ui.pid as u32);
    let before = ui_pid.map(|pid| settle_shells(ctx, pid));
    let resources_before = ui_pid.map(|pid| sample_resources(ctx, pid));
    // Preserved across the recoveries (D-0006): the office window's bounds
    // and visibility, and the durable intent backlog.
    let office_before = ui_pid.map(|pid| office_window(ctx, pid));
    let intents_before = crate::handoff::durable(ctx);
    let mut growth = Vec::new();
    for index in 1..=repeats {
        let name = format!("repeated-replacement-{index:02}");
        case(
            &name,
            &|| app.view_command_nowait("reload", json!({})),
            "MAIN_DOCUMENT_REPLACED",
        )?;
        if let Some(pid) = ui_pid {
            let sample = sample_resources(ctx, pid);
            run.append("resources.jsonl", &json!({ "case": name, "sample": sample }))
                .map_err(|e| e.to_string())?;
            growth.push(sample);
        }
    }
    let immediate = ui_pid.map(|pid| shell_counts(&layer0_windows(ctx, pid)));
    let after = ui_pid.map(|pid| settle_shells(ctx, pid));
    let resources_after = ui_pid.map(|pid| sample_resources(ctx, pid));
    let office_after = ui_pid.map(|pid| office_window(ctx, pid));
    let intents_after = crate::handoff::durable(ctx);
    // Incarnation rejection in the recreated view: the IPC suite's stale
    // epoch, context and retired-subscription cases, and the subscription of
    // the first case's view, which the recoveries retired.
    let suite = app.view_command("ipc-suite", json!({ "rounds": 100 }), Duration::from_secs(300));
    let retired_subscription = cases
        .first()
        .and_then(|c| c["hydratedSubscription"].as_str().map(str::to_owned));
    let retired_probe = retired_subscription.as_ref().map(|id| {
        app.view_command(
            "retired-subscription-probe",
            json!({ "subscriptionId": id }),
            Duration::from_secs(20),
        )
    });
    let same_ui = ui.as_ref().is_some_and(procs::Incarnation::alive);
    let measured = |s: &Value| s["settled"] == true && s["titlesReadable"] == true;
    let shells_pass = same_ui
        && matches!((&before, &after), (Some(b), Some(a))
            if measured(b) && measured(a) && a["count"] == b["count"]);
    // The desktop's own account after the last recovery (the three single
    // recoveries above and the repeats): every retired office window's native
    // objects released, which the window-server count alone cannot show.
    let retired_native = retired_native_released(&cases, 3 + repeats as usize);
    let retired_pass = retired_native["pass"] == true;
    let window_shells = json!({
        "uiPid": ui_pid,
        "retiredNative": retired_native,
        "sameUiIncarnation": same_ui,
        "recoveries": repeats,
        "before": before.as_ref().map(|b| b["count"].clone()),
        "afterImmediate": immediate.map(|(office, _)| office),
        "after": after.as_ref().map(|a| a["count"].clone()),
        "layer0": {
            "before": before.as_ref().map(|b| b["layer0"].clone()),
            "afterImmediate": immediate.map(|(_, layer0)| layer0),
            "after": after.as_ref().map(|a| a["layer0"].clone()),
        },
        "beforeSettle": before,
        "afterSettle": after,
        "pass": shells_pass,
    });
    let slope = footprint_slope(&growth);
    let sustained = footprint_slope(&growth[growth.len() / 2..]);
    let resources_pass = sustained.is_some_and(|slope| slope <= FOOTPRINT_SLOPE_MAX)
        && matches!((&resources_before, &resources_after), (Some(b), Some(a))
            if a["webContentProcesses"].as_u64().zip(b["webContentProcesses"].as_u64()).is_some_and(|(a, b)| a <= b));
    let resources = json!({
        "before": resources_before,
        "after": resources_after,
        "perRecovery": growth,
        "footprintSlopeBytesPerRecovery": slope,
        "footprintSecondHalfSlopeBytesPerRecovery": sustained,
        "footprintSlopeMaxBytes": FOOTPRINT_SLOPE_MAX,
        "pass": resources_pass,
    });
    let office_pass = matches!((&office_before, &office_after), (Some(b), Some(a))
        if b["visible"] == true && a["visible"] == true && same_frame(&b["frame"], &a["frame"]));
    let intents_pass = intents_before["pendingIntents"] == intents_after["pendingIntents"]
        && intents_after["backlogTemporaries"].as_array().is_some_and(Vec::is_empty);
    let suite_ok = suite.as_ref().is_ok_and(|s| {
        s["ok"] == true && s["result"]["roundTripsOk"] == true && s["result"]["passed"] == s["result"]["total"]
    });
    let probe_refused = retired_probe.as_ref().is_some_and(|probe| {
        probe.as_ref().is_ok_and(|p| {
            p["ok"] == true
                && p["result"]["ok"] == false
                && matches!(p["result"]["code"].as_str(), Some("UNKNOWN_SUBSCRIPTION" | "STALE_CONTEXT"))
        })
    });
    let preserved = json!({
        "office": { "before": office_before, "after": office_after, "pass": office_pass },
        "pendingIntents": {
            "before": intents_before["pendingIntents"],
            "after": intents_after["pendingIntents"],
            "pass": intents_pass,
        },
        "incarnationRejection": {
            "ipcSuite": suite.as_ref().map(|s| json!({ "passed": s["result"]["passed"], "total": s["result"]["total"], "roundTripsOk": s["result"]["roundTripsOk"] })).unwrap_or_else(|e| json!(e)),
            "retiredSubscription": retired_subscription,
            "retiredProbe": retired_probe.map(|p| p.map(|p| p["result"].clone()).unwrap_or_else(|e| json!(e))),
            "pass": suite_ok && probe_refused,
        },
    });
    let c04_pass =
        shells_pass && retired_pass && resources_pass && office_pass && intents_pass && suite_ok && probe_refused;
    let delays: Vec<Value> = cases
        .iter()
        .filter_map(|c| {
            c["recovered"]["detail"]["delayMs"]
                .as_u64()
                .map(|d| json!({ "case": c["case"], "delayMs": d }))
        })
        .collect();

    // Companion unavailable during UI bootstrap: freeze it, start a UI, thaw it.
    // (Packaged only: an attached dev UI shares the dev identifier's UI lock.)
    let _gui = ctx.gui("view recovery: bootstrap with companion frozen")?;
    if std::env::var_os("THREADSPACE_HARNESS_ATTACHED_DEV_UI").is_some() {
        let passed = cases.iter().filter(|c| c["pass"] == true).count();
        let summary = json!({ "area": "view-recovery", "mode": "attached dev UI (bootstrap case skipped)", "pass": passed == cases.len() && c04_pass, "passed": passed, "total": cases.len(), "recoveryDelays": delays, "nativeWindowShells": window_shells, "uiResources": resources, "preserved": preserved });
        run.write_json("summary.json", &summary)
            .map_err(|e| e.to_string())?;
        return Ok(json!({ "summary": summary, "dir": run.dir }));
    }
    app.stop_all();
    let companion = ctx.companion().incarnation().ok_or("no companion")?;
    let mut cursor = ctx.companion().log();
    procs::signal(companion.pid, libc::SIGSTOP);
    let launched = app.launch_packaged(&[]);
    threadspace_harness::pause_ms(8000);
    let status_while_stopped = app
        .reports("renderer-attestation", threadspace_harness::now_ms() - 9000)
        .len();
    procs::signal(companion.pid, libc::SIGCONT);
    let hydrated = app.wait_hydrated(&mut cursor, Duration::from_secs(60));
    let bootstrap = json!({
        "case": "bootstrap-with-companion-unavailable",
        "launched": launched.as_ref().map(|l| json!(l.ui)).unwrap_or_else(|e| json!(e)),
        "frozenForMs": 8000,
        "rendererAttestedWhileFrozen": status_while_stopped > 0,
        "hydratedAfterThaw": hydrated.is_some(),
        "sameCompanion": ctx.companion().incarnation() == Some(companion.clone()),
        "pass": launched.is_ok() && hydrated.is_some(),
    });
    run.append("cases.jsonl", &bootstrap)
        .map_err(|e| e.to_string())?;
    cases.push(bootstrap);

    let passed = cases.iter().filter(|c| c["pass"] == true).count();
    let summary = json!({
        "area": "view-recovery",
        "pass": passed == cases.len() && c04_pass,
        "passed": passed,
        "total": cases.len(),
        "recoveryDelays": delays,
        "nativeWindowShells": window_shells,
        "uiResources": resources,
        "preserved": preserved,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "executableSha256": environment["executableSha256"],
        "companionSha256": environment["companionSha256"],
    });
    run.write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run.dir }))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    /// A retained view-recovery run's recovery cases (bootstrap excluded).
    fn recovery_cases(run: &str) -> Vec<Value> {
        let path = format!(
            "{}/../../../evidence/M1/{run}/cases.jsonl",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path)
            .expect("retained evidence run")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("case record"))
            .filter(|case| case["case"] != "bootstrap-with-companion-unavailable")
            .collect()
    }

    /// The last recovery case, which carries the final account.
    fn last(cases: &mut [Value]) -> &mut Value {
        cases.last_mut().expect("a recovery case")
    }

    /// The final account's retired entries.
    fn retired(cases: &mut [Value]) -> &mut Vec<Value> {
        last(cases)["nativeWindows"]["retired"]
            .as_array_mut()
            .expect("retired entries")
    }

    /// The accepted run (three single recoveries and 60 repeats) passes;
    /// every mutation of its retirement data fails.
    #[test]
    fn retired_native_objects_are_a_hard_c04_oracle() {
        let accepted = recovery_cases("view-recovery/20261008T015231Z-prod");
        assert_eq!(accepted.len(), 63);
        let verdict = super::retired_native_released(&accepted, 63);
        assert_eq!(verdict["pass"], true, "{verdict}");
        assert_eq!(verdict["retired"], 63);

        // (mutation, recoveries expected, change to the accepted cases)
        type Mutation = (&'static str, usize, fn(&mut [Value]));
        let mutations: [Mutation; 12] = [
            ("account deleted", 63, |c| {
                last(c)["nativeWindows"] = Value::Null;
            }),
            ("retired entries deleted", 63, |c| {
                last(c)["nativeWindows"]["retired"] = Value::Null;
            }),
            ("no account in any case", 63, |c| {
                c.iter_mut().for_each(|c| c["nativeWindows"] = Value::Null);
            }),
            ("one retired window alive", 63, |c| {
                retired(c)[17]["windowAlive"] = json!(true);
            }),
            ("one flag unreported", 63, |c| {
                retired(c)[17]["delegateAlive"] = Value::Null;
            }),
            ("one more recovery expected", 64, |_| {}),
            ("one fewer recovery expected", 62, |_| {}),
            ("one recovery names no incarnation", 63, |c| {
                c[5]["recovered"]["detail"]["retiredIncarnation"] = Value::Null;
            }),
            ("one incarnation omitted", 63, |c| {
                retired(c).remove(5);
            }),
            ("one incarnation listed twice", 63, |c| {
                let again = retired(c)[5].clone();
                retired(c).push(again);
            }),
            ("one foreign incarnation", 63, |c| {
                retired(c)[5]["incarnation"] = json!("00000000-0000-0000-0000-000000000000");
            }),
            ("account logged before the last recovery", 63, |c| {
                let earlier = c[c.len() - 2]["recovered"]["detail"]["retiredIncarnation"].clone();
                last(c)["nativeWindows"]["afterRecoveryOf"] = earlier;
            }),
        ];
        for (mutation, recoveries, mutate) in mutations {
            let mut cases = accepted.clone();
            mutate(&mut cases);
            let verdict = super::retired_native_released(&cases, recoveries);
            assert_eq!(verdict["pass"], false, "{mutation} must fail: {verdict}");
        }
    }

    /// The retained run that kept every retired window alive (before the
    /// D-0008 containment) fails the oracle on its own data.
    #[test]
    fn retired_native_oracle_fails_the_leaking_run() {
        let leaking = recovery_cases("c04/20261007T233730Z-diagnostic");
        let verdict = super::retired_native_released(&leaking, leaking.len());
        assert_eq!(verdict["pass"], false, "{verdict}");
        assert_eq!(verdict["windowsAlive"], 8);
    }

    #[test]
    fn footprint_slope_is_the_least_squares_trend_per_sample() {
        let leak: Vec<_> = (0..5).map(|i| json!({ "footprintBytes": 1000 + 300 * i })).collect();
        assert_eq!(super::footprint_slope(&leak), Some(300.0));
        let flat = [json!({ "footprintBytes": 10 }), json!({ "footprintBytes": 30 }), json!({ "footprintBytes": 10 }), json!({ "footprintBytes": 30 })];
        assert!(super::footprint_slope(&flat).is_some_and(|s| s.abs() < 5.0));
        assert_eq!(super::footprint_slope(&[json!({ "footprintBytes": 1 })]), None);
        assert_eq!(super::footprint_slope(&[json!({}), json!({ "footprintBytes": 1 })]), None, "a missing sample is no measurement");
    }
}
