//! Bounds on draft fields (SPEC §8.5). An oversized or malformed value makes
//! the draft unresolved with a bounded reason; it is never truncated into a
//! plausible fact.

use threadspace_contracts::canonical::causal::CausalPoint;
use threadspace_contracts::canonical::fact::{FactPayload, NativeFactDraft};
use threadspace_contracts::cursor::parse_cursor;

/// Native identifiers and keys.
pub const ID_MAX_CHARS: usize = 256;
/// Display names and summaries (SPEC §16.2 uses the same title bound).
pub const LABEL_MAX_CHARS: usize = 120;
/// Reason and detail codes.
pub const REASON_MAX_CHARS: usize = 240;
/// Tool and wait categories.
pub const CATEGORY_MAX_CHARS: usize = 64;
/// Predecessor keys carried by one causal point.
pub const PREDECESSORS_MAX: usize = 32;
/// Bytes of one binding proof's evidence.
pub const PROOF_MAX_BYTES: usize = 8 * 1024;

fn bounded(value: &str, max: usize) -> bool {
    value.chars().count() <= max && !value.chars().any(char::is_control)
}

fn opt(value: Option<&String>, max: usize) -> bool {
    value.is_none_or(|text| bounded(text, max))
}

fn point(point: &CausalPoint) -> bool {
    bounded(&point.source_id, ID_MAX_CHARS)
        && bounded(&point.source_epoch, ID_MAX_CHARS)
        && bounded(&point.order_domain, ID_MAX_CHARS)
        && point.sequence.as_deref().is_none_or(|s| parse_cursor(s).is_some())
        && opt(point.native_key.as_ref(), ID_MAX_CHARS)
        && point.native_predecessor_keys.len() <= PREDECESSORS_MAX
        && point
            .native_predecessor_keys
            .iter()
            .all(|key| bounded(key, ID_MAX_CHARS))
}

fn ids(draft: &NativeFactDraft) -> bool {
    let refs = &draft.refs;
    refs.session.as_ref().is_none_or(|s| {
        bounded(&s.provider, CATEGORY_MAX_CHARS)
            && bounded(&s.profile_ref, ID_MAX_CHARS)
            && !s.native_session_id.is_empty()
            && bounded(&s.native_session_id, ID_MAX_CHARS)
    }) && [
        &refs.turn,
        &refs.input,
        &refs.activity,
        &refs.request,
        &refs.attention,
    ]
    .iter()
    .all(|value| value.as_ref().is_none_or(|v| !v.is_empty() && bounded(v, ID_MAX_CHARS)))
        && refs.surface.as_ref().is_none_or(|s| {
            bounded(&s.surface_kind, CATEGORY_MAX_CHARS)
                && bounded(&s.app_generation, ID_MAX_CHARS)
                && bounded(&s.locator, ID_MAX_CHARS)
                && bounded(&s.surface_generation, ID_MAX_CHARS)
        })
        && refs.process.as_ref().is_none_or(|p| {
            bounded(&p.boot_id, ID_MAX_CHARS)
                && bounded(&p.endpoint_id, ID_MAX_CHARS)
                && parse_cursor(&p.start_seconds).is_some()
                && p.pid > 0
        })
}

fn payload(payload: &FactPayload) -> bool {
    use FactPayload as P;
    match payload {
        P::SessionIdentified {
            display_name,
            start_source,
        } => opt(display_name.as_ref(), LABEL_MAX_CHARS) && opt(start_source.as_ref(), CATEGORY_MAX_CHARS),
        P::ExecutionAttached {
            native_runtime_id, ..
        } => opt(native_runtime_id.as_ref(), ID_MAX_CHARS),
        P::ExecutionEnded { reason }
        | P::InputRejected { reason }
        | P::SurfaceBindingUnproven { reason }
        | P::SurfaceBindingInvalidated { reason } => bounded(reason, REASON_MAX_CHARS),
        P::ProcessObserved {
            executable_identity,
        } => bounded(executable_identity, 1024),
        P::InputSubmitted { submission, .. } => submission.as_ref().is_none_or(point),
        P::OutputReady { summary } => opt(summary.as_ref(), LABEL_MAX_CHARS),
        P::TurnOutcomeObserved {
            reason, summary, ..
        } => opt(reason.as_ref(), REASON_MAX_CHARS) && opt(summary.as_ref(), LABEL_MAX_CHARS),
        P::ActivityProposed { tool_category }
        | P::ActivityStarted { tool_category }
        | P::ActivityFinished { tool_category, .. }
        | P::PermissionCheckObserved { tool_category } => {
            bounded(tool_category, CATEGORY_MAX_CHARS)
        }
        P::WaitStateObserved {
            subtype,
            generation,
            ..
        } => opt(subtype.as_ref(), CATEGORY_MAX_CHARS) && opt(generation.as_ref(), ID_MAX_CHARS),
        P::ActorIdentified { agent_type, .. } => opt(agent_type.as_ref(), CATEGORY_MAX_CHARS),
        P::SurfaceBindingRecorded { proof } => {
            opt(proof.executable_identity.as_ref(), 1024)
                && proof.evidence.to_string().len() <= PROOF_MAX_BYTES
        }
        P::ProviderSnapshotObserved { row, interval, .. } => {
            interval.start_ms <= interval.end_ms
                && row.as_ref().is_none_or(|row| {
                    opt(row.kind.as_ref(), CATEGORY_MAX_CHARS)
                        && opt(row.status.as_ref(), CATEGORY_MAX_CHARS)
                        && opt(row.waiting_for.as_ref(), CATEGORY_MAX_CHARS)
                        && opt(row.display_name.as_ref(), LABEL_MAX_CHARS)
                })
        }
        P::ObservationGapDetected { domain, detail } => {
            bounded(domain, CATEGORY_MAX_CHARS) && bounded(detail, REASON_MAX_CHARS)
        }
        P::RouteResultRecorded {
            request_id,
            reason_code,
            ..
        } => bounded(request_id, ID_MAX_CHARS) && bounded(reason_code, CATEGORY_MAX_CHARS),
        P::AttentionAcknowledged { command_id, .. } | P::AttentionSnoozed { command_id, .. } => {
            bounded(command_id, ID_MAX_CHARS)
        }
        P::AttentionResolved {
            command_id, reason, ..
        } => bounded(command_id, ID_MAX_CHARS) && bounded(reason, REASON_MAX_CHARS),
        P::NotificationDeliveryRecorded {
            request_id, detail, ..
        } => bounded(request_id, ID_MAX_CHARS) && bounded(detail, REASON_MAX_CHARS),
        P::SessionRecordChanged { .. }
        | P::ProcessExitObserved {}
        | P::ObservationLinkChanged { .. }
        | P::InputAccepted { .. }
        | P::TurnStarted {}
        | P::TurnStepObserved {}
        | P::ResponseBoundaryObserved { .. }
        | P::RequestResolved {}
        | P::ActorRelationObserved { .. }
        | P::ActorRunEnded { .. } => true,
    }
}

pub fn draft(draft: &NativeFactDraft) -> Result<(), &'static str> {
    if !ids(draft) {
        return Err("INVALID_IDENTIFIER");
    }
    if !draft.causal.as_ref().is_none_or(point) {
        return Err("INVALID_CAUSAL_POINT");
    }
    if !payload(&draft.payload) {
        return Err("INVALID_PAYLOAD");
    }
    Ok(())
}
