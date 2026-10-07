//! The synthetic provider adapter (SPEC §21.3): native-shaped events with the
//! provider's event names, normalized purely into native-keyed drafts. Its
//! envelopes carry the same identifiers a real adapter would (session, actor,
//! turn, prompt and occurrence IDs, activation refs, process samples). It is
//! compiled only into qualification builds and the synthetic harness.

use serde_json::Value;
use threadspace_contracts::canonical::causal::CausalPoint;
use threadspace_contracts::canonical::envelope::{ObservationEnvelope, ProcessRole};
use threadspace_contracts::canonical::fact::{
    AcceptanceProof, ActivityResult, ActorRelationKind, ActorRole, AttachedPresence,
    BindingMethod, BindingProof, EvidenceClass, ExecutionMode, FactPayload, InputOrigin,
    NativeFactDraft, NativeRefs, SnapshotInterval, SnapshotRow, TurnOutcome, WaitCategory,
    WaitSignal,
};
use threadspace_contracts::canonical::keys::{
    NativeActorRef, NativeExecutionRef, NativeSurfaceRef,
};
use threadspace_contracts::projection::ObservationState;
use threadspace_contracts::route::ProcessKey;

pub const ADAPTER_ID: &str = "synthetic";
pub const ADAPTER_VERSION: &str = "1";

/// Why an envelope produced no drafts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizeError {
    /// An unknown native event or discriminator: retained as unsupported.
    Unsupported(String),
    /// A known event whose payload is missing or ill-typed.
    Malformed(&'static str),
}

fn field<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
    payload.get(name).filter(|v| !v.is_null())
}

fn string(payload: &Value, name: &str) -> Option<String> {
    field(payload, name).and_then(Value::as_str).map(str::to_owned)
}

fn required(payload: &Value, name: &'static str) -> Result<String, NormalizeError> {
    string(payload, name).ok_or(NormalizeError::Malformed(name))
}

fn typed<T: serde::de::DeserializeOwned>(payload: &Value, name: &'static str) -> Result<T, NormalizeError> {
    let value = field(payload, name).ok_or(NormalizeError::Malformed(name))?;
    serde_json::from_value(value.clone()).map_err(|_| NormalizeError::Unsupported(format!("{name}: unknown value")))
}

fn optional<T: serde::de::DeserializeOwned>(payload: &Value, name: &'static str) -> Result<Option<T>, NormalizeError> {
    match field(payload, name) {
        None => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| NormalizeError::Unsupported(format!("{name}: unknown value"))),
    }
}

/// The envelope's causal point when its source supplies a qualified sequence.
fn causal(envelope: &ObservationEnvelope) -> Option<CausalPoint> {
    envelope.sequence_meaning?;
    let sequence = envelope.source_sequence.clone()?;
    Some(CausalPoint {
        source_id: envelope.source_id.clone(),
        source_epoch: envelope.source_epoch.clone(),
        order_domain: string(&envelope.payload, "orderDomain").unwrap_or_else(|| "capture".to_owned()),
        sequence: Some(sequence),
        native_key: string(&envelope.payload, "nativeKey"),
        native_predecessor_keys: field(&envelope.payload, "after")
            .and_then(Value::as_array)
            .map(|keys| keys.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
    })
}

fn provider_process(envelope: &ObservationEnvelope) -> Option<(ProcessKey, Option<String>)> {
    envelope
        .evidence
        .iter()
        .find(|sample| sample.role == ProcessRole::Provider)
        .map(|sample| (sample.key.clone(), sample.executable.clone()))
}

fn actor(envelope: &ObservationEnvelope) -> Option<NativeActorRef> {
    envelope.actor_native_id.as_ref().map(|id| NativeActorRef::Agent {
        native_agent_id: id.clone(),
    })
}

fn base_refs(envelope: &ObservationEnvelope) -> NativeRefs {
    NativeRefs {
        session: envelope.session_key.clone(),
        actor: actor(envelope),
        turn: envelope.native_turn_id.clone(),
        ..NativeRefs::default()
    }
}

fn execution(envelope: &ObservationEnvelope) -> Result<NativeExecutionRef, NormalizeError> {
    envelope
        .activation_ref
        .clone()
        .map(|activation_ref| NativeExecutionRef::Activation { activation_ref })
        .ok_or(NormalizeError::Malformed("activationRef"))
}

fn draft(refs: NativeRefs, provenance: EvidenceClass, causal: Option<CausalPoint>, payload: FactPayload) -> NativeFactDraft {
    NativeFactDraft {
        refs,
        provenance,
        causal,
        payload,
    }
}

/// Pure normalization: no IDs are allocated or looked up.
#[allow(clippy::too_many_lines)]
pub fn normalize(envelope: &ObservationEnvelope) -> Result<Vec<NativeFactDraft>, NormalizeError> {
    let payload = &envelope.payload;
    let point = causal(envelope);
    let refs = base_refs(envelope);
    let e = EvidenceClass::ProviderEvent;
    let drafts = match envelope.native_event.as_str() {
        "session.start" => {
            let mut drafts = vec![draft(
                NativeRefs { actor: None, turn: None, ..refs.clone() },
                e,
                point,
                FactPayload::SessionIdentified {
                    display_name: string(payload, "displayName"),
                    start_source: string(payload, "source"),
                },
            )];
            if refs.actor.is_some() {
                drafts.push(draft(
                    NativeRefs { turn: None, ..refs },
                    e,
                    None,
                    FactPayload::ActorIdentified {
                        role: ActorRole::Subordinate,
                        agent_type: string(payload, "agentType"),
                    },
                ));
            }
            drafts
        }
        "execution.attach" => {
            let mut drafts = Vec::new();
            let process = provider_process(envelope);
            if let Some((key, Some(executable))) = &process {
                drafts.push(draft(
                    NativeRefs {
                        process: Some(key.clone()),
                        ..NativeRefs::default()
                    },
                    EvidenceClass::Kernel,
                    point.clone(),
                    FactPayload::ProcessObserved {
                        executable_identity: executable.clone(),
                    },
                ));
            }
            drafts.push(draft(
                NativeRefs {
                    turn: None,
                    execution: Some(execution(envelope)?),
                    process: process.map(|(key, _)| key),
                    ..refs
                },
                e,
                point,
                FactPayload::ExecutionAttached {
                    mode: typed::<ExecutionMode>(payload, "mode")?,
                    presence: optional::<AttachedPresence>(payload, "presence")?
                        .unwrap_or(AttachedPresence::Live),
                    native_runtime_id: string(payload, "runtimeId"),
                    controlling_device: field(payload, "device")
                        .and_then(Value::as_u64)
                        .and_then(|d| u32::try_from(d).ok()),
                },
            ));
            drafts
        }
        "execution.end" => vec![draft(
            NativeRefs {
                turn: None,
                execution: Some(execution(envelope)?),
                ..refs
            },
            e,
            point,
            FactPayload::ExecutionEnded {
                reason: required(payload, "reason")?,
            },
        )],
        "process.exit" | "process.exec" => {
            let (key, executable) =
                provider_process(envelope).ok_or(NormalizeError::Malformed("provider process"))?;
            let process_refs = NativeRefs {
                process: Some(key),
                ..NativeRefs::default()
            };
            if envelope.native_event == "process.exit" {
                vec![draft(process_refs, EvidenceClass::Kernel, None, FactPayload::ProcessExitObserved {})]
            } else {
                vec![draft(
                    process_refs,
                    EvidenceClass::Kernel,
                    point,
                    FactPayload::ProcessObserved {
                        executable_identity: executable.ok_or(NormalizeError::Malformed("executable"))?,
                    },
                )]
            }
        }
        "prompt.submit" => vec![draft(
            NativeRefs {
                input: Some(envelope.native_prompt_id.clone().ok_or(NormalizeError::Malformed("promptId"))?),
                ..refs
            },
            e,
            point,
            FactPayload::InputSubmitted {
                origin: typed::<InputOrigin>(payload, "origin")?,
                submission: optional::<CausalPoint>(payload, "submission")?,
            },
        )],
        "prompt.accepted" => vec![draft(
            NativeRefs {
                turn: None,
                input: Some(envelope.native_prompt_id.clone().ok_or(NormalizeError::Malformed("promptId"))?),
                ..refs
            },
            e,
            point,
            FactPayload::InputAccepted {
                proof: typed::<AcceptanceProof>(payload, "proof")?,
            },
        )],
        "prompt.rejected" => vec![draft(
            NativeRefs {
                turn: None,
                input: Some(envelope.native_prompt_id.clone().ok_or(NormalizeError::Malformed("promptId"))?),
                ..refs
            },
            e,
            point,
            FactPayload::InputRejected {
                reason: required(payload, "reason")?,
            },
        )],
        "turn.start" => {
            require_turn(&refs)?;
            vec![draft(
                NativeRefs {
                    input: envelope.native_prompt_id.clone(),
                    ..refs
                },
                e,
                point,
                FactPayload::TurnStarted {},
            )]
        }
        "turn.step" => {
            require_turn(&refs)?;
            vec![draft(refs, e, point, FactPayload::TurnStepObserved {})]
        }
        "turn.complete" => {
            require_turn(&refs)?;
            let outcome = match required(payload, "reason")?.as_str() {
                "answer" => TurnOutcome::Completed,
                "aborted" => TurnOutcome::Interrupted,
                "error" => TurnOutcome::Failed,
                "refusal" => TurnOutcome::Refused,
                other => return Err(NormalizeError::Unsupported(format!("turn.complete reason {other}"))),
            };
            vec![draft(
                refs,
                e,
                point,
                FactPayload::TurnOutcomeObserved {
                    outcome,
                    reason: string(payload, "reasonCode"),
                    summary: string(payload, "summary"),
                },
            )]
        }
        "notify.output" => {
            require_turn(&refs)?;
            vec![draft(refs, e, point, FactPayload::OutputReady { summary: string(payload, "summary") })]
        }
        "Stop" => vec![draft(
            refs,
            e,
            point,
            FactPayload::ResponseBoundaryObserved {
                stop_hook_active: field(payload, "stopHookActive").and_then(Value::as_bool).unwrap_or(false),
            },
        )],
        "tool.call" => {
            let occurrence = envelope.native_occurrence_id.clone().ok_or(NormalizeError::Malformed("occurrenceId"))?;
            let tool_category = required(payload, "toolCategory")?;
            let payload_value = match required(payload, "phase")?.as_str() {
                "proposed" => FactPayload::ActivityProposed { tool_category },
                "started" => FactPayload::ActivityStarted { tool_category },
                "finished" => FactPayload::ActivityFinished {
                    tool_category,
                    result: typed::<ActivityResult>(payload, "result")?,
                },
                other => return Err(NormalizeError::Unsupported(format!("tool.call phase {other}"))),
            };
            vec![draft(NativeRefs { activity: Some(occurrence), ..refs }, e, point, payload_value)]
        }
        "tool.check" => vec![draft(
            NativeRefs {
                activity: envelope.native_occurrence_id.clone(),
                ..refs
            },
            e,
            point,
            FactPayload::PermissionCheckObserved {
                tool_category: required(payload, "toolCategory")?,
            },
        )],
        "wait" => vec![draft(
            NativeRefs {
                request: string(payload, "requestId"),
                execution: envelope
                    .activation_ref
                    .clone()
                    .map(|activation_ref| NativeExecutionRef::Activation { activation_ref }),
                ..refs
            },
            e,
            point,
            FactPayload::WaitStateObserved {
                category: typed::<WaitCategory>(payload, "category")?,
                signal: typed::<WaitSignal>(payload, "signal")?,
                subtype: string(payload, "subtype"),
                generation: string(payload, "generation"),
            },
        )],
        "request.resolved" => vec![draft(
            NativeRefs {
                request: Some(required(payload, "requestId")?),
                ..refs
            },
            e,
            point,
            FactPayload::RequestResolved {},
        )],
        "agent.spawn" => {
            let child = refs.actor.clone().ok_or(NormalizeError::Malformed("actorNativeId"))?;
            let parent = string(payload, "parentAgentId")
                .map(|native_agent_id| NativeActorRef::Agent { native_agent_id })
                .unwrap_or(NativeActorRef::Principal);
            let role = if field(payload, "teammate").and_then(Value::as_bool) == Some(true) {
                ActorRole::Teammate
            } else {
                ActorRole::Subordinate
            };
            let actor_refs = NativeRefs { turn: None, ..refs };
            vec![
                draft(actor_refs.clone(), e, None, FactPayload::ActorIdentified {
                    role,
                    agent_type: string(payload, "agentType"),
                }),
                draft(
                    NativeRefs {
                        actor: Some(child),
                        related_actor: Some(parent),
                        ..actor_refs
                    },
                    e,
                    None,
                    FactPayload::ActorRelationObserved {
                        relation: ActorRelationKind::ImmediateParent,
                    },
                ),
            ]
        }
        "agent.end" => {
            refs.actor.as_ref().ok_or(NormalizeError::Malformed("actorNativeId"))?;
            vec![draft(NativeRefs { turn: None, ..refs }, e, point, FactPayload::ActorRunEnded {
                outcome: optional::<TurnOutcome>(payload, "outcome")?,
            })]
        }
        "surface.bind" | "surface.unbind" => {
            let surface = typed::<NativeSurfaceRef>(payload, "surface")?;
            let refs = NativeRefs {
                turn: None,
                execution: Some(execution(envelope)?),
                surface: Some(surface),
                ..refs
            };
            if envelope.native_event == "surface.bind" {
                vec![draft(refs, e, point, FactPayload::SurfaceBindingRecorded {
                    proof: BindingProof {
                        method: optional::<BindingMethod>(payload, "method")?.unwrap_or(BindingMethod::NativeInventory),
                        executable_identity: string(payload, "executable"),
                        window_hint: field(payload, "windowHint").and_then(Value::as_i64),
                        tab_hint: field(payload, "tabHint").and_then(Value::as_i64),
                        evidence: field(payload, "evidence").cloned().unwrap_or(Value::Null),
                    },
                })]
            } else {
                vec![draft(refs, e, point, FactPayload::SurfaceBindingInvalidated {
                    reason: required(payload, "reason")?,
                })]
            }
        }
        "inventory.row" => vec![draft(
            NativeRefs { actor: None, turn: None, ..refs },
            EvidenceClass::ProviderSnapshot,
            point,
            FactPayload::ProviderSnapshotObserved {
                present: field(payload, "present").and_then(Value::as_bool).ok_or(NormalizeError::Malformed("present"))?,
                row: optional::<SnapshotRow>(payload, "row")?,
                interval: typed::<SnapshotInterval>(payload, "interval")?,
            },
        )],
        "observer.link" => vec![draft(
            NativeRefs { actor: None, turn: None, ..refs },
            e,
            point,
            FactPayload::ObservationLinkChanged {
                link: typed::<ObservationState>(payload, "link")?,
            },
        )],
        "gap" => vec![draft(NativeRefs::default(), EvidenceClass::Derived, None, FactPayload::ObservationGapDetected {
            domain: required(payload, "domain")?,
            detail: required(payload, "detail")?,
        })],
        other => return Err(NormalizeError::Unsupported(format!("native event {other}"))),
    };
    Ok(drafts)
}

fn require_turn(refs: &NativeRefs) -> Result<(), NormalizeError> {
    refs.turn
        .as_ref()
        .map(|_| ())
        .ok_or(NormalizeError::Malformed("turnId"))
}
