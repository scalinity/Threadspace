//! The Claude discovery scheduler (SPEC §4.4, §4.14, §19.3). One pass:
//!
//! 1. bracketed native inventory join (provider-claude);
//! 2. fresh kernel samples of every stored live activation;
//! 3. a reconciliation plan that ends activations only on proven evidence;
//! 4. a surface join for activations without a binding: exactly one live
//!    Ghostty terminal or Terminal tab whose TTY `st_rdev` equals the
//!    provider's `e_tdev`, inside an unchanged application incarnation (and,
//!    for Ghostty, an unchanged executable image with a known dictionary),
//!    with the process re-sampled afterwards;
//! 5. one registration transaction through the single writer.
//!
//! The first pass runs at startup, so sessions launched before Threadspace are
//! discovered without any new provider hook. Passes are serialized on this
//! thread; inventory requests never overlap. A surface application is
//! enumerated only when an activation needs a surface, only while it runs,
//! and only when Apple-event automation is already authorized (never
//! prompting). Each Ghostty process is addressed by its own PID.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_contracts::canonical::fact::SnapshotRow;
use threadspace_contracts::route::{
    AppGeneration, DiscoverySummary, GhosttyApplication, ProvisionalCandidate,
};
use threadspace_journal::{
    ActivationChange, DiscoveryApplication, LiveExecutionRow, ObservedSessionRecord, ProcessRecord,
    SessionWait, SurfaceRecord,
};
use threadspace_provider_claude::discovery::{DiscoveryPass, Join, discover};
use threadspace_provider_claude::inventory::{
    ClaudeCli, ClaudeInstall, InventoryRow, InventoryWait, interpret_status,
};
use threadspace_provider_claude::reconcile::{self, LiveExecution};
use threadspace_surfaces::{SURFACE_GHOSTTY, SURFACE_TERMINAL};
use threadspace_surfaces_macos::ancestry::KernelSampler;
use threadspace_surfaces_macos::ghostty::{self, GHOSTTY_BUNDLE_ID, GhosttyDictionary};
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
        threadspace_relay::paths::claude_profile_ref(&self.home)
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

/// Apple-event authorization for Ghostty, asked without prompting.
pub fn ghostty_automation_authorized(timeout: Duration) -> Result<bool, String> {
    bridge::automation_permission(GHOSTTY_BUNDLE_ID, false, timeout)
        .map(|status| status == NO_ERR)
        .map_err(|error| error.to_string())
}

/// Ghostty dictionaries probed from the installed bundle, keyed by the
/// executable image (`dev:ino`) each describes. A process keeps the
/// dictionary probed while its image was installed even after the bundle is
/// replaced on disk; an image never seen installed has no known dictionary.
static GHOSTTY_DICTIONARIES: Mutex<Vec<(String, GhosttyDictionary)>> = Mutex::new(Vec::new());
const MAX_GHOSTTY_DICTIONARIES: usize = 8;

/// The known dictionary of a mapped Ghostty image.
pub fn ghostty_dictionary(image: &str) -> Option<GhosttyDictionary> {
    GHOSTTY_DICTIONARIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(known, _)| known == image)
        .map(|(_, dictionary)| dictionary.clone())
}

/// Probes an installation's dictionary once per installed executable file.
/// The file identity must be unchanged across the probe, so the dictionary
/// belongs to it.
fn probe_installed_ghostty(bundle: &Path, executable: &str, installed: &str) -> Result<(), String> {
    if ghostty_dictionary(installed).is_some() {
        return Ok(());
    }
    let dictionary = ghostty::probe_dictionary(bundle)
        .map_err(|error| format!("GHOSTTY_DICTIONARY_FAILED: {error}"))?;
    if process::path_file_id(executable).as_deref() != Some(installed) {
        return Err("GHOSTTY_INSTALL_CHANGED".into());
    }
    let mut cache = GHOSTTY_DICTIONARIES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= MAX_GHOSTTY_DICTIONARIES {
        cache.remove(0);
    }
    cache.push((installed.to_owned(), dictionary));
    Ok(())
}

/// Installed executables (`path`, `dev:ino`) already checked to belong to a
/// bundle whose identifier is Ghostty's.
static GHOSTTY_BUNDLES: Mutex<Vec<(String, String, bool)>> = Mutex::new(Vec::new());

fn is_ghostty_bundle(bundle: &Path, executable: &str, installed: Option<&str>) -> bool {
    let Some(installed) = installed else {
        return false;
    };
    let mut known = GHOSTTY_BUNDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, _, ghostty)) = known
        .iter()
        .find(|(path, id, _)| path == executable && id == installed)
    {
        return *ghostty;
    }
    let ghostty = terminal::plist_string(&bundle.join("Contents/Info.plist"), "CFBundleIdentifier")
        .as_deref()
        == Some(GHOSTTY_BUNDLE_ID);
    if known.len() >= 32 {
        known.remove(0);
    }
    known.push((executable.to_owned(), installed.to_owned(), ghostty));
    ghostty
}

/// Why a Ghostty process cannot support automatic Return, if it cannot.
fn ghostty_limitation(
    image: Option<&str>,
    installed: Option<&str>,
    dictionary: Option<&GhosttyDictionary>,
) -> Option<&'static str> {
    match (image, dictionary) {
        (None, _) => Some("GHOSTTY_IMAGE_UNKNOWN"),
        // Updating the bundle on disk does not upgrade a running process.
        (Some(image), None) if installed != Some(image) => Some("GHOSTTY_RESTART_REQUIRED"),
        (Some(_), None) => Some("GHOSTTY_DICTIONARY_UNAVAILABLE"),
        (_, Some(dictionary)) if !dictionary.supports_focus() => {
            Some("GHOSTTY_DICTIONARY_UNSUPPORTED")
        }
        (_, Some(dictionary)) if !dictionary.has_tty_property => Some("GHOSTTY_TTY_UNAVAILABLE"),
        _ => None,
    }
}

/// Every running Ghostty process from kernel evidence, at any installation
/// path: its incarnation, the executable image it actually maps, and whether
/// that image's dictionary is known and sufficient. `probe` allows reading an
/// installation's dictionary.
///
/// Apple events addressed to a PID reach the application registered at that
/// PID's bundle path, so two processes running from one path cannot be told
/// apart: neither is ever scripted.
pub fn ghostty_applications(probe: bool) -> Result<Vec<GhosttyApplication>, String> {
    let candidates: Vec<(i32, String, PathBuf, Option<String>)> = process::executables()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|(pid, executable)| {
            let bundle = ghostty::bundle_of(&executable)?;
            let executable = executable.display().to_string();
            let installed = process::path_file_id(&executable);
            is_ghostty_bundle(&bundle, &executable, installed.as_deref())
                .then_some((pid, executable, bundle, installed))
        })
        .collect();
    let mut apps = Vec::with_capacity(candidates.len());
    for (pid, executable, bundle, installed) in &candidates {
        let shared = candidates
            .iter()
            .filter(|(_, other, _, _)| other == executable)
            .count()
            > 1;
        if probe
            && !shared
            && let Some(installed) = installed
            && let Err(error) = probe_installed_ghostty(bundle, executable, installed)
        {
            log::warn("GHOSTTY_DICTIONARY_UNAVAILABLE", json!({ "error": error }));
        }
        let first = match process::sample(*pid) {
            Ok(sample) => sample,
            Err(process::ProcessError::Vanished { .. }) => continue,
            Err(error) => return Err(error.to_string()),
        };
        let image = process::mapped_image_file_id(*pid).ok().flatten();
        match process::sample(*pid) {
            Ok(second) if second.same_incarnation(&first) => {}
            // Exited or reused while it was read: not a running process now.
            _ => continue,
        }
        let dictionary = image.as_deref().and_then(ghostty_dictionary);
        let limitation = if shared {
            Some("GHOSTTY_AMBIGUOUS_PROCESSES")
        } else {
            ghostty_limitation(image.as_deref(), installed.as_deref(), dictionary.as_ref())
        };
        apps.push(GhosttyApplication {
            generation: AppGeneration {
                pid: *pid as u32,
                start_seconds: first.start_seconds.to_string(),
                start_microseconds: first.start_microseconds,
            },
            executable: executable.clone(),
            image,
            installed: installed.clone(),
            dictionary_sha256: dictionary.map(|dictionary| dictionary.sha256),
            limitation: limitation.map(str::to_owned),
        });
    }
    Ok(apps)
}

/// Unavailable-surface reasons that can change only with an application
/// generation or image, so a no-match answer under them is final for the
/// same fingerprint.
const STABLE_LIMITATIONS: [&str; 5] = [
    "GHOSTTY_RESTART_REQUIRED",
    "GHOSTTY_AMBIGUOUS_PROCESSES",
    "GHOSTTY_IMAGE_UNKNOWN",
    "GHOSTTY_DICTIONARY_UNSUPPORTED",
    "GHOSTTY_TTY_UNAVAILABLE",
];

struct State {
    context: DiscoveryContext,
    /// (pid, birth s, birth µs, surface-application fingerprint) whose
    /// surface join gave a final answer; retried only on a forced pass.
    attempted: HashSet<(i32, u64, u32, String)>,
    versions: HashMap<PathBuf, Option<String>>,
    /// Passes applied by this companion incarnation; the sequence of each
    /// pass's inventory causal point (the epoch is the core generation the
    /// journal was opened with).
    passes: u64,
}

/// The provider's wait mapping (SPEC §11.3) of a stored or new inventory row.
fn row_wait(row: &SnapshotRow) -> Option<SessionWait> {
    let row = InventoryRow {
        kind: row.kind.clone(),
        status: row.status.clone(),
        waiting_for: row.waiting_for.clone(),
        state: row.state.clone(),
        ..InventoryRow::default()
    };
    match interpret_status(&row) {
        InventoryWait::NotWaiting => None,
        InventoryWait::Waiting { category, subtype } => Some(SessionWait { category, subtype }),
    }
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

/// Terminal.app's current tabs as surface candidates.
fn terminal_surfaces(
    state: &State,
    generation: &AppGeneration,
) -> Result<Vec<SurfaceRecord>, String> {
    match automation_authorized(QUICK) {
        Ok(true) => {}
        Ok(false) => return Err("AUTOMATION_NOT_AUTHORIZED".to_owned()),
        Err(_) => return Err("AUTOMATION_STATE_UNKNOWN".to_owned()),
    }
    let script = state
        .context
        .resources_dir
        .join("terminal-inventory.applescript");
    let started = log::now_ms();
    let tabs = terminal::enumerate(&script, ENUMERATION_TIMEOUT);
    let ended = log::now_ms();
    let after = terminal_generation().ok().flatten();
    let tabs = tabs.map_err(|error| {
        log::warn(
            "TERMINAL_ENUMERATION_FAILED",
            json!({ "error": error.to_string() }),
        );
        "TERMINAL_ENUMERATION_FAILED".to_owned()
    })?;
    if after.as_ref() != Some(generation) {
        return Err("TERMINAL_GENERATION_CHANGED".to_owned());
    }
    Ok(tabs
        .tabs
        .iter()
        .filter_map(|tab| {
            let device = tty::character_device(&tab.tty).ok()?;
            Some(SurfaceRecord {
                surface_kind: SURFACE_TERMINAL.to_owned(),
                locator: tab.tty.clone(),
                tty: tab.tty.clone(),
                device,
                terminal_generation: generation.canonical(),
                window_hint: Some(tab.window_id),
                tab_hint: Some(tab.tab_index),
                proof: json!({
                    "method": "TTY_ST_RDEV_EQ_E_TDEV",
                    "tty": tab.tty,
                    "rdev": device,
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
                }),
            })
        })
        .collect())
}

/// One scriptable Ghostty process's terminals as surface candidates, inside
/// an unchanged process and image.
fn ghostty_surfaces(state: &State, app: &GhosttyApplication) -> Result<Vec<SurfaceRecord>, String> {
    let Some(dictionary) = app.image.as_deref().and_then(ghostty_dictionary) else {
        return Err("GHOSTTY_DICTIONARY_UNAVAILABLE".to_owned());
    };
    let script = state
        .context
        .resources_dir
        .join("ghostty-inventory.applescript");
    let started = log::now_ms();
    let inventory = ghostty::enumerate(
        &script,
        &dictionary,
        app.generation.pid as i32,
        ENUMERATION_TIMEOUT,
    );
    let ended = log::now_ms();
    let unchanged = ghostty_applications(false).is_ok_and(|apps| {
        apps.iter()
            .any(|now| now.generation == app.generation && now.image == app.image)
    });
    let inventory = inventory.map_err(|error| {
        log::warn(
            "GHOSTTY_ENUMERATION_FAILED",
            json!({ "pid": app.generation.pid, "error": error.to_string() }),
        );
        "GHOSTTY_ENUMERATION_FAILED".to_owned()
    })?;
    if !unchanged {
        return Err("GHOSTTY_GENERATION_CHANGED".to_owned());
    }
    Ok(inventory
        .terminals
        .iter()
        .filter_map(|terminal| {
            let path = terminal.tty.as_deref()?;
            let device = tty::character_device(path).ok()?;
            Some(SurfaceRecord {
                surface_kind: SURFACE_GHOSTTY.to_owned(),
                locator: terminal.id.clone(),
                tty: path.to_owned(),
                device,
                terminal_generation: app.generation.canonical(),
                window_hint: None,
                tab_hint: None,
                proof: json!({
                    "method": "TTY_ST_RDEV_EQ_E_TDEV",
                    "tty": path,
                    "rdev": device,
                    "windowId": terminal.window_id,
                    "tabId": terminal.tab_id,
                    "terminalId": terminal.id,
                    "application": app,
                    "enumeration": {
                        "startedMs": started,
                        "endedMs": ended,
                        "windows": inventory.windows.len(),
                        "tabs": inventory.tabs.len(),
                        "terminals": inventory.terminals.len(),
                        "senderPid": inventory.sender_pid,
                    },
                }),
            })
        })
        .collect())
}

/// Joins each listed activation to exactly one live Ghostty terminal or
/// Terminal tab, or says why not. A pty has one controlling emulator, so a
/// single device match is the answer even if another application could not
/// be inspected; with no match, the uninspected application is the reason.
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
    let terminal = terminal_generation();
    let ghostty_apps = ghostty_applications(true);
    let mut fingerprint = Vec::new();
    match &terminal {
        Ok(Some(generation)) => fingerprint.push(format!("terminal:{}", generation.canonical())),
        Ok(None) => {}
        Err(error) => fingerprint.push(format!("terminal-error:{error}")),
    }
    match &ghostty_apps {
        Ok(apps) => fingerprint.extend(apps.iter().map(|app| {
            format!(
                "ghostty:{}:{}:{}",
                app.generation.canonical(),
                app.image.as_deref().unwrap_or("-"),
                app.limitation.as_deref().unwrap_or("OK")
            )
        })),
        Err(error) => fingerprint.push(format!("ghostty-error:{error}")),
    }
    let fingerprint = fingerprint.join(",");
    let key = |join: &Join| {
        (
            join.pid,
            join.after.sample.start_seconds,
            join.after.sample.start_microseconds,
            fingerprint.clone(),
        )
    };
    let pending: Vec<&Join> = joins
        .iter()
        .copied()
        .filter(|join| force || !state.attempted.contains(&key(join)))
        .collect();
    if pending.is_empty() {
        return out;
    }
    if matches!(terminal, Ok(None)) && matches!(&ghostty_apps, Ok(apps) if apps.is_empty()) {
        for join in pending {
            out.insert(join.pid, Err("NO_SURFACE_APPLICATION".to_owned()));
        }
        return out;
    }

    let mut candidates: Vec<SurfaceRecord> = Vec::new();
    let mut unavailable: Vec<String> = Vec::new();
    match &ghostty_apps {
        Err(error) => {
            ledger.push(json!({ "ghosttyError": error }));
            unavailable.push("GHOSTTY_STATE_UNKNOWN".to_owned());
        }
        Ok(apps) => {
            unavailable.extend(apps.iter().filter_map(|app| app.limitation.clone()));
            let usable: Vec<&GhosttyApplication> =
                apps.iter().filter(|app| app.limitation.is_none()).collect();
            if !usable.is_empty() {
                match ghostty_automation_authorized(QUICK) {
                    Ok(true) => {
                        for app in usable {
                            match ghostty_surfaces(state, app) {
                                Ok(list) => candidates.extend(list),
                                Err(reason) => unavailable.push(reason),
                            }
                        }
                    }
                    Ok(false) => unavailable.push("GHOSTTY_AUTOMATION_NOT_AUTHORIZED".to_owned()),
                    Err(_) => unavailable.push("GHOSTTY_AUTOMATION_STATE_UNKNOWN".to_owned()),
                }
            }
        }
    }
    match &terminal {
        Ok(None) => {}
        Ok(Some(generation)) => match terminal_surfaces(state, generation) {
            Ok(list) => candidates.extend(list),
            Err(reason) => unavailable.push(reason),
        },
        Err(error) => {
            ledger.push(json!({ "terminalError": error }));
            unavailable.push(if error == "MULTIPLE_TERMINAL_PROCESSES" {
                error.clone()
            } else {
                "TERMINAL_STATE_UNKNOWN".to_owned()
            });
        }
    }
    let stable = unavailable
        .iter()
        .all(|reason| STABLE_LIMITATIONS.contains(&reason.as_str()));

    for join in pending {
        let matches: Vec<&SurfaceRecord> = candidates
            .iter()
            .filter(|surface| surface.device == join.device)
            .collect();
        let resampled = process::sample_incarnation(join.pid);
        let process_ok = resampled.as_ref().is_ok_and(|now| {
            now.same_process_and_image(&join.after)
                && now.sample.controlling_device == Some(join.device)
        });
        let outcome = match (matches.as_slice(), process_ok) {
            (_, false) => Err("PROCESS_CHANGED_DURING_SURFACE_JOIN".to_owned()),
            ([only], true) => {
                let mut surface = (*only).clone();
                surface.proof["eTdev"] = json!(join.device);
                surface.proof["processAfterJoin"] =
                    resampled.as_ref().ok().map(incarnation_json).into();
                Ok(surface)
            }
            ([], true) => {
                if stable {
                    state.attempted.insert(key(join));
                }
                Err(unavailable
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "NO_MATCHING_SURFACE".to_owned()))
            }
            (_, true) => {
                state.attempted.insert(key(join));
                Err("MULTIPLE_MATCHING_SURFACES".to_owned())
            }
        };
        ledger.push(json!({
            "pid": join.pid,
            "sessionId": join.native_session_id,
            "device": join.device,
            "matches": matches.iter().map(|surface| json!({
                "kind": surface.surface_kind,
                "locator": surface.locator,
                "tty": surface.tty,
            })).collect::<Vec<_>>(),
            "unavailable": unavailable,
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
            state: session.state.clone(),
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
    state.passes += 1;
    let application = DiscoveryApplication {
        provider: PROVIDER.to_owned(),
        profile_ref: state.context.profile_ref(),
        sessions,
        changes,
        payload,
        pass: state.passes,
        wait_of: row_wait,
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
                passes: 0,
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

#[cfg(test)]
mod tests {
    use threadspace_contracts::canonical::fact::WaitCategory;

    use super::*;

    fn row(kind: &str, status: Option<&str>, waiting_for: Option<&str>, state: Option<&str>) -> SnapshotRow {
        SnapshotRow {
            kind: Some(kind.into()),
            status: status.map(str::to_owned),
            waiting_for: waiting_for.map(str::to_owned),
            display_name: Some("label".into()),
            state: state.map(str::to_owned),
        }
    }

    #[test]
    fn stored_rows_read_through_the_provider_wait_mapping() {
        let wait = |category, subtype: &str| Some(SessionWait { category, subtype: subtype.into() });
        assert_eq!(
            row_wait(&row("interactive", Some("waiting"), Some("permission prompt"), None)),
            wait(WaitCategory::Approval, "permission prompt")
        );
        assert_eq!(
            row_wait(&row("interactive", Some("waiting"), Some("input needed"), None)),
            wait(WaitCategory::Input, "input needed")
        );
        assert_eq!(row_wait(&row("background", None, None, Some("blocked"))), wait(WaitCategory::JobBlocked, ""));
        assert_eq!(row_wait(&row("background", None, None, Some("done"))), None);
        assert_eq!(row_wait(&row("interactive", Some("idle"), None, None)), None);
    }
}
