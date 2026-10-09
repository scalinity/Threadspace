//! M2 fault and boundary cases with real, independently launched Claude
//! sessions in disposable windows (MILESTONES M2 "Tests"). Each case writes
//! one verdict with the canonical evidence it read back; a case that cannot
//! be produced natively says so instead of passing.
//!
//! Test-only provider hooks (a Stop hook that blocks once, a submission
//! hook that blocks) live in the case's disposable directory and reach only
//! that session, through its own `--settings` file beside the owned hooks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_harness::evidence::Run;
use threadspace_harness::service;
use threadspace_relay::paths::redact_home;

use crate::ctx::Ctx;
use crate::m2::{
    Activation, Observed, Scratch, activate, claude_command, completed_turns, disposable, finish, integration, journal_query,
    launcher, session_view, start_observed, submit, submit_unconfirmed, wait_view,
};

fn rows(ctx: &Ctx, sql: &str) -> Vec<Value> {
    journal_query(ctx, sql)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn quote(id: &str) -> String {
    id.replace('\'', "")
}

/// Turn states of a session, oldest first.
fn turn_states(ctx: &Ctx, session: &str) -> Vec<String> {
    rows(ctx, &format!("SELECT state FROM turns WHERE session_id = '{}' ORDER BY created_cursor", quote(session)))
        .iter()
        .filter_map(|r| r["state"].as_str().map(str::to_owned))
        .collect()
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        if condition() {
            return true;
        }
        threadspace_harness::pause_ms(500);
    }
    false
}

/// Writes a settings file holding the owned session hooks plus `extra`
/// hooks for this case only.
fn case_settings(activation: &Activation, dir: &Path, extra: Value) -> Result<PathBuf, String> {
    let mut settings: Value = std::fs::read_to_string(&activation.settings)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or("owned session settings")?;
    if let (Some(hooks), Some(extra)) = (settings["hooks"].as_object_mut(), extra.as_object()) {
        for (event, groups) in extra {
            let list = hooks.entry(event.clone()).or_insert_with(|| json!([]));
            if let (Some(list), Some(groups)) = (list.as_array_mut(), groups.as_array()) {
                list.extend(groups.iter().cloned());
            }
        }
    }
    let path = dir.join("case-settings.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path)
}

fn hook_script(dir: &Path, name: &str, body: &str) -> Result<String, String> {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n# Qualification-only test hook for this disposable session.\n{body}\nexit 0\n"))
        .map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

fn hooks(event: &str, command: &str) -> Value {
    json!({ event: [{ "hooks": [{ "type": "command", "command": command }] }] })
}

/// Starts an observed session with the case's own settings file.
fn start_case(ctx: &Ctx, activation: &Activation, label: &str, settings: Option<PathBuf>, binary: Option<PathBuf>, env: &[(&str, &str)], extra: &[&str]) -> Result<Observed, String> {
    let dir = disposable(label)?;
    let mut activation = Activation {
        plugin_dir: activation.plugin_dir.clone(),
        settings: activation.settings.clone(),
        record: Value::Null,
    };
    if let Some(settings) = settings {
        activation.settings = settings;
    }
    let binary = match binary {
        Some(binary) => binary,
        None => launcher()?,
    };
    let command = claude_command(&dir, &activation, &binary, extra, env);
    start_observed(ctx, dir, &command)
}

fn verdict(name: &str, pass: bool, detail: Value) -> Value {
    json!({ "case": name, "pass": pass, "detail": detail })
}

// ------------------------------------------------------------------ cases

fn stop_continuation(ctx: &Ctx, a: &Activation, scratch: &Path) -> Result<Value, String> {
    let dir = disposable("stop-hook")?;
    let marker = dir.join("blocked");
    let script = hook_script(
        &dir,
        "block-once.sh",
        &format!(
            "if [ ! -f '{m}' ]; then : > '{m}'; printf '{{\"decision\":\"block\",\"reason\":\"Before stopping, reply with the single word continued.\"}}'; fi",
            m = marker.display()
        ),
    )?;
    let settings = case_settings(a, &dir, hooks("Stop", &script))?;
    let observed = start_case(ctx, a, "stop", Some(settings), None, &[], &[])?;
    let _ = submit(ctx, &observed, "Reply with exactly the word: ok");
    let done = wait_for(Duration::from_secs(150), || completed_turns(ctx, &observed.session_id) >= 1);
    threadspace_harness::pause_ms(3000);
    let states = turn_states(ctx, &observed.session_id);
    let items = rows(ctx, &format!("SELECT category FROM attention_items WHERE session_id = '{}'", quote(&observed.session_id)));
    let blocked = marker.exists();
    finish(&observed);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = scratch;
    // One native turn and one completion: the continuation stays inside it.
    let pass = done && blocked && states == ["COMPLETED"] && items.len() == 1;
    Ok(verdict("stop-continuation", pass, json!({ "stopBlockedOnce": blocked, "turnStates": states, "attentionItems": items })))
}

fn interrupted(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let observed = start_case(ctx, a, "interrupt", None, None, &[], &["--allowedTools", "Bash(sleep:*)"])?;
    // A tool call that keeps the turn running until it is interrupted.
    let _ = submit(ctx, &observed, "Use the Bash tool to run exactly: sleep 60. Then reply with only the word slept.");
    threadspace_harness::pause_ms(4000);
    // Esc interrupts the running turn (the trailing Return is an empty
    // submission the composer ignores).
    observed.tab.type_line("\u{1b}");
    let settled = wait_for(Duration::from_secs(90), || {
        turn_states(ctx, &observed.session_id).iter().any(|s| s == "INTERRUPTED")
    });
    threadspace_harness::pause_ms(2000);
    let abandoned = rows(
        ctx,
        "SELECT COUNT(*) AS n FROM observations WHERE source_id = 'claude.observer' AND native_event = 'turn.step' AND json_extract(payload_json, '$.phase') = 'abandoned'",
    );
    let states = turn_states(ctx, &observed.session_id);
    finish(&observed);
    Ok(verdict("interrupted", settled, json!({ "turnStates": states, "abandonedStepRecords": abandoned })))
}

fn failed(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let observed = start_case(ctx, a, "failed", None, None, &[("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"), ("CLAUDE_CODE_MAX_RETRIES", "0")], &[])?;
    let _ = submit(ctx, &observed, "Reply with exactly the word: ok");
    let settled = wait_for(Duration::from_secs(90), || turn_states(ctx, &observed.session_id).iter().any(|s| s == "FAILED"));
    let items = rows(ctx, &format!("SELECT category FROM attention_items WHERE session_id = '{}'", quote(&observed.session_id)));
    let states = turn_states(ctx, &observed.session_id);
    finish(&observed);
    let error_item = items.iter().any(|i| i["category"] == "ERROR");
    Ok(verdict("failed", settled && error_item, json!({ "turnStates": states, "attentionItems": items })))
}

fn blocked_submission(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let dir = disposable("block-hook")?;
    let script = hook_script(&dir, "block.sh", "printf '{\"decision\":\"block\",\"reason\":\"blocked by the qualification hook\"}'")?;
    let settings = case_settings(a, &dir, hooks("UserPromptSubmit", &script))?;
    let observed = start_case(ctx, a, "blocked", Some(settings), None, &[], &[])?;
    submit_unconfirmed(&observed, "Reply with exactly the word: ok");
    let rejected = wait_for(Duration::from_secs(60), || {
        rows(ctx, &format!(
            "SELECT COUNT(*) AS n FROM facts WHERE session_id = '{}' AND kind = 'INPUT_REJECTED'",
            quote(&observed.session_id)
        ))
        .first()
        .and_then(|r| r["n"].as_u64())
        .is_some_and(|n| n >= 1)
    });
    threadspace_harness::pause_ms(2000);
    let states = turn_states(ctx, &observed.session_id);
    finish(&observed);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(verdict("blocked-submission", rejected && states.is_empty(), json!({ "inputRejected": rejected, "turnStates": states })))
}

fn relay_unavailable(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let observed = start_case(ctx, a, "relay", None, None, &[], &[])?;
    let _ = submit(ctx, &observed, "Reply with exactly the word: first");
    let first = wait_for(Duration::from_secs(150), || completed_turns(ctx, &observed.session_id) >= 1);
    let stopped = service::bootstrap(&ctx.id, "stop");
    submit_unconfirmed(&observed, "Reply with exactly the word: second");
    // With no companion, the turn's records go to the local spool.
    threadspace_harness::pause_ms(20_000);
    let spooled = std::fs::read_dir(ctx.id.agent.store_dir.join("capture-spool/ready")).map(|d| d.count()).unwrap_or(0);
    let enabled = service::bootstrap(&ctx.id, "enable");
    let drained = wait_for(Duration::from_secs(90), || completed_turns(ctx, &observed.session_id) >= 2);
    let duplicates = rows(ctx, "SELECT COUNT(*) - COUNT(DISTINCT observation_id) AS n FROM observations");
    finish(&observed);
    let pass = first && stopped["ok"] == json!(true) && spooled > 0 && enabled["ok"] == json!(true) && drained;
    Ok(verdict("relay-unavailable", pass, json!({ "spooledWhileStopped": spooled, "drainedAfterRestart": drained, "duplicateObservations": duplicates, "stop": stopped["ok"], "enable": enabled["ok"] })))
}

fn incompatible(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let home = threadspace_relay::paths::home_dir().ok_or("home")?;
    let older = home.join(".local/share/claude/versions/2.1.292");
    if !older.exists() {
        return Ok(verdict("incompatible-profile", false, json!({ "blocked": "no unqualified Claude build installed" })));
    }
    let observed = start_case(ctx, a, "incompatible", None, Some(older), &[], &[])?;
    let limited = wait_view(ctx, &observed.native_session_id, Duration::from_secs(60), |v| {
        v.observer_version.as_deref() == Some("2.1.292")
    });
    submit_unconfirmed(&observed, "Reply with exactly the word: ok");
    threadspace_harness::pause_ms(30_000);
    let view = session_view(ctx, &observed.native_session_id);
    let states = turn_states(ctx, &observed.session_id);
    finish(&observed);
    let tier = view.as_ref().ok().and_then(|v| v.observer_tier);
    let pass = limited.is_ok() && format!("{tier:?}") == "Some(LowerTier)" && !states.iter().any(|s| s == "COMPLETED");
    Ok(verdict("incompatible-profile", pass, json!({ "observerTier": tier, "observerVersion": view.as_ref().ok().and_then(|v| v.observer_version.clone()), "turnStates": states })))
}

fn mod_reload(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let observed = start_case(ctx, a, "reload", None, None, &[], &[])?;
    let _ = submit(ctx, &observed, "Reply with exactly the word: before");
    let before = wait_for(Duration::from_secs(150), || completed_turns(ctx, &observed.session_id) >= 1);
    let native = session_view(ctx, &observed.native_session_id).ok().and_then(|v| v.observer_tier);
    // Saving a watched plugin folder reloads the module: a new load that
    // knows its session only from a host read.
    let register = PathBuf::from(&a.plugin_dir).join("hooks/register.ts");
    let original = std::fs::read(&register).map_err(|e| e.to_string())?;
    let mut edited = original.clone();
    edited.extend_from_slice(b"\n// qualification reload\n");
    std::fs::write(&register, &edited).map_err(|e| e.to_string())?;
    let restored = wait_view(ctx, &observed.native_session_id, Duration::from_secs(90), |v| {
        format!("{:?}", v.observer_tier) == "Some(Restored)"
    });
    submit_unconfirmed(&observed, "Reply with exactly the word: after");
    let after = wait_for(Duration::from_secs(150), || completed_turns(ctx, &observed.session_id) >= 2);
    std::fs::write(&register, &original).map_err(|e| e.to_string())?;
    let epochs = rows(ctx, &format!(
        "SELECT COUNT(DISTINCT o.source_epoch) AS n FROM observations o JOIN facts f ON f.observation_id = o.observation_id
          WHERE o.source_id = 'claude.observer' AND f.session_id = '{}'",
        quote(&observed.session_id)
    ));
    let pending = rows(ctx, &format!("SELECT COUNT(*) AS n FROM facts WHERE session_id = '{}' AND fact_json LIKE '%HOST_READ%'", quote(&observed.session_id)));
    finish(&observed);
    let pass = before && format!("{native:?}") == "Some(Native)" && restored.is_ok() && after;
    Ok(verdict("mod-reload", pass, json!({ "tierBefore": native, "restored": restored.as_ref().ok().map(|(v, ms)| json!({ "tier": v.observer_tier, "ms": ms })), "restoreError": restored.as_ref().err(), "completedAfterReload": after, "hostReadFacts": pending, "epochs": epochs })))
}

fn child_waiting(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let observed = start_case(ctx, a, "child-wait", None, None, &[], &[])?;
    let _ = submit(
        ctx,
        &observed,
        "Use the Agent tool with run_in_background set to true to start one subagent whose prompt is: Use the Bash tool to run ls /. Do not wait for it; reply with only the word started.",
    );
    let parent = wait_for(Duration::from_secs(150), || completed_turns(ctx, &observed.session_id) >= 1);
    let waiting = wait_view(ctx, &observed.native_session_id, Duration::from_secs(120), |v| {
        v.provider_status.as_deref() == Some("waiting")
    });
    threadspace_harness::pause_ms(3000);
    let items = rows(ctx, &format!("SELECT category, scope_kind, turn_id FROM attention_items WHERE session_id = '{}'", quote(&observed.session_id)));
    let states = turn_states(ctx, &observed.session_id);
    observed.tab.type_line("\u{1b}");
    finish(&observed);
    let session_wait = items.iter().any(|i| i["turn_id"].is_null() && i["category"] != "TURN_COMPLETE");
    let parent_kept = states.first().map(String::as_str) == Some("COMPLETED");
    Ok(verdict("parent-completed-child-waiting", parent && waiting.is_ok() && session_wait && parent_kept, json!({
        "parentCompleted": parent, "inventoryWaiting": waiting.as_ref().ok().map(|(v, _)| json!({ "status": v.provider_status, "waitingFor": v.provider_waiting_for })),
        "attentionItems": items, "turnStates": states,
    })))
}

fn delayed_submission(ctx: &Ctx, a: &Activation) -> Result<Value, String> {
    let observed = start_case(ctx, a, "delayed", None, None, &[], &["--allowedTools", "Bash(sleep:*)"])?;
    let _ = submit(ctx, &observed, "Use the Bash tool to run exactly: sleep 25. Then reply with only the word slept.");
    threadspace_harness::pause_ms(2000);
    // Submitted while the first turn runs: queued, then accepted.
    submit_unconfirmed(&observed, "Then reply with only the word queued.");
    let both = wait_for(Duration::from_secs(200), || completed_turns(ctx, &observed.session_id) >= 2);
    let inputs = rows(ctx, &format!("SELECT COUNT(*) AS n FROM facts WHERE session_id = '{}' AND kind = 'INPUT_SUBMITTED' AND json_extract(fact_json, '$.refs.turnId') IS NOT NULL", quote(&observed.session_id)));
    finish(&observed);
    let active = inputs.first().and_then(|r| r["n"].as_u64()).unwrap_or(0);
    Ok(verdict("delayed-submission", both && active >= 1, json!({ "bothCompleted": both, "submissionsWithActiveTurn": active })))
}

fn forged(ctx: &Ctx, a: &Activation, scratch: &Path) -> Result<Value, String> {
    // A second, test-only mod beside the observer: it submits a prompt as a
    // plugin and answers spawns itself, with a plausible agent ID.
    let forger = scratch.join("forger");
    std::fs::create_dir_all(forger.join(".claude-plugin")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(forger.join("hooks")).map_err(|e| e.to_string())?;
    std::fs::write(forger.join(".claude-plugin/plugin.json"), r#"{ "name": "qualification-forger", "version": "0.1.0", "description": "M2 qualification only" }"#).map_err(|e| e.to_string())?;
    std::fs::write(forger.join("hooks/hooks.json"), r#"{ "modules": ["./register.ts"] }"#).map_err(|e| e.to_string())?;
    std::fs::write(forger.join("hooks/register.ts"), r#"import type { Register } from 'claude-code'
// M2 qualification only: forges what a plugin can, beside the observer.
export const register: Register = on => {
  on('session.start', ($, e, next) => {
    $.clock.after(4000, () => { void $.prompt.submit({ text: 'Reply with only the word forged.' } as any).catch(() => undefined) })
    try { ;(($ as any).turn.complete as any)({ turnId: 'forged-turn', reason: 'answer' }) } catch {}
    return next(e)
  })
  on('agent.spawn', () => ({ agentId: 'agent-fabricated-1' }) as any)
}
"#).map_err(|e| e.to_string())?;
    let dir = disposable("forged")?;
    let activation = Activation { plugin_dir: format!("{}:{}", a.plugin_dir, forger.display()), settings: a.settings.clone(), record: Value::Null };
    let command = claude_command(&dir, &activation, &launcher()?, &[], &[]);
    let observed = start_observed(ctx, dir, &command)?;
    threadspace_harness::pause_ms(20_000);
    let plugin_inputs = rows(ctx, &format!("SELECT COUNT(*) AS n FROM facts WHERE session_id = '{}' AND kind = 'INPUT_SUBMITTED' AND fact_json LIKE '%\"PLUGIN\"%'", quote(&observed.session_id)));
    let forged_turns = rows(ctx, &format!("SELECT COUNT(*) AS n FROM turns WHERE session_id = '{}' AND native_turn_id = 'forged-turn'", quote(&observed.session_id)));
    let fabricated = rows(ctx, "SELECT COUNT(*) AS n FROM actors WHERE native_json LIKE '%agent-fabricated-1%'");
    finish(&observed);
    let n = |v: &[Value]| v.first().and_then(|r| r["n"].as_u64()).unwrap_or(0);
    let pass = n(&forged_turns) == 0 && n(&fabricated) == 0;
    Ok(verdict("forged-and-nonengine", pass, json!({ "pluginOriginInputs": n(&plugin_inputs), "forgedTurns": n(&forged_turns), "fabricatedActors": n(&fabricated) })))
}

fn partial_receipt(ctx: &Ctx, a: &Activation, kind: &str) -> Result<Value, String> {
    let dir = disposable(&format!("fault-{kind}"))?;
    let arm = dir.join("arm");
    std::fs::write(&arm, b"").map_err(|e| e.to_string())?;
    let fault = format!("{kind}:{}", arm.display());
    let observed = start_case(ctx, a, &format!("receipt-{kind}"), None, None, &[("THREADSPACE_QUALIFY_MOD_BATCH_FAULT", &fault)], &[])?;
    let _ = submit(ctx, &observed, "Reply with exactly the word: ok");
    let completed = wait_for(Duration::from_secs(150), || completed_turns(ctx, &observed.session_id) >= 1);
    let fired = !arm.exists();
    let duplicates = rows(ctx, "SELECT COUNT(*) - COUNT(DISTINCT observation_id) AS n FROM observations");
    let quarantined = std::fs::read_dir(ctx.id.agent.store_dir.join("capture-spool/quarantine/not-accepted")).map(|d| d.count()).unwrap_or(0);
    finish(&observed);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(verdict(&format!("mod-batch-{kind}"), fired && completed, json!({ "faultFired": fired, "turnCompletedAfterRetry": completed, "duplicateObservations": duplicates, "quarantined": quarantined })))
}

pub fn faults(ctx: &Ctx, which: &str) -> Result<Value, String> {
    let run_dir = Run::create(&ctx.evidence_root(), "faults", ctx.channel_name()).map_err(|e| e.to_string())?;
    let scratch = Scratch::new(ctx, "faults")?;
    let activation = activate(ctx, &scratch.dir)?;
    let all = which == "all";
    let mut verdicts = Vec::new();
    let mut case = |name: &str, run: &mut dyn FnMut() -> Result<Value, String>| {
        if all || which.split(',').any(|w| w == name) {
            let outcome = run().unwrap_or_else(|error| verdict(name, false, json!({ "error": error })));
            let _ = run_dir.append("cases.jsonl", &outcome);
            verdicts.push(outcome);
        }
    };
    case("stop", &mut || stop_continuation(ctx, &activation, &scratch.dir));
    case("interrupt", &mut || interrupted(ctx, &activation));
    case("failed", &mut || failed(ctx, &activation));
    case("blocked", &mut || blocked_submission(ctx, &activation));
    case("incompatible", &mut || incompatible(ctx, &activation));
    case("reload", &mut || mod_reload(ctx, &activation));
    case("child", &mut || child_waiting(ctx, &activation));
    case("delayed", &mut || delayed_submission(ctx, &activation));
    case("forged", &mut || forged(ctx, &activation, &scratch.dir));
    case("partial", &mut || partial_receipt(ctx, &activation, "partial"));
    case("malformed", &mut || partial_receipt(ctx, &activation, "malformed"));
    case("exit1", &mut || partial_receipt(ctx, &activation, "exit1"));
    case("relay", &mut || relay_unavailable(ctx, &activation));
    case("terminal", &mut || {
        crate::terminal_gates::negatives(ctx).map(|summary| verdict("terminal-negatives", summary["pass"] == json!(true), summary))
    });
    let removed = integration(ctx, "uninstall", &scratch.dir.join("session-config"), "session");
    let pass = verdicts.iter().all(|v| v["pass"] == json!(true));
    let summary = json!({
        "pass": pass,
        "cases": verdicts.iter().map(|v| json!({ "case": v["case"], "pass": v["pass"] })).collect::<Vec<_>>(),
        "integrationRemoved": removed.ok().map(|r| r["ok"].clone()),
        "activation": redact_home(&activation.plugin_dir),
        "terminalRefusals": crate::m2::refusals(),
    });
    run_dir.write_json("summary.json", &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}
