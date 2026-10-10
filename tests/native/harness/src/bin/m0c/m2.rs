//! M2 qualification: the manually launched Claude vertical slice (MILESTONES
//! M2) on the development channel. Every Claude session runs in a
//! disposable, harness-owned Terminal window and directory, started the way
//! a person types it, with the installed integration activated for that
//! session only: `CLAUDE_CODE_PLUGIN_DIRS` names the Threadspace-owned mod
//! copy and `--settings` the owned hook settings. The owner's `~/.claude` is
//! never read for writing or modified. Install/reinstall/remove cycles run
//! against disposable configuration directories.
//!
//! Canonical state is read back from the companion (a view snapshot) and,
//! read-only, from its journal; every Return is checked against the
//! harness's own selected-tab and frontmost-application readback.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_contracts::diagnostics::AutomationPermission;
use threadspace_contracts::projection::{FleetSnapshot, SessionView};
use threadspace_harness::evidence::{Run, sha256_file};
use threadspace_harness::run::run;
use threadspace_harness::terminal::{self, Tab};
use threadspace_relay::paths::redact_home;

use crate::bridge_gates::companion_snapshot;
use crate::ctx::Ctx;

/// A cheap model keeps qualification turns short; the flags are ones a
/// person passes when starting `claude`.
pub const MODEL: &str = "haiku";
const TURN_TIMEOUT: Duration = Duration::from_secs(150);
/// How long a runner waits for Terminal to answer element queries again.
pub const TERMINAL_WAIT: Duration = Duration::from_secs(600);

pub fn integration(ctx: &Ctx, op: &str, config_dir: &Path, scope: &str) -> Result<Value, String> {
    let out = run(
        &ctx.id.executable.display().to_string(),
        &["--integration", op, "--config-dir", &config_dir.display().to_string(), "--scope", scope],
        Duration::from_secs(60),
    );
    out.json().ok_or_else(|| format!("--integration {op}: no JSON ({} / {})", out.stdout.trim(), out.stderr.trim()))
}

/// The installed session-scope integration: the owned mod copy and settings.
pub struct Activation {
    pub plugin_dir: String,
    pub settings: PathBuf,
    pub record: Value,
}

pub fn activate(ctx: &Ctx, scratch: &mut Scratch<'_>) -> Result<Activation, String> {
    // Without Terminal consent no session can bind to its tab; name that
    // blocker instead of timing out on the first binding.
    match ctx.companion().request(ControlRequestBody::IntegrationStatus, Duration::from_secs(20))? {
        ControlResponseBody::IntegrationStatus { report } if report.terminal.automation == AutomationPermission::Authorized => {}
        ControlResponseBody::IntegrationStatus { report } => {
            return Err(format!(
                "BLOCKED: Terminal automation is {:?} ({}) for the {} channel; `threadspace-qualify {} request-terminal` asks the owner once",
                report.terminal.automation,
                report.terminal.automation_status_code,
                ctx.channel_name(),
                ctx.channel_name()
            ));
        }
        other => return Err(format!("integration status: unexpected {other:?}")),
    }
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

/// The command a person would type in a fresh window to start an observed
/// session (the harness types it).
pub fn claude_command(dir: &Path, activation: &Activation, binary: &Path, extra: &[&str], env: &[(&str, &str)]) -> String {
    let mut command = format!(
        "cd {} && export CLAUDE_CODE_PLUGIN_DIRS={}",
        terminal::shell_quote(&dir.display().to_string()),
        terminal::shell_quote(&activation.plugin_dir)
    );
    for (key, value) in env {
        command.push_str(&format!(" {key}={}", terminal::shell_quote(value)));
    }
    command.push_str(&format!(
        " && {} --settings {} --model {MODEL}",
        terminal::shell_quote(&binary.display().to_string()),
        terminal::shell_quote(&activation.settings.display().to_string())
    ));
    for arg in extra {
        command.push(' ');
        command.push_str(&terminal::shell_quote(arg));
    }
    command
}

pub fn launcher() -> Result<PathBuf, String> {
    if let Some(executable) = std::env::var_os("THREADSPACE_CLAUDE_EXECUTABLE") {
        let executable = PathBuf::from(executable);
        if !executable.is_absolute() || !executable.is_file() {
            return Err("THREADSPACE_CLAUDE_EXECUTABLE must name an existing absolute executable path".into());
        }
        return executable.canonicalize().map_err(|error| error.to_string());
    }
    Ok(threadspace_relay::paths::home_dir().ok_or("no home")?.join(".local/bin/claude"))
}

/// An observed Claude session in its own disposable window.
pub struct Observed {
    pub tab: Tab,
    pub dir: PathBuf,
    pub native_session_id: String,
    pub session_id: String,
}

/// Opens a window, types `command`, accepts the folder-trust prompt there
/// only, and waits for inventory and the companion to bind the session to
/// that window's TTY.
pub fn start_observed(ctx: &Ctx, dir: PathBuf, command: &str) -> Result<Observed, String> {
    terminal::wait_scriptable(TERMINAL_WAIT)?;
    let tab = {
        let _gui = ctx.gui("m2 start observed Claude")?;
        let tab = Tab::open(dir.clone(), command).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&dir);
        })?;
        threadspace_harness::pause_ms(4000);
        // The folder-trust dialog preselects "No, exit": Down then Return.
        tab.type_line("\u{1b}[B");
        tab
    };
    // A start that fails closes its own window and directory.
    let native_session_id = match wait_inventory(&dir, Duration::from_secs(60)) {
        Ok(id) => id,
        Err(error) => {
            let _ = tab.close();
            let _ = std::fs::remove_dir_all(&dir);
            return Err(error);
        }
    };
    let started = crate::terminal_gates::StartedClaude { native_session_id, pid: 0, tab };
    let Some(session_id) = started.bound_session(ctx, Duration::from_secs(90)) else {
        let error = format!("companion never bound {} to {}", started.native_session_id, started.tab.tty);
        let _ = started.tab.close();
        let _ = std::fs::remove_dir_all(&dir);
        return Err(error);
    };
    // Inventory and binding follow the process, not the prompt editor:
    // text typed before the editor is ready is held unsubmitted.
    let ready = Instant::now();
    while ready.elapsed() < Duration::from_secs(30) && !prompt_ready(&started.tab.contents()) {
        threadspace_harness::pause_ms(500);
    }
    threadspace_harness::pause_ms(1000);
    Ok(Observed {
        native_session_id: started.native_session_id.clone(),
        session_id,
        dir,
        tab: started.tab,
    })
}

/// Claude's banner and prompt footer are drawn and the trust dialog is gone.
fn prompt_ready(screen: &str) -> bool {
    screen.contains("Claude Code v")
        && !screen.contains("Enter to confirm")
        && (screen.contains("shift+tab to cycle") || screen.contains("? for shortcuts"))
}

/// The interactive inventory row whose cwd is `dir`.
fn wait_inventory(dir: &Path, timeout: Duration) -> Result<String, String> {
    let home = threadspace_relay::paths::home_dir().ok_or("no home")?;
    let install = threadspace_provider_claude::inventory::ClaudeInstall::resolve(&home.join(".local/bin/claude"))
        .ok_or("claude not installed")?;
    let cli = threadspace_provider_claude::inventory::ClaudeCli {
        binary: install.binary,
        home,
        timeout: Duration::from_secs(8),
        now_ms: threadspace_harness::now_ms,
    };
    let wanted = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let started = Instant::now();
    use threadspace_provider_claude::inventory::Inventory;
    while started.elapsed() < timeout {
        if let Ok(snapshot) = cli.fetch()
            && let Some(row) = snapshot.rows.iter().find(|row| {
                row.is_interactive()
                    && row.cwd.as_deref().is_some_and(|cwd| PathBuf::from(cwd).canonicalize().ok() == Some(wanted.clone()))
                    && row.full_session_id().is_some()
            })
        {
            return Ok(row.full_session_id().unwrap_or_default().to_owned());
        }
        threadspace_harness::pause_ms(1000);
    }
    Err(format!("no inventory row for {}", redact_home(&dir.display().to_string())))
}

pub fn view<'a>(snapshot: &'a FleetSnapshot, native: &str) -> Vec<&'a SessionView> {
    snapshot.sessions.iter().filter(|s| s.native_session_id == native).collect()
}

/// The companion's view of one session (it must be exactly one).
pub fn session_view(ctx: &Ctx, native: &str) -> Result<SessionView, String> {
    let (_, snapshot) = companion_snapshot(ctx)?;
    match view(&snapshot, native).as_slice() {
        [one] => Ok((*one).clone()),
        many => Err(format!("{} views of session {native}", many.len())),
    }
}

/// Read-only queries against the companion's journal.
pub fn journal_query(ctx: &Ctx, sql: &str) -> Result<String, String> {
    let db = ctx.id.agent.journal.display().to_string();
    let out = run(
        "/usr/bin/sqlite3",
        &["-readonly", "-json", &format!("file:{db}?mode=ro"), sql],
        Duration::from_secs(10),
    );
    if out.ok { Ok(out.stdout) } else { Err(out.stderr) }
}

/// Completed native turns of a session, as the canonical turns table holds.
pub fn completed_turns(ctx: &Ctx, session_id: &str) -> usize {
    let sql = format!(
        "SELECT COUNT(*) AS n FROM turns WHERE session_id = '{}' AND state = 'COMPLETED'",
        session_id.replace('\'', "")
    );
    journal_query(ctx, &sql)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|rows| rows[0]["n"].as_u64())
        .unwrap_or(0) as usize
}

/// Inputs the companion recorded for a session.
fn submitted_inputs(ctx: &Ctx, session_id: &str) -> usize {
    let sql = format!("SELECT COUNT(*) AS n FROM inputs WHERE session_id = '{}'", session_id.replace('\'', ""));
    journal_query(ctx, &sql)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|rows| rows[0]["n"].as_u64())
        .unwrap_or(0) as usize
}

/// Waits until `condition` holds for the session's view.
pub fn wait_view(ctx: &Ctx, native: &str, timeout: Duration, condition: impl Fn(&SessionView) -> bool) -> Result<(SessionView, u64), String> {
    let started = Instant::now();
    let mut last = None;
    while started.elapsed() < timeout {
        if let Ok(view) = session_view(ctx, native) {
            if condition(&view) {
                return Ok((view, started.elapsed().as_millis() as u64));
            }
            last = Some(view);
        }
        threadspace_harness::pause_ms(250);
    }
    Err(format!("view condition not met in {timeout:?}; last {:?}", last.map(|v| (v.turn_state, v.observation, v.observer_tier))))
}

/// Types a prompt and waits for the session's completed-turn count to reach
/// `expected`; returns the elapsed milliseconds.
pub fn prompt_and_complete(ctx: &Ctx, observed: &Observed, text: &str, expected: usize) -> Result<u64, String> {
    let started = Instant::now();
    submit(ctx, observed, text)?;
    while started.elapsed() < TURN_TIMEOUT {
        if completed_turns(ctx, &observed.session_id) >= expected {
            return Ok(started.elapsed().as_millis() as u64);
        }
        threadspace_harness::pause_ms(250);
    }
    Err(format!("turn {expected} did not complete within {TURN_TIMEOUT:?}"))
}

/// Submits a prompt to an idle session and confirms it from the companion's
/// inputs. A return the editor did not take is sent again every 4 s (the
/// text is already in the prompt; a return on an empty prompt is ignored);
/// answers how many returns were resent.
pub fn submit(ctx: &Ctx, observed: &Observed, text: &str) -> Result<u32, String> {
    let before = submitted_inputs(ctx, &observed.session_id);
    observed.tab.submit_line(text);
    let started = Instant::now();
    let mut resent = 0;
    while submitted_inputs(ctx, &observed.session_id) <= before {
        if started.elapsed() > Duration::from_secs(40) {
            return Err(format!("prompt was never submitted ({resent} returns resent)"));
        }
        if started.elapsed() > Duration::from_secs(4 * (u64::from(resent) + 1)) {
            observed.tab.type_line("\r");
            resent += 1;
        }
        threadspace_harness::pause_ms(250);
    }
    Ok(resent)
}

/// Types a prompt and sends the return again after the paste window, for a
/// session whose inputs the companion may not record (stopped, blocked or
/// limited); a return on an empty prompt is ignored.
pub fn submit_unconfirmed(observed: &Observed, text: &str) {
    observed.tab.submit_line(text);
    threadspace_harness::pause_ms(4000);
    observed.tab.type_line("\r");
}

/// The UI's own controls, through the qualification command handlers.
pub fn ui(ctx: &Ctx, command: &str, args: Value) -> Result<Value, String> {
    ctx.app().view_command(command, args, Duration::from_secs(40))
}

/// Ensures one hydrated packaged UI.
pub fn ensure_ui(ctx: &Ctx) -> Result<Value, String> {
    let app = ctx.app();
    // A filter no session matches keeps the liveness probe's report small.
    if !app.processes().is_empty() && ui(ctx, "m2-fleet", json!({ "sessionId": uuid::Uuid::nil().to_string() })).is_ok() {
        return Ok(json!({ "reused": true }));
    }
    app.stop_all();
    let mut cursor = ctx.companion().log();
    let launched = app.launch_packaged(&[])?;
    let hydrated = app.wait_hydrated(&mut cursor, Duration::from_secs(30)).is_some();
    Ok(json!({ "uiPid": launched.ui.pid, "hydrated": hydrated }))
}

/// Presses the UI's Return for a session and reads back independently.
pub fn press_return(ctx: &Ctx, observed: &Observed) -> Value {
    let started = Instant::now();
    let pressed = ui(ctx, "m2-press-return", json!({ "sessionId": observed.session_id }));
    let elapsed = started.elapsed().as_millis() as u64;
    threadspace_harness::pause_ms(300);
    let selected = terminal::selected_tty_read();
    let front = crate::terminal_gates::frontmost_bundle();
    let result = pressed.as_ref().ok().and_then(|p| p["result"].get("route").cloned()).unwrap_or(Value::Null);
    let exact = result["surfaceResult"] == "EXACT_NATIVE_SURFACE" && result["sessionVerification"] == "CURRENT_NATIVE_REVALIDATED";
    let readback = selected.as_ref().ok().map(String::as_str) == Some(observed.tab.tty.as_str());
    json!({
        "sessionId": observed.session_id,
        "expectedTty": observed.tab.tty,
        "ui": pressed.as_ref().ok(),
        "uiError": pressed.as_ref().err(),
        "route": result,
        "exact": exact,
        "independentSelectedTty": selected.as_ref().ok(),
        "independentReadbackError": selected.as_ref().err(),
        "independentFrontmost": front,
        "readbackMatches": readback,
        "wrongTarget": exact && selected.is_ok() && !readback,
        "harnessElapsedMs": elapsed,
    })
}

/// Brings a different harness-owned window to the front, so the next Return
/// has to move focus.
pub fn front_other(ctx: &Ctx, other: &Tab) {
    let _gui = ctx.gui("m2 front another window");
    other.select();
    threadspace_harness::pause_ms(400);
}

#[path = "m2_ownership.rs"]
mod m2_ownership;
pub use m2_ownership::{Scratch, disposable};

impl<'a> Scratch<'a> {
    pub fn new(ctx: &'a Ctx, label: &str) -> Result<Self, String> {
        Self::with_runner(
            label,
            Box::new(move |op, config, scope| integration(ctx, op, config, scope)),
        )
    }
}

/// Closes a harness-owned window (ownership proven by `Tab::close`).
pub fn finish(observed: &Observed) -> Value {
    let closed = observed.tab.close();
    let _ = std::fs::remove_dir_all(&observed.dir);
    closed
}

// ------------------------------------------------------------------ cycles

/// Owner-like settings with foreign hooks, matchers, MCP servers, env and
/// unicode: everything an install must leave as it was.
fn complex_settings() -> Value {
    json!({
        "$schema": "https://json.schemastore.org/claude-code-settings.json",
        "permissions": { "allow": ["Bash(git status:*)"], "deny": ["Read(./.env)"] },
        "env": { "CLAUDE_CODE_PLUGIN_DIRS": "/opt/owner/plugins", "EDITOR": "vim", "GREETING": "héllo — 世界" },
        "hooks": {
            "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "/opt/owner/bin/audit --pre" }] }],
            "Stop": [{ "hooks": [{ "type": "command", "command": "/opt/owner/bin/notify-stop", "timeout": 5 }] }],
            "WorktreeCreate": [{ "hooks": [{ "type": "command", "command": "/opt/owner/bin/worktree" }] }]
        },
        "mcpServers": { "owner-docs": { "command": "/opt/owner/bin/docs-mcp", "args": ["--port", "0"] } },
        "statusLine": { "type": "command", "command": "/opt/owner/bin/status" },
        "unknownFutureKey": [1, 2.5, true, null]
    })
}

fn owned_count(settings: &Value, command_fragment: &str) -> usize {
    settings["hooks"]
        .as_object()
        .map(|events| {
            events
                .values()
                .flat_map(|groups| groups.as_array().into_iter().flatten())
                .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
                .filter(|hook| hook["command"].as_str().is_some_and(|c| c.contains(command_fragment)))
                .count()
        })
        .unwrap_or(0)
}

/// `count` install → reinstall → remove cycles on an empty and an
/// owner-like configuration (MILESTONES M2: ten cycles create no duplicate
/// handlers and preserve unrelated settings).
pub fn cycles(ctx: &Ctx, count: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "integration-cycles", ctx.channel_name()).map_err(|e| e.to_string())?;
    let mut all_ok = true;
    let mut fixtures = Vec::new();
    for (label, original) in [("empty", None), ("owner-like", Some(complex_settings()))] {
        let mut scratch = Scratch::new(ctx, &format!("cycles-{label}"))?;
        let config = scratch.dir.join("session-config");
        std::fs::create_dir(&config).map_err(|e| e.to_string())?;
        let settings = config.join("settings.json");
        let original_bytes = original.as_ref().map(|v| format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default()));
        if let Some(bytes) = &original_bytes {
            std::fs::write(&settings, bytes).map_err(|e| e.to_string())?;
        }
        let original_sha = sha256_file(&settings);
        for cycle in 1..=count {
            let install = scratch.install_user()?;
            let reinstall = scratch.reinstall()?;
            let applied: Value = std::fs::read_to_string(&settings)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            let owned = owned_count(&applied, "integrations/claude/bin/threadspace-hook");
            let worktree_owned = ["WorktreeCreate", "WorktreeRemove"].iter().any(|event| {
                applied["hooks"][event]
                    .as_array()
                    .is_some_and(|groups| groups.iter().any(|g| g.to_string().contains("threadspace-hook")))
            });
            let plugin_dirs = applied["env"]["CLAUDE_CODE_PLUGIN_DIRS"].as_str().unwrap_or_default().to_owned();
            let ours = plugin_dirs.split(':').filter(|p| p.contains("integrations/claude/observer")).count();
            let foreign_kept = original.as_ref().is_none_or(|o| {
                applied["permissions"] == o["permissions"]
                    && applied["mcpServers"] == o["mcpServers"]
                    && applied["statusLine"] == o["statusLine"]
                    && applied["unknownFutureKey"] == o["unknownFutureKey"]
                    && applied["env"]["GREETING"] == o["env"]["GREETING"]
                    && plugin_dirs.starts_with("/opt/owner/plugins")
                    && owned_count(&applied, "/opt/owner/bin/") == 3
            });
            let removed = scratch.remove_integration()?;
            let restored = sha256_file(&settings) == original_sha;
            let ok = install["ok"] == json!(true)
                && reinstall["ok"] == json!(true)
                && removed["ok"] == json!(true)
                && removed["detail"]["complete"] == json!(true)
                && owned == 15
                && !worktree_owned
                && ours == 1
                && foreign_kept
                && restored;
            all_ok &= ok;
            run_dir
                .append("cycles.jsonl", &json!({
                    "fixture": label, "cycle": cycle, "ok": ok, "ownedHooks": owned, "worktreeOwned": worktree_owned,
                    "ownedPluginDirs": ours, "foreignKept": foreign_kept, "originalRestored": restored,
                    "appliedSha256": install["detail"]["appliedSha256"], "reinstallAppliedSha256": reinstall["detail"]["appliedSha256"],
                    "restoredOriginal": removed["detail"]["restoredOriginal"], "conflicts": removed["detail"]["conflicts"],
                }))
                .map_err(|e| e.to_string())?;
        }
        fixtures.push(json!({ "fixture": label, "originalSha256": original_sha, "finalSha256": sha256_file(&settings) }));
    }
    let summary = json!({ "pass": all_ok, "cycles": count, "fixtures": fixtures });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

// ---------------------------------------------------------------- vertical

/// The vertical slice: one session, `cycles` prompt → completion → Return
/// → follow-up cycles, each a tool-using native turn, keeping one Session
/// and one worker; then a clean exit leaves the worker as history.
pub fn vertical(ctx: &Ctx, cycles: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "vertical", ctx.channel_name()).map_err(|e| e.to_string())?;
    let mut scratch = Scratch::new(ctx, "vertical")?;
    let activation = activate(ctx, &mut scratch)?;
    let ui_state = ensure_ui(ctx)?;
    let recording = Recording::start()?;
    let start_cursor = journal_cursor(ctx);
    let dir = disposable("vertical-work")?;
    let command = claude_command(&dir, &activation, &launcher()?, &["--allowedTools", "Bash(echo:*)"], &[]);
    let observed = start_observed(ctx, dir, &command)?;
    let other = Tab::open_inert(disposable("vertical-other")?)?;
    let mut pass = true;
    let mut completed = 0usize;
    for cycle in 1..=cycles {
        let terminal_wait = terminal::wait_scriptable(TERMINAL_WAIT);
        let prompt = format!("Use the Bash tool to run exactly: echo threadspace-m2-{cycle}. Then reply with only the word done.");
        completed += 1;
        let turn_ms = prompt_and_complete(ctx, &observed, &prompt, completed);
        let after = session_view(ctx, &observed.native_session_id);
        let fleet = ui(ctx, "m2-fleet", json!({ "sessionId": observed.session_id }));
        front_other(ctx, &other);
        let returned = {
            let _gui = ctx.gui("m2 vertical Return");
            press_return(ctx, &observed)
        };
        completed += 1;
        let follow_ms = prompt_and_complete(ctx, &observed, "Reply with only the word again.", completed);
        let again = session_view(ctx, &observed.native_session_id);
        let sessions = companion_snapshot(ctx).map_or(0, |(_, snapshot)| view(&snapshot, &observed.native_session_id).len());
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
            && returned["exact"] == json!(true)
            && returned["readbackMatches"] == json!(true);
        pass &= ok;
        run_dir
            .append("cycles.jsonl", &json!({
                "cycle": cycle, "ok": ok, "terminalWaitMs": terminal_wait.as_ref().ok(), "terminalWaitError": terminal_wait.as_ref().err(), "turnMs": turn_ms.as_ref().ok(), "turnError": turn_ms.as_ref().err(),
                "followUpMs": follow_ms.as_ref().ok(), "followUpError": follow_ms.as_ref().err(),
                "sessionViews": sessions, "workers": workers, "fleetError": fleet.as_ref().err(),
                "fleetRow": fleet.as_ref().ok().and_then(|f| f["result"]["rows"].get(0).cloned()),
                "fleetRowsInView": fleet.as_ref().ok().map(|f| f["result"]["rowsInView"].clone()),
                "afterTurn": after.as_ref().ok().map(|v| json!({ "turnState": v.turn_state, "observation": v.observation, "observerTier": v.observer_tier, "observerVersion": v.observer_version, "presence": v.execution_presence })),
                "return": returned,
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
    observed.tab.submit_line("/exit");
    let exited = wait_view(ctx, &observed.native_session_id, Duration::from_secs(60), |v| {
        format!("{:?}", v.execution_presence) == "Ended"
    });
    let trace = trace(ctx, &observed.native_session_id, &observed.session_id, start_cursor);
    let trace_rows = trace.as_array().map_or(0, Vec::len);
    run_dir.write_json("trace.json", &trace).map_err(|e| e.to_string())?;
    let history = ui(ctx, "m2-fleet", json!({ "sessionId": observed.session_id }));
    let worker_kept = history
        .as_ref()
        .ok()
        .and_then(|f| f["result"]["rows"].as_array())
        .is_some_and(|rows| rows.iter().any(|r| r["sessionId"] == json!(observed.session_id)));
    pass &= exited.is_ok() && worker_kept;
    let closed = finish(&observed);
    let _ = other.close();
    let _ = std::fs::remove_dir_all(&other.dir);
    let removed = scratch.remove_integration();
    let recording = recording.map(|mut recording| recording.stop());
    let summary = json!({
        "pass": pass,
        "cycles": cycles,
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
        "windowClosed": closed,
        "activation": activation.record["detail"]["pluginDir"].as_str().map(redact_home),
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
        "terminalRefusals": refusals(),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

/// Terminal scripting refusals the harness waited out in this run.
pub fn refusals() -> Value {
    let (count, ms) = terminal::refusals();
    json!({ "count": count, "waitedMs": ms })
}

/// An uncut 1 fps time-lapse of the main display for a run, written only to
/// the private directory `THREADSPACE_M2_RECORDING_DIR` outside the
/// repository (it shows whatever the display shows); the evidence keeps its
/// digest. A video recording cannot be used: while any screen video
/// capture runs, Terminal refuses every scripting element query, which
/// both the harness and the companion's Return need. Stills do not.
struct Recording {
    dir: PathBuf,
    movie: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<u64>>,
    started: Instant,
}

const FRAME_INTERVAL: Duration = Duration::from_secs(1);

impl Recording {
    fn start() -> Result<Option<Self>, String> {
        let Some(root) = std::env::var_os("THREADSPACE_M2_RECORDING_DIR").map(PathBuf::from) else {
            return Ok(None);
        };
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let repo = std::env::current_dir().and_then(|d| d.canonicalize()).map_err(|e| e.to_string())?;
        if root.starts_with(&repo) {
            return Err("THREADSPACE_M2_RECORDING_DIR must be outside the repository".into());
        }
        let name = format!("m2-vertical-{}", threadspace_harness::now_ms());
        let dir = root.join(&name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (frames, flag) = (dir.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            let mut count = 0u64;
            let begun = Instant::now();
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let frame = frames.join(format!("frame-{count:05}.jpg"));
                run("/usr/sbin/screencapture", &["-x", "-t", "jpg", "-D1", &frame.display().to_string()], Duration::from_secs(5));
                count += 1;
                // Frames stay on a fixed one-second grid, so the movie plays in real time.
                let next = FRAME_INTERVAL * count as u32;
                if let Some(wait) = next.checked_sub(begun.elapsed()) {
                    std::thread::sleep(wait);
                }
            }
            count
        });
        Ok(Some(Self { movie: root.join(format!("{name}.mp4")), dir, stop, worker: Some(worker), started: Instant::now() }))
    }

    /// Stops the frames, assembles them at one frame per second, keeps the
    /// movie and removes the frames once the movie exists.
    fn stop(&mut self) -> Value {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let frames = self.worker.take().and_then(|w| w.join().ok()).unwrap_or(0);
        let wall_ms = self.started.elapsed().as_millis() as u64;
        let pattern = self.dir.join("frame-%05d.jpg").display().to_string();
        let movie = self.movie.display().to_string();
        let assembled = run(
            "/opt/homebrew/bin/ffmpeg",
            &["-v", "error", "-y", "-framerate", "1", "-i", &pattern, "-vf", "scale=trunc(iw/4)*2:trunc(ih/4)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", &movie],
            Duration::from_secs(600),
        );
        let bytes = std::fs::metadata(&self.movie).map(|m| m.len()).ok();
        if assembled.ok && bytes.is_some_and(|b| b > 0) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
        let digest = run("/usr/bin/shasum", &["-a", "256", &movie], Duration::from_secs(120));
        json!({
            "kind": "uncut time-lapse, one still per second, played at one frame per second",
            "file": self.movie.file_name().map(|n| n.to_string_lossy().into_owned()),
            "privateDirectory": "THREADSPACE_M2_RECORDING_DIR (outside the repository)",
            "frames": frames,
            "frameIntervalMs": FRAME_INTERVAL.as_millis() as u64,
            "wallMs": wall_ms,
            "assembled": assembled.ok,
            "assembleError": (!assembled.ok).then(|| assembled.stderr.trim().to_owned()),
            "framesKept": !(assembled.ok && bytes.is_some_and(|b| b > 0)),
            "sha256": digest.stdout.split_whitespace().next(),
            "bytes": bytes,
        })
    }
}

/// A run that ends early still stops its frames.
impl Drop for Recording {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.stop();
        }
    }
}

/// The session's hook, observer and inventory observations after `after`,
/// in ingest order, each with the canonical fact kinds it produced: the
/// journal IDs that join the native trace to canonical state. Payloads are
/// not copied.
fn trace(ctx: &Ctx, native: &str, session_id: &str, after: u64) -> Value {
    let (native, session) = (native.replace('\'', ""), session_id.replace('\'', ""));
    journal_query(ctx, &format!(
        "SELECT o.ingest_seq AS cursor, o.observation_id AS observationId, o.source_id AS source, o.native_event AS nativeEvent,
                o.sequence_meaning AS sequenceMeaning, o.captured_wall_ms AS capturedWallMs, o.received_wall_ms AS receivedWallMs,
                (SELECT json_group_array(f.kind) FROM facts f WHERE f.observation_id = o.observation_id) AS facts
           FROM observations o
          WHERE o.ingest_seq > {after}
            AND (json_extract(o.payload_json, '$.sessionKey.nativeSessionId') = '{native}'
                 OR EXISTS (SELECT 1 FROM facts f WHERE f.observation_id = o.observation_id AND f.session_id = '{session}'))
          ORDER BY o.ingest_seq"
    ))
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    .map(|rows| {
        Value::Array(
            rows.as_array()
                .into_iter()
                .flatten()
                .map(|row| {
                    let mut row = row.clone();
                    if let Some(facts) = row["facts"].as_str().and_then(|t| serde_json::from_str::<Value>(t).ok()) {
                        row["facts"] = facts;
                    }
                    row
                })
                .collect(),
        )
    })
    .unwrap_or(Value::Null)
}

/// The journal's last ingest sequence (the projection cursor).
pub(crate) fn journal_cursor(ctx: &Ctx) -> u64 {
    journal_query(ctx, "SELECT COALESCE(MAX(ingest_seq), 0) AS n FROM observations")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|rows| rows[0]["n"].as_u64())
        .unwrap_or(0)
}

/// The original vertical runner has no cross-runtime clock calibration.
/// Retain its observation count, but never turn pre-transaction receipt time
/// into a durable-commit or DOM latency PASS. The focused F4 begin/end
/// collector and `tests/native/tools/m2_latency.py` own those gates.
fn latency(ctx: &Ctx, native: &str, after: u64) -> Value {
    let observed = journal_query(ctx, &format!(
        "SELECT source_id, COUNT(*) AS observations FROM observations
          WHERE ingest_seq > {after} AND source_id IN ('claude.hook', 'claude.observer')
            AND json_extract(payload_json, '$.sessionKey.nativeSessionId') = '{}'
          GROUP BY source_id",
        native.replace('\'', "")
    ));
    json!({
        "schemaVersion": 2,
        "status": "INCOMPLETE",
        "normalPathPass": false,
        "reason": "This runner does not independently collect the calibrated capture/SQLite-COMMIT/DOM population. Use m2-latency-begin/end and the F4 calculator.",
        "targets": { "captureToCommitP95Ms": 100, "commitToDomP95Ms": 100, "eventToDomP95Ms": 250 },
        "observationsBySource": observed.as_ref().ok().and_then(|text| serde_json::from_str::<Value>(text).ok()),
        "observationReadError": observed.as_ref().err(),
    })
}

/// Presses the inspector's Mark handled for each open attention item of a
/// session and reads back, for each, the view's resolution, the journaled
/// owner command and the item's resolution in canonical state.
fn mark_handled(ctx: &Ctx, session_id: &str) -> Value {
    let sid = session_id.replace('\'', "");
    let open: Vec<Value> = journal_query(ctx, &format!(
        "SELECT id, category FROM attention_items WHERE session_id = '{sid}' AND resolved_at_ms IS NULL ORDER BY created_at_ms LIMIT 25"
    ))
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    .and_then(|v| v.as_array().cloned())
    .unwrap_or_default();
    let mut items = Vec::new();
    for item in &open {
        let id = item["id"].as_str().unwrap_or_default().replace('\'', "");
        let pressed = ui(ctx, "m2-press-mark-handled", json!({ "attentionId": id }));
        let result = pressed.as_ref().ok().map(|p| p["result"].clone()).unwrap_or(Value::Null);
        let journal = journal_query(ctx, &format!(
            "SELECT i.resolved_at_ms IS NOT NULL AS resolved, i.resolution_reason AS reason,
                    (SELECT COUNT(*) FROM attention_commands c WHERE c.attention_id = i.id AND c.observation_id IS NOT NULL) AS commands
               FROM attention_items i WHERE i.id = '{id}'"
        ))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|rows| rows[0].clone())
        .unwrap_or(Value::Null);
        let ok = result["resolvedInView"] == json!(true)
            && result["listedAfter"] == json!(false)
            && journal["resolved"] == json!(1)
            && journal["commands"].as_u64().unwrap_or(0) >= 1;
        items.push(json!({
            "attentionId": id, "category": item["category"], "ok": ok,
            "view": { "clicked": result["clicked"], "resolvedInView": result["resolvedInView"], "listedAfter": result["listedAfter"], "receipt": result["receipt"], "error": result["error"] },
            "uiError": pressed.as_ref().err(), "journal": journal,
        }));
    }
    json!({
        "pass": !items.is_empty() && items.iter().all(|i| i["ok"] == json!(true)),
        "openBefore": open.len(),
        "items": items,
    })
}

// ------------------------------------------------------------------ routes

/// `count` exact Returns through the UI's Return across three observed
/// sessions, each preceded by fronting a different window, each verified
/// by independent readback; zero wrong targets.
pub fn routes(ctx: &Ctx, count: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "routes", ctx.channel_name()).map_err(|e| e.to_string())?;
    let mut scratch = Scratch::new(ctx, "routes")?;
    let activation = activate(ctx, &mut scratch)?;
    ensure_ui(ctx)?;
    let mut sessions = Vec::new();
    for index in 0..3 {
        let dir = disposable(&format!("routes-{index}"))?;
        let command = claude_command(&dir, &activation, &launcher()?, &[], &[]);
        sessions.push(start_observed(ctx, dir, &command)?);
    }
    let (mut exact, mut wrong, mut latencies) = (0u32, 0u32, Vec::new());
    for index in 0..count as usize {
        let target = &sessions[index % sessions.len()];
        let other = &sessions[(index + 1) % sessions.len()];
        let terminal_wait = terminal::wait_scriptable(TERMINAL_WAIT);
        let mut result = {
            let _gui = ctx.gui("m2 route");
            other.tab.select();
            threadspace_harness::pause_ms(400);
            press_return(ctx, target)
        };
        if result["exact"] == json!(true) && result["readbackMatches"] == json!(true) {
            exact += 1;
        }
        if result["wrongTarget"] == json!(true) {
            wrong += 1;
        }
        if let Some(ms) = result["route"]["latencyMs"].as_u64() {
            latencies.push(ms);
        }
        result["terminalWaitMs"] = json!(terminal_wait.as_ref().ok());
        result["terminalWaitError"] = json!(terminal_wait.as_ref().err());
        run_dir.append("routes.jsonl", &result).map_err(|e| e.to_string())?;
    }
    latencies.sort_unstable();
    let p95 = latencies.get((latencies.len() * 95).div_ceil(100).saturating_sub(1)).copied();
    for observed in &sessions {
        observed.tab.submit_line("/exit");
    }
    threadspace_harness::pause_ms(3000);
    let closed: Vec<Value> = sessions.iter().map(finish).collect();
    let removed = scratch.remove_integration();
    let summary = json!({
        "pass": exact == count && wrong == 0,
        "requested": count, "exactVerified": exact, "wrongTargets": wrong,
        "routeLatencyP95Ms": p95, "routeLatencyMaxMs": latencies.last(),
        "windowsClosed": closed,
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
        "terminalRefusals": refusals(),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}
