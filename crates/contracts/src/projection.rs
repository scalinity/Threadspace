//! Provider-neutral view models streamed to the renderer (SPEC §15.1, §18.4).
//! The renderer consumes these; it never sees provider hook names or rows.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::route::RouteSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum TurnState {
    Unknown,
    Queued,
    Working,
    Waiting,
    Completed,
    Interrupted,
    Failed,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ExecutionPresence {
    Live,
    Detached,
    Parked,
    Ended,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ObservationState {
    Current,
    Stale,
    Disconnected,
    Conflict,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum AttentionCategory {
    InputRequired,
    ApprovalRequired,
    TurnComplete,
    Error,
    Blocked,
    HandoffReady,
    OwnerDecisionRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum NotificationState {
    NotRequested,
    Pending,
    Submitted,
    ConfirmedPresent,
    Uncertain,
    Failed,
}

/// A native process incarnation (SPEC §3.2 `ProcessKey`), as displayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessView {
    pub pid: u32,
    pub boot_id: String,
    pub start_seconds: String,
    pub start_microseconds: u32,
    pub executable_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BindingView {
    pub binding_id: String,
    pub surface_kind: String,
    pub proof: String,
    pub revision: String,
    /// Native locator (a Terminal tab's TTY path). A locator, not identity.
    pub locator: String,
    /// Controlling device (`e_tdev`) the binding was proven against.
    pub device_number: Option<u32>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionView {
    pub session_id: String,
    pub provider: String,
    pub native_session_id: String,
    pub display_name: String,
    pub activation: Option<String>,
    pub turn_state: TurnState,
    pub execution_presence: ExecutionPresence,
    pub observation: ObservationState,
    pub process: Option<ProcessView>,
    pub binding: Option<BindingView>,
    /// Valid bindings across live attachments; more than one requires a chooser.
    pub live_bindings: u32,
    /// Why the most recent binding stopped being valid, if it did.
    pub last_invalidation: Option<String>,
    /// Provider-reported activity from native inventory (`busy`, `idle`,
    /// `waiting`), shown as reported, not as a turn outcome.
    pub provider_status: Option<String>,
    pub provider_waiting_for: Option<String>,
    pub last_route: Option<RouteSummary>,
    /// True for M0 fixture records; never true for provider-observed sessions.
    pub fixture: bool,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttentionView {
    pub attention_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub category: AttentionCategory,
    pub priority: u8,
    pub summary: Option<String>,
    pub created_at_ms: i64,
    pub acknowledged_at_ms: Option<i64>,
    pub resolved_at_ms: Option<i64>,
    pub notification_state: NotificationState,
    pub revision: String,
}

/// "Needs attention" = unacknowledged and unresolved; "Awaiting action" =
/// acknowledged and unresolved (SPEC §7.2). Both counts are always visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttentionCounts {
    pub needs_attention: u32,
    pub awaiting_action: u32,
}

/// A projection at one committed cursor (SPEC §18.4). When the complete view
/// would exceed the snapshot bound it is a bounded initial view: `complete`
/// is false, the totals stay exact, and the remaining rows are paged through
/// `ui_query` from the continuation positions. Outstanding counts are never
/// omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FleetSnapshot {
    pub view_revision: String,
    pub sessions: Vec<SessionView>,
    pub attention: Vec<AttentionView>,
    pub counts: AttentionCounts,
    pub complete: bool,
    pub total_sessions: u32,
    pub total_attention: u32,
    /// Page from here (`FleetPage.after`) when `sessions` is partial.
    pub sessions_after: Option<String>,
    /// Page from here (`AttentionPage.after`) when `attention` is partial.
    pub attention_after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum EntityKind {
    Session,
    Attention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EntityRef {
    pub entity: EntityKind,
    pub id: String,
}

/// Full entity upserts or explicit tombstones; never a partial update of an
/// entity the renderer may not hold (SPEC §18.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectionPatch {
    pub from_cursor: String,
    pub to_cursor: String,
    pub view_revision: String,
    pub session_upserts: Vec<SessionView>,
    pub attention_upserts: Vec<AttentionView>,
    pub tombstones: Vec<EntityRef>,
    pub counts: AttentionCounts,
    /// Paged lists of these kinds are stale and must be re-read; sent when a
    /// change touches more entities than fit as full upserts in one frame.
    pub page_invalidations: Vec<EntityKind>,
}

/// A validated internal intent, queued natively until a view has applied its
/// snapshot (SPEC §7.5, §18.8). It carries identifiers, never commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeIntent {
    pub intent_id: String,
    pub action: IntentAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum IntentAction {
    /// Open the inspector for an attention item. `outstanding` reflects the
    /// item as re-read when the native response arrived.
    OpenAttention {
        attention_id: String,
        session_id: String,
        outstanding: bool,
        source: IntentSource,
        /// The verified Return the companion attempted for this response, if
        /// the item was outstanding and observation enabled.
        route: Option<RouteSummary>,
        /// False when observation is disabled or in maintenance: the view
        /// shows the service state, and nothing was routed.
        observation_enabled: bool,
    },
    /// Qualification builds only: a command for the view's qualification
    /// harness (renderer fault injection, self-tests). Release views ignore it.
    QualificationCommand {
        command: String,
        #[ts(type = "unknown")]
        args: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum IntentSource {
    NotificationResponse,
}
