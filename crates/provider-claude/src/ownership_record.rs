//! Independent reload ownership proof (M2 F1B).
//!
//! The observer's host read is a claim. This module checks the helper's
//! actual parent ProcessKey/image against two fresh native inventory reads
//! and kernel samples. The helper creates a token only after this succeeds,
//! durably captures this envelope, and then returns the token. The observer
//! may seal it only while the original immutable Session generation is still
//! current. Only a qualified native Turn started after that seal may use it.
//! Neither a historical attachment nor an interceptable process result is a
//! proof. No inventory row is interpreted as a Turn or actor lifecycle.

use serde::Deserialize;
use serde_json::json;
use threadspace_contracts::canonical::envelope::{
    CaptureClock, OBSERVATION_SCHEMA_VERSION, ObservationEnvelope, ProcessRole, ProcessSample,
};
use threadspace_contracts::canonical::fact::{
    EvidenceClass, FactPayload, NativeFactDraft, NativeRefs, ObserverOwnership,
};
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_state_engine::normalize::{Normalized, retain};

use crate::profiles::{observer_profile, version_from_executable};

pub const ADAPTER_ID: &str = "threadspace-observer-ownership";
pub const SOURCE_ID: &str = "claude.observer.ownership";
pub const EVENT: &str = "ownership.proven";
pub const PROTOCOL_VERSION: u32 = 1;
const KEYS: &[&str] = &[
    "sourceEpoch", "sessionGeneration", "proofToken", "inventoryStartMs", "inventoryEndMs",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeRequest {
    pub protocol_version: u32,
    pub source_epoch: String,
    pub session_generation: u64,
    pub session_id: String,
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && value.bytes().all(|b| b.is_ascii_graphic())
}

impl ProbeRequest {
    pub fn valid(&self) -> bool {
        self.protocol_version == PROTOCOL_VERSION
            && identifier(&self.source_epoch)
            && identifier(&self.session_id)
            && self.session_generation > 0
    }
}

/// The caller supplies a fresh, native-created token after `corroborate`
/// succeeds. No record supplied by observer middleware is reused as proof.
#[allow(clippy::too_many_arguments)]
pub fn envelope(
    request: &ProbeRequest,
    provider: ProcessSample,
    profile_ref: String,
    clock: CaptureClock,
    observation_id: String,
    proof_token: String,
    interval: (i64, i64),
) -> ObservationEnvelope {
    ObservationEnvelope {
        schema_version: OBSERVATION_SCHEMA_VERSION,
        observation_id,
        source_id: SOURCE_ID.into(),
        source_epoch: proof_token.clone(),
        source_sequence: None,
        sequence_meaning: None,
        callback_entry_sequence: None,
        callback_result_sequence: None,
        adapter_id: ADAPTER_ID.into(),
        adapter_version: "1".into(),
        provider_version: provider.executable.as_deref().and_then(version_from_executable),
        native_event: EVENT.into(),
        session_key: Some(NativeSessionRef {
            provider: "claude".into(),
            profile_ref,
            native_session_id: request.session_id.clone(),
        }),
        actor_native_id: None,
        native_turn_id: None,
        native_prompt_id: None,
        native_occurrence_id: None,
        activation_ref: None,
        captured_at: clock,
        evidence: vec![provider],
        payload: json!({
            "sourceEpoch": request.source_epoch,
            "sessionGeneration": request.session_generation,
            "proofToken": proof_token,
            "inventoryStartMs": interval.0,
            "inventoryEndMs": interval.1,
        }),
    }
}

pub fn normalize(envelope: &ObservationEnvelope) -> Normalized {
    let retained = retain(&envelope.payload, KEYS);
    let proof = || -> Option<Vec<NativeFactDraft>> {
        if envelope.source_id != SOURCE_ID || envelope.native_event != EVENT {
            return None;
        }
        let provider = envelope.evidence.iter().find(|p| p.role == ProcessRole::Provider)?;
        let executable = provider.executable.clone()?;
        observer_profile(&version_from_executable(&executable)?)?;
        let source_epoch = retained.get("sourceEpoch")?.as_str()?;
        let proof_token = retained.get("proofToken")?.as_str()?;
        let session_generation = retained.get("sessionGeneration")?.as_u64()?;
        if !identifier(source_epoch) || !identifier(proof_token) || session_generation == 0 {
            return None;
        }
        let refs = NativeRefs {
            session: Some(envelope.session_key.clone()?),
            process: Some(provider.key.clone()),
            observer_ownership: Some(ObserverOwnership {
                source_epoch: source_epoch.into(),
                session_generation,
                proof_token: proof_token.into(),
                executable_identity: executable.clone(),
            }),
            ..NativeRefs::default()
        };
        Some(vec![
            NativeFactDraft {
                refs: refs.clone(),
                provenance: EvidenceClass::Kernel,
                causal: None,
                payload: FactPayload::ProcessObserved { executable_identity: executable },
            },
            NativeFactDraft {
                refs,
                provenance: EvidenceClass::ProviderSnapshot,
                causal: None,
                payload: FactPayload::ObserverOwnershipCorroborated {},
            },
        ])
    }();
    Normalized {
        unsupported: proof.is_none().then(|| "UNQUALIFIED_OWNERSHIP_PROOF".into()),
        drafts: proof.unwrap_or_default(),
        retained,
    }
}

