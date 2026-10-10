//! Focused owned real-Claude smoke and proof-before-Turn reload sequence.
//! Records positive witnesses separately from still-required negative/replay gates.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::evidence::{Run, sha256_file};
use threadspace_harness::idle::wait_for_idle;
use threadspace_harness::run::{osascript, run};
use threadspace_harness::terminal::Tab;

use crate::ctx::Ctx;
use crate::m2;
use crate::m2_minimized::{OwnedTab, native_witness};
use crate::terminal_gates::StartedClaude;

const OPEN: &str = r#"on run argv
tell application "Terminal"
  set t to do script (item 1 of argv)
  delay 0.4
  set custom title of t to (item 2 of argv)
  set p to tty of t
  repeat with w in windows
    if (count of tabs of w) is 1 and tty of tab 1 of w is p then
      return ((id of w) as text) & (character id 9) & p
    end if
  end repeat
end tell
return ""
end run"#;

fn snapshot(ctx: &Ctx, session: &str) -> Result<Value, String> {
    let sql = format!("SELECT o.ingest_seq, o.observation_id, o.source_id, o.source_epoch, o.native_event,
        o.payload_version, o.payload_json, f.kind, f.fact_json FROM facts f JOIN observations o
        ON o.observation_id=f.observation_id WHERE f.session_id='{session}' ORDER BY o.ingest_seq,f.fact_index");
    let raw = m2::journal_query(ctx, &sql)?;
    if raw.trim().is_empty() {
        return Ok(json!([]));
    }
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write(run: &Run, name: &str, value: &Value) -> Result<(), String> {
    run.write_json(name, value)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn integration_removed(value: &Value) -> bool {
    value["ok"] == true && value["detail"]["complete"] == true
}

pub fn qualify(ctx: &Ctx) -> Result<Value, String> {
    execute(ctx, false)
}

pub fn latency(ctx: &Ctx) -> Result<Value, String> {
    execute(ctx, true)
}

fn execute(ctx: &Ctx, latency: bool) -> Result<Value, String> {
    if ctx.channel_name() != "dev" {
        return Err("observer native qualification is Dev-only".into());
    }
    let evidence = Run::create(
        &ctx.repo.join("evidence/M2/remediation-3"),
        if latency {
            "latency-fixture"
        } else {
            "observer-native"
        },
        "dev",
    )
    .map_err(|error| error.to_string())?;
    let git = run(
        "/usr/bin/git",
        &["rev-parse", "HEAD"],
        Duration::from_secs(3),
    );
    write(
        &evidence,
        "source-identity.json",
        &json!({"sourceCommit":git.stdout.trim(),
        "harnessSha256":std::env::current_exe().ok().as_deref().and_then(sha256_file),
        "installed":ctx.environment(),"observerSourceSha256":sha256_file(&ctx.repo.join("packages/provider-mod/hooks/register.ts"))}),
    )?;
    let preflight = crate::terminal_diagnostic::probe()?;
    write(&evidence, "preflight.json", &preflight)?;
    if preflight["verdict"] != "TERMINAL SCRIPTABILITY RESTORED" {
        return Err("bounded Terminal prerequisite failed; no fixture launched".into());
    }
    let mut scratch = m2::Scratch::new(ctx, "observer-native")?;
    let activation = m2::activate(ctx, &mut scratch)?;
    write(
        &evidence,
        "integration-acquisition.json",
        &activation.record,
    )?;
    let mut owned = None;
    let mut uncertain_open = false;
    let register = std::path::Path::new(&activation.plugin_dir).join("hooks/register.ts");
    let original = std::fs::read(&register).map_err(|error| error.to_string())?;
    let mut reload_written = false;
    let mut smoke_pass = false;
    let mut reload_positive = false;
    let manifest = std::path::Path::new(&activation.plugin_dir).join(".claude-plugin/plugin.json");
    let manifest_original = std::fs::read(&manifest).map_err(|error| error.to_string())?;
    let settings_original =
        std::fs::read(&activation.settings).map_err(|error| error.to_string())?;
    let mut latency_start = None;
    let mut owned_ui = None;
    let operation = (|| -> Result<(), String> {
        if latency {
            // Never replace or terminate a UI acquired by another session.
            if !ctx.app().processes().is_empty() {
                return Err(
                    "F4 needs an exclusively acquired Dev UI; existing UI left untouched".into(),
                );
            }
            let mut cursor = ctx.companion().log();
            let ui = ctx.app().launch_packaged(&[])?;
            owned_ui = Some(ui.ui);
            let hydrated = ctx
                .app()
                .wait_hydrated(&mut cursor, Duration::from_secs(30));
            write(
                &evidence,
                "owned-ui.json",
                &json!({"incarnation":owned_ui,"hydrated":hydrated}),
            )?;
            if hydrated.is_none() {
                return Err("owned Dev UI did not hydrate".into());
            }
            let mut document: Value =
                serde_json::from_slice(&manifest_original).map_err(|e| e.to_string())?;
            document["userConfig"]["captureArgv"]["default"]
                .as_array_mut()
                .ok_or("missing owned capture argv")?
                .push(json!("--qualification-latency"));
            std::fs::write(
                &manifest,
                serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            // The hook denominator is an explicitly issued deterministic
            // workload. Native Claude supplies the real observer population;
            // ordinary provider hooks were separately exercised by the smoke.
            let mut settings: Value =
                serde_json::from_slice(&settings_original).map_err(|e| e.to_string())?;
            settings["hooks"] = json!({});
            std::fs::write(
                &activation.settings,
                serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let start = crate::m2_latency::begin(ctx)?;
            write(&evidence, "latency-start.json", &start)?;
            latency_start = start["runDirectory"].as_str().map(std::path::PathBuf::from);
        }
        let idle = wait_for_idle(&ctx.native, 10.0, Duration::from_secs(180));
        write(&evidence, "idle-gate.json", &json!(idle))?;
        if !idle.satisfied {
            return Err("owner-idle prerequisite failed; no window opened".into());
        }
        let dir = scratch.dir.join("target");
        std::fs::create_dir(&dir).map_err(|error| error.to_string())?;
        let marker = uuid::Uuid::new_v4().to_string();
        let binary = m2::launcher()?;
        let version = run(
            &binary.display().to_string(),
            &["--version"],
            Duration::from_secs(3),
        );
        if !version.ok || !version.stdout.contains("2.1.295") {
            return Err("exact Claude 2.1.295 executable required".into());
        }
        let command = m2::claude_command(
            &dir,
            &activation,
            &binary,
            &[
                "--allowedTools",
                "Bash(echo:*)",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
            ],
            &[],
        );
        {
            let _gui = ctx.gui("m2 owned native observer launch")?;
            uncertain_open = true;
            let title = format!("TSQ-{marker}");
            let opened = osascript(OPEN, &[&command, &title], Duration::from_secs(5));
            write(
                &evidence,
                "open.json",
                &json!({"stdout":opened.stdout,"stderr":opened.stderr,"ok":opened.ok,
                "status":opened.status,"timedOut":opened.timed_out,"elapsedMs":opened.elapsed_ms}),
            )?;
            let raw = opened.stdout.trim();
            let (window, tty) = raw
                .split_once('\t')
                .ok_or("bounded open produced no acquired handle")?;
            owned = Some(OwnedTab::acquire(Tab {
                marker,
                dir: dir.clone(),
                window_id: window.parse().map_err(|_| "invalid window")?,
                tty: tty.into(),
            })?);
            uncertain_open = false;
            threadspace_harness::pause_ms(4000);
            let resource = owned.as_ref().ok_or("no owned resource")?;
            let ownership = resource.ownership();
            write(&evidence, "launch-ownership.json", &ownership)?;
            if ownership["owned"] != true {
                return Err("owned launch proof unavailable; no input issued".into());
            }
            resource.tab.type_line("\u{1b}[B");
        }
        let resource = owned.as_ref().ok_or("no owned resource")?;
        let endpoint: Value = serde_json::from_str(&m2::journal_query(
            ctx,
            "SELECT value AS id FROM store_meta WHERE key='endpoint_id'",
        )?)
        .map_err(|error| error.to_string())?;
        let endpoint = endpoint[0]["id"].as_str().ok_or("no local endpoint")?;
        let started = Instant::now();
        let native = loop {
            let native = native_witness(&resource.tab, endpoint, None);
            evidence
                .append("discovery.jsonl", &native)
                .map_err(|error| error.to_string())?;
            if native["qualified"] == true {
                break native;
            }
            if started.elapsed() >= Duration::from_secs(60) {
                return Err("no qualified native owned Session".into());
            }
            threadspace_harness::pause_ms(500);
        };
        let native_id = native["nativeSessionId"]
            .as_str()
            .ok_or("native Session missing")?;
        let started = StartedClaude {
            native_session_id: native_id.into(),
            pid: native["processKey"]["pid"].as_i64().unwrap_or_default() as i32,
            tab: resource.tab.clone(),
        };
        let session = started
            .bound_session(ctx, Duration::from_secs(20))
            .ok_or("no canonical native binding")?;
        let observed = m2::Observed {
            tab: resource.tab.clone(),
            dir,
            native_session_id: native_id.into(),
            session_id: session.clone(),
        };
        write(
            &evidence,
            "fixture.json",
            &json!({"native":native,"canonicalSession":session,"tab":resource.tab,
            "providerVersion":version.stdout.trim(),"mcpScope":"explicit empty strict fixture; no owner MCP services launched"}),
        )?;
        threadspace_harness::pause_ms(3000);
        let baseline = m2::completed_turns(ctx, &session);
        let smoke = m2::prompt_and_complete(
            ctx,
            &observed,
            "Use Bash to run echo THREADSPACE_M2_NATIVE_SMOKE, then reply done.",
            baseline + 1,
        );
        let after = native_witness(&resource.tab, endpoint, Some(native_id));
        let rows = snapshot(ctx, &session)?;
        write(&evidence, "smoke-journal.json", &rows)?;
        smoke_pass = smoke.is_ok()
            && after["qualified"] == true
            && after["processKey"] == native["processKey"];
        write(
            &evidence,
            "smoke-result.json",
            &json!({"pass":smoke_pass,"durationMs":smoke.as_ref().ok(),
            "error":smoke.as_ref().err(),"completedBefore":baseline,"completedAfter":m2::completed_turns(ctx,&session),
            "beforeNative":native,"afterNative":after,"sessionView":m2::session_view(ctx,native_id).ok()}),
        )?;
        smoke?;
        if latency {
            let output = evidence.dir.join("hook-issuance");
            let helper = ctx
                .id
                .companion_executable
                .parent()
                .ok_or("no helper parent")?
                .join("threadspace-hook");
            let issued = run(
                "/usr/bin/python3",
                &[
                    &ctx.repo
                        .join("tests/native/tools/m2_hook_census.py")
                        .display()
                        .to_string(),
                    "--helper",
                    &helper.display().to_string(),
                    "--out",
                    &output.display().to_string(),
                    "--journal",
                    &ctx.id.agent.journal.display().to_string(),
                    "--count",
                    "20",
                ],
                Duration::from_secs(40),
            );
            write(
                &evidence,
                "hook-issuance-result.json",
                &json!({"ok":issued.ok,"stdout":issued.stdout,"stderr":issued.stderr,"status":issued.status,"elapsedMs":issued.elapsed_ms}),
            )?;
            if !issued.ok {
                return Err(
                    "controlled independent hook issuance failed; raw evidence retained".into(),
                );
            }
            {
                let _gui = ctx.gui("m2 owned measured provider exit")?;
                resource.tab.type_line("/exit");
            }
            let ended = m2::wait_view(ctx, native_id, Duration::from_secs(20), |view| {
                format!("{:?}", view.execution_presence) == "Ended"
            });
            write(
                &evidence,
                "provider-close.json",
                &json!({"observed":ended.as_ref().ok(),"error":ended.as_ref().err(),"journal":snapshot(ctx,&session)?}),
            )?;
            threadspace_harness::pause_ms(1000);
            let dir = latency_start.as_ref().ok_or("no latency start")?;
            write(
                &evidence,
                "latency-end.json",
                &crate::m2_latency::end(ctx, dir)?,
            )?;
            return Ok(());
        }
        let cursor: Value = serde_json::from_str(&m2::journal_query(
            ctx,
            "SELECT COALESCE(MAX(ingest_seq),0) AS n FROM observations",
        )?)
        .map_err(|error| error.to_string())?;
        let cursor = cursor[0]["n"].as_i64().ok_or("no pre-reload cursor")?;
        let mut edited = original.clone();
        edited.extend_from_slice(
            format!(
                "\n// M2 owned reload qualification {}\n",
                uuid::Uuid::new_v4()
            )
            .as_bytes(),
        );
        std::fs::write(&register, &edited).map_err(|error| error.to_string())?;
        reload_written = true;
        let lower = m2::wait_view(ctx, native_id, Duration::from_secs(20), |view| {
            format!("{:?}", view.observer_tier) == "Some(LowerTier)"
        });
        write(
            &evidence,
            "reload-lower-tier.json",
            &json!({"pass":lower.is_ok(),"observed":lower.as_ref().ok(),"error":lower.as_ref().err(),"preReloadCursor":cursor}),
        )?;
        lower?;
        let started = Instant::now();
        let sealed = loop {
            let rows = snapshot(ctx, &session)?;
            write(&evidence, "reload-proof-and-seal.json", &rows)?;
            let fresh = |event: &str| {
                rows.as_array().is_some_and(|rows| {
                    rows.iter().any(|row| {
                        row["ingest_seq"].as_i64().is_some_and(|seq| seq > cursor)
                            && row["native_event"] == event
                    })
                })
            };
            if fresh("ownership.proven") && fresh("ownership.seal") {
                break rows;
            }
            if started.elapsed() >= Duration::from_secs(20) {
                return Err(
                    "fresh independently admitted proof and observer seal unavailable".into(),
                );
            }
            threadspace_harness::pause_ms(250);
        };
        write(
            &evidence,
            "pre-turn-seal.json",
            &json!({"native":native_witness(&resource.tab,endpoint,Some(native_id)),"journal":sealed}),
        )?;
        let completed = m2::completed_turns(ctx, &session);
        let post = m2::prompt_and_complete(
            ctx,
            &observed,
            "Reply exactly THREADSPACE_M2_POST_SEAL.",
            completed + 1,
        );
        write(
            &evidence,
            "post-seal-journal.json",
            &snapshot(ctx, &session)?,
        )?;
        let restored = m2::wait_view(ctx, native_id, Duration::from_secs(10), |view| {
            format!("{:?}", view.observer_tier) == "Some(Restored)"
        });
        reload_positive = post.is_ok() && restored.is_ok();
        write(
            &evidence,
            "reload-result.json",
            &json!({"positivePass":reload_positive,"postSealTurnMs":post.as_ref().ok(),
            "turnError":post.as_ref().err(),"restored":restored.as_ref().ok(),"restoreError":restored.as_ref().err(),
            "negativeAndReplayQualification":"NOT EXECUTED by this focused positive runner"}),
        )?;
        post?;
        restored?;
        Ok(())
    })();
    let mut restoration_errors = Vec::new();
    if reload_written && let Err(error) = std::fs::write(&register, &original) {
        restoration_errors.push(format!("owned register restoration: {error}"));
    }
    if latency {
        if let Err(error) = std::fs::write(&manifest, &manifest_original) {
            restoration_errors.push(format!("owned manifest restoration: {error}"));
        }
        if let Err(error) = std::fs::write(&activation.settings, &settings_original) {
            restoration_errors.push(format!("owned settings restoration: {error}"));
        }
    }
    let cleanup = owned.as_ref().map(|resource| resource.cleanup(ctx));
    let closed = !uncertain_open && cleanup.as_ref().is_none_or(|value| value["closed"] == true);
    let removal = if closed {
        Some(scratch.remove_integration())
    } else {
        None
    };
    let removed = removal
        .as_ref()
        .is_some_and(|result| result.as_ref().is_ok_and(integration_removed));
    let ui_exit = owned_ui.as_ref().and_then(|ui| ctx.app().stop(ui, false));
    let summary = json!({"sourceCommit":git.stdout.trim(),"runDirectory":evidence.dir,"operationError":operation.err(),
        "nativeSmokePositive":smoke_pass,"reloadPositive":reload_positive,"cleanup":cleanup,
        "integrationRemoval":removal.map(|result|match result {Ok(value)=>value,Err(error)=>json!({"error":error})}),
        "uncertainOpen":uncertain_open,"ownedResourcesClosed":closed&&removed,
        "restorationErrors":restoration_errors,"ownedUiExitMs":ui_exit,"latencyRun":latency_start,
        "F1A":"INCOMPLETE until original ownership/control rows are independently assessed",
        "F1B":"INCOMPLETE until native causal/replay and negative rows are assessed"});
    if !closed {
        std::mem::forget(scratch);
    }
    write(&evidence, "summary.json", &summary)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_uninstall_never_reports_all_acquired_resources_closed() {
        assert!(!integration_removed(
            &json!({"ok":true,"detail":{"complete":false,"retainedRecord":"owned"}})
        ));
        assert!(!integration_removed(
            &json!({"ok":false,"detail":{"complete":true}})
        ));
        assert!(!integration_removed(&json!({"ok":true,"detail":{}})));
        assert!(integration_removed(
            &json!({"ok":true,"detail":{"complete":true}})
        ));
        assert!(!integration_removed(
            &json!({"ok":true,"detail":{"installed":false}})
        ));
    }
}
