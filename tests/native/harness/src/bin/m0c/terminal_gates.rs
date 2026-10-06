//! G08 Terminal reliability negatives with real Claude sessions in disposable,
//! harness-owned Terminal windows. Every route is checked against an
//! independent readback (the harness's own selected-tab query and
//! LaunchServices' frontmost application); a wrong target is a route the
//! companion reports exact whose independently read selected tab is not the
//! expected session's TTY.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_contracts::route::RouteRequest;
use threadspace_harness::evidence::Run;
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::procs;
use threadspace_harness::run::run;
use threadspace_harness::terminal::{self, Tab};
use threadspace_provider_claude::inventory::{ClaudeCli, ClaudeInstall, Inventory};

use crate::bridge_gates::companion_snapshot;
use crate::ctx::Ctx;

pub struct ClaudeTab {
    pub tab: Tab,
    pub native_session_id: String,
    pub session_id: String,
    pub pid: i32,
}

fn cli() -> Option<ClaudeCli> {
    let home = threadspace_relay::paths::home_dir()?;
    let install = ClaudeInstall::resolve(&home.join(".local/bin/claude"))?;
    Some(ClaudeCli {
        binary: install.binary,
        home,
        timeout: Duration::from_secs(8),
        now_ms: threadspace_harness::now_ms,
    })
}

fn same_dir(a: &str, b: &Path) -> bool {
    let left = PathBuf::from(a)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(a));
    let right = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    left == right
}

/// Launches a direct interactive Claude session in a new disposable window,
/// accepts the folder-trust prompt in that window only, and waits for the
/// companion to bind it to that window's TTY.
pub fn spawn_claude(ctx: &Ctx, dir: PathBuf) -> Result<ClaudeTab, String> {
    let launcher = threadspace_relay::paths::home_dir()
        .ok_or("no home")?
        .join(".local/bin/claude");
    let command = format!(
        "cd {} && exec {}",
        terminal::shell_quote(&dir.display().to_string()),
        terminal::shell_quote(&launcher.display().to_string())
    );
    let tab = {
        let _gui = ctx.gui("g08 spawn Claude window")?;
        let tab = Tab::open(dir.clone(), &command)?;
        threadspace_harness::pause_ms(4000);
        // The folder-trust dialog preselects "No, exit" (Claude Code
        // 2.1.291): Down selects "Yes, I trust this folder", Return confirms.
        tab.type_line("\u{1b}[B");
        tab
    };
    let cli = cli().ok_or("claude not installed")?;
    let started = Instant::now();
    let (native, pid) = loop {
        if started.elapsed() > Duration::from_secs(60) {
            return Err("the new Claude session never appeared in inventory".into());
        }
        if let Ok(snapshot) = cli.fetch()
            && let Some(row) = snapshot.rows.iter().find(|row| {
                row.is_interactive()
                    && row.cwd.as_deref().is_some_and(|cwd| same_dir(cwd, &dir))
                    && row.full_session_id().is_some()
            })
        {
            break (
                row.full_session_id().unwrap_or_default().to_owned(),
                row.live_pid().unwrap_or(0),
            );
        }
        threadspace_harness::pause_ms(1000);
    };
    loop {
        if started.elapsed() > Duration::from_secs(120) {
            return Err(format!(
                "companion never bound session {native} to {}",
                tab.tty
            ));
        }
        if let Ok((_, snapshot)) = companion_snapshot(ctx)
            && let Some(session) = snapshot
                .sessions
                .iter()
                .find(|s| s.native_session_id == native)
            && session
                .binding
                .as_ref()
                .is_some_and(|b| b.locator == tab.tty)
        {
            return Ok(ClaudeTab {
                session_id: session.session_id.clone(),
                native_session_id: native,
                pid,
                tab,
            });
        }
        threadspace_harness::pause_ms(1000);
    }
}

fn frontmost_bundle() -> Option<String> {
    let front = run("/usr/bin/lsappinfo", &["front"], Duration::from_secs(3));
    let info = run(
        "/usr/bin/lsappinfo",
        &["info", "-only", "bundleid", front.stdout.trim()],
        Duration::from_secs(3),
    );
    info.stdout
        .split("bundleID=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .map(str::to_owned)
}

/// One Return through the companion plus an independent readback.
pub fn route(ctx: &Ctx, session_id: &str, expected_tty: Option<&str>) -> Value {
    let request = RouteRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_owned(),
        chosen_binding_id: None,
        expected_binding_revision: None,
    };
    // Callers hold the shared GUI lock around each case (setup + route).
    let started = Instant::now();
    let outcome = ctx.companion().request(
        ControlRequestBody::ReturnToSession { route: request },
        Duration::from_secs(25),
    );
    let elapsed = started.elapsed().as_millis() as u64;
    threadspace_harness::pause_ms(300);
    let selected = terminal::selected_tty();
    let front = frontmost_bundle();
    match outcome {
        Ok(ControlResponseBody::Routed { result }) => {
            let exact = format!("{:?}", result.surface_result) == "ExactNativeSurface"
                && format!("{:?}", result.session_verification) == "CurrentNativeRevalidated";
            let readback_matches = expected_tty.is_some_and(|tty| selected.as_deref() == Some(tty));
            json!({
                "surfaceResult": result.surface_result,
                "sessionVerification": result.session_verification,
                "inputReadiness": result.input_readiness,
                "reasonCode": result.reason_code,
                "focusPerformed": result.focus_performed,
                "latencyMs": result.latency_ms,
                "harnessElapsedMs": elapsed,
                "exact": exact,
                "independentSelectedTty": selected,
                "independentFrontmost": front,
                "expectedTty": expected_tty,
                "wrongTarget": exact && expected_tty.is_some() && !readback_matches,
            })
        }
        Ok(other) => json!({ "error": format!("unexpected {other:?}") }),
        Err(error) => {
            json!({ "refused": error, "independentSelectedTty": selected, "independentFrontmost": front, "wrongTarget": false })
        }
    }
}

/// Runs one case segment while holding the shared GUI lock. A lock that
/// cannot be taken (I/O error) does not stop the case; it is recorded by the
/// guard's absence only.
fn locked<T>(ctx: &Ctx, label: &str, action: impl FnOnce() -> T) -> T {
    let _gui = ctx.gui(label);
    action()
}

fn terminal_pid() -> Result<u32, String> {
    let list = terminal::terminal_process();
    match list.as_slice() {
        [one] => Ok(one.pid as u32),
        _ => Err(format!(
            "expected one Terminal process, found {}",
            list.len()
        )),
    }
}

pub fn negatives(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "g08-terminal", ctx.channel_name())
        .map_err(|e| e.to_string())?;
    let root = PathBuf::from(format!(
        "/private/tmp/ts-m0c-route-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let terminal_before = terminal::terminal_process();
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
    let a = spawn_claude(ctx, root.join("a"))?;
    let b = spawn_claude(ctx, root.join("b"))?;
    let spare = locked(ctx, "g08 open spare window", || {
        Tab::open_inert(root.join("spare"))
    })?;
    let tpid = terminal_pid()?;
    run_dir.write_json("sessions.json", &json!([
        { "label": "a", "sessionId": a.session_id, "nativeSessionId": a.native_session_id, "tty": a.tab.tty, "windowId": a.tab.window_id, "pid": a.pid },
        { "label": "b", "sessionId": b.session_id, "nativeSessionId": b.native_session_id, "tty": b.tab.tty, "windowId": b.tab.window_id, "pid": b.pid },
        { "label": "spare", "tty": spare.tty, "windowId": spare.window_id },
    ])).map_err(|e| e.to_string())?;
    let exact_ok = |r: &Value| r["exact"] == true && r["wrongTarget"] == false;

    let baseline = locked(ctx, "g08 baseline", || {
        route(ctx, &a.session_id, Some(&a.tab.tty))
    });
    record(
        &mut cases,
        "baseline-exact",
        baseline.clone(),
        exact_ok(&baseline),
    )?;

    // Reorder: another window in front, then Return.
    let reorder = locked(ctx, "g08 reorder", || {
        spare.select();
        ctx.native.ax_action(tpid, "raise", Some(&spare.title()));
        route(ctx, &a.session_id, Some(&a.tab.tty))
    });
    record(&mut cases, "reorder", reorder.clone(), exact_ok(&reorder))?;

    // Move: the target window to new bounds.
    let moved = locked(ctx, "g08 move", || {
        a.tab.set_bounds(120, 140, 900, 640);
        route(ctx, &a.session_id, Some(&a.tab.tty))
    });
    record(&mut cases, "move", moved.clone(), exact_ok(&moved))?;

    // Minimize, then Return must unminimize and read back exactly.
    let (minimized, restore, after) = locked(ctx, "g08 minimize", || {
        let minimized = ctx.native.ax_action(tpid, "minimize", Some(&a.tab.title()));
        let restore = route(ctx, &a.session_id, Some(&a.tab.tty));
        let after = ctx.native.ax_window(tpid, Some(&a.tab.title()));
        (minimized, restore, after)
    });
    let minimize_ok = exact_ok(&restore) && after["minimized"] == false;
    record(
        &mut cases,
        "minimize-then-return",
        json!({ "minimize": minimized["performed"], "route": restore, "windowAfter": after }),
        minimize_ok,
    )?;

    // Fullscreen (its own Space): record the actual outcome; never a wrong target.
    let (full, full_route, exit) = locked(ctx, "g08 fullscreen", || {
        let full = ctx
            .native
            .ax_action(tpid, "fullscreen", Some(&a.tab.title()));
        spare.select();
        ctx.native.ax_action(tpid, "raise", Some(&spare.title()));
        let full_route = route(ctx, &a.session_id, Some(&a.tab.tty));
        let exit = ctx
            .native
            .ax_action(tpid, "exit-fullscreen", Some(&a.tab.title()));
        (full, full_route, exit)
    });
    let full_ok = full_route["wrongTarget"] == false;
    record(
        &mut cases,
        "fullscreen-space-then-return",
        json!({ "enteredFullscreen": full["after"]["fullScreen"], "route": full_route, "exitFullscreen": exit["performed"] }),
        full_ok,
    )?;

    // Foreground mismatch: provider stopped in the background.
    let background = locked(ctx, "g08 foreground mismatch", || {
        procs::signal(b.pid, libc::SIGTSTP);
        threadspace_harness::pause_ms(1500);
        let background = route(ctx, &b.session_id, Some(&b.tab.tty));
        procs::signal(b.pid, libc::SIGCONT);
        b.tab.type_line("fg");
        threadspace_harness::pause_ms(1500);
        background
    });
    let readiness_ok = background["inputReadiness"] != "FOREGROUND_COMPATIBLE"
        && background["wrongTarget"] == false;
    record(
        &mut cases,
        "foreground-process-mismatch",
        background.clone(),
        readiness_ok,
    )?;

    // Selection/readback race: another window is raised while the route runs.
    let race_results: Vec<Value> = (0..5)
        .map(|_| {
            locked(ctx, "g08 selection race", || {
                let spare_title = spare.title();
                let native_bin = ctx.repo.join("target/native-tools/ts-native");
                let racer = std::thread::spawn(move || {
                    threadspace_harness::pause_ms(450);
                    let _ = run(
                        &native_bin.display().to_string(),
                        &["ax-action", &tpid.to_string(), "raise", &spare_title],
                        Duration::from_secs(10),
                    );
                });
                let raced = route(ctx, &a.session_id, Some(&a.tab.tty));
                let _ = racer.join();
                raced
            })
        })
        .collect();
    let race_wrong = race_results
        .iter()
        .filter(|r| r["wrongTarget"] == true)
        .count();
    record(
        &mut cases,
        "selection-readback-race",
        json!({ "routes": race_results, "wrongTargets": race_wrong }),
        race_wrong == 0,
    )?;

    // Target close, then a stale TTY pathname reused by a new tab.
    let b_tty = b.tab.tty.clone();
    let (closed, gone) = locked(ctx, "g08 target close", || {
        let closed = b.tab.close();
        threadspace_harness::pause_ms(2000);
        (closed, route(ctx, &b.session_id, None))
    });
    record(
        &mut cases,
        "target-closed",
        json!({ "close": closed, "route": gone }),
        gone["exact"] != true && gone["wrongTarget"] == false,
    )?;
    let (reuse, stale) = locked(ctx, "g08 stale tty", || {
        let reuse = Tab::open_inert(root.join("reuse"));
        let stale = route(ctx, &b.session_id, None);
        (reuse, stale)
    });
    let reuse = reuse?;
    let reused_path = reuse.tty == b_tty;
    let stale_ok = stale["exact"] != true && stale["focusPerformed"] != true;
    record(
        &mut cases,
        "stale-tty-pathname",
        json!({ "newTabTty": reuse.tty, "reusedClosedPath": reused_path, "route": stale }),
        stale_ok,
    )?;

    // Target closed while the route is running.
    let c = spawn_claude(ctx, root.join("c"))?;
    let (during, closer) = locked(ctx, "g08 close during route", || {
        let window = c.tab.window_id;
        let tty = c.tab.tty.clone();
        let dir = c.tab.dir.clone();
        let marker = c.tab.marker.clone();
        let c_closer = std::thread::spawn(move || {
            threadspace_harness::pause_ms(300);
            Tab {
                marker,
                window_id: window,
                tty,
                dir,
            }
            .close()
        });
        let during = route(ctx, &c.session_id, Some(&c.tab.tty));
        (
            during,
            c_closer.join().unwrap_or_else(|_| json!("closer panicked")),
        )
    });
    record(
        &mut cases,
        "target-closed-during-route",
        json!({ "route": during, "close": closer }),
        during["wrongTarget"] == false,
    )?;

    // Terminal restart cannot run from here: this harness's own session runs
    // inside Terminal.app, so quitting it ends the qualification itself.
    record(
        &mut cases,
        "terminal-restart",
        json!({ "status": "NOT_RUN", "reason": "Terminal.app hosts the session running this harness; quitting it would end the qualification and every other Terminal session. Stale Terminal generations are covered by route-model tests (crates/surfaces) and by M0B's closed/recreated-tab evidence." }),
        false,
    )?;

    locked(ctx, "g08 cleanup", || {
        for tab in [&a.tab, &spare, &reuse] {
            let closed = tab.close();
            let _ = run_dir.append("cleanup.jsonl", &closed);
        }
    });
    let terminal_after = terminal::terminal_process();
    let wrong_total: usize = cases
        .iter()
        .map(|c| {
            usize::from(c["detail"]["wrongTarget"] == true)
                + c["detail"]["wrongTargets"].as_u64().unwrap_or(0) as usize
        })
        .sum();
    let required: Vec<&Value> = cases
        .iter()
        .filter(|c| c["case"] != "terminal-restart")
        .collect();
    let summary = json!({
        "gate": "G08",
        "pass": required.iter().all(|c| c["pass"] == true) && wrong_total == 0,
        "terminalRestart": "NOT_RUN (hosts this session)",
        "wrongTargets": wrong_total,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "terminalIncarnationUnchanged": terminal_before == terminal_after,
        "disposableRoot": root.display().to_string(),
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
