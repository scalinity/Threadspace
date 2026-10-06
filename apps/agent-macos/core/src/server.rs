//! The control endpoint (SPEC §8.1). Every accepted connection is checked
//! with `getpeereid`; the first frame must be `Hello` naming the current core
//! generation; the qualification role exists only in qualification builds.
//! Slow native operations (permission prompts, Terminal probes) run on
//! bounded per-connection workers so ACK-path requests are never stuck
//! behind them.

use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::control::{
    CONTROL_PROTOCOL_VERSION, ClientRole, ControlError, ControlErrorCode, ControlMessage, ControlRequest,
    ControlRequestBody, ControlResponseBody,
};
use threadspace_contracts::diagnostics::ProcessIdentity;
use threadspace_contracts::limits::CONTROL_FRAME_MAX_BYTES;
use threadspace_relay::frame::{read_frame, write_frame};
use threadspace_relay::peer::{current_euid, peer_credentials};
use threadspace_surfaces_macos::process;

use crate::log;
use crate::native_ops;
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
}

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

pub fn spawn(listener: UnixListener, context: Arc<CoreContext>) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().name("control-accept".into()).spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let context = Arc::clone(&context);
                    let connection_id = NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed);
                    let spawned = thread::Builder::new()
                        .name(format!("control-{connection_id}"))
                        .spawn(move || serve(stream, connection_id, &context));
                    if spawned.is_err() {
                        log::error("CONNECTION_THREAD_FAILED", json!({ "connectionId": connection_id }));
                    }
                }
                Err(error) => log::warn("ACCEPT_FAILED", json!({ "error": error.to_string() })),
            }
        }
    })
}

fn role_permitted(role: ClientRole) -> bool {
    match role {
        ClientRole::Ui => true,
        ClientRole::Qualification => cfg!(feature = "qualification"),
    }
}

fn serve(stream: UnixStream, connection_id: u64, context: &CoreContext) {
    let peer = match peer_credentials(&stream) {
        Ok(peer) if peer.euid == current_euid() => peer,
        Ok(peer) => {
            log::warn("PEER_REJECTED", json!({ "connectionId": connection_id, "euid": peer.euid }));
            return;
        }
        Err(error) => {
            log::warn("PEER_UNREADABLE", json!({ "connectionId": connection_id, "error": error.to_string() }));
            return;
        }
    };
    let peer_sample = process::sample(peer.pid).ok();
    let peer_executable = process::executable_path(peer.pid).ok().map(|path| path.display().to_string());

    let Ok(mut write_half) = stream.try_clone() else { return };
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
    loop {
        let request: ControlRequest = match read_frame(&mut read_half, CONTROL_FRAME_MAX_BYTES) {
            Ok(request) => request,
            Err(threadspace_relay::frame::FrameError::Malformed(error)) => {
                log::warn("REQUEST_MALFORMED", json!({ "connectionId": connection_id, "error": error.to_string() }));
                break;
            }
            Err(_) => break,
        };
        dispatch(request, role, connection_id, context, &outbound, &slow);
    }
    let _ = context.writer.send(WriterCommand::ConnectionClosed { connection_id });
    log::info("CLIENT_DISCONNECTED", json!({ "connectionId": connection_id }));
    drop(outbound);
    let _ = writer_thread.join();
}

fn hello(stream: &mut impl Read, outbound: &Outbound, context: &CoreContext) -> Option<ClientRole> {
    let request: ControlRequest = read_frame(stream, CONTROL_FRAME_MAX_BYTES).ok()?;
    let ControlRequestBody::Hello { protocol_version, role, expected_core_generation } = request.body else {
        respond(outbound, request.request_id, Err(ControlError::new(ControlErrorCode::HelloRequired, "Hello must be first")));
        return None;
    };
    let refusal = if protocol_version != CONTROL_PROTOCOL_VERSION {
        Some(ControlError::new(ControlErrorCode::UnsupportedProtocol, "unsupported control protocol"))
    } else if expected_core_generation != context.core_generation {
        Some(ControlError::new(ControlErrorCode::StaleGeneration, "stale core generation"))
    } else if !role_permitted(role) {
        Some(ControlError::new(ControlErrorCode::RoleNotPermitted, "role not permitted by this build"))
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
        respond(outbound, request_id, Err(ControlError::new(ControlErrorCode::Busy, "writer queue full")));
    }
}

fn dispatch(
    request: ControlRequest,
    #[cfg_attr(not(feature = "qualification"), allow(unused_variables))] role: ClientRole,
    connection_id: u64,
    context: &CoreContext,
    outbound: &Outbound,
    slow: &Arc<AtomicUsize>,
) {
    let request_id = request.request_id;
    let outbound_clone = outbound.clone();
    match request.body {
        ControlRequestBody::Hello { .. } => {
            respond(outbound, request_id, Err(ControlError::new(ControlErrorCode::BadRequest, "already greeted")));
        }
        ControlRequestBody::AttachView { subscription_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::AttachView { connection_id, request_id, subscription_id, outbound: outbound_clone },
        ),
        ControlRequestBody::DetachView { subscription_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::DetachView { request_id, subscription_id, outbound: outbound_clone },
        ),
        ControlRequestBody::ViewHydrated { subscription_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::ViewHydrated { request_id, subscription_id, outbound: outbound_clone },
        ),
        ControlRequestBody::IntentConsumed { intent_id } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::IntentConsumed { request_id, intent_id, outbound: outbound_clone },
        ),
        ControlRequestBody::AcknowledgeAttention { command_id, attention_id, expected_revision } => to_writer(
            context,
            outbound,
            request_id,
            WriterCommand::Acknowledge { request_id, command_id, attention_id, expected_revision, outbound: outbound_clone },
        ),
        #[cfg(feature = "qualification")]
        ControlRequestBody::QualifyRaiseAttention { label } => {
            if role != ClientRole::Qualification {
                respond(outbound, request_id, Err(ControlError::new(ControlErrorCode::RoleNotPermitted, "qualification role required")));
                return;
            }
            let label: String = label.chars().take(threadspace_contracts::limits::LABEL_MAX_CHARS).collect();
            to_writer(context, outbound, request_id, WriterCommand::RaiseAttention { request_id, label, outbound: outbound_clone });
        }
        body @ (ControlRequestBody::Diagnostics
        | ControlRequestBody::IntegrationStatus
        | ControlRequestBody::RequestNotificationAuthorization
        | ControlRequestBody::RequestTerminalAutomation) => {
            if slow.fetch_add(1, Ordering::AcqRel) >= MAX_SLOW_IN_FLIGHT {
                slow.fetch_sub(1, Ordering::AcqRel);
                respond(outbound, request_id, Err(ControlError::new(ControlErrorCode::Busy, "too many native operations in flight")));
                return;
            }
            let in_flight = Arc::clone(slow);
            let writer = context.writer.clone();
            let snapshot = native_ops::OpsContext::from(context);
            let spawned = thread::Builder::new().name("native-op".into()).spawn(move || {
                let result = native_ops::run(body, &snapshot, &writer);
                respond(&outbound_clone, request_id, result);
                in_flight.fetch_sub(1, Ordering::AcqRel);
            });
            if spawned.is_err() {
                slow.fetch_sub(1, Ordering::AcqRel);
                respond(outbound, request_id, Err(ControlError::new(ControlErrorCode::Internal, "could not start native operation")));
            }
        }
    }
}
