//! Pure normalization of Claude conventional hook envelopes (SPEC §11.2) for
//! the CLAUDE_CLASSIC_LIMITED contract. Conventional hooks carry no native
//! turn identity, so nothing here yields a turn start or outcome: Stop is a
//! response boundary, a prompt is a submission without acceptance, and tool
//! hooks are activity phases. A hook the profile does not interpret
//! identifies its session only; an unknown hook name drives nothing.

use serde_json::Value;
use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_contracts::canonical::fact::{
    ActivityResult, ActorRole, EvidenceClass, FactPayload, InputOrigin, NativeFactDraft,
    NativeRefs,
};
use threadspace_contracts::canonical::keys::NativeActorRef;
use threadspace_state_engine::normalize::{Normalized, retain};

pub const ADAPTER_ID: &str = "claude.hook";

/// Payload keys the capture helper may have retained; nothing else is kept.
pub const RETAINED_KEYS: &[&str] = &[
    "source",
    "reason",
    "stopHookActive",
    "agentType",
    "toolCategory",
    "notificationType",
    "permissionMode",
    "isInterrupt",
];

/// Hooks this profile records as session identity only.
const IDENTITY_ONLY: &[&str] = &[
    "SessionEnd",
    "StopFailure",
    "Notification",
    "PermissionDenied",
    "TeammateIdle",
    "TaskCreated",
    "TaskCompleted",
    "CwdChanged",
    "DirectoryAdded",
    "Elicitation",
    "ElicitationResult",
    "PreCompact",
    "PostCompact",
    "PostToolBatch",
];

fn text(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn normalize(envelope: &ObservationEnvelope) -> Normalized {
    let retained = retain(&envelope.payload, RETAINED_KEYS);
    let unsupported = |reason: String| Normalized {
        drafts: Vec::new(),
        retained: retained.clone(),
        unsupported: Some(reason),
    };
    let Some(session) = envelope.session_key.clone() else {
        return unsupported("MALFORMED: session".into());
    };
    let actor = envelope
        .actor_native_id
        .clone()
        .map(|native_agent_id| NativeActorRef::Agent { native_agent_id });
    let refs = NativeRefs {
        session: Some(session),
        actor: actor.clone(),
        ..NativeRefs::default()
    };
    let draft = |refs: NativeRefs, payload: FactPayload| NativeFactDraft {
        refs,
        provenance: EvidenceClass::ProviderEvent,
        causal: None,
        payload,
    };
    let identified = |start_source: Option<String>| {
        draft(
            NativeRefs {
                actor: None,
                ..refs.clone()
            },
            FactPayload::SessionIdentified {
                display_name: None,
                start_source,
            },
        )
    };
    let tool = text(&retained, "toolCategory").unwrap_or_else(|| "unknown".into());
    let activity = || NativeRefs {
        activity: envelope.native_occurrence_id.clone(),
        ..refs.clone()
    };
    let drafts = match envelope.native_event.as_str() {
        "SessionStart" => vec![identified(text(&retained, "source"))],
        "UserPromptSubmit" => vec![
            identified(None),
            draft(
                NativeRefs {
                    // Conventional input has no native input ID; the stable
                    // observation UUID names this submission.
                    input: Some(
                        envelope
                            .native_prompt_id
                            .clone()
                            .unwrap_or_else(|| envelope.observation_id.clone()),
                    ),
                    ..refs.clone()
                },
                FactPayload::InputSubmitted {
                    origin: InputOrigin::Unclassified,
                    submission: None,
                },
            ),
        ],
        "Stop" | "SubagentStop" => vec![
            identified(None),
            draft(
                refs.clone(),
                FactPayload::ResponseBoundaryObserved {
                    stop_hook_active: retained
                        .get("stopHookActive")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            ),
        ],
        "SubagentStart" if actor.is_some() => vec![
            identified(None),
            draft(
                refs.clone(),
                FactPayload::ActorIdentified {
                    role: ActorRole::Subordinate,
                    agent_type: text(&retained, "agentType"),
                },
            ),
        ],
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
            if envelope.native_occurrence_id.is_some() =>
        {
            let payload = match envelope.native_event.as_str() {
                "PreToolUse" => FactPayload::ActivityProposed {
                    tool_category: tool,
                },
                "PostToolUse" => FactPayload::ActivityFinished {
                    tool_category: tool,
                    result: ActivityResult::Success,
                },
                _ => FactPayload::ActivityFinished {
                    tool_category: tool,
                    result: if retained.get("isInterrupt").and_then(Value::as_bool) == Some(true) {
                        ActivityResult::Interrupted
                    } else {
                        ActivityResult::Failure
                    },
                },
            };
            vec![identified(None), draft(activity(), payload)]
        }
        "PermissionRequest" => vec![
            identified(None),
            draft(
                refs.clone(),
                FactPayload::PermissionCheckObserved {
                    tool_category: tool,
                },
            ),
        ],
        event if IDENTITY_ONLY.contains(&event) => vec![identified(None)],
        other => return unsupported(format!("UNSUPPORTED: hook {other}")),
    };
    Normalized {
        drafts,
        retained,
        unsupported: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use threadspace_contracts::canonical::envelope::{CaptureClock, ClockQuality};
    use threadspace_contracts::canonical::fact::CanonicalFactKind;
    use threadspace_contracts::canonical::keys::NativeSessionRef;

    fn envelope(event: &str, payload: Value) -> ObservationEnvelope {
        ObservationEnvelope {
            schema_version: 1,
            observation_id: "00000000-0000-4000-8000-000000000009".into(),
            source_id: "claude.hook".into(),
            source_epoch: "hook".into(),
            source_sequence: None,
            sequence_meaning: None,
            callback_entry_sequence: None,
            callback_result_sequence: None,
            adapter_id: ADAPTER_ID.into(),
            adapter_version: "1".into(),
            provider_version: None,
            native_event: event.into(),
            session_key: Some(NativeSessionRef {
                provider: "claude".into(),
                profile_ref: "claude-cli:~/.claude".into(),
                native_session_id: "s-1".into(),
            }),
            actor_native_id: None,
            native_turn_id: None,
            native_prompt_id: None,
            native_occurrence_id: Some("toolu_1".into()),
            activation_ref: None,
            captured_at: CaptureClock {
                endpoint_id: None,
                boot_id: None,
                monotonic_ns: None,
                wall_time_ms: 1,
                clock_quality: ClockQuality::ReceiptOnly,
            },
            evidence: Vec::new(),
            payload,
        }
    }

    fn kinds(normalized: &Normalized) -> Vec<CanonicalFactKind> {
        normalized.drafts.iter().map(|d| d.payload.kind()).collect()
    }

    #[test]
    fn stop_is_a_response_boundary_never_an_outcome_or_end() {
        let n = normalize(&envelope("Stop", json!({ "stopHookActive": false })));
        assert_eq!(
            kinds(&n),
            vec![CanonicalFactKind::SessionIdentified, CanonicalFactKind::ResponseBoundaryObserved]
        );
        for event in ["Stop", "SessionEnd", "StopFailure"] {
            let n = normalize(&envelope(event, json!({})));
            assert!(!kinds(&n).iter().any(|k| matches!(
                k,
                CanonicalFactKind::TurnOutcomeObserved | CanonicalFactKind::ExecutionEnded
            )));
        }
    }

    #[test]
    fn unknown_hooks_drive_nothing_and_bodies_are_not_retained() {
        let n = normalize(&envelope("TurnTeleport", json!({})));
        assert!(n.drafts.is_empty() && n.unsupported.is_some());
        let n = normalize(&envelope(
            "PreToolUse",
            json!({ "toolCategory": "Bash", "tool_input": { "command": "SECRET" } }),
        ));
        assert!(!n.retained.to_string().contains("SECRET"));
        assert_eq!(kinds(&n)[1], CanonicalFactKind::ActivityProposed);
    }
}
