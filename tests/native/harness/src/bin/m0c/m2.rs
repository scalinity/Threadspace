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

pub fn activate(ctx: &Ctx, scratch: &Path) -> Result<Activation, String> {
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
    let config = scratch.join("session-config");
    std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    let installed = integration(ctx, "install", &config, "session")?;
    if installed["ok"] != json!(true) {
        return Err(format!("session install failed: {installed}"));
    }
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

/// A runner's scratch directory and the session-scope install made in it,
/// both removed however the runner ends (a second uninstall after an
/// explicit one answers NOT_INSTALLED).
pub struct Scratch<'a> {
    ctx: &'a Ctx,
    pub dir: PathBuf,
}

impl<'a> Scratch<'a> {
    pub fn new(ctx: &'a Ctx, label: &str) -> Result<Self, String> {
        Ok(Self { ctx, dir: disposable(label)? })
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        let _ = integration(self.ctx, "uninstall", &self.dir.join("session-config"), "session");
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn disposable(label: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("ts-m2-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
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
        let config = disposable(&format!("cycles-{label}"))?;
        let settings = config.join("settings.json");
        let original_bytes = original.as_ref().map(|v| format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default()));
        if let Some(bytes) = &original_bytes {
            std::fs::write(&settings, bytes).map_err(|e| e.to_string())?;
        }
        let original_sha = sha256_file(&settings);
        for cycle in 1..=count {
            let install = integration(ctx, "install", &config, "user")?;
            let reinstall = integration(ctx, "install", &config, "user")?;
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
            let removed = integration(ctx, "uninstall", &config, "user")?;
            let restored = sha256_file(&settings) == original_sha;
            let ok = install["ok"] == json!(true)
                && reinstall["ok"] == json!(true)
                && removed["ok"] == json!(true)
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
        let _ = std::fs::remove_dir_all(&config);
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
    let scratch = Scratch::new(ctx, "vertical")?;
    let activation = activate(ctx, &scratch.dir)?;
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
    let removed = integration(ctx, "uninstall", &scratch.dir.join("session-config"), "session");
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

/// An uncut recording of the main display for a run, written only to the
/// private directory `THREADSPACE_M2_RECORDING_DIR` outside the repository
/// (it shows whatever the display shows); the evidence keeps its digest.
struct Recording {
    child: std::process::Child,
    path: PathBuf,
    started: Instant,
    stopped: bool,
}

impl Recording {
    fn start() -> Result<Option<Self>, String> {
        let Some(dir) = std::env::var_os("THREADSPACE_M2_RECORDING_DIR").map(PathBuf::from) else {
            return Ok(None);
        };
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let dir = dir.canonicalize().map_err(|e| e.to_string())?;
        let repo = std::env::current_dir().and_then(|d| d.canonicalize()).map_err(|e| e.to_string())?;
        if dir.starts_with(&repo) {
            return Err("THREADSPACE_M2_RECORDING_DIR must be outside the repository".into());
        }
        let path = dir.join(format!("m2-vertical-{}.mov", threadspace_harness::now_ms()));
        let child = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", "-v", "-D1"])
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("screencapture: {e}"))?;
        Ok(Some(Self { child, path, started: Instant::now(), stopped: false }))
    }

    /// Ends the recording (SIGINT finalizes the movie) and describes it.
    fn stop(&mut self) -> Value {
        let wall_ms = self.started.elapsed().as_millis() as u64;
        self.stopped = true;
        threadspace_harness::procs::signal(self.child.id() as i32, libc::SIGINT);
        let status = self.child.wait().ok().and_then(|s| s.code());
        let bytes = std::fs::metadata(&self.path).map(|m| m.len()).ok();
        let digest = run("/usr/bin/shasum", &["-a", "256", &self.path.display().to_string()], Duration::from_secs(120));
        json!({
            "file": self.path.file_name().map(|n| n.to_string_lossy().into_owned()),
            "privateDirectory": "THREADSPACE_M2_RECORDING_DIR (outside the repository)",
            "sha256": digest.stdout.split_whitespace().next(),
            "bytes": bytes,
            "wallMs": wall_ms,
            "exit": status,
        })
    }
}

/// A run that ends early still finalizes its recording.
impl Drop for Recording {
    fn drop(&mut self) {
        if !self.stopped {
            threadspace_harness::procs::signal(self.child.id() as i32, libc::SIGINT);
            let _ = self.child.wait();
        }
    }
}

/// The journal's last ingest sequence (the projection cursor).
fn journal_cursor(ctx: &Ctx) -> u64 {
    journal_query(ctx, "SELECT COALESCE(MAX(ingest_seq), 0) AS n FROM observations")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|rows| rows[0]["n"].as_u64())
        .unwrap_or(0)
}

fn percentiles(mut values: Vec<i64>) -> Value {
    values.sort_unstable();
    let at = |q: usize| values.get((values.len() * q).div_ceil(100).saturating_sub(1)).copied();
    json!({ "n": values.len(), "p50": at(50), "p95": at(95), "max": values.last() })
}

/// SPEC §20.2 latency for one session's observations after `after`: local
/// capture → commit (`received − captured`, per source; the observer's
/// includes its bounded drain), commit → applied DOM (the view's DOM first
/// showing that observation's own patch cursor) and native event → DOM.
fn latency(ctx: &Ctx, native: &str, after: u64) -> Value {
    let marks = ui(ctx, "m2-latency", json!({ "afterCursor": after.to_string() }))
        .ok()
        .and_then(|r| r["result"]["marks"].as_array().cloned())
        .unwrap_or_default();
    let dom: std::collections::HashMap<u64, i64> = marks
        .iter()
        .filter_map(|m| Some((m["cursor"].as_str()?.parse().ok()?, m["domWallMs"].as_f64()? as i64)))
        .collect();
    let rows: Vec<Value> = journal_query(ctx, &format!(
        "SELECT ingest_seq, source_id, captured_wall_ms, received_wall_ms FROM observations
          WHERE ingest_seq > {after} AND source_id IN ('claude.hook', 'claude.observer')
            AND json_extract(payload_json, '$.sessionKey.nativeSessionId') = '{}'",
        native.replace('\'', "")
    ))
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    .and_then(|v| v.as_array().cloned())
    .unwrap_or_default();
    #[derive(Default)]
    struct Spans {
        commit: Vec<i64>,
        view: Vec<i64>,
        total: Vec<i64>,
    }
    let mut by_source: std::collections::BTreeMap<String, Spans> = Default::default();
    for row in &rows {
        let (Some(seq), Some(source), Some(captured), Some(received)) =
            (row["ingest_seq"].as_u64(), row["source_id"].as_str(), row["captured_wall_ms"].as_i64(), row["received_wall_ms"].as_i64())
        else {
            continue;
        };
        let entry = by_source.entry(source.to_owned()).or_default();
        entry.commit.push(received - captured);
        if let Some(&shown) = dom.get(&seq) {
            entry.view.push(shown - received);
            entry.total.push(shown - captured);
        }
    }
    let sources: serde_json::Map<String, Value> = by_source
        .into_iter()
        .map(|(source, spans)| {
            (source, json!({ "captureToCommitMs": percentiles(spans.commit), "commitToDomMs": percentiles(spans.view), "eventToDomMs": percentiles(spans.total) }))
        })
        .collect();
    let hook = &sources.get("claude.hook").cloned().unwrap_or(Value::Null);
    let within = |v: &Value, limit: i64| v["p95"].as_i64().is_some_and(|p| p <= limit);
    json!({
        "targets": { "captureToCommitP95Ms": 100, "commitToDomP95Ms": 100, "eventToDomP95Ms": 250 },
        "viewMarks": marks.len(),
        "observations": rows.len(),
        "bySource": sources,
        "normalPathPass": within(&hook["captureToCommitMs"], 100) && within(&hook["commitToDomMs"], 100) && within(&hook["eventToDomMs"], 250),
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
    let scratch = Scratch::new(ctx, "routes")?;
    let activation = activate(ctx, &scratch.dir)?;
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
    let removed = integration(ctx, "uninstall", &scratch.dir.join("session-config"), "session");
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
