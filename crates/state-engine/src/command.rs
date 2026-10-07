//! Owner commands as resolved facts (SPEC §5.5, §7.2). Admission validates a
//! command against the current item and records its result; this builds the
//! one fact every admission path journals for it, so the pure harness and the
//! SQLite journal reduce identical entries.

use sha2::{Digest, Sha256};
use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
use threadspace_contracts::canonical::fact::{
    CanonicalRefs, EvidenceClass, FactPayload, NativeRefs, ResolvedFact,
};
use threadspace_contracts::canonical::FACT_PAYLOAD_VERSION;

/// The source every owner command is journaled under.
pub const OWNER_SOURCE: &str = "owner";

/// The immutable fingerprint of a command's payload: a retry with the same
/// fingerprint returns the committed result; a different one is a conflict.
/// The expected revision is part of the request, so it is part of the print.
pub fn fingerprint(command: &OwnerCommand) -> String {
    let payload = serde_json::to_vec(command).unwrap_or_default();
    Sha256::digest(&payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn fact(
    fact_id: String,
    observation_id: &str,
    session_id: &str,
    command: &OwnerCommand,
    at_ms: i64,
) -> ResolvedFact {
    let payload = match &command.action {
        OwnerAction::Acknowledge => FactPayload::AttentionAcknowledged {
            command_id: command.command_id.clone(),
            at_ms,
        },
        OwnerAction::Resolve { reason } => FactPayload::AttentionResolved {
            command_id: command.command_id.clone(),
            at_ms,
            reason: reason.clone(),
        },
        OwnerAction::Snooze { until_ms } => FactPayload::AttentionSnoozed {
            command_id: command.command_id.clone(),
            at_ms,
            until_ms: *until_ms,
        },
    };
    ResolvedFact {
        fact_id,
        observation_id: observation_id.to_owned(),
        fact_index: 0,
        refs: CanonicalRefs {
            session_id: Some(session_id.to_owned()),
            attention_id: Some(command.attention_id.clone()),
            ..CanonicalRefs::default()
        },
        native: NativeRefs {
            attention: Some(command.attention_id.clone()),
            ..NativeRefs::default()
        },
        provenance: EvidenceClass::OwnerCommand,
        causal: None,
        payload_version: FACT_PAYLOAD_VERSION,
        payload,
    }
}
