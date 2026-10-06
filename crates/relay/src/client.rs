//! The verifying control client. A connection is accepted only when the live
//! socket peer is this user's current companion incarnation named by the
//! locator: same effective UID, the locator's PID with the same kernel birth
//! and executable, and a `HelloAck` carrying the locator's core generation.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use threadspace_contracts::control::{
    CONTROL_PROTOCOL_VERSION, ClientRole, ControlError, ControlMessage, ControlOutcome,
    ControlRequest, ControlRequestBody, ControlResponseBody,
};
use threadspace_contracts::diagnostics::ProcessIdentity;
use threadspace_contracts::limits::CONTROL_FRAME_MAX_BYTES;
use threadspace_surfaces_macos::process;

use crate::frame::{FrameError, read_frame, write_frame};
use crate::locator::{self, LocatorError, RuntimeLocator};
use crate::peer::{PeerCredentials, current_euid, peer_credentials};

#[derive(Debug)]
pub enum ClientError {
    Locator(LocatorError),
    Connect(io::Error),
    PeerRejected(String),
    Frame(FrameError),
    Rejected(ControlError),
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Locator(error) => write!(f, "{error}"),
            Self::Connect(error) => write!(f, "connect: {error}"),
            Self::PeerRejected(detail) => write!(f, "companion peer rejected: {detail}"),
            Self::Frame(error) => write!(f, "{error}"),
            Self::Rejected(error) => write!(f, "companion refused: {error}"),
            Self::Protocol(detail) => write!(f, "protocol: {detail}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<FrameError> for ClientError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloInfo {
    pub core_generation: String,
    pub store_generation: String,
    pub companion: ProcessIdentity,
}

#[derive(Debug)]
pub struct VerifiedConnection {
    pub stream: UnixStream,
    pub hello: HelloInfo,
    pub peer: PeerCredentials,
    pub locator: RuntimeLocator,
}

/// Checks the socket peer against the locator before any request is sent.
pub fn verify_peer(peer: &PeerCredentials, locator: &RuntimeLocator) -> Result<(), ClientError> {
    if peer.euid != current_euid() {
        return Err(ClientError::PeerRejected(format!(
            "peer euid {} is not ours",
            peer.euid
        )));
    }
    let expected = &locator.companion;
    if peer.pid < 0 || peer.pid as u32 != expected.pid {
        return Err(ClientError::PeerRejected(format!(
            "peer pid {} is not locator pid {}",
            peer.pid, expected.pid
        )));
    }
    let sample =
        process::sample(peer.pid).map_err(|error| ClientError::PeerRejected(error.to_string()))?;
    if sample.start_seconds.to_string() != expected.start_seconds
        || sample.start_microseconds != expected.start_microseconds
    {
        return Err(ClientError::PeerRejected(
            "peer process incarnation differs from locator".into(),
        ));
    }
    let executable = process::executable_path(peer.pid)
        .map_err(|error| ClientError::PeerRejected(error.to_string()))?;
    if executable.to_string_lossy() != expected.executable_path {
        return Err(ClientError::PeerRejected(
            "peer executable differs from locator".into(),
        ));
    }
    Ok(())
}

pub fn connect(
    locator_path: &Path,
    role: ClientRole,
    timeout: Duration,
) -> Result<VerifiedConnection, ClientError> {
    let locator = locator::read(locator_path).map_err(ClientError::Locator)?;
    let mut stream = UnixStream::connect(&locator.control_socket).map_err(ClientError::Connect)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(ClientError::Connect)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(ClientError::Connect)?;
    let peer = peer_credentials(&stream).map_err(ClientError::Connect)?;
    verify_peer(&peer, &locator)?;

    let hello = ControlRequest {
        request_id: 0,
        body: ControlRequestBody::Hello {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            role,
            expected_core_generation: locator.core_generation.clone(),
        },
    };
    write_frame(&mut stream, &hello, CONTROL_FRAME_MAX_BYTES)?;
    let reply: ControlMessage = read_frame(&mut stream, CONTROL_FRAME_MAX_BYTES)?;
    let body = match reply {
        ControlMessage::Response {
            request_id: 0,
            outcome: ControlOutcome::Ok(body),
        } => *body,
        ControlMessage::Response {
            outcome: ControlOutcome::Err(error),
            ..
        } => return Err(ClientError::Rejected(error)),
        other => {
            return Err(ClientError::Protocol(format!(
                "unexpected hello reply {other:?}"
            )));
        }
    };
    let ControlResponseBody::HelloAck {
        protocol_version,
        core_generation,
        store_generation,
        companion,
    } = body
    else {
        return Err(ClientError::Protocol("hello was not acknowledged".into()));
    };
    if protocol_version != CONTROL_PROTOCOL_VERSION || core_generation != locator.core_generation {
        return Err(ClientError::PeerRejected("stale core generation".into()));
    }
    if companion != locator.companion {
        return Err(ClientError::PeerRejected(
            "companion identity differs from locator".into(),
        ));
    }
    Ok(VerifiedConnection {
        stream,
        hello: HelloInfo {
            core_generation,
            store_generation,
            companion,
        },
        peer,
        locator,
    })
}

/// A single-threaded request/response client (used by qualification tools).
/// Pushed patches and intents received while waiting are discarded.
#[derive(Debug)]
pub struct BlockingClient {
    connection: VerifiedConnection,
    next_request_id: u64,
}

impl BlockingClient {
    pub fn new(connection: VerifiedConnection) -> Self {
        Self {
            connection,
            next_request_id: 1,
        }
    }

    pub fn hello(&self) -> &HelloInfo {
        &self.connection.hello
    }

    pub fn request(
        &mut self,
        body: ControlRequestBody,
    ) -> Result<ControlResponseBody, ClientError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        write_frame(
            &mut self.connection.stream,
            &ControlRequest { request_id, body },
            CONTROL_FRAME_MAX_BYTES,
        )?;
        loop {
            let message: ControlMessage =
                read_frame(&mut self.connection.stream, CONTROL_FRAME_MAX_BYTES)?;
            if let ControlMessage::Response {
                request_id: id,
                outcome,
            } = message
                && id == request_id
            {
                return match outcome {
                    ControlOutcome::Ok(body) => Ok(*body),
                    ControlOutcome::Err(error) => Err(ClientError::Rejected(error)),
                };
            }
        }
    }

    /// Lengthens the read timeout for requests that wait on the owner (for
    /// example a native permission prompt).
    pub fn set_read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.connection.stream.set_read_timeout(Some(timeout))
    }
}
