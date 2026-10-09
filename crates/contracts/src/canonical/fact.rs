//! Canonical fact vocabulary (SPEC §5.1, §5.2).
//!
//! A fact's payload carries only data; its identity references travel
//! beside it. A native-keyed draft (`NativeFactDraft`) names objects the way
//! the source does, and admission resolves those names to canonical IDs to
//! make a `ResolvedFact`. Both share one payload type, so resolution can
//! change references and never meaning. Unknown discriminators fail to
//! deserialize: they cannot become a known fact.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::causal::CausalPoint;
use super::keys::{NativeActorRef, NativeExecutionRef, NativeSessionRef, NativeSurfaceRef};
use crate::projection::{NotificationState, ObservationState};
use crate::route::{InputReadiness, ProcessKey, SessionVerification, SurfaceResult};

/// Provenance, not a confidence score (SPEC §5.1).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum EvidenceClass {
    ProviderEvent,
    ProviderSnapshot,
    Kernel,
    UserAttested,
    OwnerCommand,
    Derived,
    SemanticSelfReport,
    UiInferred,
    /// A provider event whose session attribution rests on a
    /// middleware-interceptable host read (a reloaded observer's
    /// `$.session.id()`). Its lifecycle evidence is retained, and applied only
    /// once kernel/inventory evidence shows its provider process runs that
    /// Session (D-0005, D-0010).
    HostRead,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum CanonicalFactKind {
    SessionIdentified,
    SessionRecordChanged,
    ExecutionAttached,
    ExecutionEnded,
    ProcessObserved,
    ProcessExitObserved,
    ObservationLinkChanged,
    InputSubmitted,
    InputAccepted,
    InputRejected,
    TurnStarted,
    TurnStepObserved,
    ResponseBoundaryObserved,
    OutputReady,
    TurnOutcomeObserved,
    ActivityProposed,
    ActivityStarted,
    ActivityFinished,
    PermissionCheckObserved,
    WaitStateObserved,
    RequestResolved,
    ActorIdentified,
    ActorRelationObserved,
    ActorRunEnded,
    SurfaceBindingRecorded,
    SurfaceBindingUnproven,
    SurfaceBindingInvalidated,
    ProviderSnapshotObserved,
    ObservationGapDetected,
    RouteResultRecorded,
    AttentionAcknowledged,
    AttentionResolved,
    AttentionSnoozed,
    NotificationDeliveryRecorded,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ExecutionMode {
    TerminalEmbedded,
    SharedDaemon,
    SupervisedBackground,
    Desktop,
    Remote,
    Headless,
}

/// What an attachment fact positively observed. ENDED comes only from an
/// explicit end or a qualified process exit, never from silence.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum AttachedPresence {
    Live,
    Detached,
    Parked,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum SessionRecordState {
    Known,
    Archived,
    ProviderDeleted,
}

/// The original native origin of an input attempt (SPEC §7.3). Only the two
/// human origins can ever qualify for follow-up resolution.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum InputOrigin {
    HumanComposer,
    HumanBridge,
    Scheduled,
    TaskNotification,
    Peer,
    Sdk,
    Plugin,
    Unclassified,
}

impl InputOrigin {
    pub fn is_human(self) -> bool {
        matches!(self, Self::HumanComposer | Self::HumanBridge)
    }
}

/// Allowlisted proof flags for provider acceptance (SPEC §7.3, §11.4); never
/// text or full traces.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcceptanceProof {
    pub engine_dispatch: bool,
    pub core_settled: bool,
    pub original_origin_protected: bool,
    pub dropped: bool,
}

impl AcceptanceProof {
    /// Verified core acceptance: a success-shaped result alone is not.
    pub fn qualified(&self) -> bool {
        self.engine_dispatch && self.core_settled && self.original_origin_protected && !self.dropped
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum TurnOutcome {
    Completed,
    Interrupted,
    Failed,
    Refused,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ActivityResult {
    Success,
    Failure,
    Interrupted,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum WaitSignal {
    Positive,
    Cleared,
}

/// A native wait category. Each maps to one attention category.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum WaitCategory {
    Approval,
    Input,
    JobBlocked,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ActorRole {
    Principal,
    Subordinate,
    Teammate,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ActorRelationKind {
    ImmediateParent,
    TreeRoot,
    Teammate,
    ForkedFrom,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum BindingMethod {
    NativeInventory,
    HookAncestry,
    ExplicitLaunch,
    UserPairing,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BindingProof {
    pub method: BindingMethod,
    /// The executable image the binding was proven with.
    pub executable_identity: Option<String>,
    pub window_hint: Option<i64>,
    pub tab_hint: Option<i64>,
    /// Bounded, sanitized native evidence (readbacks, samples).
    #[ts(type = "unknown")]
    pub evidence: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SnapshotRow {
    pub kind: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
    pub display_name: Option<String>,
    /// A background row's job state (`working`, `blocked`, `done`, ...):
    /// display metadata, never a turn outcome. Absent before M2.
    #[serde(default)]
    pub state: Option<String>,
}

/// A snapshot without a native revision is an interval observation (SPEC
/// §4.15): it says what the source reported between these instants.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SnapshotInterval {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum FactPayload {
    SessionIdentified {
        display_name: Option<String>,
        /// Native start source (startup, resume, clear, compact, fork).
        start_source: Option<String>,
    },
    SessionRecordChanged {
        record_state: SessionRecordState,
    },
    ExecutionAttached {
        mode: ExecutionMode,
        presence: AttachedPresence,
        native_runtime_id: Option<String>,
        controlling_device: Option<u32>,
    },
    ExecutionEnded {
        reason: String,
    },
    ProcessObserved {
        executable_identity: String,
    },
    ProcessExitObserved {},
    ObservationLinkChanged {
        link: ObservationState,
        /// The reporting observer runs a qualified profile. A report from
        /// before this field is read as its reducer treated every report:
        /// qualified.
        #[serde(default = "qualified_by_default")]
        qualified: bool,
        /// The observer's provider version, as the kernel read it.
        #[serde(default)]
        version: Option<String>,
    },
    InputSubmitted {
        origin: InputOrigin,
        /// The original submission point when it differs from the capture
        /// point (upstream-delayed submission).
        submission: Option<CausalPoint>,
    },
    InputAccepted {
        proof: AcceptanceProof,
    },
    InputRejected {
        reason: String,
    },
    TurnStarted {},
    TurnStepObserved {},
    /// Stop: a response boundary that may be continued or vetoed. It never
    /// ends an execution and never certifies an outcome (INV-07).
    ResponseBoundaryObserved {
        stop_hook_active: bool,
    },
    OutputReady {
        summary: Option<String>,
    },
    TurnOutcomeObserved {
        outcome: TurnOutcome,
        reason: Option<String>,
        summary: Option<String>,
    },
    ActivityProposed {
        tool_category: String,
    },
    ActivityStarted {
        tool_category: String,
    },
    ActivityFinished {
        tool_category: String,
        result: ActivityResult,
    },
    /// A permission preflight occurred; not proof that a human dialog is open.
    PermissionCheckObserved {
        tool_category: String,
    },
    WaitStateObserved {
        category: WaitCategory,
        signal: WaitSignal,
        subtype: Option<String>,
        /// Native generation the wait belongs to, when the source names one.
        generation: Option<String>,
    },
    RequestResolved {},
    ActorIdentified {
        role: ActorRole,
        agent_type: Option<String>,
    },
    ActorRelationObserved {
        relation: ActorRelationKind,
    },
    ActorRunEnded {
        outcome: Option<TurnOutcome>,
    },
    SurfaceBindingRecorded {
        proof: BindingProof,
    },
    SurfaceBindingUnproven {
        reason: String,
    },
    SurfaceBindingInvalidated {
        reason: String,
    },
    ProviderSnapshotObserved {
        present: bool,
        row: Option<SnapshotRow>,
        interval: SnapshotInterval,
    },
    ObservationGapDetected {
        domain: String,
        detail: String,
    },
    RouteResultRecorded {
        request_id: String,
        surface_result: SurfaceResult,
        session_verification: SessionVerification,
        input_readiness: InputReadiness,
        reason_code: String,
        focus_performed: bool,
    },
    AttentionAcknowledged {
        command_id: String,
        at_ms: i64,
    },
    AttentionResolved {
        command_id: String,
        at_ms: i64,
        reason: String,
    },
    AttentionSnoozed {
        command_id: String,
        at_ms: i64,
        until_ms: i64,
    },
    NotificationDeliveryRecorded {
        request_id: String,
        state: NotificationState,
        detail: String,
    },
}

fn qualified_by_default() -> bool {
    true
}

impl FactPayload {
    pub fn kind(&self) -> CanonicalFactKind {
        use CanonicalFactKind as K;
        match self {
            Self::SessionIdentified { .. } => K::SessionIdentified,
            Self::SessionRecordChanged { .. } => K::SessionRecordChanged,
            Self::ExecutionAttached { .. } => K::ExecutionAttached,
            Self::ExecutionEnded { .. } => K::ExecutionEnded,
            Self::ProcessObserved { .. } => K::ProcessObserved,
            Self::ProcessExitObserved {} => K::ProcessExitObserved,
            Self::ObservationLinkChanged { .. } => K::ObservationLinkChanged,
            Self::InputSubmitted { .. } => K::InputSubmitted,
            Self::InputAccepted { .. } => K::InputAccepted,
            Self::InputRejected { .. } => K::InputRejected,
            Self::TurnStarted {} => K::TurnStarted,
            Self::TurnStepObserved {} => K::TurnStepObserved,
            Self::ResponseBoundaryObserved { .. } => K::ResponseBoundaryObserved,
            Self::OutputReady { .. } => K::OutputReady,
            Self::TurnOutcomeObserved { .. } => K::TurnOutcomeObserved,
            Self::ActivityProposed { .. } => K::ActivityProposed,
            Self::ActivityStarted { .. } => K::ActivityStarted,
            Self::ActivityFinished { .. } => K::ActivityFinished,
            Self::PermissionCheckObserved { .. } => K::PermissionCheckObserved,
            Self::WaitStateObserved { .. } => K::WaitStateObserved,
            Self::RequestResolved {} => K::RequestResolved,
            Self::ActorIdentified { .. } => K::ActorIdentified,
            Self::ActorRelationObserved { .. } => K::ActorRelationObserved,
            Self::ActorRunEnded { .. } => K::ActorRunEnded,
            Self::SurfaceBindingRecorded { .. } => K::SurfaceBindingRecorded,
            Self::SurfaceBindingUnproven { .. } => K::SurfaceBindingUnproven,
            Self::SurfaceBindingInvalidated { .. } => K::SurfaceBindingInvalidated,
            Self::ProviderSnapshotObserved { .. } => K::ProviderSnapshotObserved,
            Self::ObservationGapDetected { .. } => K::ObservationGapDetected,
            Self::RouteResultRecorded { .. } => K::RouteResultRecorded,
            Self::AttentionAcknowledged { .. } => K::AttentionAcknowledged,
            Self::AttentionResolved { .. } => K::AttentionResolved,
            Self::AttentionSnoozed { .. } => K::AttentionSnoozed,
            Self::NotificationDeliveryRecorded { .. } => K::NotificationDeliveryRecorded,
        }
    }
}

/// Native-keyed references of a draft. Only an explicit reconciler reference
/// (`NativeExecutionRef::Canonical`, `attention`) names a canonical record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeRefs {
    pub session: Option<NativeSessionRef>,
    pub actor: Option<NativeActorRef>,
    pub related_actor: Option<NativeActorRef>,
    /// Endpoint may be empty for a local capture; admission fills it.
    pub process: Option<ProcessKey>,
    pub execution: Option<NativeExecutionRef>,
    pub turn: Option<String>,
    pub input: Option<String>,
    pub activity: Option<String>,
    pub request: Option<String>,
    pub surface: Option<NativeSurfaceRef>,
    pub attention: Option<String>,
}

/// Canonical references of a resolved fact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CanonicalRefs {
    pub namespace_id: Option<String>,
    pub session_id: Option<String>,
    pub actor_id: Option<String>,
    pub related_actor_id: Option<String>,
    pub process_id: Option<String>,
    pub execution_id: Option<String>,
    pub turn_id: Option<String>,
    pub input_id: Option<String>,
    pub activity_id: Option<String>,
    /// The native request ID itself; requests are not allocated records.
    pub request: Option<String>,
    pub surface_id: Option<String>,
    pub binding_id: Option<String>,
    pub attention_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeFactDraft {
    pub refs: NativeRefs,
    pub provenance: EvidenceClass,
    pub causal: Option<CausalPoint>,
    pub payload: FactPayload,
}

/// How an entry reached admission. Catch-up output intents are held for
/// eligibility reconciliation (SPEC §7.6); bootstrap seeds create none.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum Delivery {
    Live,
    Catchup,
    Bootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolvedFact {
    pub fact_id: String,
    pub observation_id: String,
    /// Position among the facts of one observation.
    pub fact_index: u32,
    pub refs: CanonicalRefs,
    /// The draft's native references, kept as relationship evidence: the
    /// reducer records native keys from them, never by re-resolving.
    pub native: NativeRefs,
    pub provenance: EvidenceClass,
    pub causal: Option<CausalPoint>,
    pub payload_version: u32,
    pub payload: FactPayload,
}

/// What admission journals for one accepted observation or owner command:
/// the header the reducer may use, and its resolved facts in order. `cursor`
/// is the observation's ingest position (a local commit cursor, SPEC §5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JournalEntry {
    pub cursor: i64,
    /// The endpoint that admitted the entry (the local endpoint for captures).
    pub endpoint_id: String,
    pub observation_id: String,
    pub source_id: String,
    pub source_epoch: String,
    pub source_sequence: Option<String>,
    pub sequence_meaning: Option<super::envelope::SequenceMeaning>,
    pub captured_wall_ms: i64,
    pub delivery: Delivery,
    pub facts: Vec<ResolvedFact>,
}
