//! Capture-scoped activation evidence shared by the two real Claude adapters.
use threadspace_contracts::canonical::envelope::{ObservationEnvelope, ProcessSample};
use threadspace_contracts::canonical::fact::{
    ActivationEvidenceKind, AttachedPresence, EvidenceClass, ExecutionMode, FactPayload,
    NativeActivation, NativeFactDraft, NativeRefs,
};
use threadspace_contracts::canonical::keys::NativeExecutionRef;

pub fn source_starts(source: Option<&str>) -> bool {
    matches!(source, Some("startup" | "resume" | "clear"))
}

pub fn evidence(
    envelope: &ObservationEnvelope,
    kind: ActivationEvidenceKind,
    generation: Option<String>,
) -> NativeActivation {
    NativeActivation {
        kind,
        entry_sequence: envelope.callback_entry_sequence.clone(),
        start_source: envelope
            .payload
            .get("source")
            .or_else(|| envelope.payload.get("detail").and_then(|d| d.get("source")))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        generation,
    }
}

pub fn attach(
    envelope: &ObservationEnvelope,
    mut refs: NativeRefs,
    generation: Option<String>,
    provider: Option<&ProcessSample>,
) -> Vec<NativeFactDraft> {
    let Some(provider) = provider else {
        return Vec::new();
    };
    let Some(executable) = &provider.executable else {
        return Vec::new();
    };
    refs.process = Some(provider.key.clone());
    refs.actor = None;
    refs.turn = None;
    refs.execution = Some(NativeExecutionRef::Activation {
        activation_ref: generation.as_ref().zip(envelope.callback_entry_sequence.as_ref())
            .map(|(generation,entry)|format!("observer:{generation}:{entry}"))
            .or_else(||envelope.activation_ref.clone())
            .unwrap_or_else(|| format!("claude-start:{}", envelope.observation_id)),
    });
    refs.activation = Some(evidence(
        envelope,
        ActivationEvidenceKind::Start,
        generation,
    ));
    vec![
        NativeFactDraft {
            refs: NativeRefs {
                process: refs.process.clone(),
                ..NativeRefs::default()
            },
            provenance: EvidenceClass::Kernel,
            causal: None,
            payload: FactPayload::ProcessObserved {
                executable_identity: executable.clone(),
            },
        },
        NativeFactDraft {
            refs,
            provenance: EvidenceClass::ProviderEvent,
            causal: None,
            payload: FactPayload::ExecutionAttached {
                mode: if provider.controlling_device.is_some() {
                    ExecutionMode::TerminalEmbedded
                } else {
                    ExecutionMode::Headless
                },
                presence: AttachedPresence::Live,
                native_runtime_id: None,
                controlling_device: provider.controlling_device,
            },
        },
    ]
}
