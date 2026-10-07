//! The Claude discovery scheduler (SPEC §4.4, §4.14, §19.3). One pass:
//!
//! 1. bracketed native inventory join (provider-claude);
//! 2. fresh kernel samples of every stored live activation;
//! 3. a reconciliation plan that ends activations only on proven evidence;
//! 4. a Terminal surface join for activations without a binding: exactly one
//!    live tab whose TTY `st_rdev` equals the provider's `e_tdev`, inside an
//!    unchanged Terminal incarnation, with the process re-sampled afterwards;
//! 5. one registration transaction through the single writer.
//!
//! The first pass runs at startup, so sessions launched before Threadspace are
//! discovered without any new provider hook. Passes are serialized on this
//! thread; inventory requests never overlap. Terminal is enumerated only when
//! an activation needs a surface, only while it runs, and only when Apple-event
//! automation is already authorized (never prompting).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_contracts::route::{AppGeneration, DiscoverySummary, ProvisionalCandidate};
use threadspace_journal::{
    ActivationChange, DiscoveryApplication, LiveExecutionRow, ObservedSessionRecord, ProcessRecord,
    SurfaceRecord,
};
use threadspace_provider_claude::discovery::{DiscoveryPass, Join, discover};
use threadspace_provider_claude::inventory::{ClaudeCli, ClaudeInstall};
use threadspace_provider_claude::reconcile::{self, LiveExecution};
use threadspace_surfaces_macos::ancestry::KernelSampler;
use threadspace_surfaces_macos::process::{self, Incarnation};
use threadspace_surfaces_macos::terminal::{self, TERMINAL_BUNDLE_ID};
use threadspace_surfaces_macos::tty;

use crate::bridge;
use crate::log;
use crate::writer::WriterCommand;

pub const PROVIDER: &str = "claude";
/// SPEC §19.3 initial live inventory interval.
const INTERVAL: Duration = Duration::from_secs(5);
const INVENTORY_TIMEOUT: Duration = Duration::from_secs(5);
const QUICK: Duration = Duration::from_secs(3);
/// A Terminal enumeration outside any route.
pub const ENUMERATION_TIMEOUT: Duration = Duration::from_secs(2);
const NO_ERR: i32 = 0;

pub enum Trigger {
    /// Run a pass now. `force_surface` retries surface joins already
    /// attempted for the same incarnation and Terminal generation.
    Refresh {
        force_surface: bool,
        reply: Option<Sender<DiscoverySummary>>,
    },
}

#[derive(Clone)]
pub struct DiscoveryContext {
    pub writer: SyncSender<WriterCommand>,
    pub boot_id: String,
    pub home: PathBuf,
    pub resources_dir: PathBuf,
}

impl DiscoveryContext {
    pub fn launcher(&self) -> PathBuf {
        self.home.join(".local/bin/claude")
    }

    /// Provider namespace: the default CLI profile on this endpoint, keyed by
    /// its canonical config directory (SPEC §4.1).
    pub fn profile_ref(&self) -> String {
        format!("claude-cli:{}", self.home.join(".claude").display())
    }

    pub fn cli(&self, install: &ClaudeInstall, timeout: Duration) -> ClaudeCli {
        ClaudeCli {
            binary: install.binary.clone(),
            home: self.home.clone(),
            timeout,
            now_ms: log::now_ms,
        }
    }
}

/// Terminal.app's current incarnation from kernel evidence: the one live
/// process running Terminal's executable, sampled for its birth. `None` when
/// it is not running; several such processes are an error, never a choice.
/// (NSRunningApplication's list for Terminal's bundle identifier also names
/// transient osascript processes while they run, so it is not used.)
pub fn terminal_generation() -> Result<Option<AppGeneration>, String> {
    let pids = process::pids_with_executable(terminal::TERMINAL_EXECUTABLE)
        .map_err(|error| error.to_string())?;
    let pid = match pids.as_slice() {
        [] => return Ok(None),
        [pid] => *pid,
        _ => return Err("MULTIPLE_TERMINAL_PROCESSES".to_owned()),
    };
    let sample = process::sample(pid).map_err(|error| error.to_string())?;
    Ok(Some(AppGeneration {
        pid: pid as u32,
        start_seconds: sample.start_seconds.to_string(),
        start_microseconds: sample.start_microseconds,
    }))
}

/// Apple-event authorization for Terminal, asked without prompting.
pub fn automation_authorized(timeout: Duration) -> Result<bool, String> {
    bridge::automation_permission(TERMINAL_BUNDLE_ID, false, timeout)
        .map(|status| status == NO_ERR)
        .map_err(|error| error.to_string())
}

struct State {
    context: DiscoveryContext,
    /// (pid, birth s, birth µs, Terminal generation) whose surface join gave
    /// a final answer (no tab / several tabs); retried only on a forced pass.
    attempted: HashSet<(i32, u64, u32, String)>,
    versions: HashMap<PathBuf, Option<String>>,
}

fn writer_call<T>(
    writer: &SyncSender<WriterCommand>,
    build: impl FnOnce(Sender<T>) -> WriterCommand,
) -> Option<T> {
    let (reply, answer) = mpsc::channel();
    writer.send(build(reply)).ok()?;
    answer.recv_timeout(Duration::from_secs(10)).ok()
}

fn incarnation_json(incarnation: &Incarnation) -> Value {
    json!({
        "pid": incarnation.sample.pid,
        "startSeconds": incarnation.sample.start_seconds.to_string(),
        "startMicroseconds": incarnation.sample.start_microseconds,
        "executable": incarnation.executable.canonical(),
        "controllingDevice": incarnation.sample.controlling_device,
        "pgid": incarnation.sample.pgid,
        "tpgid": incarnation.sample.tpgid,
        "status": incarnation.sample.status,
    })
}

/// Identity-relevant row fields only: no cwd, name or prompt text.
fn rows_json(snapshot: &threadspace_provider_claude::inventory::InventorySnapshot) -> Value {
    json!({
        "requestStartedMs": snapshot.request_started_ms,
        "requestEndedMs": snapshot.request_ended_ms,
        "rows": snapshot.rows.iter().map(|row| json!({
            "pid": row.pid,
            "sessionId": row.session_id,
            "kind": row.kind,
            "status": row.status,
            "waitingFor": row.waiting_for,
        })).collect::<Vec<_>>(),
    })
}

fn process_record(boot_id: &str, join: &Join) -> ProcessRecord {
    ProcessRecord {
        boot_id: boot_id.to_owned(),
        pid: join.after.sample.pid as u32,
        start_seconds: join.after.sample.start_seconds,
        start_microseconds: join.after.sample.start_microseconds,
        executable: join.after.executable.canonical(),
    }
}

/// Joins each listed activation to exactly one Terminal tab, or says why not.
fn surface_join(
    state: &mut State,
    joins: &[&Join],
    force: bool,
    ledger: &mut Vec<Value>,
) -> HashMap<i32, Result<SurfaceRecord, String>> {
    let mut out = HashMap::new();
    if joins.is_empty() {
        return out;
    }
    let generation = match terminal_generation() {
        Ok(Some(generation)) => generation,
        Ok(None) => {
            for join in joins {
                out.insert(join.pid, Err("TERMINAL_NOT_RUNNING".to_owned()));
            }
            return out;
        }
        Err(error) => {
            for join in joins {
                out.insert(join.pid, Err("TERMINAL_STATE_UNKNOWN".to_owned()));
            }
            ledger.push(json!({ "terminalError": error }));
            return out;
        }
    };
    let pending: Vec<&Join> = joins
        .iter()
        .copied()
        .filter(|join| {
            force
                || !state.attempted.contains(&(
                    join.pid,
                    join.after.sample.start_seconds,
                    join.after.sample.start_microseconds,
                    generation.canonical(),
                ))
        })
        .collect();
    if pending.is_empty() {
        return out;
    }
    match automation_authorized(QUICK) {
        Ok(true) => {}
        Ok(false) => {
            for join in pending {
                out.insert(join.pid, Err("AUTOMATION_NOT_AUTHORIZED".to_owned()));
            }
            return out;
        }
        Err(_) => {
            for join in pending {
                out.insert(join.pid, Err("AUTOMATION_STATE_UNKNOWN".to_owned()));
            }
            return out;
        }
    }
    let script = state
        .context
        .resources_dir
        .join("terminal-inventory.applescript");
    let started = log::now_ms();
    let tabs = terminal::enumerate(&script, ENUMERATION_TIMEOUT);
    let ended = log::now_ms();
    let after = terminal_generation().ok().flatten();
    let tabs = match tabs {
        Ok(tabs) => tabs,
        Err(error) => {
            log::warn(
                "TERMINAL_ENUMERATION_FAILED",
                json!({ "error": error.to_string() }),
            );
            for join in pending {
                out.insert(join.pid, Err("TERMINAL_ENUMERATION_FAILED".to_owned()));
            }
            return out;
        }
    };
    if after.as_ref() != Some(&generation) {
        for join in pending {
            out.insert(join.pid, Err("TERMINAL_GENERATION_CHANGED".to_owned()));
        }
        return out;
    }
    // stat every tab once: tty path → st_rdev of a character device.
    let devices: Vec<(usize, Option<u32>)> = tabs
        .tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| (index, tty::character_device(&tab.tty).ok()))
        .collect();
    for join in pending {
        let matches: Vec<usize> = devices
            .iter()
            .filter(|(_, rdev)| *rdev == Some(join.device))
            .map(|(index, _)| *index)
            .collect();
        let key = (
            join.pid,
            join.after.sample.start_seconds,
            join.after.sample.start_microseconds,
            generation.canonical(),
        );
        let resampled = process::sample_incarnation(join.pid);
        let process_ok = resampled.as_ref().is_ok_and(|now| {
            now.same_process_and_image(&join.after)
                && now.sample.controlling_device == Some(join.device)
        });
        let outcome = match (matches.as_slice(), process_ok) {
            (_, false) => Err("PROCESS_CHANGED_DURING_SURFACE_JOIN".to_owned()),
            ([], true) => {
                state.attempted.insert(key);
                Err("NO_MATCHING_TAB".to_owned())
            }
            ([index], true) => {
                let tab = &tabs.tabs[*index];
                Ok(SurfaceRecord {
                    tty: tab.tty.clone(),
                    device: join.device,
                    terminal_generation: generation.canonical(),
                    window_hint: tab.window_id,
                    tab_hint: tab.tab_index,
                    proof: json!({
                        "method": "TTY_ST_RDEV_EQ_E_TDEV",
                        "tty": tab.tty,
                        "rdev": join.device,
                        "eTdev": join.device,
                        "windowIdHint": tab.window_id,
                        "tabIndexHint": tab.tab_index,
                        "terminalGeneration": generation,
                        "enumeration": {
                            "startedMs": started,
                            "endedMs": ended,
                            "windows": tabs.windows,
                            "tabs": tabs.tabs.len(),
                            "senderPid": tabs.sender_pid,
                        },
                        "processAfterJoin": resampled.as_ref().ok().map(incarnation_json),
                    }),
                })
            }
            (_, true) => {
                state.attempted.insert(key);
                Err("MULTIPLE_MATCHING_TABS".to_owned())
            }
        };
        ledger.push(json!({
            "pid": join.pid,
            "sessionId": join.native_session_id,
            "device": join.device,
            "matches": matches.iter().map(|index| json!({
                "tty": tabs.tabs[*index].tty,
                "windowId": tabs.tabs[*index].window_id,
                "tabIndex": tabs.tabs[*index].tab_index,
            })).collect::<Vec<_>>(),
            "outcome": outcome.as_ref().map(|_| "BOUND").unwrap_or_else(|reason| reason.as_str()),
        }));
        out.insert(join.pid, outcome);
    }
    out
}

fn summary_error(started: i64, error: String) -> DiscoverySummary {
    DiscoverySummary {
        started_at_ms: started,
        ended_at_ms: log::now_ms(),
        inventory_rows: 0,
        joined: 0,
        provisional: vec![],
        executions_started: 0,
        executions_ended: 0,
        bindings_recorded: 0,
        bindings_invalidated: 0,
        surface_unbound: 0,
        committed_cursor: None,
        error: Some(error),
    }
}

fn run_pass(state: &mut State, force_surface: bool) -> DiscoverySummary {
    let started = log::now_ms();
    // Provider polling runs only while capture admission is open: observation
    // enabled, no maintenance phase, machine awake (SPEC §19.5).
    if !crate::state::RUNTIME.admission_open() {
        return summary_error(started, "ADMISSION_CLOSED".into());
    }
    let Some(install) = ClaudeInstall::resolve(&state.context.launcher()) else {
        return summary_error(started, "CLAUDE_NOT_INSTALLED".into());
    };
    let cli = state.context.cli(&install, INVENTORY_TIMEOUT);
    let version = state
        .versions
        .entry(install.binary.clone())
        .or_insert_with(|| cli.version())
        .clone();
    let Some(Ok(live_rows)) = writer_call(&state.context.writer, |reply| {
        WriterCommand::LiveExecutions {
            provider: PROVIDER,
            reply,
        }
    }) else {
        return summary_error(started, "WRITER_UNAVAILABLE".into());
    };
    let pass: DiscoveryPass = match discover(&cli, &KernelSampler, |path| install.qualifies(path)) {
        Ok(pass) => pass,
        Err(error) => return summary_error(started, error.to_string()),
    };

    let live: Vec<LiveExecution> = live_rows.iter().map(live_execution).collect();
    let liveness: HashMap<String, Result<Incarnation, process::ProcessError>> = live
        .iter()
        .map(|execution| {
            (
                execution.execution_id.clone(),
                process::sample_incarnation(execution.pid),
            )
        })
        .collect();
    let plan = reconcile::plan(&live, &liveness, &pass);
    let needs = reconcile::needs_surface(&plan, &live, &pass);
    let mut surface_ledger = Vec::new();
    let mut surfaces = surface_join(state, &needs, force_surface, &mut surface_ledger);

    let mut changes = Vec::new();
    for (execution_id, reason) in &plan.ends {
        changes.push(ActivationChange::End {
            execution_id: execution_id.clone(),
            reason: reason.code().to_owned(),
        });
    }
    for index in &plan.new_activations {
        let join = &pass.joins[*index];
        changes.push(ActivationChange::Start {
            native_session_id: join.native_session_id.clone(),
            process: process_record(&state.context.boot_id, join),
            device: join.device,
            surface: surfaces
                .remove(&join.pid)
                .unwrap_or_else(|| Err("SURFACE_NOT_ATTEMPTED".to_owned())),
        });
    }
    for (execution_id, index) in &plan.continuing {
        if let Some(surface) = surfaces.remove(&pass.joins[*index].pid) {
            changes.push(ActivationChange::Surface {
                execution_id: execution_id.clone(),
                surface,
            });
        }
    }

    let sessions: Vec<ObservedSessionRecord> = pass
        .sessions
        .iter()
        .map(|session| ObservedSessionRecord {
            native_session_id: session.native_session_id.clone(),
            kind: session.kind.clone(),
            display_name: session.name.clone(),
            status: session.status.clone(),
            waiting_for: session.waiting_for.clone(),
        })
        .collect();

    let payload = json!({
        "adapter": "provider-claude/direct-cli",
        "binary": install.binary.display().to_string(),
        "launcher": install.launcher.display().to_string(),
        "binaryVersion": version,
        "argv": ["agents", "--json", "--all"],
        "firstLookup": rows_json(&pass.first),
        "secondLookup": pass.second.as_ref().map(rows_json),
        "secondLookupError": pass.second_error,
        "joins": pass.joins.iter().map(|join| json!({
            "sessionId": join.native_session_id,
            "pid": join.pid,
            "device": join.device,
            "before": incarnation_json(&join.before),
            "after": incarnation_json(&join.after),
        })).collect::<Vec<_>>(),
        "provisional": pass.provisional.iter().map(|p| json!({
            "pid": p.pid, "sessionId": p.session_id, "kind": p.kind, "reason": p.reason,
        })).collect::<Vec<_>>(),
        "liveExecutions": live.iter().map(|execution| json!({
            "executionId": execution.execution_id,
            "sessionId": execution.native_session_id,
            "pid": execution.pid,
            "sample": liveness.get(&execution.execution_id).map(|sample| match sample {
                Ok(incarnation) => incarnation_json(incarnation),
                Err(error) => json!({ "error": error.code() }),
            }),
        })).collect::<Vec<_>>(),
        "ends": plan.ends.iter().map(|(id, reason)| json!({ "executionId": id, "reason": reason.code() })).collect::<Vec<_>>(),
        "newActivations": plan.new_activations.len(),
        "surfaceJoins": surface_ledger,
        "forcedSurface": force_surface,
    });
    let application = DiscoveryApplication {
        provider: PROVIDER.to_owned(),
        profile_ref: state.context.profile_ref(),
        sessions,
        changes,
        payload,
    };
    let applied = writer_call(&state.context.writer, |reply| {
        WriterCommand::ApplyDiscovery {
            application: Box::new(application),
            reply,
        }
    });
    let outcome = match applied {
        Some(Ok(outcome)) => outcome,
        Some(Err(error)) => return summary_error(started, error),
        None => return summary_error(started, "WRITER_UNAVAILABLE".into()),
    };
    let summary = DiscoverySummary {
        started_at_ms: started,
        ended_at_ms: log::now_ms(),
        inventory_rows: pass.first.rows.len() as u32,
        joined: pass.joins.len() as u32,
        provisional: pass
            .provisional
            .iter()
            .map(|p| ProvisionalCandidate {
                pid: p.pid,
                session_id: p.session_id.clone(),
                kind: p.kind.clone(),
                reason: p.reason.to_owned(),
            })
            .collect(),
        executions_started: outcome.executions_started,
        executions_ended: outcome.executions_ended,
        bindings_recorded: outcome.bindings_recorded,
        bindings_invalidated: outcome.bindings_invalidated,
        surface_unbound: surface_ledger
            .iter()
            .filter(|entry| entry["outcome"] != "BOUND")
            .count() as u32,
        committed_cursor: outcome
            .change
            .as_ref()
            .map(|change| change.cursor.to_string()),
        error: None,
    };
    if outcome.change.is_some() || force_surface {
        log::info(
            "DISCOVERY_PASS",
            json!({
                "rows": summary.inventory_rows,
                "joined": summary.joined,
                "provisional": summary.provisional.len(),
                "started": summary.executions_started,
                "ended": summary.executions_ended,
                "bound": summary.bindings_recorded,
                "invalidated": summary.bindings_invalidated,
                "cursor": summary.committed_cursor,
                "elapsedMs": summary.ended_at_ms - summary.started_at_ms,
            }),
        );
    }
    summary
}

fn live_execution(row: &LiveExecutionRow) -> LiveExecution {
    LiveExecution {
        execution_id: row.execution_id.clone(),
        native_session_id: row.native_session_id.clone(),
        pid: row.pid as i32,
        start_seconds: row.start_seconds,
        start_microseconds: row.start_microseconds,
        executable: row.executable.clone(),
        has_valid_binding: row.has_valid_binding,
    }
}

pub fn spawn(context: DiscoveryContext) -> std::io::Result<SyncSender<Trigger>> {
    let (trigger, triggers): (SyncSender<Trigger>, Receiver<Trigger>) = mpsc::sync_channel(16);
    thread::Builder::new()
        .name("claude-discovery".into())
        .spawn(move || {
            let mut state = State {
                context,
                attempted: HashSet::new(),
                versions: HashMap::new(),
            };
            // Late start: discover sessions that predate this companion now,
            // without waiting for any provider hook.
            let first = run_pass(&mut state, true);
            log::info(
                "DISCOVERY_STARTUP_PASS",
                json!({ "joined": first.joined, "rows": first.inventory_rows, "error": first.error }),
            );
            loop {
                match triggers.recv_timeout(INTERVAL) {
                    Ok(Trigger::Refresh {
                        force_surface,
                        reply,
                    }) => {
                        let summary = run_pass(&mut state, force_surface);
                        if let Some(reply) = reply {
                            let _ = reply.send(summary);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        run_pass(&mut state, false);
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            log::warn("DISCOVERY_STOPPED", json!({}));
        })?;
    Ok(trigger)
}
