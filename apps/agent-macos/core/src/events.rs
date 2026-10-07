//! The private capture event socket (SPEC §8.1, §8.2). It admits observation
//! batches through the single writer and answers with per-record receipts
//! sent only after commit; it executes no focus, shell, approval or provider
//! action. A closed admission, a full writer queue or a malformed frame is a
//! typed refusal the sender answers by spooling.

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::canonical::capture::{
    CAPTURE_PROTOCOL_VERSION, CaptureBatch, CaptureRefusal, CaptureReply,
};
use threadspace_contracts::canonical::fact::Delivery;
use threadspace_contracts::limits::capture::{BATCH_MAX_RECORDS, FRAME_MAX_BYTES};
use threadspace_relay::frame::{FrameError, read_frame, write_frame};
use threadspace_relay::peer::{current_euid, peer_credentials};

use crate::log;
use crate::state::RUNTIME;
use crate::writer::WriterCommand;

/// Concurrent capture connections; more are closed and their senders spool.
const MAX_CONNECTIONS: usize = 64;
const IO_TIMEOUT: Duration = Duration::from_secs(1);
const COMMIT_WAIT: Duration = Duration::from_secs(2);

static OPEN: AtomicUsize = AtomicUsize::new(0);

fn refuse(code: CaptureRefusal) -> CaptureReply {
    CaptureReply::Refused {
        protocol_version: CAPTURE_PROTOCOL_VERSION,
        code,
    }
}

pub fn spawn(listener: UnixListener, writer: SyncSender<WriterCommand>) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().name("events-accept".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            if OPEN.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                OPEN.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            let writer = writer.clone();
            let spawned = thread::Builder::new().name("events-conn".into()).spawn(move || {
                serve(stream, &writer);
                OPEN.fetch_sub(1, Ordering::AcqRel);
            });
            if spawned.is_err() {
                OPEN.fetch_sub(1, Ordering::AcqRel);
            }
        }
    })
}

fn serve(mut stream: UnixStream, writer: &SyncSender<WriterCommand>) {
    match peer_credentials(&stream) {
        Ok(peer) if peer.euid == current_euid() => {}
        _ => return,
    }
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let reply = match read_frame::<_, CaptureBatch>(&mut stream, FRAME_MAX_BYTES) {
        Err(FrameError::TooLarge { .. }) => refuse(CaptureRefusal::TooLarge),
        Err(FrameError::Malformed(_)) => refuse(CaptureRefusal::Malformed),
        Err(_) => return,
        Ok(batch) if batch.protocol_version != CAPTURE_PROTOCOL_VERSION => {
            refuse(CaptureRefusal::ProtocolMismatch)
        }
        Ok(batch) if batch.records.len() > BATCH_MAX_RECORDS => refuse(CaptureRefusal::TooLarge),
        Ok(_) if !RUNTIME.admission_open() => refuse(CaptureRefusal::AdmissionClosed),
        Ok(batch) => admit(batch, writer),
    };
    let _ = write_frame(&mut stream, &reply, FRAME_MAX_BYTES);
}

fn admit(batch: CaptureBatch, writer: &SyncSender<WriterCommand>) -> CaptureReply {
    let (reply_tx, reply_rx) = sync_channel(1);
    let command = WriterCommand::AdmitCaptured {
        envelopes: batch.records,
        delivery: Delivery::Live,
        reply: reply_tx,
    };
    match writer.try_send(command) {
        Ok(()) => {}
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
            return refuse(CaptureRefusal::Busy);
        }
    }
    match reply_rx.recv_timeout(COMMIT_WAIT) {
        Ok(Ok(receipts)) => CaptureReply::Receipts {
            protocol_version: CAPTURE_PROTOCOL_VERSION,
            receipts,
        },
        Ok(Err(code)) => refuse(code),
        Err(_) => {
            log::warn("CAPTURE_COMMIT_WAIT_EXPIRED", json!({}));
            refuse(CaptureRefusal::Busy)
        }
    }
}
