//! M2 Ghostty qualification: ordinary Claude sessions typed into panes of a
//! disposable Ghostty copy (see `threadspace_harness::ghostty`), observed by
//! the installed Dev companion through its session-scope integration, and
//! returned to by the companion's Return and the packaged UI's Return.
//!
//! The copy runs at its own private bundle path, so PID-addressed events
//! reach exactly it; the owner's Ghostty receives no event. Every Return is
//! checked against independent readback: the copy's front window, selected
//! tab and focused terminal, and NSWorkspace's frontmost PID from a separate
//! process. Focus-moving steps wait for the owner to be idle.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_contracts::diagnostics::AutomationPermission;
use threadspace_contracts::route::RouteRequest;
use threadspace_harness::evidence::Run;
use threadspace_harness::ghostty::{Fixture, Pane, frontmost_pid};
use threadspace_harness::idle::wait_for_idle;
use threadspace_relay::paths::redact_home;

use crate::bridge_gates::companion_snapshot;
use crate::ctx::Ctx;
use crate::m2::{
    Activation, Recording, Scratch, claude_command, completed_turns, disposable, ensure_ui,
    journal_cursor, latency, launcher, mark_handled, session_view, submitted_inputs, trace, ui,
    view, wait_inventory, wait_view,
};

const TURN_TIMEOUT: Duration = Duration::from_secs(150);
const GHOSTTY: &str = "/Applications/Ghostty.app";

/// An observed Claude session in a fixture pane.
pub struct Observed {
    pub pane: Pane,
    pub native_session_id: String,
    pub session_id: String,
}

/// The companion's Apple-event consent for Ghostty, asked through the
/// consent-only request: it sends no event to any Ghostty process (the
/// integration status would inventory every scriptable one, an owner's
/// included). Consent already decided returns at once; an undecided one shows
/// the system prompt, which is the owner's step.
fn ghostty_authorized(ctx: &Ctx) -> Result<Value, String> {
    match ctx.companion().request(ControlRequestBody::RequestGhosttyAutomation, Duration::from_secs(200))? {
        ControlResponseBody::TerminalAutomation { automation, status_code } => {
            if automation != AutomationPermission::Authorized {
                return Err(format!(
                    "BLOCKED: Ghostty automation is {automation:?} ({status_code}) for the {} channel; the owner must allow it in the system prompt or System Settings › Privacy & Security › Automation",
                    ctx.channel_name()
                ));
            }
            Ok(json!({ "automation": automation, "statusCode": status_code }))
        }
        other => Err(format!("Ghostty automation request: unexpected {other:?}")),
    }
}

/// The session-scope integration, after the Ghostty consent check.
pub fn activate(ctx: &Ctx, scratch: &mut Scratch<'_>) -> Result<Activation, String> {
    ghostty_authorized(ctx)?;
    let installed = scratch.install_session()?;
    let detail = &installed["detail"];
    let plugin_dir = detail["pluginDir"].as_str().ok_or("no plugin dir in the install record")?.to_owned();
    let owned = PathBuf::from(&plugin_dir)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or("owned dir")?;
    Ok(Activation {
        plugin_dir,
        settings: owned.join("session/settings.json"),
        record: installed,
    })
}

/// A new disposable copy of the installed Ghostty under the ignored
/// `artifacts/qualification/` directory.
pub fn fixture(ctx: &Ctx, label: &str) -> Result<Fixture, String> {
    let root = ctx
        .repo
        .join("artifacts/qualification")
        .join(format!("ghostty-{label}-{}", crate::stamp()));
    std::fs::create_dir_all(root.parent().ok_or("root")?).map_err(|e| e.to_string())?;
    Fixture::launch(Path::new(GHOSTTY), &root, &ctx.repo.join("tests/native/tools/ghostty_fixture.js"))
}

fn owner_idle(ctx: &Ctx) -> Value {
    serde_json::to_value(wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800))).unwrap_or(Value::Null)
}

/// Types `command` into a fresh pane the way a person does, accepts the
/// folder-trust prompt there only, and waits for inventory and the
/// companion's Ghostty binding of that pane's native terminal ID.
pub fn start_observed(ctx: &Ctx, fixture: &Fixture, pane: Pane, command: &str) -> Result<Observed, String> {
    if !fixture.owns_tty(&pane) {
        return Err(format!("pane {} is not held by the fixture's login process", pane.terminal_id));
    }
    threadspace_harness::pause_ms(1500);
    fixture.submit(&pane, command)?;
    threadspace_harness::pause_ms(4000);
    // The folder-trust dialog preselects "No, exit": Down then Return.
    fixture.key(&pane, "arrowDown", None)?;
    threadspace_harness::pause_ms(300);
    fixture.key(&pane, "enter", None)?;
    let native_session_id = wait_inventory(&pane.dir, Duration::from_secs(60))?;
    let started = Instant::now();
    let session_id = loop {
        if let Ok((_, snapshot)) = companion_snapshot(ctx)
            && let Some(session) = snapshot.sessions.iter().find(|s| s.native_session_id == native_session_id)
            && session.binding.as_ref().is_some_and(|b| b.surface_kind == "ghostty" && b.locator == pane.terminal_id)
        {
            break session.session_id.clone();
        }
        if started.elapsed() > Duration::from_secs(90) {
            let reason = session_view(ctx, &native_session_id).ok().map(|v| format!("binding {:?}, last invalidation {:?}, last route {:?}", v.binding, v.last_invalidation, v.last_route));
            return Err(format!("companion never bound {native_session_id} to Ghostty terminal {}: {reason:?}", pane.terminal_id));
        }
        threadspace_harness::pause_ms(1000);
    };
    // Binding follows the process, not the editor: give the editor time.
    threadspace_harness::pause_ms(3000);
    Ok(Observed { pane, native_session_id, session_id })
}

/// Submits a prompt and confirms it from the companion's inputs; a Return
/// the editor did not take is pressed again every 4 s.
pub fn submit(ctx: &Ctx, fixture: &Fixture, observed: &Observed, text: &str) -> Result<u32, String> {
    let before = submitted_inputs(ctx, &observed.session_id);
    fixture.submit(&observed.pane, text)?;
    let started = Instant::now();
    let mut resent = 0;
    while submitted_inputs(ctx, &observed.session_id) <= before {
        if started.elapsed() > Duration::from_secs(40) {
            return Err(format!("prompt was never submitted ({resent} returns resent)"));
        }
        if started.elapsed() > Duration::from_secs(4 * (u64::from(resent) + 1)) {
            fixture.key(&observed.pane, "enter", None)?;
            resent += 1;
        }
        threadspace_harness::pause_ms(250);
    }
    Ok(resent)
}

pub fn prompt_and_complete(ctx: &Ctx, fixture: &Fixture, observed: &Observed, text: &str, expected: usize) -> Result<u64, String> {
    let started = Instant::now();
    submit(ctx, fixture, observed, text)?;
    while started.elapsed() < TURN_TIMEOUT {
        if completed_turns(ctx, &observed.session_id) >= expected {
            return Ok(started.elapsed().as_millis() as u64);
        }
        threadspace_harness::pause_ms(250);
    }
    Err(format!("turn {expected} did not complete within {TURN_TIMEOUT:?}"))
}

/// Independent readback after a Return, compared with the expected pane.
/// A wrong target is focus on any other terminal; a front application other
/// than the fixture's after the route is a separate disagreement (another
/// application took the front), never counted as a pass.
fn readback(fixture: &Fixture, expected: &Pane, route: &Value) -> Value {
    threadspace_harness::pause_ms(250);
    let native = fixture.readback();
    let front_pid = frontmost_pid();
    let exact = route["surfaceResult"] == "EXACT_NATIVE_SURFACE"
        && route["sessionVerification"] == "CURRENT_NATIVE_REVALIDATED";
    let focus_matches = native.as_ref().is_ok_and(|r| {
        r["terminalId"] == json!(expected.terminal_id) && r["tabSelected"] == json!(true)
    });
    let front_matches = native.as_ref().is_ok_and(|r| r["frontmost"] == json!(true))
        && front_pid == Some(fixture.app.pid);
    json!({
        "expectedTerminal": expected.terminal_id,
        "route": route,
        "exact": exact,
        "independent": native.as_ref().ok(),
        "independentError": native.as_ref().err(),
        "frontmostPid": front_pid,
        "fixturePid": fixture.app.pid,
        "focusMatches": focus_matches,
        "frontMatches": front_matches,
        "readbackMatches": focus_matches && front_matches,
        "wrongTarget": exact && native.is_ok() && !focus_matches,
    })
}

/// Owner keyboard/pointer input since `gate`: an idle time shorter than the
/// time since the idle gate passed means someone used the Mac during the
/// measured window, so the sample is disturbed (retained, never counted).
fn disturbed(ctx: &Ctx, gate: Instant) -> Value {
    let idle = ctx.native.idle_seconds();
    let window = gate.elapsed().as_secs_f64();
    json!({ "idleSeconds": idle, "windowSeconds": window, "disturbed": idle + 0.05 < window })
}

/// One Return through the companion's control interface.
pub fn companion_return(ctx: &Ctx, fixture: &Fixture, observed: &Observed) -> Value {
    let request = RouteRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id: observed.session_id.clone(),
        chosen_binding_id: None,
        expected_binding_revision: None,
    };
    let started = Instant::now();
    let answered = ctx
        .companion()
        .request(ControlRequestBody::ReturnToSession { route: request }, Duration::from_secs(10));
    let elapsed = started.elapsed().as_millis() as u64;
    let route = match &answered {
        Ok(ControlResponseBody::Routed { result }) => serde_json::to_value(result).unwrap_or(Value::Null),
        other => json!({ "error": format!("{other:?}") }),
    };
    let mut value = readback(fixture, &observed.pane, &route);
    value["harnessElapsedMs"] = json!(elapsed);
    value
}

/// The packaged UI's Return button for the session.
pub fn press_return(ctx: &Ctx, fixture: &Fixture, observed: &Observed) -> Value {
    let started = Instant::now();
    let pressed = ui(ctx, "m2-press-return", json!({ "sessionId": observed.session_id }));
    let elapsed = started.elapsed().as_millis() as u64;
    let route = pressed.as_ref().ok().and_then(|p| p["result"].get("route").cloned()).unwrap_or(Value::Null);
    let mut value = readback(fixture, &observed.pane, &route);
    value["harnessElapsedMs"] = json!(elapsed);
    value["uiError"] = json!(pressed.as_ref().err());
    value
}

/// Puts another pane in front so the next Return has to move focus; `ok`
/// only when independent readback shows that pane focused and frontmost.
fn front_other(ctx: &Ctx, fixture: &Fixture, other: &Pane) -> Value {
    let _gui = ctx.gui("m2 ghostty front another pane");
    let focused = fixture.focus(other);
    threadspace_harness::pause_ms(400);
    let readback = fixture.readback();
    let ok = readback.as_ref().is_ok_and(|r| r["terminalId"] == json!(other.terminal_id) && r["frontmost"] == json!(true))
        && frontmost_pid() == Some(fixture.app.pid);
    json!({ "ok": ok, "focused": focused.as_ref().ok(), "error": focused.as_ref().err(), "readback": readback.as_ref().ok() })
}

fn session_count(ctx: &Ctx, native: &str) -> usize {
    companion_snapshot(ctx).map_or(0, |(_, snapshot)| view(&snapshot, native).len())
}

fn route_ok(value: &Value) -> bool {
    value["exact"] == json!(true) && value["readbackMatches"] == json!(true) && value["wrongTarget"] == json!(false)
}

/// One bound session and `returns` companion Returns from another pane.
pub fn smoke(ctx: &Ctx, returns: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "ghostty-smoke", ctx.channel_name()).map_err(|e| e.to_string())?;
    let integration = ghostty_authorized(ctx)?;
    let mut scratch = Scratch::new(ctx, "ghostty-smoke")?;
    let activation = activate(ctx, &mut scratch)?;
    let fixture = fixture(ctx, "smoke")?;
    let work = disposable("ghostty-smoke-work")?;
    let other_dir = disposable("ghostty-smoke-other")?;
    let pane = fixture.window(&work)?;
    let other = fixture.tab(&pane, &other_dir)?;
    fixture.select_tab(&pane)?;
    let command = claude_command(&work, &activation, &launcher()?, &[], &[]);
    let started = start_observed(ctx, &fixture, pane, &command);
    let mut results = Vec::new();
    let mut pass = started.is_ok();
    if let Ok(observed) = &started {
        for index in 0..returns {
            let idle = owner_idle(ctx);
            let fronted = front_other(ctx, &fixture, &other);
            let returned = {
                let _gui = ctx.gui("m2 ghostty smoke Return");
                companion_return(ctx, &fixture, observed)
            };
            pass &= fronted["ok"] == json!(true) && route_ok(&returned);
            results.push(json!({ "index": index, "ownerIdle": idle, "fronted": fronted, "return": returned }));
        }
        let _ = fixture.submit(&observed.pane, "/exit");
        let _ = wait_view(ctx, &observed.native_session_id, Duration::from_secs(60), |v| format!("{:?}", v.execution_presence) == "Ended");
    }
    let closed = fixture.close();
    let removed = scratch.remove_integration();
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(&other_dir);
    pass &= closed["ok"] == json!(true);
    let summary = json!({
        "pass": pass,
        "integration": integration,
        "fixture": { "pid": fixture.app.pid, "image": fixture.image, "dictionarySha256": fixture.dictionary_sha256, "executable": redact_home(&fixture.executable.display().to_string()) },
        "startError": started.as_ref().err(),
        "nativeSessionId": started.as_ref().ok().map(|o| o.native_session_id.clone()),
        "sessionId": started.as_ref().ok().map(|o| o.session_id.clone()),
        "returns": results,
        "fixtureClosed": closed,
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

/// MILESTONES M2: `cycles` prompt/completion/Return/follow-up cycles in one
/// Ghostty pane keep one Session and worker; explicit Mark handled; `/exit`
/// keeps the worker.
pub fn vertical(ctx: &Ctx, cycles: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "ghostty-vertical", ctx.channel_name()).map_err(|e| e.to_string())?;
    let integration = ghostty_authorized(ctx)?;
    let mut scratch = Scratch::new(ctx, "ghostty-vertical")?;
    let activation = activate(ctx, &mut scratch)?;
    let ui_state = ensure_ui(ctx)?;
    let fixture = fixture(ctx, "vertical")?;
    let recording = Recording::start()?;
    let start_cursor = journal_cursor(ctx);
    let work = disposable("ghostty-vertical-work")?;
    let other_dir = disposable("ghostty-vertical-other")?;
    let pane = fixture.window(&work)?;
    let other = fixture.window(&other_dir)?;
    let command = claude_command(&work, &activation, &launcher()?, &["--allowedTools", "Bash(echo:*)"], &[]);
    let observed = match start_observed(ctx, &fixture, pane, &command) {
        Ok(observed) => observed,
        Err(error) => {
            let closed = fixture.close();
            let _ = scratch.remove_integration();
            run_dir.write_json("summary.json", &json!({ "pass": false, "startError": error, "fixtureClosed": closed })).map_err(|e| e.to_string())?;
            return Err(error);
        }
    };
    let mut pass = true;
    let mut completed = 0usize;
    for cycle in 1..=cycles {
        let prompt = format!("Use the Bash tool to run exactly: echo threadspace-m2-{cycle}. Then reply with only the word done.");
        completed += 1;
        let turn_ms = prompt_and_complete(ctx, &fixture, &observed, &prompt, completed);
        let after = session_view(ctx, &observed.native_session_id);
        let fleet = ui(ctx, "m2-fleet", json!({ "sessionId": observed.session_id }));
        // A Return disturbed by owner input is retried within its cycle; every
        // attempt is retained and only an undisturbed exact one counts.
        let mut return_attempts = Vec::new();
        let (mut idle, mut fronted, mut returned) = (Value::Null, Value::Null, Value::Null);
        for _ in 0..3 {
            idle = owner_idle(ctx);
            let gate = Instant::now();
            fronted = front_other(ctx, &fixture, &other);
            returned = {
                let _gui = ctx.gui("m2 ghostty vertical Return");
                press_return(ctx, &fixture, &observed)
            };
            let owner = disturbed(ctx, gate);
            let retry = !(fronted["ok"] == json!(true) && route_ok(&returned))
                && owner["disturbed"] == json!(true)
                && returned["wrongTarget"] != json!(true);
            return_attempts.push(json!({ "ownerIdle": idle, "owner": owner, "fronted": fronted, "return": returned }));
            if !retry {
                break;
            }
        }
        completed += 1;
        let follow_ms = prompt_and_complete(ctx, &fixture, &observed, "Reply with only the word again.", completed);
        let again = session_view(ctx, &observed.native_session_id);
        let sessions = session_count(ctx, &observed.native_session_id);
        let workers = fleet
            .as_ref()
            .ok()
            .and_then(|f| f["result"]["rows"].as_array())
            .map(|rows| rows.iter().filter(|r| r["sessionId"] == json!(observed.session_id)).count())
            .unwrap_or(0);
        let ok = turn_ms.is_ok()
            && follow_ms.is_ok()
            && after.as_ref().is_ok_and(|v| format!("{:?}", v.turn_state) == "Completed")
            && again.as_ref().is_ok_and(|v| v.session_id == observed.session_id)
            && sessions == 1
            && workers == 1
            && fronted["ok"] == json!(true)
            && route_ok(&returned);
        pass &= ok;
        run_dir
            .append("cycles.jsonl", &json!({
                "cycle": cycle, "ok": ok, "turnMs": turn_ms.as_ref().ok(), "turnError": turn_ms.as_ref().err(),
                "followUpMs": follow_ms.as_ref().ok(), "followUpError": follow_ms.as_ref().err(),
                "sessionViews": sessions, "workers": workers, "fleetError": fleet.as_ref().err(),
                "afterTurn": after.as_ref().ok().map(|v| json!({ "turnState": v.turn_state, "observation": v.observation, "observerTier": v.observer_tier, "observerVersion": v.observer_version, "presence": v.execution_presence })),
                "ownerIdle": idle, "fronted": fronted, "return": returned, "returnAttempts": return_attempts,
            }))
            .map_err(|e| e.to_string())?;
    }
    let latency = latency(ctx, &observed.native_session_id, start_cursor);
    let handled = mark_handled(ctx, &observed.session_id);
    let still_one = ui(ctx, "m2-fleet", json!({ "sessionId": observed.session_id }))
        .ok()
        .and_then(|f| f["result"]["rows"].as_array().map(Vec::len))
        == Some(1);
    pass &= handled["pass"] == json!(true) && still_one;
    let _ = fixture.submit(&observed.pane, "/exit");
    let exited = wait_view(ctx, &observed.native_session_id, Duration::from_secs(60), |v| format!("{:?}", v.execution_presence) == "Ended");
    let trace = trace(ctx, &observed.native_session_id, &observed.session_id, start_cursor);
    let trace_rows = trace.as_array().map_or(0, Vec::len);
    run_dir.write_json("trace.json", &trace).map_err(|e| e.to_string())?;
    let worker_kept = ui(ctx, "m2-fleet", json!({ "sessionId": observed.session_id }))
        .ok()
        .and_then(|f| f["result"]["rows"].as_array().cloned())
        .is_some_and(|rows| rows.iter().any(|r| r["sessionId"] == json!(observed.session_id)));
    pass &= exited.is_ok() && worker_kept;
    let recording = recording.map(|mut recording| recording.stop());
    let closed = fixture.close();
    pass &= closed["ok"] == json!(true);
    let removed = scratch.remove_integration();
    let _ = std::fs::remove_dir_all(&observed.pane.dir);
    let _ = std::fs::remove_dir_all(&other_dir);
    let summary = json!({
        "pass": pass,
        "cycles": cycles,
        "integration": integration,
        "fixture": { "pid": fixture.app.pid, "image": fixture.image, "dictionarySha256": fixture.dictionary_sha256 },
        "recording": recording,
        "nativeSessionId": observed.native_session_id,
        "sessionId": observed.session_id,
        "ui": ui_state,
        "exit": exited.as_ref().ok().map(|(v, ms)| json!({ "presence": v.execution_presence, "observation": v.observation, "ms": ms })),
        "exitError": exited.as_ref().err(),
        "latency": latency,
        "traceRows": trace_rows,
        "markHandled": handled,
        "workerAfterMarkHandled": still_one,
        "workerKeptAfterExit": worker_kept,
        "fixtureClosed": closed,
        "activation": activation.record["detail"]["pluginDir"].as_str().map(redact_home),
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

/// MILESTONES M2: `count` exact Returns across three sessions (two in split
/// panes of one tab, one in a tab of a second window), each preceded by
/// fronting a different pane, then rearrangement and negative cases.
pub fn routes(ctx: &Ctx, count: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "ghostty-routes", ctx.channel_name()).map_err(|e| e.to_string())?;
    let integration = ghostty_authorized(ctx)?;
    let mut scratch = Scratch::new(ctx, "ghostty-routes")?;
    let activation = activate(ctx, &mut scratch)?;
    // Every third Return is the packaged UI's own button.
    let ui_state = ensure_ui(ctx)?;
    let fixture = fixture(ctx, "routes")?;
    let dirs: Vec<PathBuf> = (0..4).map(|i| disposable(&format!("ghostty-routes-{i}"))).collect::<Result<_, _>>()?;
    let left = fixture.window(&dirs[0])?;
    let right = fixture.split(&left, "right", &dirs[1])?;
    let second = fixture.window(&dirs[2])?;
    let spare = fixture.tab(&second, &dirs[3])?;
    let binary = launcher()?;
    let mut sessions = Vec::new();
    let mut start_errors = Vec::new();
    for pane in [left, right, second.clone()] {
        let command = claude_command(&pane.dir, &activation, &binary, &[], &[]);
        match start_observed(ctx, &fixture, pane, &command) {
            Ok(observed) => sessions.push(observed),
            Err(error) => start_errors.push(error),
        }
    }
    let mut pass = start_errors.is_empty() && sessions.len() == 3;
    let mut exact = 0u32;
    let mut wrong = 0u32;
    let mut disturbed_attempts = 0u32;
    let mut attempts = 0u32;
    if pass {
        // Owner input during a route window disturbs only that sample: it is
        // retained and not counted, and another attempt is made, at most ten
        // times over. Any undisturbed miss fails the run.
        while exact < count && attempts < count + 10 {
            let index = attempts;
            attempts += 1;
            let target = &sessions[index as usize % sessions.len()];
            let other = if index % 2 == 0 { spare.clone() } else { sessions[(index as usize + 1) % sessions.len()].pane.clone() };
            let idle = owner_idle(ctx);
            let gate = Instant::now();
            let fronted = front_other(ctx, &fixture, &other);
            let returned = {
                let _gui = ctx.gui("m2 ghostty route");
                if index % 3 == 0 { press_return(ctx, &fixture, target) } else { companion_return(ctx, &fixture, target) }
            };
            let owner = disturbed(ctx, gate);
            let clean = fronted["ok"] == json!(true) && route_ok(&returned);
            if returned["wrongTarget"] == json!(true) {
                wrong += 1;
            }
            let outcome = if clean {
                exact += 1;
                "EXACT"
            } else if owner["disturbed"] == json!(true) && returned["wrongTarget"] != json!(true) {
                disturbed_attempts += 1;
                "DISTURBED"
            } else {
                pass = false;
                "FAILED"
            };
            run_dir
                .append("routes.jsonl", &json!({ "index": index, "via": if index % 3 == 0 { "ui" } else { "companion" }, "outcome": outcome, "ownerIdle": idle, "owner": owner, "fronted": fronted, "return": returned }))
                .map_err(|e| e.to_string())?;
            if !pass {
                break;
            }
        }
        pass &= exact == count && wrong == 0;
    }
    // Independent evidence: run whenever the three sessions started.
    let scenarios = if sessions.len() == 3 { rearrangements(ctx, &fixture, &sessions, &spare, &run_dir) } else { json!(null) };
    pass &= scenarios["pass"] == json!(true);
    for observed in &sessions {
        let _ = fixture.submit(&observed.pane, "/exit");
    }
    for observed in &sessions {
        let _ = wait_view(ctx, &observed.native_session_id, Duration::from_secs(60), |v| format!("{:?}", v.execution_presence) == "Ended");
    }
    let closed = fixture.close();
    pass &= closed["ok"] == json!(true);
    let removed = scratch.remove_integration();
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
    let summary = json!({
        "pass": pass,
        "count": count,
        "exact": exact,
        "attempts": attempts,
        "disturbedAttempts": disturbed_attempts,
        "wrongTargets": wrong,
        "integration": integration,
        "ui": ui_state,
        "fixture": { "pid": fixture.app.pid, "image": fixture.image, "dictionarySha256": fixture.dictionary_sha256 },
        "sessions": sessions.iter().map(|o| json!({ "sessionId": o.session_id, "nativeSessionId": o.native_session_id, "terminalId": o.pane.terminal_id, "tty": o.pane.tty })).collect::<Vec<_>>(),
        "startErrors": start_errors,
        "scenarios": scenarios,
        "fixtureClosed": closed,
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

/// Rearranged and failing surfaces. Each case proves its setup by
/// independent readback before its Return counts: a moved tab's index
/// changed, the window is fullscreen or minimized by Accessibility, another
/// pane is in front. A setup that did not happen fails its case.
fn rearrangements(ctx: &Ctx, fixture: &Fixture, sessions: &[Observed], spare: &Pane, run_dir: &Run) -> Value {
    let mut cases = Vec::new();
    let mut pass = true;
    let mut record = |name: &str, setup_ok: bool, setup: Value, returned: Value, ok: bool| {
        let ok = setup_ok && ok;
        pass &= ok;
        let _ = run_dir.append("scenarios.jsonl", &json!({ "case": name, "ok": ok, "setupOk": setup_ok, "setup": setup, "return": returned }));
        cases.push(json!({ "case": name, "ok": ok, "setupOk": setup_ok }));
    };
    let pid = fixture.app.pid.to_string();
    let tool = ctx.repo.join("target/native-tools/ts-native");
    // Accessibility actions run in an owned control window of the copy.
    let control_dir = disposable("ghostty-control").unwrap_or_else(|_| fixture.root.clone());
    let control = fixture.window(&control_dir);
    let ax = |args: &[&str]| -> Value {
        match &control {
            Ok(control) => fixture.native(control, &tool, args).unwrap_or_else(|error| json!({ "error": error })),
            Err(error) => json!({ "error": error }),
        }
    };
    threadspace_harness::pause_ms(1500);

    // A tab moves within its window.
    let target = &sessions[2];
    let before = fixture.locate(&target.pane);
    let moved = fixture.action(&target.pane, "move_tab:1");
    let after = fixture.locate(&target.pane);
    owner_idle(ctx);
    let fronted = front_other(ctx, fixture, spare);
    let setup_ok = moved.as_ref().is_ok_and(|m| m["performed"] == json!(true))
        && matches!((&before, &after), (Ok(b), Ok(a)) if a.tab_index >= 0 && a.tab_index != b.tab_index)
        && fronted["ok"] == json!(true);
    let returned = {
        let _gui = ctx.gui("m2 ghostty moved tab");
        companion_return(ctx, fixture, target)
    };
    let ok = route_ok(&returned);
    record("tab-moved", setup_ok, json!({ "before": before.as_ref().ok(), "moved": moved.as_ref().ok(), "movedError": moved.as_ref().err(), "after": after.as_ref().ok(), "fronted": fronted }), returned, ok);

    // A split session's window is fullscreen; another window is in front.
    let full = &sessions[0];
    let marker = format!("ts-full-{}", uuid::Uuid::new_v4());
    let titled = fixture.title(&full.pane, &marker);
    owner_idle(ctx);
    let _ = fixture.focus(&full.pane);
    let entered = ax(&["ax-action", &pid, "fullscreen", &marker]);
    threadspace_harness::pause_ms(1500);
    let state = ax(&["ax-window", &pid, &marker]);
    let fronted = front_other(ctx, fixture, &sessions[2].pane);
    let setup_ok = titled.is_ok() && state["fullScreen"] == json!(true) && fronted["ok"] == json!(true);
    let returned = {
        let _gui = ctx.gui("m2 ghostty fullscreen");
        companion_return(ctx, fixture, full)
    };
    let ok = route_ok(&returned);
    let exited = ax(&["ax-action", &pid, "exit-fullscreen", &marker]);
    threadspace_harness::pause_ms(1500);
    record("fullscreen", setup_ok, json!({ "titled": titled.is_ok(), "entered": entered, "state": state, "exited": exited, "fronted": fronted }), returned, ok);

    // The second window is minimized; Return must restore and focus it.
    let mini = &sessions[2];
    let marker = format!("ts-mini-{}", uuid::Uuid::new_v4());
    let titled = fixture.title(&mini.pane, &marker);
    owner_idle(ctx);
    let _ = fixture.focus(&mini.pane);
    let minimized = ax(&["ax-action", &pid, "minimize", &marker]);
    threadspace_harness::pause_ms(1000);
    let state = ax(&["ax-window", &pid, &marker]);
    let setup_ok = titled.is_ok() && state["minimized"] == json!(true);
    let returned = {
        let _gui = ctx.gui("m2 ghostty minimized");
        companion_return(ctx, fixture, mini)
    };
    let restored = ax(&["ax-window", &pid, &marker]);
    let ok = route_ok(&returned) && restored["minimized"] == json!(false);
    record("minimized", setup_ok, json!({ "titled": titled.is_ok(), "minimized": minimized, "state": state, "restored": restored }), returned, ok);

    // The split's right session exits and its pane closes: Return is refused
    // and focus does not move.
    let gone = &sessions[1];
    let _ = fixture.submit(&gone.pane, "/exit");
    let ended = wait_view(ctx, &gone.native_session_id, Duration::from_secs(60), |v| format!("{:?}", v.execution_presence) == "Ended");
    let closed = fixture.close_pane(&gone.pane);
    owner_idle(ctx);
    let fronted = front_other(ctx, fixture, spare);
    let before = fixture.readback();
    let returned = {
        let _gui = ctx.gui("m2 ghostty closed pane");
        companion_return(ctx, fixture, gone)
    };
    let after = fixture.readback();
    let unmoved = before.as_ref().ok().is_some() && before.as_ref().ok() == after.as_ref().ok();
    let setup_ok = ended.is_ok() && closed.is_ok() && fronted["ok"] == json!(true);
    let ok = returned["exact"] == json!(false) && returned["route"]["focusPerformed"] == json!(false) && unmoved;
    record("closed-pane", setup_ok, json!({ "ended": ended.is_ok(), "closed": closed.as_ref().ok(), "fronted": fronted, "before": before.as_ref().ok(), "after": after.as_ref().ok() }), returned, ok);
    let _ = std::fs::remove_dir_all(&control_dir);
    json!({ "pass": pass, "control": control.as_ref().ok().map(|c| c.terminal_id.clone()), "cases": cases })
}
