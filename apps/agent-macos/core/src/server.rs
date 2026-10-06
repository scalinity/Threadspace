//! The control endpoint (SPEC §8.1). Every accepted connection is checked
//! with `getpeereid`; the first frame must be `Hello` naming the current core
//! generation; the qualification role exists only in qualification builds.
//! Slow native operations (permission prompts, Terminal probes) run on
//! bounded per-connection workers so ACK-path requests are never stuck
//! behind them.

use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use serde_json::json;
use threadspace_contracts::control::{
    CONTROL_PROTOCOL_VERSION, ClientRole, ControlError, ControlErrorCode, ControlMessage,
    ControlRequest, ControlRequestBody, ControlResponseBody,
};
use threadspace_contracts::diagnostics::ProcessIdentity;
use threadspace_contracts::limits::CONTROL_FRAME_MAX_BYTES;
use threadspace_relay::frame::{read_frame, write_frame};
use threadspace_relay::peer::{current_euid, peer_credentials};
use threadspace_surfaces_macos::process;

use crate::discovery::{DiscoveryContext, Trigger};
use crate::log;
use crate::native_ops;
use crate::state::RUNTIME;
use crate::writer::{Outbound, WriterCommand, respond};

const OUTBOUND_CAPACITY: usize = 256;
const MAX_SLOW_IN_FLIGHT: usize = 4;

pub struct CoreContext {
    pub bundle_identifier: String,
    pub core_generation: String,
    pub store_generation: String,
    pub identity: ProcessIdentity,
    pub started_at_ms: i64,
    pub resources_dir: PathBuf,
    pub writer: SyncSender<WriterCommand>,
    pub claude: DiscoveryContext,
    pub discovery: SyncSender<Trigger>,
}

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);
/// Connections that completed Hello and are still open.
static OPEN_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
/// With observation disabled, the companion exits after this long without a
/// connected client: there is nothing to observe, and a deliberate successful
/// exit is permitted after observation stop (SPEC §18.9).
const DISABLED_IDLE_EXIT: Duration = Duration::from_secs(120);

pub fn spawn_idle_exit() {
    let _ = thread::Builder::new()
        .name("disabled-idle-exit".into())
        .spawn(|| {
            let mut idle_since = Instant::now();
            loop {
                thread::sleep(Duration::from_secs(5));
                if RUNTIME.observation_enabled() {
                    return;
                }
                if OPEN_CONNECTIONS.load(Ordering::Acquire) > 0 {
                    idle_since = Instant::now();
                } else if idle_since.elapsed() >= DISABLED_IDLE_EXIT {
                    log::info("DISABLED_IDLE_EXIT", json!({}));
                    std::process::exit(0);
                }
            }
        });
}

pub fn spawn(
    listener: UnixListener,
    context: Arc<CoreContext>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("control-accept".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let context = Arc::clone(&context);
                        let connection_id = NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed);
                        let spawned = thread::Builder::new()
                            .name(format!("control-{connection_id}"))
                            .spawn(move || serve(stream, connection_id, &context));
                        if spawned.is_err() {
                            log::error(
                                "CONNECTION_THREAD_FAILED",
                                json!({ "connectionId": connection_id }),
                            );
                        }
                    }
                    Err(error) => log::warn("ACCEPT_FAILED", json!({ "error": error.to_string() })),
                }
            }
        })
}

fn role_permitted(role: ClientRole) -> bool {
    match role {
        ClientRole::Ui | ClientRole::Bootstrap => true,
        ClientRole::Qualification => cfg!(feature = "qualification"),
    }
}

fn serve(stream: UnixStream, connection_id: u64, context: &CoreContext) {
    let peer = match peer_credentials(&stream) {
        Ok(peer) if peer.euid == current_euid() => peer,
        Ok(peer) => {
            log::warn(
                "PEER_REJECTED",
                json!({ "connectionId": connection_id, "euid": peer.euid }),
            );
            return;
        }
        Err(error) => {
            log::warn(
                "PEER_UNREADABLE",
                json!({ "connectionId": connection_id, "error": error.to_string() }),
            );
            return;
        }
    };
    let peer_sample = process::sample(peer.pid).ok();
    let peer_executable = process::executable_path(peer.pid)
        .ok()
        .map(|path| path.display().to_string());

    let Ok(mut write_half) = stream.try_clone() else {
        return;
    };
    let _ = write_half.set_write_timeout(Some(Duration::from_secs(2)));
    let (outbound, queue) = mpsc::sync_channel::<ControlMessage>(OUTBOUND_CAPACITY);
    let writer_thread = thread::spawn(move || {
        for message in queue {
            if write_frame(&mut write_half, &message, CONTROL_FRAME_MAX_BYTES).is_err() {
                break;
            }
        }
    });

    let mut read_half = stream;
    let _ = read_half.set_read_timeout(Some(Duration::from_secs(5)));
    let role = match hello(&mut read_half, &outbound, context) {
        Some(role) => role,
        None => {
            drop(outbound);
            let _ = writer_thread.join();
            return;
        }
    };
    log::info(
        "CLIENT_CONNECTED",
        json!({
            "connectionId": connection_id,
            "role": role,
            "peerPid": peer.pid,
            "peerStartSeconds": peer_sample.as_ref().map(|sample| sample.start_seconds),
            "peerExecutable": peer_executable,
        }),
    );
    let _ = read_half.set_read_timeout(None);
    let slow = Arc::new(AtomicUsize::new(0));
    OPEN_CONNECTIONS.fetch_add(1, Ordering::AcqRel);
    let peer_is_companion = peer_executable
        .as_deref()
        .is_some_and(|path| path == context.identity.executable_path);
    loop {
        let request: ControlRequest = match read_frame(&mut read_half, CONTROL_FRAME_MAX_BYTES) {
            Ok(request) => request,
            Err(threadspace_relay::frame::FrameError::Malformed(error)) => {
                log::warn(
                    "REQUEST_MALFORMED",
                    json!({ "connectionId": connection_id, "error": error.to_string() }),
                );
                break;
            }
            Err(_) => break,
        };
        dispatch(
            request,
            role,
            peer_is_companion,
            connection_id,
            context,
            &outbound,
            &slow,
        );
    }
    OPEN_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    let _ = context
        .writer
        .send(WriterCommand::ConnectionClosed { connection_id });
    log::info(
        "CLIENT_DISCONNECTED",
        json!({ "connectionId": connection_id }),
    );
    drop(outbound);
    let _ = writer_thread.join();
}

fn hello(stream: &mut impl Read, outbound: &Outbound, context: &CoreContext) -> Option<ClientRole> {
    let request: ControlRequest = read_frame(stream, CONTROL_FRAME_MAX_BYTES).ok()?;
    let ControlRequestBody::Hello {
        protocol_version,
        role,
        expected_core_generation,
    } = request.body
    else {
        respond(
            outbound,
            request.request_id,
            Err(ControlError::new(
                ControlErrorCode::HelloRequired,
                "Hello must be first",
            )),
        );
        return None;
    };
    let refusal = if protocol_version != CONTROL_PROTOCOL_VERSION {
        Some(ControlError::new(
            ControlErrorCode::UnsupportedProtocol,
            "unsupported control protocol",
        ))
    } else if expected_core_generation != context.core_generation {
        Some(ControlError::new(
            ControlErrorCode::StaleGeneration,
            "stale core generation",
        ))
    } else if !role_permitted(role) {
        Some(ControlError::new(
            ControlErrorCode::RoleNotPermitted,
            "role not permitted by this build",
        ))
    } else {
        None
    };
    if let Some(error) = refusal {
        respond(outbound, request.request_id, Err(error));
        return None;
    }
    respond(
        outbound,
        request.request_id,
        Ok(ControlResponseBody::HelloAck {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            core_generation: context.core_generation.clone(),
            store_generation: context.store_generation.clone(),
            companion: context.identity.clone(),
        }),
    );
    Some(role)
}

fn to_writer(context: &CoreContext, outbound: &Outbound, request_id: u64, command: WriterCommand) {
    if context.writer.try_send(command).is_err() {
        respond(
            outbound,
            request_id,
            Err(ControlError::new(
                ControlErrorCode::Busy,
                "writer queue full",
            )),
        );
    }
}

fn refuse(outbound: &Outbound, request_id: u64, code: ControlErrorCode, detail: &str) {
    respond(outbound, request_id, Err(ControlError::new(code, detail)));
}

fn dispatch(
    request: ControlRequest,
    role: ClientRole,
    peer_is_companion: bool,
    connection_id: u64,
    context: &CoreContext,
    outbound: &Outbound,
    slow: &Arc<AtomicUsize>,
) {
    let request_id = request.request_id;
    let outbound_clone = outbound.clone();
    match request.body {
        ControlRequestBody::Hello { .. } => {
            respond(
                outbound,
                request_id,
                Err(ControlError::new(
                    ControlErrorCode::BadRequest,
                    "already greeted",
                )),
            );
        }
        ControlRequestBody::AttachView { subscription_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::AttachView {
                connection_id,
                request_id,
                subscription_id,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::DetachView { subscription_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::DetachView {
                request_id,
                subscription_id,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::ViewHydrated { subscription_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::ViewHydrated {
                request_id,
                subscription_id,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::FleetPage { after, limit } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::Page {
                request_id,
                attention: false,
                after,
                limit,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::AttentionPage { after, limit } => {
            to_writer(
                context,
                outbound,
                request_id,
                WriterCommand::Page {
                    request_id,
                    attention: true,
                    after,
                    limit,
                    outbound: outbound_clone,
                },
            );
        }
        ControlRequestBody::ResolveAttention {
            command_id,
            attention_id,
            expected_revision,
            reason,
        } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::Resolve {
                request_id,
                command_id,
                attention_id,
                expected_revision,
                reason,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::PrepareMaintenance { .. }
        | ControlRequestBody::CancelMaintenance
        | ControlRequestBody::SetObservationEnabled { .. }
            if role != ClientRole::Bootstrap =>
        {
            refuse(
                outbound,
                request_id,
                ControlErrorCode::RoleNotPermitted,
                "bootstrap role required",
            );
        }
        ControlRequestBody::PrepareMaintenance { purpose } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::PrepareMaintenance {
                request_id,
                purpose,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::CancelMaintenance => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::CancelMaintenance {
                request_id,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::SetObservationEnabled { enabled } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::SetObservationEnabled {
                request_id,
                enabled,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::ForwardNotificationResponse {
            notification_request_id,
            attention_id,
        } => {
            // Only another instance of this exact companion executable may
            // forward a response it received (SPEC §7.5).
            if !peer_is_companion {
                refuse(
                    outbound,
                    request_id,
                    ControlErrorCode::RoleNotPermitted,
                    "only the companion executable may forward a response",
                );
                return;
            }
            log::info(
                "NOTIFICATION_RESPONSE_RECEIVED_FORWARDED",
                json!({ "requestId": notification_request_id, "attentionId": attention_id }),
            );
            if context
                .writer
                .try_send(WriterCommand::NotificationResponse {
                    notification_request_id,
                    attention_id,
                })
                .is_ok()
            {
                respond(outbound, request_id, Ok(ControlResponseBody::Done));
            } else {
                refuse(
                    outbound,
                    request_id,
                    ControlErrorCode::Busy,
                    "writer queue full",
                );
            }
        }
        ControlRequestBody::IntentConsumed { intent_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::IntentConsumed {
                request_id,
                intent_id,
                outbound: outbound_clone,
            },
        ),
        ControlRequestBody::AcknowledgeAttention {
            command_id,
            attention_id,
            expected_revision,
        } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::Acknowledge {
                request_id,
                command_id,
                attention_id,
                expected_revision,
                outbound: outbound_clone,
            },
        ),
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyRaiseAttention { .. }
        | ControlRequestBody::QualifyAdmit { .. }
        | ControlRequestBody::QualifyClearNotifications
        | ControlRequestBody::QualifySyntheticChanges { .. }
        | ControlRequestBody::QualifyPopulate { .. }
        | ControlRequestBody::QualifyViewCommand { .. }
        | ControlRequestBody::QualifyArmFault { .. }
            if role != ClientRole::Qualification =>
        {
            refuse(
                outbound,
                request_id,
                ControlErrorCode::RoleNotPermitted,
                "qualification role required",
            );
        }
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifySyntheticChanges {
            count,
            duration_ms,
            sessions,
        } => {
            let run_id = uuid::Uuid::new_v4().to_string();
            let writer = context.writer.clone();
            let started = spawn_synthetic_run(writer, run_id.clone(), count, duration_ms, sessions);
            match started {
                Ok(()) => respond(
                    outbound,
                    request_id,
                    Ok(ControlResponseBody::SyntheticChangesStarted {
                        run_id,
                        first_cursor: String::new(),
                    }),
                ),
                Err(error) => refuse(outbound, request_id, ControlErrorCode::Internal, &error),
            }
        }
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyAdmit {
            observation_id,
            captured_wall_ms,
        } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::Admit {
                request_id,
                observation_id,
                captured_wall_ms,
                outbound: outbound_clone,
            },
        ),
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyClearNotifications => {
            let outcome = crate::bridge::call(
                |correlation_id| crate::bridge::BridgeRequest::ClearNotifications {
                    correlation_id,
                },
                Duration::from_secs(10),
            );
            let reply = match outcome {
                Ok(crate::bridge::BridgeEvent::NotificationsCleared { removed, .. }) => {
                    log::info(
                        "QUALIFICATION_NOTIFICATIONS_CLEARED",
                        json!({ "removed": removed }),
                    );
                    Ok(ControlResponseBody::NotificationsCleared { removed })
                }
                other => Err(ControlError::new(
                    ControlErrorCode::Unavailable,
                    format!("{other:?}"),
                )),
            };
            respond(outbound, request_id, reply);
        }
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyPopulate {
            sessions,
            name_bytes,
        } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::Populate {
                request_id,
                sessions,
                name_bytes,
                outbound: outbound_clone,
            },
        ),
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyViewCommand { command, args } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::ViewCommand {
                request_id,
                command,
                args,
                outbound: outbound_clone,
            },
        ),
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyArmFault { fault } => {
            match fault {
                threadspace_contracts::control::QualificationFault::FailNextMaintenanceBackup => {
                    RUNTIME.arm_backup_failure()
                }
            }
            log::info("QUALIFICATION_FAULT_ARMED", json!({ "fault": fault }));
            respond(outbound, request_id, Ok(ControlResponseBody::Done));
        }
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyRaiseAttention { label, session_id } => {
            if role != ClientRole::Qualification {
                respond(
                    outbound,
                    request_id,
                    Err(ControlError::new(
                        ControlErrorCode::RoleNotPermitted,
                        "qualification role required",
                    )),
                );
                return;
            }
            let label: String = label
                .chars()
                .take(threadspace_contracts::limits::LABEL_MAX_CHARS)
                .collect();
            to_writer(
                context,
                outbound,
                request_id,
                WriterCommand::RaiseAttention {
                    request_id,
                    label,
                    session_id,
                    outbound: outbound_clone,
                },
            );
        }
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyExportObservations { .. }
            if role != ClientRole::Qualification =>
        {
            respond(
                outbound,
                request_id,
                Err(ControlError::new(
                    ControlErrorCode::RoleNotPermitted,
                    "qualification role required",
                )),
            );
        }
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyExportObservations {
            after_cursor,
            limit,
        } => {
            let Some(after_cursor) = threadspace_contracts::cursor::parse_cursor(&after_cursor)
            else {
                respond(
                    outbound,
                    request_id,
                    Err(ControlError::new(
                        ControlErrorCode::BadRequest,
                        "afterCursor must be a cursor",
                    )),
                );
                return;
            };
            let (reply, answer) = mpsc::channel();
            to_writer(
                context,
                outbound,
                request_id,
                WriterCommand::ExportObservations {
                    after_cursor,
                    limit,
                    reply,
                },
            );
            let outcome = match answer.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(rows)) => Ok(ControlResponseBody::ObservationsExported {
                    observations: rows
                        .into_iter()
                        .filter_map(|row| serde_json::to_value(row).ok())
                        .collect(),
                }),
                Ok(Err(error)) => Err(ControlError::new(ControlErrorCode::Internal, error)),
                Err(_) => Err(ControlError::new(
                    ControlErrorCode::Unavailable,
                    "writer did not answer",
                )),
            };
            respond(outbound, request_id, outcome);
        }
        ControlRequestBody::ReturnToSession { .. } | ControlRequestBody::RefreshEvidence
            if !RUNTIME.admission_open() =>
        {
            let (code, detail) = if !RUNTIME.observation_enabled() {
                (
                    ControlErrorCode::ObservationDisabled,
                    "observation is disabled",
                )
            } else if !RUNTIME.writes_open() {
                (
                    ControlErrorCode::MaintenanceGated,
                    "maintenance holds the store",
                )
            } else {
                (
                    ControlErrorCode::Unavailable,
                    "observation is suspended for sleep",
                )
            };
            refuse(outbound, request_id, code, detail);
        }
        body @ (ControlRequestBody::Diagnostics
        | ControlRequestBody::IntegrationStatus
        | ControlRequestBody::RequestNotificationAuthorization
        | ControlRequestBody::RequestTerminalAutomation
        | ControlRequestBody::ReturnToSession { .. }
        | ControlRequestBody::RefreshEvidence) => {
            if slow.fetch_add(1, Ordering::AcqRel) >= MAX_SLOW_IN_FLIGHT {
                slow.fetch_sub(1, Ordering::AcqRel);
                respond(
                    outbound,
                    request_id,
                    Err(ControlError::new(
                        ControlErrorCode::Busy,
                        "too many native operations in flight",
                    )),
                );
                return;
            }
            let in_flight = Arc::clone(slow);
            let writer = context.writer.clone();
            let snapshot = native_ops::OpsContext::from(context);
            let spawned = thread::Builder::new()
                .name("native-op".into())
                .spawn(move || {
                    let result = native_ops::run(body, &snapshot, &writer);
                    respond(&outbound_clone, request_id, result);
                    in_flight.fetch_sub(1, Ordering::AcqRel);
                });
            if spawned.is_err() {
                slow.fetch_sub(1, Ordering::AcqRel);
                respond(
                    outbound,
                    request_id,
                    Err(ControlError::new(
                        ControlErrorCode::Internal,
                        "could not start native operation",
                    )),
                );
            }
        }
        // Cargo feature unification can expose qualification-only request
        // variants in the contracts crate while this companion was built
        // without the `qualification` feature; such requests are refused.
        #[allow(unreachable_patterns)]
        _ => respond(
            outbound,
            request_id,
            Err(ControlError::new(
                ControlErrorCode::RoleNotPermitted,
                "operation not available in this build",
            )),
        ),
    }
}

/// Qualification only: commits `count` synthetic changes spread evenly over
/// `duration_ms` on a driver thread, each its own writer transaction and
/// broadcast, then logs the run's committed count and last cursor.
#[cfg(feature = "qualification")]
fn spawn_synthetic_run(
    writer: std::sync::mpsc::SyncSender<WriterCommand>,
    run_id: String,
    count: u32,
    duration_ms: u32,
    sessions: u32,
) -> Result<(), String> {
    let count = count.min(100_000);
    let sessions = sessions.clamp(1, 64);
    thread::Builder::new()
        .name("synthetic-run".into())
        .spawn(move || {
            let started = Instant::now();
            let interval = Duration::from_micros(u64::from(duration_ms) * 1000 / u64::from(count.max(1)));
            let mut committed = 0u32;
            let mut failed = 0u32;
            let mut last_cursor = 0i64;
            log::info(
                "SYNTHETIC_RUN_STARTED",
                json!({ "runId": run_id, "count": count, "durationMs": duration_ms, "sessions": sessions }),
            );
            for sequence in 0..count {
                let due = interval * sequence;
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    thread::sleep(wait);
                }
                let (reply, answer) = mpsc::channel();
                if writer
                    .send(WriterCommand::SyntheticChange {
                        run_id: run_id.clone(),
                        slot: sequence % sessions,
                        sequence: u64::from(sequence),
                        reply: Some(reply),
                    })
                    .is_err()
                {
                    break;
                }
                match answer.recv_timeout(Duration::from_secs(10)) {
                    Ok(Ok(cursor)) => {
                        committed += 1;
                        last_cursor = cursor;
                    }
                    _ => failed += 1,
                }
            }
            log::info(
                "SYNTHETIC_RUN_DONE",
                json!({
                    "runId": run_id,
                    "committed": committed,
                    "failed": failed,
                    "lastCursor": last_cursor,
                    "elapsedMs": started.elapsed().as_millis() as u64,
                }),
            );
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}
