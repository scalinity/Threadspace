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
    let tab = {
        let _gui = ctx.gui("m2 start observed Claude")?;
        let tab = Tab::open(dir.clone(), command)?;
        threadspace_harness::pause_ms(4000);
        // The folder-trust dialog preselects "No, exit": Down then Return.
        tab.type_line("\u{1b}[B");
        tab
    };
    let started = crate::terminal_gates::StartedClaude {
        native_session_id: wait_inventory(&dir, Duration::from_secs(60))?,
        pid: 0,
        tab,
    };
    let session_id = started
        .bound_session(ctx, Duration::from_secs(90))
        .ok_or_else(|| format!("companion never bound {} to {}", started.native_session_id, started.tab.tty))?;
    Ok(Observed {
        native_session_id: started.native_session_id.clone(),
        session_id,
        dir,
        tab: started.tab,
    })
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
    observed.tab.type_line(text);
    let started = Instant::now();
    while started.elapsed() < TURN_TIMEOUT {
        if completed_turns(ctx, &observed.session_id) >= expected {
            return Ok(started.elapsed().as_millis() as u64);
        }
        threadspace_harness::pause_ms(250);
    }
    Err(format!("turn {expected} did not complete within {TURN_TIMEOUT:?}"))
}

/// The UI's own controls, through the qualification command handlers.
pub fn ui(ctx: &Ctx, command: &str, args: Value) -> Result<Value, String> {
    ctx.app().view_command(command, args, Duration::from_secs(40))
}

/// Ensures one hydrated packaged UI.
pub fn ensure_ui(ctx: &Ctx) -> Result<Value, String> {
    let app = ctx.app();
    if !app.processes().is_empty() && ui(ctx, "m2-fleet", json!({})).is_ok() {
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
    let result = pressed.as_ref().ok().and_then(|p| p.get("route").cloned()).unwrap_or(Value::Null);
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
    let scratch = disposable("vertical")?;
    let activation = activate(ctx, &scratch)?;
    let ui_state = ensure_ui(ctx)?;
    let dir = disposable("vertical-work")?;
    let command = claude_command(&dir, &activation, &launcher()?, &["--allowedTools", "Bash(echo:*)"], &[]);
    let observed = start_observed(ctx, dir, &command)?;
    let other = Tab::open_inert(disposable("vertical-other")?)?;
    let mut pass = true;
    let mut completed = 0usize;
    for cycle in 1..=cycles {
        let prompt = format!("Use the Bash tool to run exactly: echo threadspace-m2-{cycle}. Then reply with only the word done.");
        completed += 1;
        let turn_ms = prompt_and_complete(ctx, &observed, &prompt, completed);
        let after = session_view(ctx, &observed.native_session_id);
        let fleet = ui(ctx, "m2-fleet", json!({}));
        front_other(ctx, &other);
        let returned = {
            let _gui = ctx.gui("m2 vertical Return");
            press_return(ctx, &observed)
        };
        completed += 1;
        let follow_ms = prompt_and_complete(ctx, &observed, "Reply with only the word again.", completed);
        let again = session_view(ctx, &observed.native_session_id);
        let (_, snapshot) = companion_snapshot(ctx)?;
        let sessions = view(&snapshot, &observed.native_session_id).len();
        let workers = fleet
            .as_ref()
            .ok()
            .and_then(|f| f["rows"].as_array())
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
                "cycle": cycle, "ok": ok, "turnMs": turn_ms.as_ref().ok(), "turnError": turn_ms.as_ref().err(),
                "followUpMs": follow_ms.as_ref().ok(), "followUpError": follow_ms.as_ref().err(),
                "sessionViews": sessions, "workers": workers,
                "afterTurn": after.as_ref().ok().map(|v| json!({ "turnState": v.turn_state, "observation": v.observation, "observerTier": v.observer_tier, "observerVersion": v.observer_version, "presence": v.execution_presence })),
                "return": returned,
            }))
            .map_err(|e| e.to_string())?;
    }
    observed.tab.type_line("/exit");
    let exited = wait_view(ctx, &observed.native_session_id, Duration::from_secs(60), |v| {
        format!("{:?}", v.execution_presence) == "Ended"
    });
    let history = ui(ctx, "m2-fleet", json!({}));
    let worker_kept = history
        .as_ref()
        .ok()
        .and_then(|f| f["rows"].as_array())
        .is_some_and(|rows| rows.iter().any(|r| r["sessionId"] == json!(observed.session_id)));
    pass &= exited.is_ok() && worker_kept;
    let closed = finish(&observed);
    let _ = other.close();
    let removed = integration(ctx, "uninstall", &scratch.join("session-config"), "session");
    let _ = std::fs::remove_dir_all(&scratch);
    let summary = json!({
        "pass": pass,
        "cycles": cycles,
        "nativeSessionId": observed.native_session_id,
        "sessionId": observed.session_id,
        "ui": ui_state,
        "exit": exited.as_ref().ok().map(|(v, ms)| json!({ "presence": v.execution_presence, "observation": v.observation, "ms": ms })),
        "exitError": exited.as_ref().err(),
        "workerKeptAfterExit": worker_kept,
        "windowClosed": closed,
        "activation": activation.record["detail"]["pluginDir"].as_str().map(redact_home),
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

// ------------------------------------------------------------------ routes

/// `count` exact Returns through the UI's Return across three observed
/// sessions, each preceded by fronting a different window, each verified
/// by independent readback; zero wrong targets.
pub fn routes(ctx: &Ctx, count: u32) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "routes", ctx.channel_name()).map_err(|e| e.to_string())?;
    let scratch = disposable("routes")?;
    let activation = activate(ctx, &scratch)?;
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
        let result = {
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
        run_dir.append("routes.jsonl", &result).map_err(|e| e.to_string())?;
    }
    latencies.sort_unstable();
    let p95 = latencies.get((latencies.len() * 95).div_ceil(100).saturating_sub(1)).copied();
    for observed in &sessions {
        observed.tab.type_line("/exit");
    }
    threadspace_harness::pause_ms(3000);
    let closed: Vec<Value> = sessions.iter().map(finish).collect();
    let _ = integration(ctx, "uninstall", &scratch.join("session-config"), "session");
    let _ = std::fs::remove_dir_all(&scratch);
    let summary = json!({
        "pass": exact == count && wrong == 0,
        "requested": count, "exactVerified": exact, "wrongTargets": wrong,
        "routeLatencyP95Ms": p95, "routeLatencyMaxMs": latencies.last(),
        "windowsClosed": closed,
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}
