//! G08 Terminal reliability negatives with real Claude sessions in disposable,
//! harness-owned Terminal windows. Every route is checked against an
//! independent readback (the harness's own selected-tab query and
//! LaunchServices' frontmost application); a wrong target is a route the
//! companion reports exact whose independently read selected tab is not the
//! expected session's TTY.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody, QualificationFault};
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
    spawn(ctx, dir, true)
}

/// As `spawn_claude`, but Claude runs as a job of the window's interactive
/// shell (as when someone types `claude`), so stopping it hands the
/// terminal's foreground back to the shell.
pub fn spawn_claude_job(ctx: &Ctx, dir: PathBuf) -> Result<ClaudeTab, String> {
    spawn(ctx, dir, false)
}

fn spawn(ctx: &Ctx, dir: PathBuf, exec: bool) -> Result<ClaudeTab, String> {
    let started = start_claude(ctx, dir, exec)?;
    match started.bound_session(ctx, Duration::from_secs(90)) {
        Some(session_id) => Ok(started.into_bound(session_id)),
        None => Err(format!(
            "companion never bound session {} to {}",
            started.native_session_id, started.tab.tty
        )),
    }
}

/// A direct interactive Claude session in its own disposable window, proven
/// by Claude's own inventory, before any companion has necessarily seen it.
pub struct StartedClaude {
    pub tab: Tab,
    pub native_session_id: String,
    pub pid: i32,
}

/// Starts the session and accepts the folder-trust prompt in that window
/// only; needs no companion.
pub fn start_claude(ctx: &Ctx, dir: PathBuf, exec: bool) -> Result<StartedClaude, String> {
    let launcher = threadspace_relay::paths::home_dir()
        .ok_or("no home")?
        .join(".local/bin/claude");
    let command = format!(
        "cd {} && {}{}",
        terminal::shell_quote(&dir.display().to_string()),
        if exec { "exec " } else { "" },
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
    loop {
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
            return Ok(StartedClaude {
                native_session_id: row.full_session_id().unwrap_or_default().to_owned(),
                pid: row.live_pid().unwrap_or(0),
                tab,
            });
        }
        threadspace_harness::pause_ms(1000);
    }
}

impl StartedClaude {
    /// The Threadspace session the running companion bound to this window's
    /// TTY, if it does so within `timeout`.
    pub fn bound_session(&self, ctx: &Ctx, timeout: Duration) -> Option<String> {
        let started = Instant::now();
        loop {
            if let Ok((_, snapshot)) = companion_snapshot(ctx)
                && let Some(session) = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.native_session_id == self.native_session_id)
                && session
                    .binding
                    .as_ref()
                    .is_some_and(|b| b.locator == self.tab.tty)
            {
                return Some(session.session_id.clone());
            }
            if started.elapsed() >= timeout {
                return None;
            }
            threadspace_harness::pause_ms(1000);
        }
    }

    pub fn into_bound(self, session_id: String) -> ClaudeTab {
        ClaudeTab {
            session_id,
            native_session_id: self.native_session_id,
            pid: self.pid,
            tab: self.tab,
        }
    }
}

/// `(process group, terminal foreground process group)` of `pid`.
fn foreground_group(pid: i32) -> Option<(i64, i64)> {
    let out = run(
        "/bin/ps",
        &["-o", "pgid=,tpgid=", "-p", &pid.to_string()],
        Duration::from_secs(3),
    );
    let mut fields = out.stdout.split_whitespace().map(|f| f.parse::<i64>().ok());
    Some((fields.next()??, fields.next()??))
}

pub(crate) fn frontmost_bundle() -> Option<String> {
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
    let request_id = request.request_id.clone();
    // Callers hold the shared GUI lock around each case (setup + route).
    let started = Instant::now();
    let outcome = ctx.companion().request(
        ControlRequestBody::ReturnToSession { route: request },
        Duration::from_secs(25),
    );
    let elapsed = started.elapsed().as_millis() as u64;
    threadspace_harness::pause_ms(300);
    let readback = terminal::selected_tty_read();
    let selected = readback.as_ref().ok().cloned();
    let front = frontmost_bundle();
    match outcome {
        Ok(ControlResponseBody::Routed { result }) => {
            let exact = format!("{:?}", result.surface_result) == "ExactNativeSurface"
                && format!("{:?}", result.session_verification) == "CurrentNativeRevalidated";
            let readback_matches = expected_tty.is_some_and(|tty| selected.as_deref() == Some(tty));
            json!({
                "requestId": request_id,
                "focus": result.evidence.focus,
                "surfaceResult": result.surface_result,
                "sessionVerification": result.session_verification,
                "inputReadiness": result.input_readiness,
                "reasonCode": result.reason_code,
                "focusPerformed": result.focus_performed,
                "latencyMs": result.latency_ms,
                "harnessElapsedMs": elapsed,
                "exact": exact,
                "independentSelectedTty": selected,
                "independentReadbackError": readback.as_ref().err(),
                "independentFrontmost": front,
                "expectedTty": expected_tty,
                // Wrong only when a different tab is read back; an exact
                // claim that cannot be read back is unverified, not wrong.
                "wrongTarget": exact && expected_tty.is_some() && selected.is_some() && !readback_matches,
                "unverifiedExact": exact && expected_tty.is_some() && selected.is_none(),
            })
        }
        Ok(other) => json!({ "error": format!("unexpected {other:?}") }),
        Err(error) => {
            json!({ "requestId": request_id, "refused": error, "independentSelectedTty": selected, "independentFrontmost": front, "wrongTarget": false })
        }
    }
}

/// Runs one case segment while holding the shared GUI lock. A lock that
/// cannot be taken (I/O error) does not stop the case; it is recorded by the
/// guard's absence only.
pub(crate) fn locked<T>(ctx: &Ctx, label: &str, action: impl FnOnce() -> T) -> T {
    let _gui = ctx.gui(label);
    action()
}

pub(crate) fn terminal_pid() -> Result<u32, String> {
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
    let b = spawn_claude_job(ctx, root.join("b"))?;
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

    // Minimize, then Return must unminimize and read back exactly. The
    // window is addressed by its recorded ID: Claude sets its own title.
    let (minimized, restore, after) = locked(ctx, "g08 minimize", || {
        let minimized = a.tab.set_miniaturized(true);
        threadspace_harness::pause_ms(1000);
        let restore = route(ctx, &a.session_id, Some(&a.tab.tty));
        let after = a.tab.miniaturized();
        (minimized, restore, after)
    });
    let minimize_ok = minimized == Some(true) && exact_ok(&restore) && after == Some(false);
    record(
        &mut cases,
        "minimize-then-return",
        json!({ "minimizedByWindowId": minimized, "route": restore, "miniaturizedAfterReturn": after }),
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

    // Foreground mismatch: the provider is stopped and its shell holds the
    // terminal's foreground. `b` runs as a shell job; SIGSTOP cannot be
    // caught. `fg` is typed only once the shell owns the foreground, so it
    // never reaches Claude as a prompt.
    let (foreground_before, foreground_stopped, background, foreground_after) =
        locked(ctx, "g08 foreground mismatch", || {
            let before = foreground_group(b.pid);
            procs::signal(b.pid, libc::SIGSTOP);
            threadspace_harness::pause_ms(1500);
            let stopped = foreground_group(b.pid);
            let shell_has_foreground = stopped.as_ref().is_some_and(|(pgid, tpgid)| pgid != tpgid);
            let background = route(ctx, &b.session_id, Some(&b.tab.tty));
            if shell_has_foreground {
                b.tab.type_line("fg");
            } else {
                procs::signal(b.pid, libc::SIGCONT);
            }
            threadspace_harness::pause_ms(1500);
            (before, stopped, background, foreground_group(b.pid))
        });
    let shell_held = foreground_stopped
        .as_ref()
        .is_some_and(|(pgid, tpgid)| pgid != tpgid);
    let readiness_ok = shell_held
        && background["inputReadiness"] != "FOREGROUND_COMPATIBLE"
        && background["wrongTarget"] == false;
    record(
        &mut cases,
        "foreground-process-mismatch",
        json!({
            "claudeGroupAndForeground": { "before": foreground_before, "stopped": foreground_stopped, "after": foreground_after },
            "route": background,
        }),
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
    let race_unverified = race_results
        .iter()
        .filter(|r| r["unverifiedExact"] == true)
        .count();
    record(
        &mut cases,
        "selection-readback-race",
        json!({ "routes": race_results, "wrongTargets": race_wrong, "unverifiedExact": race_unverified }),
        race_wrong == 0 && race_unverified == 0,
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
        json!({ "status": "BLOCKED", "reason": "quitting Terminal.app ends every Terminal session on this Mac, including unrelated owner sessions and the one running this harness; the case can run only when no other Terminal session is open. Stale Terminal generations are covered by route-model tests (crates/surfaces) and by M0B's closed/recreated-tab evidence." }),
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
        "terminalRestart": "BLOCKED (quitting Terminal would end unrelated owner sessions)",
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

/// Every Terminal window's ID and selected-tab TTY, and the front window.
/// The separator is defined outside the `tell`: inside it, `tab` names
/// Terminal's tab class and would be written out as the word "tab".
const WINDOW_SELECTION: &str = r#"set separator to character id 9
tell application "Terminal"
  set out to "front" & separator & (id of front window) & linefeed
  repeat with w in windows
    set out to out & (id of w) & separator & (tty of selected tab of w) & linefeed
  end repeat
  return out
end tell"#;

/// Brings one window forward as Terminal orders its own windows; macOS shows
/// that window's Space.
const RAISE_WINDOW: &str = r#"on run argv
tell application "Terminal"
  set index of window id ((item 1 of argv) as integer) to 1
  activate
end tell
end run"#;

pub(crate) fn window_selection() -> Value {
    let out = threadspace_harness::run::osascript(WINDOW_SELECTION, &[], Duration::from_secs(10));
    let mut front = None;
    let mut windows = serde_json::Map::new();
    for line in out.stdout.lines() {
        let mut fields = line.split('\t');
        match (fields.next(), fields.next()) {
            (Some("front"), Some(id)) => front = id.trim().parse::<i64>().ok(),
            (Some(id), Some(tty)) => {
                windows.insert(id.trim().to_owned(), json!(tty.trim()));
            }
            _ => {}
        }
    }
    json!({ "atMs": threadspace_harness::now_ms(), "front": front, "selected": windows, "error": (!out.ok).then(|| out.stderr.trim().to_owned()) })
}

pub(crate) fn raise_window(window_id: i64) -> bool {
    threadspace_harness::run::osascript(
        RAISE_WINDOW,
        &[&window_id.to_string()],
        Duration::from_secs(10),
    )
    .ok
}

/// Reads `read` until `done` holds or `timeout` passes; returns the last
/// reading, whether it held and how long it took.
pub(crate) fn poll(
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
        threadspace_harness::pause_ms(150);
    }
}

/// H-10 (G08 remediation): an exact baseline route; a fullscreen/Space
/// transition proven by the target's own CoreGraphics window number; and a
/// target close that genuinely overlaps a route held at each qualification
/// barrier, proven by ordered native and companion timestamps.
pub fn remediation(ctx: &Ctx) -> Result<Value, String> {
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/h10-terminal",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let root = PathBuf::from(format!(
        "/private/tmp/ts-m0c-h10-{}",
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
    run_dir
        .write_json("environment.json", &ctx.environment())
        .map_err(|e| e.to_string())?;
    let a = spawn_claude(ctx, root.join("a"))?;
    let spare = locked(ctx, "h10 open spare window", || {
        Tab::open_inert(root.join("spare"))
    })?;
    let tpid = terminal_pid()?;
    let owned: Vec<i64> = vec![a.tab.window_id, spare.window_id];
    // Distinct frames: each window's AX element is found from its CoreGraphics number.
    locked(ctx, "h10 frames", || {
        a.tab.set_bounds(137, 151, 1001, 707);
        spare.set_bounds(211, 233, 1011, 733);
    });
    let window = |id: i64| {
        ctx.native
            .json(&["window-state", &tpid.to_string(), &id.to_string()])
    };
    let exact_ok = |r: &Value| r["exact"] == true && r["wrongTarget"] == false;

    // 1. Baseline exact route (routing regression).
    let baseline = locked(ctx, "h10 baseline", || {
        route(ctx, &a.session_id, Some(&a.tab.tty))
    });
    record(
        &mut cases,
        "baseline-exact-route",
        baseline.clone(),
        exact_ok(&baseline),
    )?;

    // 2. Fullscreen and its own Space, entered and left, with routes.
    let fullscreen = locked(ctx, "h10 fullscreen", || {
        let pre = (window(a.tab.window_id), window(spare.window_id));
        let enter = ctx.native.json(&[
            "ax-action-number",
            &tpid.to_string(),
            &a.tab.window_id.to_string(),
            "fullscreen",
        ]);
        let (entered, entered_ok, enter_ms) = poll(
            Duration::from_secs(10),
            || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
            |v| {
                v["target"]["ax"]["fullScreen"] == true
                    && v["target"]["fullscreenFrame"] == true
                    && v["target"]["onScreen"] == true
                    && v["spare"]["onScreen"] == false
            },
        );
        let left_space = raise_window(spare.window_id);
        let (away, away_ok, away_ms) = poll(
            Duration::from_secs(10),
            || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
            |v| v["target"]["onScreen"] == false && v["spare"]["onScreen"] == true,
        );
        let routed = route(ctx, &a.session_id, Some(&a.tab.tty));
        let (returned, returned_ok, return_ms) = poll(
            Duration::from_secs(5),
            || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
            |v| {
                v["target"]["onScreen"] == true
                    && v["target"]["fullscreenFrame"] == true
                    && v["spare"]["onScreen"] == false
            },
        );
        let exit = ctx.native.json(&[
            "ax-action-number",
            &tpid.to_string(),
            &a.tab.window_id.to_string(),
            "exit-fullscreen",
        ]);
        let pre_bounds = pre.0["bounds"].clone();
        let (exited, exited_ok, exit_ms) = poll(
            Duration::from_secs(10),
            || json!({ "target": window(a.tab.window_id), "spare": window(spare.window_id) }),
            |v| {
                v["target"]["ax"]["fullScreen"] == false
                    && v["target"]["bounds"] == pre_bounds
                    && v["target"]["onScreen"] == true
                    && v["spare"]["onScreen"] == true
            },
        );
        let after_exit = route(ctx, &a.session_id, Some(&a.tab.tty));
        json!({
            "pre": { "target": pre.0, "spare": pre.1 },
            "enter": enter, "entered": entered, "enteredWitnessed": entered_ok, "enterWaitMs": enter_ms,
            "raisedSpareInDesktopSpace": left_space, "away": away, "awayWitnessed": away_ok, "awayWaitMs": away_ms,
            "routeWhileFullscreen": routed, "returned": returned, "returnWitnessed": returned_ok, "returnWaitMs": return_ms,
            "exit": exit, "exited": exited, "exitWitnessed": exited_ok, "exitWaitMs": exit_ms,
            "routeAfterExit": after_exit,
        })
    });
    let pre_not_fullscreen = fullscreen["pre"]["target"]["ax"]["fullScreen"] == false
        && fullscreen["pre"]["target"]["axMatches"] == 1;
    let fullscreen_checks = json!({
        "preNotFullscreenUniqueWindow": pre_not_fullscreen,
        "enteredFullscreenAndOwnSpace": fullscreen["enteredWitnessed"] == true,
        "leftItsSpace": fullscreen["awayWitnessed"] == true,
        "routeWhileFullscreenExact": exact_ok(&fullscreen["routeWhileFullscreen"]),
        "routeBroughtItsSpaceBack": fullscreen["returnWitnessed"] == true,
        "exitedFullscreenBoundsRestored": fullscreen["exitWitnessed"] == true,
        "routeAfterExitExact": exact_ok(&fullscreen["routeAfterExit"]),
    });
    let fullscreen_pass = fullscreen_checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true));
    record(
        &mut cases,
        "fullscreen-space-entry-route-exit-route",
        json!({ "checks": fullscreen_checks, "observed": fullscreen }),
        fullscreen_pass,
    )?;

    // 3. Target closed while a route is held at each barrier.
    let mut extra = Vec::new();
    for (point, fault) in [
        ("BEFORE_FOCUS", QualificationFault::HoldNextRouteBeforeFocus),
        (
            "BEFORE_READBACK",
            QualificationFault::HoldNextRouteBeforeReadback,
        ),
    ] {
        let c = spawn_claude(ctx, root.join(format!("c-{}", point.to_lowercase())))?;
        let c_id = c.tab.window_id;
        let mut owned_now = owned.clone();
        owned_now.push(c_id);
        let detail = locked(ctx, "h10 close during held route", || {
            raise_window(spare.window_id);
            threadspace_harness::pause_ms(800);
            let before = window_selection();
            let mut log = ctx.companion().log();
            let mut seen = Vec::new();
            let armed = ctx.companion().request(
                ControlRequestBody::QualifyArmFault { fault },
                Duration::from_secs(5),
            );
            std::thread::scope(|scope| {
                let routing = scope.spawn(|| route(ctx, &c.session_id, Some(&c.tab.tty)));
                let reached = log.wait_for(
                    "ROUTE_BARRIER_REACHED",
                    |l| l["point"] == point,
                    Duration::from_secs(20),
                    &mut seen,
                );
                let close_started_ms = threadspace_harness::now_ms();
                let closed = c.tab.close();
                let close_returned_ms = threadspace_harness::now_ms();
                let (gone, gone_ok, gone_ms) = poll(
                    Duration::from_secs(5),
                    || window(c_id),
                    |w| w["exists"] == false,
                );
                let provider_exited = procs::Incarnation::of(c.pid).is_none();
                seen.extend(log.read_new());
                let held_through_close = reached.is_some()
                    && !seen.iter().any(|l| {
                        l["event"] == "ROUTE_RESULT" || l["event"] == "ROUTE_BARRIER_RELEASED"
                    });
                let release_sent_ms = threadspace_harness::now_ms();
                let release = ctx.companion().request(
                    ControlRequestBody::QualifyReleaseRouteBarrier,
                    Duration::from_secs(5),
                );
                let routed = routing
                    .join()
                    .unwrap_or_else(|_| json!({ "error": "route thread panicked" }));
                let released = log.wait_for(
                    "ROUTE_BARRIER_RELEASED",
                    |l| l["point"] == point,
                    Duration::from_secs(10),
                    &mut seen,
                );
                let request_id = routed["requestId"].as_str().unwrap_or_default().to_owned();
                let result_line = log.wait_for(
                    "ROUTE_RESULT",
                    |l| l["requestId"].as_str() == Some(request_id.as_str()),
                    Duration::from_secs(10),
                    &mut seen,
                );
                let after = window_selection();
                json!({
                    "armed": format!("{armed:?}"),
                    "selectionBefore": before, "selectionAfter": after,
                    "barrierReached": reached, "closeStartedMs": close_started_ms, "closeReturnedMs": close_returned_ms,
                    "close": closed, "targetWindowGone": gone_ok, "targetWindowGoneAfterMs": gone_ms, "targetWindowState": gone,
                    "providerExitedBeforeRelease": provider_exited, "routeHeldThroughClose": held_through_close,
                    "releaseSentMs": release_sent_ms, "release": format!("{release:?}"), "barrierReleased": released,
                    "route": routed, "routeResultLogged": result_line,
                })
            })
        });
        let reached_ms = detail["barrierReached"]["reachedAtMs"].as_i64();
        let released_ms = detail["barrierReleased"]["releasedAtMs"].as_i64();
        let result_ms = detail["routeResultLogged"]["ts"].as_i64();
        let close_started = detail["closeStartedMs"].as_i64();
        let ordered = matches!((reached_ms, close_started, released_ms, result_ms), (Some(r), Some(c0), Some(rel), Some(res)) if r <= c0 && c0 < rel && rel <= res)
            && detail["targetWindowGone"] == true
            && detail["releaseSentMs"].as_i64() >= detail["closeReturnedMs"].as_i64();
        // Threadspace focused nothing but the target: the companion's own focus
        // evidence names only the target window, and no other window's
        // selected tab changed.
        let focus = &detail["route"]["focus"];
        let focused_only_target =
            focus["targetWindowId"].is_null() || focus["targetWindowId"] == c_id;
        let unrelated_changes: Vec<String> = detail["selectionBefore"]["selected"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(id, _)| !owned_now.iter().any(|o| o.to_string() == **id))
                    .filter(|(id, tty)| detail["selectionAfter"]["selected"][id.as_str()] != **tty)
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let checks = json!({
            "barrierReachedForTarget": detail["barrierReached"]["tty"] == c.tab.tty || point == "BEFORE_READBACK",
            "routeHeldThroughClose": detail["routeHeldThroughClose"] == true,
            "orderedOverlap": ordered,
            "providerGoneBeforeRelease": detail["providerExitedBeforeRelease"] == true,
            "notExact": detail["route"]["exact"] != true,
            "typedResult": detail["route"]["reasonCode"].as_str().is_some_and(|r| r != "OK"),
            "noWrongTarget": detail["route"]["wrongTarget"] == false,
            "focusedOnlyTheTarget": focused_only_target,
            "noUnrelatedSelectionChange": unrelated_changes.is_empty(),
        });
        let pass = checks
            .as_object()
            .is_some_and(|m| m.values().all(|v| v == true));
        record(
            &mut cases,
            &format!(
                "target-closed-while-route-held-{}",
                point.to_lowercase().replace('_', "-")
            ),
            json!({
                "checks": checks, "point": point, "target": { "windowId": c_id, "tty": c.tab.tty, "pid": c.pid, "sessionId": c.session_id },
                "unrelatedWindowsWhoseSelectionChanged": unrelated_changes, "observed": detail,
            }),
            pass,
        )?;
        extra.push(c);
    }

    locked(ctx, "h10 cleanup", || {
        for tab in [&a.tab, &spare] {
            let _ = run_dir.append("cleanup.jsonl", &tab.close());
        }
    });
    let terminal_after = terminal::terminal_process();
    let wrong_total = cases
        .iter()
        .filter(|c| {
            c["detail"]["wrongTarget"] == true
                || c["detail"]["observed"]["route"]["wrongTarget"] == true
                || ["routeWhileFullscreen", "routeAfterExit"]
                    .iter()
                    .any(|k| c["detail"]["observed"][*k]["wrongTarget"] == true)
        })
        .count();
    let summary = json!({
        "issue": "H-10",
        "gate": "G08",
        "pass": cases.iter().all(|c| c["pass"] == true) && wrong_total == 0,
        "wrongTargets": wrong_total,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "terminalIncarnationUnchanged": terminal_before == terminal_after,
        "terminalRestart": "not attempted: deferred to M15 by D-0006",
        "disposableRoot": root.display().to_string(),
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}
