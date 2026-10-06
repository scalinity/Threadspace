//! Rust core of `ThreadspaceAgent.app`, the independently supervised login-item
//! companion (SPEC §2.1, §18.9). The Swift AppKit shell owns the run loop,
//! the strongly retained notification delegate and the Apple API calls; this
//! core owns the journal writer, the private control socket, the notification
//! outbox and native intents.
//!
//! C ABI (see `apps/agent-macos/include/threadspace_agent.h`):
//! - `ts_core_start(config_json, callback)` starts the core and returns an exit
//!   status: 0 running, 75 another writer holds the store, 78 unsupported OS,
//!   64 bad configuration, 70 fatal startup error.
//! - `ts_core_deliver(event_json)` carries Apple-layer answers and notification
//!   responses into the core.

mod bridge;
mod discovery;
mod forward;
mod log;
mod maintenance;
mod native_ops;
mod notify;
mod respond;
mod route;
mod server;
mod state;
mod writer;

use std::collections::VecDeque;
use std::ffi::{CStr, c_char};
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Deserialize;
use serde_json::json;
use threadspace_contracts::diagnostics::ProcessIdentity;
use threadspace_journal::{Journal, LockError, WriterLock};
use threadspace_relay::locator::{self, LOCATOR_SCHEMA, RuntimeLocator};
use threadspace_relay::paths::AgentPaths;
use threadspace_relay::runtime::{
    bind_private_socket, create_runtime_dir, remove_stale_runtime_dir,
};
use threadspace_surfaces_macos::process;
use uuid::Uuid;

use crate::bridge::{BridgeCallback, BridgeEvent};
use crate::state::RUNTIME;
use crate::writer::{WriterCommand, WriterSetup};
use threadspace_contracts::control::MaintenancePhase;

pub const EXIT_RUNNING: i32 = 0;
pub const EXIT_CONFIG: i32 = 64;
pub const EXIT_FATAL: i32 = 70;
pub const EXIT_WRITER_LOCK_HELD: i32 = 75;
pub const EXIT_UNSUPPORTED_OS: i32 = 78;

const MAX_EVENT_BYTES: usize = 16 * 1024;
const MAX_PRESTART_EVENTS: usize = 32;
const DEFAULT_ACTION: &str = "com.apple.UNNotificationDefaultActionIdentifier";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartConfig {
    bundle_identifier: String,
    bundle_path: String,
    resources_path: String,
}

struct Running {
    writer: SyncSender<WriterCommand>,
    discovery: Option<SyncSender<discovery::Trigger>>,
    _lock: WriterLock,
}

static RUNNING: OnceLock<Running> = OnceLock::new();
static PRESTART: Mutex<VecDeque<BridgeEvent>> = Mutex::new(VecDeque::new());

fn own_identity() -> Result<ProcessIdentity, String> {
    let pid = std::process::id() as i32;
    let sample = process::sample(pid).map_err(|error| error.to_string())?;
    Ok(ProcessIdentity {
        pid: pid as u32,
        boot_id: process::boot_session_id().map_err(|error| error.to_string())?,
        start_seconds: sample.start_seconds.to_string(),
        start_microseconds: sample.start_microseconds,
        executable_path: process::executable_path(pid)
            .map_err(|error| error.to_string())?
            .display()
            .to_string(),
    })
}

fn private_dir(path: &Path) -> std::io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

/// Validated, ID-only handling of notification responses; queued until the
/// writer exists (SPEC §7.5).
fn handle_event(event: BridgeEvent) {
    if let BridgeEvent::Power { phase } = &event {
        power_transition(phase);
        return;
    }
    let BridgeEvent::NotificationResponse {
        schema,
        request_id,
        action_identifier,
        attention_id,
        ..
    } = event
    else {
        return;
    };
    if schema != 1 || action_identifier != DEFAULT_ACTION {
        log::info(
            "NOTIFICATION_RESPONSE_IGNORED",
            json!({ "schema": schema, "actionIdentifier": action_identifier, "requestId": request_id }),
        );
        return;
    }
    if forward::active() {
        forward::notification_response(&request_id, &attention_id);
        return;
    }
    match RUNNING.get() {
        Some(running) => {
            if running
                .writer
                .try_send(WriterCommand::NotificationResponse {
                    notification_request_id: request_id,
                    attention_id,
                })
                .is_err()
            {
                log::warn(
                    "NOTIFICATION_RESPONSE_DROPPED",
                    json!({ "reason": "writer queue full" }),
                );
            }
        }
        None => {
            if let Ok(mut queue) = PRESTART.lock() {
                if queue.len() >= MAX_PRESTART_EVENTS {
                    queue.pop_front();
                }
                queue.push_back(BridgeEvent::NotificationResponse {
                    schema,
                    request_id,
                    action_identifier,
                    attention_id,
                    session_id: String::new(),
                });
            }
        }
    }
}

fn start(config: StartConfig, callback: BridgeCallback) -> i32 {
    bridge::install(callback);
    let Some(paths) = AgentPaths::for_agent(&config.bundle_identifier) else {
        return EXIT_CONFIG;
    };
    log::init(&paths.log_dir);
    log::info(
        "CORE_START",
        json!({
            "bundleIdentifier": config.bundle_identifier,
            "bundlePath": threadspace_relay::paths::redact_home(&config.bundle_path),
            "qualificationBuild": cfg!(feature = "qualification"),
        }),
    );
    if !process::meets_minimum_macos() {
        log::error(
            "UNSUPPORTED_OS",
            json!({ "version": format!("{:?}", process::os_product_version().ok()) }),
        );
        return EXIT_UNSUPPORTED_OS;
    }
    if let Err(error) = private_dir(&paths.store_dir) {
        log::error("STORE_DIR_FAILED", json!({ "error": error.to_string() }));
        return EXIT_FATAL;
    }
    let lock = match WriterLock::acquire(&paths.store_dir) {
        Ok(lock) => lock,
        Err(LockError::Held { .. }) => {
            // Another instance owns the store. This one only forwards a
            // notification response it may have been launched for, then
            // exits; it never becomes a second writer (SPEC §7.5).
            log::warn("WRITER_LOCK_HELD", json!({ "mode": "forwarder" }));
            forward::enter(paths.locator.clone());
            let queued: Vec<BridgeEvent> = PRESTART
                .lock()
                .map(|mut queue| queue.drain(..).collect())
                .unwrap_or_default();
            for event in queued {
                handle_event(event);
            }
            return EXIT_RUNNING;
        }
        Err(error) => {
            log::error("WRITER_LOCK_FAILED", json!({ "error": error.to_string() }));
            return EXIT_FATAL;
        }
    };
    log::info("WRITER_LOCK_ACQUIRED", json!({}));

    let core_generation = Uuid::new_v4().to_string();
    let started_at_ms = log::now_ms();
    let mut journal = match Journal::open(&paths.journal, &core_generation, started_at_ms) {
        Ok(journal) => journal,
        Err(error) => {
            log::error("JOURNAL_OPEN_FAILED", json!({ "error": error.to_string() }));
            return EXIT_FATAL;
        }
    };
    match journal.sqlite_diagnostics() {
        Ok(sqlite) => log::info(
            "JOURNAL_OPEN",
            json!({
                "sqliteVersion": sqlite.version,
                "sqliteSourceId": sqlite.source_id,
                "journalMode": sqlite.journal_mode,
                "synchronous": sqlite.synchronous,
                "foreignKeys": sqlite.foreign_keys,
                "cursor": sqlite.cursor,
                "storeGeneration": journal.store_generation(),
            }),
        ),
        Err(error) => log::warn(
            "JOURNAL_DIAGNOSTICS_FAILED",
            json!({ "error": error.to_string() }),
        ),
    }
    let store_generation = journal.store_generation().to_owned();
    let observation_enabled = journal.observation_enabled().unwrap_or(true);
    let maintenance = match journal.maintenance_phase() {
        Ok((phase, _)) if phase == "PREPARED" => MaintenancePhase::Prepared,
        // A crash while preparing leaves no completed backup: reopen.
        Ok((phase, _)) if phase == "PREPARING" => {
            let _ = journal.record_maintenance_phase(
                "NONE",
                None,
                json!({ "reason": "preparation interrupted by restart" }),
                log::now_ms(),
            );
            MaintenancePhase::None
        }
        _ => MaintenancePhase::None,
    };
    if maintenance == MaintenancePhase::Prepared {
        // Quiescent: a companion restarted inside a prepared maintenance
        // transaction cannot resume writes by itself (SPEC §19.5).
        log::warn("MAINTENANCE_QUIESCENT_START", json!({}));
    }
    RUNTIME.set_observation_enabled(observation_enabled);
    RUNTIME.set_maintenance(maintenance);
    log::info(
        "OBSERVATION_STATE",
        json!({ "observationEnabled": observation_enabled, "maintenance": maintenance }),
    );
    let identity = match own_identity() {
        Ok(identity) => identity,
        Err(error) => {
            log::error("IDENTITY_UNAVAILABLE", json!({ "error": error }));
            return EXIT_FATAL;
        }
    };

    if let Ok(previous) = locator::read(&paths.locator) {
        remove_stale_runtime_dir(Path::new(&previous.runtime_dir));
    }
    let (listener, socket_path, runtime_dir) = match create_runtime_dir().and_then(|dir| {
        bind_private_socket(&dir, "control.sock").map(|(listener, path)| (listener, path, dir))
    }) {
        Ok(bound) => bound,
        Err(error) => {
            log::error("SOCKET_BIND_FAILED", json!({ "error": error.to_string() }));
            return EXIT_FATAL;
        }
    };

    let (writer_tx, writer_rx) = mpsc::sync_channel(256);
    let (notify_tx, notify_rx) = mpsc::sync_channel(64);
    let (respond_tx, respond_rx) = mpsc::sync_channel(16);
    let Some(home) = threadspace_relay::paths::home_dir() else {
        log::error("HOME_UNAVAILABLE", json!({}));
        return EXIT_FATAL;
    };
    let claude = discovery::DiscoveryContext {
        writer: writer_tx.clone(),
        boot_id: identity.boot_id.clone(),
        home,
        resources_dir: PathBuf::from(&config.resources_path),
    };
    let mut discovery_trigger = None;
    let spawned = writer::spawn(WriterSetup {
        journal,
        commands: writer_rx,
        notifier: notify_tx,
        responder: respond_tx,
        identity: identity.clone(),
        store_dir: paths.store_dir.clone(),
    })
    .and_then(|_| notify::spawn(notify_rx, writer_tx.clone()))
    .and_then(|_| discovery::spawn(claude.clone()))
    .and_then(|discovery| {
        discovery_trigger = Some(discovery.clone());
        respond::spawn(
            respond_rx,
            writer_tx.clone(),
            claude.clone(),
            discovery.clone(),
        )
        .map(|_| discovery)
    })
    .and_then(|discovery| {
        let context = Arc::new(server::CoreContext {
            bundle_identifier: config.bundle_identifier.clone(),
            core_generation: core_generation.clone(),
            store_generation: store_generation.clone(),
            identity: identity.clone(),
            started_at_ms,
            resources_dir: PathBuf::from(&config.resources_path),
            writer: writer_tx.clone(),
            claude,
            discovery,
        });
        server::spawn(listener, context)
    });
    if let Err(error) = spawned {
        log::error("THREAD_SPAWN_FAILED", json!({ "error": error.to_string() }));
        return EXIT_FATAL;
    }

    let runtime_locator = RuntimeLocator {
        schema: LOCATOR_SCHEMA,
        bundle_identifier: config.bundle_identifier,
        runtime_dir: runtime_dir.display().to_string(),
        control_socket: socket_path.display().to_string(),
        core_generation: core_generation.clone(),
        store_generation: store_generation.clone(),
        companion: identity.clone(),
        written_at_ms: log::now_ms(),
    };
    if let Err(error) = locator::write_atomic(&paths.locator, &runtime_locator) {
        log::error(
            "LOCATOR_WRITE_FAILED",
            json!({ "error": error.to_string() }),
        );
        return EXIT_FATAL;
    }
    if RUNNING
        .set(Running {
            writer: writer_tx,
            discovery: discovery_trigger,
            _lock: lock,
        })
        .is_err()
    {
        return EXIT_FATAL;
    }
    log::info(
        "CORE_READY",
        json!({
            "coreGeneration": core_generation,
            "storeGeneration": store_generation,
            "pid": identity.pid,
            "startSeconds": identity.start_seconds,
            "startMicroseconds": identity.start_microseconds,
            "bootId": identity.boot_id,
            "controlSocket": runtime_locator.control_socket,
        }),
    );
    let queued: Vec<BridgeEvent> = PRESTART
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default();
    for event in queued {
        handle_event(event);
    }
    if !observation_enabled {
        server::spawn_idle_exit();
    }
    EXIT_RUNNING
}

/// Sleep/wake (SPEC §19.5): suspend polling on sleep; on wake resample the
/// boot session, journal the transition and revalidate through a fresh
/// discovery pass. Monotonic clocks are never compared across boots.
fn power_transition(phase: &str) {
    let now = log::now_ms();
    let boot = process::boot_session_id().ok();
    match phase {
        "WILL_SLEEP" => {
            RUNTIME.note_sleep(now);
            log::info("POWER_WILL_SLEEP", json!({ "bootId": boot }));
        }
        "DID_WAKE" => {
            RUNTIME.note_wake(now, boot.clone());
            log::info("POWER_DID_WAKE", json!({ "bootId": boot }));
        }
        other => {
            log::warn("POWER_EVENT_UNKNOWN", json!({ "phase": other }));
            return;
        }
    }
    let Some(running) = RUNNING.get() else {
        return;
    };
    let event = if phase == "WILL_SLEEP" {
        "OBSERVER_SUSPENDED_FOR_SLEEP"
    } else {
        "OBSERVER_RESUMED_AFTER_WAKE"
    };
    let _ = running.writer.try_send(WriterCommand::RecordLifecycle {
        native_event: event,
        payload: json!({ "bootId": boot, "wallMs": now }),
    });
    if phase == "DID_WAKE"
        && let Some(discovery) = running.discovery.as_ref()
        && discovery
            .try_send(discovery::Trigger::Refresh {
                force_surface: true,
                reply: None,
            })
            .is_ok()
    {
        RUNTIME.note_wake_revalidation();
    }
}

/// Starts the companion core.
///
/// # Safety
/// `config_json` must be a valid NUL-terminated UTF-8 string for the duration
/// of the call; `callback` must remain callable for the life of the process
/// and must copy the string it receives before returning.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ts_core_start(
    config_json: *const c_char,
    callback: Option<BridgeCallback>,
) -> i32 {
    // This core runs inside a Swift executable, so Rust's runtime never set
    // SIGPIPE to ignored; a write to a closed client socket must not kill the
    // companion.
    // SAFETY: installing SIG_IGN for SIGPIPE has no preconditions.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let (Some(callback), false) = (callback, config_json.is_null()) else {
        return EXIT_CONFIG;
    };
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    let text = unsafe { CStr::from_ptr(config_json) };
    let Ok(config) = serde_json::from_slice::<StartConfig>(text.to_bytes()) else {
        return EXIT_CONFIG;
    };
    if RUNNING.get().is_some() {
        return EXIT_RUNNING;
    }
    start(config, callback)
}

/// Delivers one Apple-layer event (JSON) to the core.
///
/// # Safety
/// `event_json` must be a valid NUL-terminated string for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ts_core_deliver(event_json: *const c_char) {
    if event_json.is_null() {
        return;
    }
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(event_json) }.to_bytes();
    if bytes.len() > MAX_EVENT_BYTES {
        log::warn("BRIDGE_EVENT_REJECTED", json!({ "reason": "oversized" }));
        return;
    }
    match serde_json::from_slice::<BridgeEvent>(bytes) {
        Ok(event) => {
            if let Some(uncorrelated) = bridge::route(event) {
                handle_event(uncorrelated);
            }
        }
        Err(error) => log::warn(
            "BRIDGE_EVENT_REJECTED",
            json!({ "reason": error.to_string() }),
        ),
    }
}
