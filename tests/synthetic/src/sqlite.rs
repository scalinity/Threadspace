//! The real admission path for synthetic histories: an on-disk single-writer
//! journal (SQLite 3.53.4, WAL, synchronous=FULL) in a disposable directory,
//! with a seeded allocator so a run's journal is reproducible byte for byte.

use std::path::{Path, PathBuf};

use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::command::OwnerCommand;
use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_contracts::canonical::fact::Delivery;
use threadspace_contracts::canonical::records::CanonicalState;
use threadspace_contracts::ui::ReceiptStatus;
use threadspace_journal::{EnvelopeAdmission, Journal, JournalError};
use threadspace_state_engine::ids::SeededAllocator;
use threadspace_state_engine::synthetic::normalize_envelope;

use crate::runner::{Admit, Receipt};

/// A disposable store directory, removed on drop unless kept.
pub struct TempStore {
    pub dir: PathBuf,
    keep: bool,
}

impl TempStore {
    pub fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "threadspace-m1-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp store");
        Self { dir, keep: false }
    }

    pub fn at(dir: &Path) -> Self {
        std::fs::create_dir_all(dir).expect("create store");
        Self {
            dir: dir.to_path_buf(),
            keep: true,
        }
    }

    pub fn journal_path(&self) -> PathBuf {
        self.dir.join("journal.sqlite3")
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

pub struct SqliteRunner {
    pub journal: Journal,
    pub store: TempStore,
    pub now_ms: i64,
    pub delivery: Delivery,
}

impl SqliteRunner {
    pub fn open(store: TempStore, seed: u64) -> Result<Self, JournalError> {
        let journal = Journal::open_with(
            &store.journal_path(),
            "synthetic-core-1",
            crate::rng::VirtualClock::EPOCH_MS,
            Box::new(SeededAllocator::new(seed)),
            false,
        )?;
        Ok(Self {
            journal,
            store,
            now_ms: crate::rng::VirtualClock::EPOCH_MS,
            delivery: Delivery::Live,
        })
    }

    /// Closes and reopens the same store: a process restart.
    pub fn restart(self, seed: u64) -> Result<Self, JournalError> {
        let Self {
            journal,
            store,
            now_ms,
            delivery,
        } = self;
        drop(journal);
        let journal = Journal::open_with(
            &store.journal_path(),
            "synthetic-core-2",
            now_ms,
            Box::new(SeededAllocator::new(seed)),
            false,
        )?;
        Ok(Self {
            journal,
            store,
            now_ms,
            delivery,
        })
    }
}

impl Admit for SqliteRunner {
    fn observe(&mut self, envelope: &ObservationEnvelope) -> Receipt {
        self.now_ms += 1;
        let admission = EnvelopeAdmission {
            envelope,
            normalized: normalize_envelope(envelope),
        };
        match self
            .journal
            .admit_batch(&[admission], self.delivery, self.now_ms)
        {
            Ok(outcome) => {
                let record = outcome.records.into_iter().next();
                Receipt {
                    status: record.as_ref().map_or(RecordStatus::NotAccepted, |r| r.status),
                    cursor: record.as_ref().and_then(|r| r.cursor),
                    reason: record.and_then(|r| r.reason),
                }
            }
            Err(error) => Receipt {
                status: RecordStatus::NotAccepted,
                cursor: None,
                reason: Some(error.to_string()),
            },
        }
    }

    fn owner(&mut self, command: &OwnerCommand, at_ms: i64) -> Receipt {
        self.now_ms += 1;
        match self.journal.admit_owner_command(command, at_ms) {
            Ok(outcome) => Receipt {
                status: match outcome.receipt.status {
                    ReceiptStatus::Committed => RecordStatus::Committed,
                    ReceiptStatus::AlreadyCommitted => RecordStatus::AlreadyCommitted,
                },
                cursor: outcome.receipt.cursor.parse().ok(),
                reason: None,
            },
            Err(error) => Receipt {
                status: RecordStatus::NotAccepted,
                cursor: None,
                reason: Some(error.to_string()),
            },
        }
    }

    fn state(&self) -> &CanonicalState {
        self.journal.canonical_state()
    }
}
