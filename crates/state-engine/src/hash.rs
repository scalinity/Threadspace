//! Exact replay digests. Serialized state is canonical: struct fields in
//! declaration order, every map a `BTreeMap` (sorted keys), no floats.

use serde::Serialize;
use sha2::{Digest, Sha256};
use threadspace_contracts::canonical::fact::JournalEntry;
use threadspace_contracts::canonical::records::CanonicalState;

pub fn canonical_json<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The exact digest of a canonical state, IDs and cursors included.
pub fn state_hash(state: &CanonicalState) -> String {
    sha256_hex(&canonical_json(state))
}

/// A running digest over journal entries in cursor order.
#[derive(Debug, Clone)]
pub struct JournalDigest {
    hasher: Sha256,
    pub entries: u64,
    pub facts: u64,
    pub first_cursor: Option<i64>,
    pub last_cursor: Option<i64>,
}

impl Default for JournalDigest {
    fn default() -> Self {
        Self {
            hasher: Sha256::new(),
            entries: 0,
            facts: 0,
            first_cursor: None,
            last_cursor: None,
        }
    }
}

impl JournalDigest {
    pub fn add(&mut self, entry: &JournalEntry) {
        self.hasher.update(canonical_json(entry));
        self.hasher.update(b"\n");
        self.entries += 1;
        self.facts += entry.facts.len() as u64;
        self.first_cursor.get_or_insert(entry.cursor);
        self.last_cursor = Some(entry.cursor);
    }

    pub fn finish(self) -> String {
        self.hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
