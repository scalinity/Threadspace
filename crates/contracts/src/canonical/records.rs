//! Canonical records (SPEC §3.1, §9.2): the reducer's state, materialized
//! into projection tables and serialized whole into checkpoints.
//!
//! Records keep the evidence that produced them as sets (outcomes, end
//! reasons, wait positives and clears, resolution causes), and every
//! displayed state is derived from those sets. That is what makes
//! independently admitted arrival orders converge (INV-14): no field is
//! "whatever arrived last". Collections are `BTreeMap`/`BTreeSet`, so the
//! serialized state, and its hash, is canonical.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::causal::CausalPoint;
use super::command::OwnerAction;
use super::envelope::SequenceMeaning;
use super::fact::{
    AcceptanceProof, ActivityResult, ActorRelationKind, ActorRole, AttachedPresence,
    BindingMethod, ExecutionMode, InputOrigin, SessionRecordState, SnapshotInterval, SnapshotRow,
    TurnOutcome, WaitCategory,
};
use super::keys::{NativeActorRef, NativeSurfaceRef};
use crate::projection::{
    AttentionCategory, ExecutionPresence, NotificationState, ObservationState, TurnState,
};
use crate::route::{InputReadiness, ProcessKey, SessionVerification, SurfaceResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NamespaceRecord {
    pub id: String,
    pub provider: String,
    pub endpoint_id: String,
    pub profile_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InventoryObservation {
    pub present: bool,
    pub row: Option<SnapshotRow>,
    pub interval: SnapshotInterval,
    pub point: Option<CausalPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteRecord {
    pub request_id: String,
    pub surface_result: SurfaceResult,
    pub session_verification: SessionVerification,
    pub input_readiness: InputReadiness,
    pub reason_code: String,
    pub focus_performed: bool,
}

/// A persistent provider conversation (SPEC §4.1): unique on namespace and
/// native session ID, independent of every activation, turn and surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionRecord {
    pub id: String,
    pub namespace_id: String,
    pub native_session_id: String,
    pub record_state: SessionRecordState,
    pub display_name: Option<String>,
    pub start_sources: BTreeSet<String>,
    /// Last explicit observer link fact, if any.
    pub link: Option<ObservationState>,
    pub inventory: Option<InventoryObservation>,
    pub last_route: Option<RouteRecord>,
    /// True only for the M0 fixture worker.
    pub fixture: bool,
    // Derived.
    pub execution_presence: ExecutionPresence,
    pub observation: ObservationState,
    pub turn_state: TurnState,
    pub created_cursor: i64,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActorRecord {
    pub id: String,
    pub session_id: String,
    pub native: NativeActorRef,
    pub role: ActorRole,
    pub agent_types: BTreeSet<String>,
    pub runs_ended: u32,
    pub created_cursor: i64,
    pub revision: i64,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActorRelationRecord {
    pub actor_id: String,
    pub related_actor_id: String,
    pub relation: ActorRelationKind,
}

/// A kernel process incarnation (SPEC §4.2). Distinct from its PID: a reused
/// PID with a new birth is a different record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessRecord {
    pub id: String,
    pub key: ProcessKey,
    /// Every executable image observed for this incarnation. More than one
    /// means the image was replaced in place (`exec`).
    pub images: BTreeSet<ProcessImage>,
    /// The causally latest image when one is determinable; `None` when the
    /// observations do not order (then no image is assumed current).
    pub current_executable: Option<String>,
    pub exited: bool,
    pub created_cursor: i64,
    pub revision: i64,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessImage {
    pub executable: String,
    pub point: Option<CausalPoint>,
}

/// One logical activation of a Session/Actor in a runtime (SPEC §4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExecutionRecord {
    pub id: String,
    pub session_id: String,
    pub actor_id: String,
    pub activation_ref: String,
    pub process_id: Option<String>,
    /// Per-session allocation order; presentation, not identity.
    pub activation: u64,
    pub mode: Option<ExecutionMode>,
    pub attached: Option<AttachedPresence>,
    pub native_runtime_id: Option<String>,
    pub controlling_device: Option<u32>,
    pub end_reasons: BTreeSet<String>,
    pub surface_status: Option<String>,
    // Derived.
    pub presence: ExecutionPresence,
    pub started_cursor: Option<i64>,
    pub ended_cursor: Option<i64>,
    pub created_cursor: i64,
    pub revision: i64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum TurnIdentityKind {
    Native,
    LocalProvisional,
}

/// A native model/agent turn (SPEC §6.1), scoped to Session and Actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TurnRecord {
    pub id: String,
    pub session_id: String,
    pub actor_id: String,
    pub execution_ids: BTreeSet<String>,
    pub native_turn_id: Option<String>,
    pub identity_kind: TurnIdentityKind,
    /// From the originating actor's role when the turn was observed; a later
    /// role change never promotes old parent-owned output (SPEC §7.1).
    pub owner_facing: bool,
    pub started: bool,
    pub stepped: bool,
    pub activity_seen: bool,
    pub queued_inputs: BTreeSet<String>,
    pub started_by_inputs: BTreeSet<String>,
    pub outcomes: BTreeSet<TurnOutcome>,
    pub outcome_reasons: BTreeSet<String>,
    pub output_ready: bool,
    pub output_points: BTreeSet<CausalPoint>,
    pub summary: Option<String>,
    // Derived.
    pub state: TurnState,
    pub outcome_conflict: bool,
    pub created_cursor: i64,
    pub revision: i64,
}

/// A submitted input and its provenance (SPEC §6.1, §7.3); distinct from the
/// Turn it may start or steer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InputRecord {
    pub id: String,
    pub session_id: String,
    pub actor_id: String,
    pub native_key: String,
    pub origin: Option<InputOrigin>,
    pub submission: Option<CausalPoint>,
    /// The turn running at original submission, kept apart from any later turn.
    pub active_turn_id: Option<String>,
    pub acceptances: BTreeSet<AcceptanceProof>,
    pub rejections: BTreeSet<String>,
    pub started_turns: BTreeSet<String>,
    pub created_cursor: i64,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActivityRecord {
    pub id: String,
    pub session_id: String,
    pub actor_id: String,
    pub turn_id: Option<String>,
    pub native_occurrence_id: String,
    pub tool_categories: BTreeSet<String>,
    pub proposed: bool,
    pub started: bool,
    pub finished: BTreeSet<ActivityResult>,
    pub permission_checked: bool,
    pub created_cursor: i64,
    pub revision: i64,
}

/// A native surface (SPEC §4.10, §4.12), distinct from any binding to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceSurfaceRecord {
    pub id: String,
    pub endpoint_id: String,
    pub native: NativeSurfaceRef,
    pub created_cursor: i64,
    pub revision: i64,
}

/// A versioned relation between one activation and one surface. Invalidation
/// is permanent: nothing revives a binding (SPEC §4.12, INV-03/04).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SurfaceBindingRecord {
    pub id: String,
    pub session_id: String,
    pub execution_id: String,
    pub surface_id: String,
    /// None until the proof itself is admitted: an invalidation can arrive
    /// first, and the binding is then invalid from birth.
    pub method: Option<BindingMethod>,
    pub executable_identity: Option<String>,
    pub window_hint: Option<i64>,
    pub tab_hint: Option<i64>,
    #[ts(type = "unknown")]
    pub proof: serde_json::Value,
    pub evidence_observation: Option<String>,
    /// Where the proof was made: a different image observed causally after
    /// it invalidates the binding; one before it cannot (D-0007 §5).
    pub proof_point: Option<CausalPoint>,
    pub invalidations: BTreeSet<String>,
    // Derived.
    pub valid: bool,
    pub invalidation_reason: Option<String>,
    pub recorded_cursor: Option<i64>,
    pub invalidated_cursor: Option<i64>,
    pub created_cursor: i64,
    pub revision: i64,
}

/// One state of an aggregate wait episode, derived from its scope's
/// positive witnesses and clear barriers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WaitEpisode {
    pub index: u32,
    pub episode_id: String,
    pub active: bool,
    /// A positive incomparable with a clear: never auto-cleared.
    pub uncertain: bool,
    pub attention_id: Option<String>,
}

/// An owner command on a wait item, kept with the evidence it was made on:
/// the positive witnesses of the episode the owner acted on. A late clear
/// that repartitions the episodes moves the decision with that evidence,
/// never onto a positive it did not cover (D-0007 §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WaitOwnerDecision {
    pub command_id: String,
    pub action: OwnerAction,
    pub at_ms: i64,
    pub positives: BTreeSet<CausalPoint>,
    /// The episode also held positives without a causal point.
    pub unordered: bool,
}

/// An aggregate native wait scope (SPEC §7.1): namespace/Session, known
/// actor or execution, category and native generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WaitScopeRecord {
    pub key: String,
    pub session_id: String,
    pub actor_id: Option<String>,
    pub execution_id: Option<String>,
    pub turn_id: Option<String>,
    pub category: WaitCategory,
    pub subtypes: BTreeSet<String>,
    pub generation: Option<String>,
    pub positives: BTreeSet<CausalPoint>,
    pub clears: BTreeSet<CausalPoint>,
    /// Positives without any causal point: active until a clear, uncertain after.
    pub unordered_positives: u32,
    pub unordered_clears: u32,
    pub episodes: Vec<WaitEpisode>,
    /// Ordered by command ID. Absent from checkpoints of earlier M1 builds.
    #[serde(default)]
    pub owner_decisions: Vec<WaitOwnerDecision>,
    pub created_cursor: i64,
    pub revision: i64,
}

/// A native exact request (SPEC §7.1), resolved only by its own native ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExactRequestRecord {
    pub key: String,
    pub session_id: String,
    pub actor_id: Option<String>,
    pub turn_id: Option<String>,
    pub native_request_id: String,
    pub category: Option<WaitCategory>,
    pub positive: bool,
    pub resolved: bool,
    pub attention_id: Option<String>,
    pub created_cursor: i64,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AttentionScope {
    ExactRequest { native_request_id: String },
    SessionWaitCategory { episode_id: String, wait_kind: String },
    TurnOutput { turn_id: String },
    OwnerDecision { decision_id: String },
}

impl AttentionScope {
    pub fn scope_kind(&self) -> &'static str {
        match self {
            Self::ExactRequest { .. } => "EXACT_REQUEST",
            Self::SessionWaitCategory { .. } => "SESSION_WAIT_CATEGORY",
            Self::TurnOutput { .. } => "TURN_OUTPUT",
            Self::OwnerDecision { .. } => "OWNER_DECISION",
        }
    }

    pub fn scope_key(&self) -> &str {
        match self {
            Self::ExactRequest { native_request_id } => native_request_id,
            Self::SessionWaitCategory { episode_id, .. } => episode_id,
            Self::TurnOutput { turn_id } => turn_id,
            Self::OwnerDecision { decision_id } => decision_id,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ResolutionKind {
    /// Explicit owner resolution ("Mark handled") with its reason.
    Owner,
    /// A fresh native aggregate wait clear: "wait ended", not "approved".
    WaitEnded,
    RequestResolved,
    /// Verified accepted human input causally after the output (SPEC §7.3).
    HumanFollowup,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolutionCause {
    pub kind: ResolutionKind,
    pub detail: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum SummaryAuthority {
    NativeMetadata,
    SelfReported,
    User,
    None,
}

/// A durable owner-action record (SPEC §7.1). Acknowledgement and resolution
/// are separate; both are sets of causes so replay order cannot erase one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttentionRecord {
    pub id: String,
    pub session_id: String,
    pub actor_id: Option<String>,
    pub turn_id: Option<String>,
    pub category: AttentionCategory,
    pub scope: AttentionScope,
    pub priority: u8,
    pub summary: Option<String>,
    pub summary_authority: SummaryAuthority,
    pub created_by_fact: String,
    pub created_by_observation: String,
    pub created_at_ms: i64,
    pub acknowledgements: BTreeSet<String>,
    pub acknowledged_at_ms: Option<i64>,
    pub resolutions: BTreeSet<ResolutionCause>,
    pub resolved_at_ms: Option<i64>,
    pub resolution_reason: Option<String>,
    pub snoozed_until_ms: Option<i64>,
    pub notification_state: NotificationState,
    pub created_cursor: i64,
    pub revision: i64,
}

impl AttentionRecord {
    pub fn resolved(&self) -> bool {
        !self.resolutions.is_empty()
    }

    pub fn acknowledged(&self) -> bool {
        !self.acknowledgements.is_empty()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum OutboxState {
    /// Live intent awaiting OS submission (after commit).
    Pending,
    /// Catch-up intent held until eligibility is reconciled (SPEC §7.6).
    Held,
    /// Resolved, acknowledged or otherwise ineligible before submission.
    Suppressed,
    Submitted,
    ConfirmedPresent,
    Uncertain,
    Failed,
}

/// A notification intent created in the same transaction as its attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OutboxRecord {
    pub request_id: String,
    pub attention_id: String,
    pub state: OutboxState,
    pub detail: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub created_cursor: i64,
    pub revision: i64,
}

/// The compact accepted-human causal frontier of one comparable scope
/// (SPEC §7.3): retained through checkpoints so reverse delivery resolves
/// the same outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HumanFrontier {
    pub key: String,
    pub session_id: String,
    pub actor_id: String,
    pub source_id: String,
    pub source_epoch: String,
    pub order_domain: String,
    pub max_sequence: Option<String>,
    pub predecessor_keys: BTreeSet<String>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SequenceRange {
    pub first: String,
    pub last: String,
}

/// Sequence coverage of one source epoch. A missing sequence is a coverage
/// gap of that source, not provider event loss or a completion (SPEC §5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceCoverage {
    pub key: String,
    pub source_id: String,
    pub source_epoch: String,
    pub meaning: SequenceMeaning,
    pub seen: Vec<SequenceRange>,
    pub gaps: Vec<SequenceRange>,
    pub reported_gaps: BTreeSet<String>,
    pub revision: i64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum OwnerActionKind {
    Acknowledge,
    Resolve,
    Snooze,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandEffect {
    pub command_id: String,
    pub attention_id: String,
    pub action: OwnerActionKind,
    pub cursor: i64,
}

/// The whole canonical state at one applied cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CanonicalState {
    pub reducer_version: u32,
    pub through_cursor: i64,
    pub namespaces: BTreeMap<String, NamespaceRecord>,
    pub sessions: BTreeMap<String, SessionRecord>,
    pub actors: BTreeMap<String, ActorRecord>,
    pub relations: BTreeSet<ActorRelationRecord>,
    pub processes: BTreeMap<String, ProcessRecord>,
    pub executions: BTreeMap<String, ExecutionRecord>,
    pub turns: BTreeMap<String, TurnRecord>,
    pub inputs: BTreeMap<String, InputRecord>,
    pub activities: BTreeMap<String, ActivityRecord>,
    pub surfaces: BTreeMap<String, SourceSurfaceRecord>,
    pub bindings: BTreeMap<String, SurfaceBindingRecord>,
    pub waits: BTreeMap<String, WaitScopeRecord>,
    pub requests: BTreeMap<String, ExactRequestRecord>,
    pub attention: BTreeMap<String, AttentionRecord>,
    /// Scope key to attention ID, so a migrated or derived item is found by
    /// scope and never duplicated.
    pub attention_by_scope: BTreeMap<String, String>,
    pub outbox: BTreeMap<String, OutboxRecord>,
    pub frontiers: BTreeMap<String, HumanFrontier>,
    pub coverage: BTreeMap<String, SourceCoverage>,
    pub commands: BTreeMap<String, CommandEffect>,
}
