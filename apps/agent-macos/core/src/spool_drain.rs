//! Drains the local capture spool into the journal (SPEC §8.4): ready
//! records are admitted as catch-up deliveries and removed only after their
//! durable receipt; a record the journal refuses is quarantined, never
//! deleted; expired records and saturation markers become a recorded
//! coverage gap. Drained only while capture admission is open.

use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::Duration;

use serde_json::json;
use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::fact::Delivery;
use threadspace_contracts::limits::capture::SPOOL_MAX_AGE_MS;
use threadspace_relay::spool::Spool;

use crate::log;
use crate::state::RUNTIME;
use crate::writer::WriterCommand;

const INTERVAL: Duration = Duration::from_secs(2);
const BATCH: usize = 64;
const PER_PASS: usize = 1024;
const COMMIT_WAIT: Duration = Duration::from_secs(10);

pub fn spawn(store_dir: PathBuf, writer: SyncSender<WriterCommand>) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().name("spool-drain".into()).spawn(move || {
        let spool = Spool::at(&store_dir);
        loop {
            if RUNTIME.admission_open() {
                drain(&spool, &writer);
            }
            thread::sleep(INTERVAL);
        }
    })
}

/// One drain pass; returns the number of records whose receipt committed.
pub fn drain(spool: &Spool, writer: &SyncSender<WriterCommand>) -> usize {
    let _ = spool.sweep_pending(Duration::from_secs(3600));
    let expired = spool
        .expired(Duration::from_millis(SPOOL_MAX_AGE_MS.unsigned_abs()))
        .unwrap_or_default();
    let dropped = spool.dropped().unwrap_or_default();
    if !expired.is_empty() || !dropped.is_empty() {
        // The loss is journaled before its evidence leaves the spool; until
        // then nothing drains, so an expired record is never delivered.
        let (reply_tx, reply_rx) = sync_channel(1);
        let recorded = writer
            .send(WriterCommand::RecordCaptureLoss {
                dropped: dropped.clone(),
                expired: expired.len(),
                reply: reply_tx,
            })
            .is_ok()
            && reply_rx.recv_timeout(COMMIT_WAIT).unwrap_or(false);
        if !recorded {
            return 0;
        }
        spool.clear_dropped(&dropped);
        for record in &expired {
            let _ = spool.quarantine(record, "expired");
        }
    }
    let Ok(ready) = spool.ready(PER_PASS) else { return 0 };
    let mut committed = 0;
    for chunk in ready.chunks(BATCH) {
        let mut records = Vec::new();
        let mut envelopes = Vec::new();
        for record in chunk {
            match spool.read(record) {
                Ok(envelope) => {
                    records.push(record.clone());
                    envelopes.push(envelope);
                }
                Err(_) => {
                    let _ = spool.quarantine(record, "unreadable");
                }
            }
        }
        if envelopes.is_empty() {
            continue;
        }
        let (reply_tx, reply_rx) = sync_channel(1);
        if writer
            .send(WriterCommand::AdmitCaptured {
                envelopes,
                delivery: Delivery::Catchup,
                reply: reply_tx,
            })
            .is_err()
        {
            return committed;
        }
        let Ok(Ok(receipts)) = reply_rx.recv_timeout(COMMIT_WAIT) else {
            return committed;
        };
        for record in &records {
            match receipts.iter().find(|r| r.observation_id == record.observation_id).map(|r| r.status) {
                Some(RecordStatus::Committed | RecordStatus::AlreadyCommitted) => {
                    if spool.remove(record).is_ok() {
                        committed += 1;
                    }
                }
                Some(RecordStatus::NotAccepted) => {
                    let _ = spool.quarantine(record, "not-accepted");
                }
                _ => {}
            }
        }
    }
    if committed > 0 {
        log::info("SPOOL_DRAINED", json!({ "committed": committed }));
    }
    committed
}
