//! The renderer bridge: the five Tauri commands `ui_connect`, `ui_ack`,
//! `ui_disconnect`, `ui_query`, `ui_action` and the ordered `UiFrame`
//! stream (SPEC §18.3–18.5).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::diagnostics::{
    AutomationPermission, CompanionDiagnostics, CompanionIntegration, DesktopDiagnostics,
    NotificationSettings, ServiceReport,
};
use crate::limits;
use crate::projection::{NativeIntent, ProjectionPatch};

pub const UI_PROTOCOL_VERSION: u32 = 1;

// ---------------------------------------------------------------- connect

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UiConnectRequest {
    pub protocol_version: u32,
    /// Random UUID created by the renderer before installing its Channel handler.
    pub view_epoch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StreamLimits {
    pub snapshot_max_bytes: u32,
    pub snapshot_max_frames: u32,
    pub frame_max_bytes: u32,
    pub window_max_frames: u32,
    pub window_max_bytes: u32,
    pub heartbeat_interval_ms: u32,
    pub hydration_deadline_ms: u32,
}

impl StreamLimits {
    pub const FROZEN: StreamLimits = StreamLimits {
        snapshot_max_bytes: limits::SNAPSHOT_MAX_BYTES as u32,
        snapshot_max_frames: limits::SNAPSHOT_MAX_FRAMES as u32,
        frame_max_bytes: limits::FRAME_MAX_BYTES as u32,
        window_max_frames: limits::WINDOW_MAX_FRAMES as u32,
        window_max_bytes: limits::WINDOW_MAX_BYTES as u32,
        heartbeat_interval_ms: limits::HEARTBEAT_INTERVAL_MS,
        hydration_deadline_ms: limits::HYDRATION_DEADLINE_MS,
    };
}

/// Confirms registration only. Readiness comes from an applied snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UiConnectReply {
    pub subscription_id: String,
    pub view_epoch: String,
    pub core_generation: String,
    pub store_generation: String,
    pub limits: StreamLimits,
}

// ----------------------------------------------------------------- stream

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FrameHeader {
    pub protocol_version: u32,
    pub store_generation: String,
    pub core_generation: String,
    pub subscription_id: String,
    pub view_epoch: String,
    /// Contiguous per-subscription application sequence, starting at 1.
    pub stream_seq: u32,
    /// The journal cursor this frame represents; an ACK must name it exactly.
    pub cursor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum UiFrameBody {
    SnapshotBegin {
        view_revision: String,
        chunk_count: u32,
        total_bytes: u32,
    },
    /// A UTF-8 slice of the serialized `FleetSnapshot`; staged until `SnapshotEnd`.
    SnapshotChunk { index: u32, data: String },
    /// Replace the renderer projection atomically, then ACK this frame.
    SnapshotEnd { view_revision: String },
    ProjectionPatch { patch: ProjectionPatch },
    BridgeHeartbeat,
    NativeIntent { intent: NativeIntent },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UiFrame {
    pub header: FrameHeader,
    pub body: UiFrameBody,
}

// -------------------------------------------------------- ack / disconnect

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UiAckRequest {
    pub subscription_id: String,
    pub view_epoch: String,
    pub highest_applied_stream_seq: u32,
    pub applied_journal_cursor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UiAckReply {
    pub acknowledged_through: u32,
    pub hydrated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UiDisconnectRequest {
    pub subscription_id: String,
    pub view_epoch: String,
}

// ------------------------------------------------------------------ query

/// Carried by subscribed queries and actions; validated against the current
/// native view registration and companion generations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UiCallContext {
    pub subscription_id: String,
    pub view_epoch: String,
    pub core_generation: String,
    pub store_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", deny_unknown_fields)]
#[ts(export)]
pub enum UiQuery {
    /// Bootstrap status; may omit context.
    ConnectionStatus {},
    FleetPage {},
    AttentionPage {},
    SessionDetail {},
    ProjectDetail {},
    Diagnostics {},
    IntegrationStatus {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UiQueryRequest {
    pub query: UiQuery,
    pub context: Option<UiCallContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum CompanionLink {
    Connected {
        core_generation: String,
        store_generation: String,
        companion_pid: u32,
    },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConnectionStatus {
    pub ui_protocol_version: u32,
    pub app_identifier: String,
    pub companion: CompanionLink,
    pub active_subscriptions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosticsReport {
    pub desktop: DesktopDiagnostics,
    pub companion: Option<CompanionDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IntegrationReport {
    pub service: ServiceReport,
    pub companion: Option<CompanionIntegration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum UiQueryResult {
    ConnectionStatus(ConnectionStatus),
    Diagnostics(Box<DiagnosticsReport>),
    IntegrationStatus(Box<IntegrationReport>),
}

// ----------------------------------------------------------------- action

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub enum UiAction {
    AcknowledgeAttention {
        attention_id: String,
    },
    /// Explicit setup step (SPEC §19.1): ask the companion to request native
    /// notification authorization under its own identity.
    RequestNotificationAuthorization {},
    /// Explicit setup step (SPEC §19.1): ask the companion to request
    /// Apple-event automation consent for Terminal under its own identity.
    RequestTerminalAutomation {},
    /// Qualification builds only: persist a renderer-produced report (for
    /// example backend attestation) as native evidence.
    RecordQualificationReport {
        report_kind: String,
        report: serde_json::Value,
    },
    // Defined by SPEC §18.3 and §19; implemented in later milestones.
    ReturnToSession {},
    ResolveAttention {},
    SnoozeAttention {},
    UpdateLayout {},
    UpdatePreferences {},
    SetProjectHome {},
    LinkSurface {},
    RefreshEvidence {},
    EnableObservation {},
    StopObservation {},
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct UiActionRequest {
    pub action: UiAction,
    pub expected_revision: Option<String>,
    /// Idempotency key for durable owner commands.
    pub request_id: String,
    pub context: UiCallContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ReceiptStatus {
    /// Committed by this request.
    Committed,
    /// The same request ID and payload were committed earlier.
    AlreadyCommitted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandReceipt {
    pub command_id: String,
    pub status: ReceiptStatus,
    pub cursor: String,
    pub target_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum UiActionResult {
    CommandCommitted {
        receipt: CommandReceipt,
    },
    NotificationAuthorization {
        granted: bool,
        settings: NotificationSettings,
    },
    TerminalAutomation {
        automation: AutomationPermission,
        status_code: i32,
    },
    QualificationReportRecorded {
        file_name: String,
    },
}

// ------------------------------------------------------------------ error

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum UiErrorCode {
    InvalidRequest,
    UnsupportedProtocol,
    /// The calling native WebView incarnation is not the active office view.
    StaleView,
    UnknownSubscription,
    /// Context generations or epoch no longer match the current registration.
    StaleContext,
    NotImplementedForMilestone,
    CompanionUnavailable,
    CompanionRejected,
    Conflict,
    TooManyInFlight,
    ReplyTooLarge,
    SnapshotExceedsBound,
    AckRejected,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UiError {
    pub code: UiErrorCode,
    pub retryable: bool,
    pub detail: String,
}

impl UiError {
    pub fn new(code: UiErrorCode, detail: impl Into<String>) -> Self {
        let retryable = matches!(
            code,
            UiErrorCode::CompanionUnavailable
                | UiErrorCode::TooManyInFlight
                | UiErrorCode::StaleContext
                | UiErrorCode::UnknownSubscription
        );
        let detail: String = detail.into();
        let detail = detail.chars().take(limits::ERROR_DETAIL_MAX_CHARS).collect();
        Self {
            code,
            retryable,
            detail,
        }
    }

    pub fn invalid(detail: impl Into<String>) -> Self {
        Self::new(UiErrorCode::InvalidRequest, detail)
    }

    pub fn not_implemented(operation: &str) -> Self {
        Self::new(
            UiErrorCode::NotImplementedForMilestone,
            format!("{operation} is not implemented in milestone M0A"),
        )
    }
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for UiError {}

/// Deserializes a raw command argument into a strict contract type, mapping
/// every shape error to a typed `INVALID_REQUEST` (a TypeScript generic on
/// `invoke<T>` is not wire validation).
pub fn parse_request<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, UiError> {
    serde_json::from_value(value).map_err(|error| UiError::invalid(error.to_string()))
}

/// Validates a renderer-supplied UUID string (epochs, subscription and request IDs).
pub fn parse_uuid(value: &str, field: &str) -> Result<uuid::Uuid, UiError> {
    uuid::Uuid::parse_str(value)
        .ok()
        .filter(|parsed| parsed.hyphenated().to_string() == value)
        .ok_or_else(|| UiError::invalid(format!("{field} must be a canonical lowercase UUID")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn heartbeat_is_a_bare_tag() {
        let body = serde_json::to_value(UiFrameBody::BridgeHeartbeat).expect("serializes");
        assert_eq!(body, json!({ "kind": "BridgeHeartbeat" }));
    }

    #[test]
    fn unknown_fields_are_malformed() {
        let result: Result<UiConnectRequest, _> = parse_request(json!({
            "protocolVersion": 1,
            "viewEpoch": "6f1b3c2e-4a5d-4e6f-8a9b-0c1d2e3f4a5b",
            "extra": true
        }));
        assert_eq!(result.expect_err("must reject").code, UiErrorCode::InvalidRequest);
    }

    #[test]
    fn unknown_action_kind_is_malformed_but_later_actions_parse() {
        let unknown: Result<UiAction, _> = parse_request(json!({ "kind": "RunShell", "cmd": "ls" }));
        assert_eq!(unknown.expect_err("must reject").code, UiErrorCode::InvalidRequest);

        let later: UiAction = parse_request(json!({ "kind": "ReturnToSession" })).expect("parses");
        assert_eq!(later, UiAction::ReturnToSession {});
    }

    // Serde ignores extra fields on internally tagged *unit* variants even with
    // `deny_unknown_fields`; empty struct variants are therefore used throughout.
    #[test]
    fn payloadless_actions_reject_unexpected_payloads() {
        let result: Result<UiAction, _> =
            parse_request(json!({ "kind": "ReturnToSession", "sessionId": "x" }));
        assert!(result.is_err(), "later actions carry no M0A payload");
        let query: Result<UiQuery, _> = parse_request(json!({ "kind": "Diagnostics", "sql": "x" }));
        assert!(query.is_err(), "queries accept no extra fields");
    }

    #[test]
    fn acknowledge_attention_requires_its_field() {
        let missing: Result<UiAction, _> = parse_request(json!({ "kind": "AcknowledgeAttention" }));
        assert!(missing.is_err());
        let ok: UiAction = parse_request(json!({
            "kind": "AcknowledgeAttention",
            "attentionId": "6f1b3c2e-4a5d-4e6f-8a9b-0c1d2e3f4a5b"
        }))
        .expect("parses");
        assert!(matches!(ok, UiAction::AcknowledgeAttention { .. }));
    }

    #[test]
    fn uuid_validation_is_canonical() {
        assert!(parse_uuid("6f1b3c2e-4a5d-4e6f-8a9b-0c1d2e3f4a5b", "viewEpoch").is_ok());
        assert!(parse_uuid("6F1B3C2E-4A5D-4E6F-8A9B-0C1D2E3F4A5B", "viewEpoch").is_err());
        assert!(parse_uuid("not-a-uuid", "viewEpoch").is_err());
    }

    #[test]
    fn error_detail_is_bounded() {
        let error = UiError::invalid("x".repeat(10_000));
        assert_eq!(error.detail.chars().count(), limits::ERROR_DETAIL_MAX_CHARS);
    }
}
