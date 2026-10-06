//! The outer application's native bootstrap (SPEC §18.9): it alone registers,
//! queries and unregisters the single `SMAppService.loginItem(identifier:)` for
//! the nested `ThreadspaceAgent.app`. It runs without constructing React or a
//! WebView:
//!
//!   Threadspace.app/Contents/MacOS/Threadspace --service status|register|unregister
//!   Threadspace.app/Contents/MacOS/Threadspace --service prepare|cancel|stop|enable
//!
//! `stop` and `enable` carry the SPEC §19.5 ordering: the companion records
//! the preference and prepares while still supervised; only a PREPARED
//! answer allows unregistering, after which the old ProcessKey's exit and the
//! writer lock's release are verified. No custom LaunchAgent is registered.

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use serde_json::{Value, json};
use threadspace_contracts::control::{
    ClientRole, ControlErrorCode, ControlRequestBody, ControlResponseBody, MaintenancePurpose,
};
use threadspace_contracts::diagnostics::{ProcessIdentity, ServiceReport, ServiceStatus};
use threadspace_relay::client::{BlockingClient, ClientError, connect};
use threadspace_relay::paths::AgentPaths;
use threadspace_surfaces_macos::process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCommand {
    Status,
    Register,
    Unregister,
    Prepare,
    Cancel,
    Stop,
    Enable,
}

impl ServiceCommand {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "status" => Some(Self::Status),
            "register" => Some(Self::Register),
            "unregister" => Some(Self::Unregister),
            "prepare" => Some(Self::Prepare),
            "cancel" => Some(Self::Cancel),
            "stop" => Some(Self::Stop),
            "enable" => Some(Self::Enable),
            _ => None,
        }
    }
}

/// Generous: a consistent backup of a large store can take a while.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(120);
const EXIT_WAIT: Duration = Duration::from_secs(15);
const START_WAIT: Duration = Duration::from_secs(30);
/// A notification cold start hands the store to the login item's companion,
/// which launchd relaunches (at most every ten seconds) until it holds the
/// writer lock.
const HANDOVER_WAIT: Duration = Duration::from_secs(45);

fn service(agent_identifier: &str) -> Retained<SMAppService> {
    let identifier = NSString::from_str(agent_identifier);
    // SAFETY: a plain class-method call with a valid NSString argument.
    unsafe { SMAppService::loginItemServiceWithIdentifier(&identifier) }
}

fn status_of(service: &SMAppService) -> ServiceStatus {
    // SAFETY: reading a property of a live SMAppService.
    let status = unsafe { service.status() };
    match status {
        SMAppServiceStatus::NotRegistered => ServiceStatus::NotRegistered,
        SMAppServiceStatus::Enabled => ServiceStatus::Enabled,
        SMAppServiceStatus::RequiresApproval => ServiceStatus::RequiresApproval,
        SMAppServiceStatus::NotFound => ServiceStatus::NotFound,
        _ => ServiceStatus::Unknown,
    }
}

pub fn report(agent_identifier: &str) -> ServiceReport {
    ServiceReport {
        agent_identifier: agent_identifier.to_owned(),
        status: status_of(&service(agent_identifier)),
    }
}

/// Registers or unregisters the login item and reports the status around it.
fn change_registration(agent_identifier: &str, register: bool) -> Value {
    let service = service(agent_identifier);
    let before = status_of(&service);
    // SAFETY: plain method calls on a live SMAppService; errors are returned.
    let outcome = if register {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    let error = outcome.err().map(|error| {
        format!(
            "{} {} {}",
            error.domain(),
            error.code(),
            error.localizedDescription()
        )
    });
    json!({
        "operation": if register { "register" } else { "unregister" },
        "statusBefore": before,
        "statusAfter": status_of(&service),
        "error": error,
    })
}

fn bootstrap_client(paths: &AgentPaths) -> Result<BlockingClient, String> {
    connect(&paths.locator, ClientRole::Bootstrap, CONTROL_TIMEOUT)
        .map(BlockingClient::new)
        .map_err(|error| error.to_string())
}

fn request(
    client: &mut BlockingClient,
    body: ControlRequestBody,
) -> Result<ControlResponseBody, String> {
    client.request(body).map_err(|error| error.to_string())
}

/// Whether the kernel still has this exact incarnation (PID and birth).
fn incarnation_alive(identity: &ProcessIdentity) -> bool {
    process::sample(identity.pid as i32).is_ok_and(|sample| {
        sample.start_seconds.to_string() == identity.start_seconds
            && sample.start_microseconds == identity.start_microseconds
    })
}

/// True when no process holds the store's writer lock. A shared,
/// non-blocking probe on a read-only descriptor; never held.
pub fn writer_lock_free(store_dir: &Path) -> Result<bool, String> {
    let file = OpenOptions::new()
        .read(true)
        .open(store_dir.join("writer.lock"))
        .map_err(|error| error.to_string())?;
    // SAFETY: `file` owns the descriptor for the duration of both calls.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
    if rc == 0 {
        // SAFETY: as above.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(error.to_string())
    }
}

fn step(steps: &mut Vec<Value>, name: &str, started: Instant, ok: bool, detail: Value) {
    steps.push(json!({ "step": name, "ok": ok, "elapsedMs": started.elapsed().as_millis() as u64, "detail": detail }));
}

/// Stop Observation (SPEC §19.5): record disabled, prepare while supervised,
/// then unregister and verify the old incarnation exited and released the
/// writer lock. A failed preparation aborts before unregistering.
pub fn stop(agent_identifier: &str) -> (bool, Value) {
    let started = Instant::now();
    let mut steps = Vec::new();
    let Some(paths) = AgentPaths::for_agent(agent_identifier) else {
        return (false, json!({ "error": "invalid agent identifier" }));
    };
    let mut client = match bootstrap_client(&paths) {
        Ok(client) => client,
        Err(error) => {
            step(&mut steps, "connect", started, false, json!(error));
            return (false, json!({ "steps": steps, "unregistered": false }));
        }
    };
    let companion = client.connection().hello.companion.clone();
    step(
        &mut steps,
        "connect",
        started,
        true,
        json!({ "companionPid": companion.pid, "startSeconds": companion.start_seconds, "startMicroseconds": companion.start_microseconds }),
    );
    let disabled = request(
        &mut client,
        ControlRequestBody::SetObservationEnabled { enabled: false },
    );
    let disabled_ok = matches!(disabled, Ok(ControlResponseBody::Done));
    step(
        &mut steps,
        "record-observation-disabled",
        started,
        disabled_ok,
        json!(disabled.err()),
    );
    if !disabled_ok {
        return (false, json!({ "steps": steps, "unregistered": false }));
    }
    let prepared = request(
        &mut client,
        ControlRequestBody::PrepareMaintenance {
            purpose: MaintenancePurpose::Stop,
        },
    );
    let report = match prepared {
        Ok(ControlResponseBody::MaintenancePrepared { report }) => report,
        other => {
            step(
                &mut steps,
                "prepare-maintenance",
                started,
                false,
                json!(format!("{other:?}")),
            );
            // The companion reopened admission; restore the enabled preference
            // so a failed stop leaves observation as it was.
            let restored = request(
                &mut client,
                ControlRequestBody::SetObservationEnabled { enabled: true },
            );
            step(
                &mut steps,
                "restore-observation-enabled",
                started,
                matches!(restored, Ok(ControlResponseBody::Done)),
                json!(restored.err()),
            );
            return (
                false,
                json!({ "steps": steps, "unregistered": false, "aborted": "preparation failed; service left registered" }),
            );
        }
    };
    step(
        &mut steps,
        "prepare-maintenance",
        started,
        true,
        json!({ "phase": report.phase, "backupFile": report.backup_file, "backupCursor": report.backup_cursor, "backupBytes": report.backup_bytes }),
    );
    drop(client);
    let unregistered = change_registration(agent_identifier, false);
    let unregister_ok = unregistered["error"].is_null();
    step(
        &mut steps,
        "unregister",
        started,
        unregister_ok,
        unregistered,
    );
    let wait = Instant::now();
    while incarnation_alive(&companion) && wait.elapsed() < EXIT_WAIT {
        thread::sleep(Duration::from_millis(100));
    }
    let exited = !incarnation_alive(&companion);
    step(
        &mut steps,
        "verify-old-incarnation-exited",
        started,
        exited,
        json!({ "waitedMs": wait.elapsed().as_millis() as u64 }),
    );
    let lock = writer_lock_free(&paths.store_dir);
    let lock_ok = matches!(lock, Ok(true));
    step(
        &mut steps,
        "verify-writer-lock-released",
        started,
        lock_ok,
        json!(format!("{lock:?}")),
    );
    let ok = unregister_ok && exited && lock_ok;
    (ok, json!({ "steps": steps, "unregistered": unregister_ok }))
}

/// Enable Observation (SPEC §19.5): register/start the companion, then let it
/// alone leave any prepared phase and record the enabled preference.
pub fn enable(agent_identifier: &str) -> (bool, Value) {
    let started = Instant::now();
    let mut steps = Vec::new();
    let Some(paths) = AgentPaths::for_agent(agent_identifier) else {
        return (false, json!({ "error": "invalid agent identifier" }));
    };
    let before = status_of(&service(agent_identifier));
    if before != ServiceStatus::Enabled {
        let registered = change_registration(agent_identifier, true);
        let ok = registered["error"].is_null();
        step(&mut steps, "register", started, ok, registered);
        if !ok {
            return (false, json!({ "steps": steps }));
        }
    } else {
        step(
            &mut steps,
            "register",
            started,
            true,
            json!("already enabled"),
        );
    }
    let wait = Instant::now();
    let mut client = loop {
        match bootstrap_client(&paths) {
            Ok(client) => break client,
            Err(error) if wait.elapsed() >= START_WAIT => {
                step(&mut steps, "connect", started, false, json!(error));
                return (false, json!({ "steps": steps }));
            }
            Err(_) => thread::sleep(Duration::from_millis(250)),
        }
    };
    step(
        &mut steps,
        "connect",
        started,
        true,
        json!({ "companionPid": client.connection().hello.companion.pid, "waitedMs": wait.elapsed().as_millis() as u64 }),
    );
    let phase = match request(&mut client, ControlRequestBody::Diagnostics) {
        Ok(ControlResponseBody::Diagnostics { report }) => report.maintenance_phase,
        _ => "UNKNOWN".to_owned(),
    };
    if phase != "NONE" {
        let cancelled = request(&mut client, ControlRequestBody::CancelMaintenance);
        let ok = matches!(cancelled, Ok(ControlResponseBody::Done));
        step(
            &mut steps,
            "leave-maintenance",
            started,
            ok,
            json!({ "phase": phase, "error": cancelled.err() }),
        );
        if !ok {
            return (false, json!({ "steps": steps }));
        }
    }
    // Only the login item's companion opens observation (SPEC §19.5); a
    // notification cold start refuses, exits and hands the store over.
    let handover = Instant::now();
    let mut handovers = 0;
    let enabled = loop {
        match client.request(ControlRequestBody::SetObservationEnabled { enabled: true }) {
            Err(ClientError::Rejected(error))
                if error.code == ControlErrorCode::NotSupervised && handover.elapsed() < HANDOVER_WAIT =>
            {
                handovers += 1;
                thread::sleep(Duration::from_secs(1));
                client = loop {
                    match bootstrap_client(&paths) {
                        Ok(client) => break client,
                        Err(error) if handover.elapsed() >= HANDOVER_WAIT => {
                            step(&mut steps, "handover", started, false, json!(error));
                            return (false, json!({ "steps": steps }));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(250)),
                    }
                };
            }
            other => break other.map_err(|error| error.to_string()),
        }
    };
    if handovers > 0 {
        step(
            &mut steps,
            "handover",
            started,
            enabled.is_ok(),
            json!({ "refusals": handovers, "companionPid": client.connection().hello.companion.pid, "waitedMs": handover.elapsed().as_millis() as u64 }),
        );
    }
    let ok = matches!(enabled, Ok(ControlResponseBody::Done));
    step(
        &mut steps,
        "record-observation-enabled",
        started,
        ok,
        json!(enabled.err()),
    );
    (ok, json!({ "steps": steps }))
}

/// Runs one bootstrap command and prints a JSON result line. Exit 0 on success.
pub fn run_cli(command: ServiceCommand, app_identifier: &str) -> i32 {
    let agent_identifier = threadspace_relay::paths::agent_identifier_for(app_identifier);
    let (ok, detail) = match command {
        ServiceCommand::Status => (
            true,
            json!({ "status": status_of(&service(&agent_identifier)) }),
        ),
        ServiceCommand::Register | ServiceCommand::Unregister => {
            let result =
                change_registration(&agent_identifier, command == ServiceCommand::Register);
            (result["error"].is_null(), result)
        }
        ServiceCommand::Prepare | ServiceCommand::Cancel => {
            let outcome = AgentPaths::for_agent(&agent_identifier)
                .ok_or_else(|| "invalid agent identifier".to_owned())
                .and_then(|paths| bootstrap_client(&paths))
                .and_then(|mut client| {
                    let body = if command == ServiceCommand::Prepare {
                        ControlRequestBody::PrepareMaintenance {
                            purpose: MaintenancePurpose::Update {
                                target_version: "qualification".into(),
                            },
                        }
                    } else {
                        ControlRequestBody::CancelMaintenance
                    };
                    request(&mut client, body)
                });
            match outcome {
                Ok(body) => (true, json!(format!("{body:?}"))),
                Err(error) => (false, json!({ "error": error })),
            }
        }
        ServiceCommand::Stop => stop(&agent_identifier),
        ServiceCommand::Enable => enable(&agent_identifier),
    };
    println!(
        "{}",
        json!({
            "operation": format!("{command:?}").to_lowercase(),
            "appIdentifier": app_identifier,
            "agentIdentifier": agent_identifier,
            "ok": ok,
            "detail": detail,
            "statusAfter": status_of(&service(&agent_identifier)),
        })
    );
    i32::from(!ok)
}
