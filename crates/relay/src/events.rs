//! The capture side of the event socket (SPEC §8.1–§8.2): one bounded batch
//! out, one receipt or refusal back, within the caller's deadline. The
//! socket can only admit observations; it executes nothing.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use threadspace_contracts::canonical::capture::{CAPTURE_PROTOCOL_VERSION, CaptureBatch, CaptureReply};
use threadspace_contracts::limits::capture::FRAME_MAX_BYTES;

use crate::frame::{FrameError, read_frame, write_frame};

#[derive(Debug)]
pub enum SendError {
    Connect(io::Error),
    Frame(FrameError),
    Deadline,
    Protocol,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(error) => write!(f, "connect: {error}"),
            Self::Frame(error) => write!(f, "{error}"),
            Self::Deadline => f.write_str("receipt deadline passed"),
            Self::Protocol => f.write_str("protocol mismatch"),
        }
    }
}

/// Sends a batch and waits at most until `deadline` for its reply.
pub fn send(socket: &Path, batch: &CaptureBatch, deadline: Instant) -> Result<CaptureReply, SendError> {
    let remaining = || {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(SendError::Deadline)
    };
    let mut stream = UnixStream::connect(socket).map_err(SendError::Connect)?;
    stream
        .set_write_timeout(Some(remaining()?))
        .map_err(SendError::Connect)?;
    write_frame(&mut stream, batch, FRAME_MAX_BYTES).map_err(SendError::Frame)?;
    stream
        .set_read_timeout(Some(remaining()?.max(Duration::from_millis(1))))
        .map_err(SendError::Connect)?;
    let reply: CaptureReply = read_frame(&mut stream, FRAME_MAX_BYTES).map_err(|error| match error {
        FrameError::Io(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
            SendError::Deadline
        }
        other => SendError::Frame(other),
    })?;
    let version = match &reply {
        CaptureReply::Receipts { protocol_version, .. } | CaptureReply::Refused { protocol_version, .. } => {
            *protocol_version
        }
    };
    if version != CAPTURE_PROTOCOL_VERSION {
        return Err(SendError::Protocol);
    }
    Ok(reply)
}
