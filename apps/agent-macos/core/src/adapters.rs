//! Provider adapter registry for captured envelopes (SPEC §10): pure
//! normalization by the envelope's adapter. An unknown adapter's envelope is
//! retained with an empty payload and drives nothing.

use serde_json::json;
use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_state_engine::normalize::Normalized;

pub fn normalize(envelope: &ObservationEnvelope) -> Normalized {
    match envelope.adapter_id.as_str() {
        threadspace_provider_claude::hooks::ADAPTER_ID => {
            threadspace_provider_claude::hooks::normalize(envelope)
        }
        threadspace_provider_claude::observer::ADAPTER_ID => {
            threadspace_provider_claude::observer::normalize(envelope)
        }
        // The synthetic provider exists only in qualification builds.
        #[cfg(feature = "qualification")]
        threadspace_state_engine::synthetic::ADAPTER_ID => {
            threadspace_state_engine::synthetic::normalize_envelope(envelope)
        }
        other => Normalized {
            drafts: Vec::new(),
            retained: json!({}),
            unsupported: Some(format!("UNSUPPORTED: adapter {}", other.chars().take(64).collect::<String>())),
        },
    }
}
