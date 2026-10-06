//! The private companion control protocol on `control.sock` (SPEC §8.1).
//! Length-prefixed versioned JSON frames between the companion and its
//! registered native clients. The renderer never speaks this protocol.

use serde::{Deserialize, Serialize};

use crate::diagnostics::{
    AutomationPermission, CompanionDiagnostics, CompanionIntegration, NotificationSettings,
    ProcessIdentity,
};
use crate::projection::{FleetSnapshot, NativeIntent, ProjectionPatch};
use crate::route::{DiscoverySummary, RouteRequest, RouteResult};
use crate::ui::CommandReceipt;

pub const CONTROL_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientRole {
    /// The desktop shell's native bridge.
    Ui,
    /// The outer application's native bootstrap (SPEC §18.9, §19.5): service
    /// preparation, observation preference and maintenance ordering.
    Bootstrap,
    /// The qualification harness; accepted only by qualification builds.
    Qualification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlRequest {
    pub request_id: u64,
    pub body: ControlRequestBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ControlRequestBody {
    /// Must be the first request. `expected_core_generation` comes from the
    /// owner-only runtime locator; a mismatch means a stale locator.
    Hello {
        protocol_version: u32,
        role: ClientRole,
        expected_core_generation: String,
    },
    /// Serialized on the single writer: capture the projection at committed
    /// cursor S, register the subscription, and reply before any change after S.
    AttachView {
        subscription_id: String,
    },
    DetachView {
        subscription_id: String,
    },
    /// The renderer applied the snapshot; pending native intents may now flow.
    ViewHydrated {
        subscription_id: String,
    },
    IntentConsumed {
        intent_id: String,
    },
    /// A page of sessions/open attention for a bounded view (SPEC §18.4).
    FleetPage {
        after: Option<String>,
        limit: u32,
    },
    AttentionPage {
        after: Option<String>,
        limit: u32,
    },
    Diagnostics,
    IntegrationStatus,
    AcknowledgeAttention {
        command_id: String,
        attention_id: String,
        expected_revision: Option<String>,
    },
    RequestNotificationAuthorization,
    RequestTerminalAutomation,
    /// Return-to-Agent (SPEC §13.2): the companion revalidates the target
    /// against fresh native evidence, focuses it and reads it back.
    ReturnToSession {
        route: RouteRequest,
    },
    /// Run one discovery pass now (SPEC §4.4, §4.14) and report it.
    RefreshEvidence,
    /// Explicit owner resolution ("Mark handled", SPEC §7.2); always carries a reason.
    ResolveAttention {
        command_id: String,
        attention_id: String,
        expected_revision: Option<String>,
        reason: String,
    },
    /// Bootstrap only (SPEC §19.5): durably record the phase, gate new
    /// admission, finish accepted work and create a consistent backup while
    /// still supervised; answers `MaintenancePrepared` and keeps running,
    /// holding the writer lock. Nothing remains to be done after the answer.
    PrepareMaintenance {
        purpose: MaintenancePurpose,
    },
    /// Bootstrap only: leave a prepared maintenance phase and reopen admission.
    CancelMaintenance,
    /// Bootstrap only: durably record the owner's observation preference
    /// (`EnableObservation` / the stop half of `StopObservation`).
    SetObservationEnabled {
        enabled: bool,
    },
    /// Sent by a second companion instance that lost the writer lock: it
    /// forwards a notification response it received to the verified incumbent.
    ForwardNotificationResponse {
        notification_request_id: String,
        attention_id: String,
    },
    /// Qualification only: commit a synthetic fixture turn plus a completed-turn
    /// attention item and its notification intent.
    #[cfg(feature = "qualification")]
    QualifyRaiseAttention {
        label: String,
        /// Attach the item to an existing (for example provider-observed)
        /// Session instead of the synthetic fixture Session.
        session_id: Option<String>,
    },
    /// Qualification only: commit `count` synthetic fixture changes spread
    /// over `duration_ms`, each its own transaction and broadcast.
    #[cfg(feature = "qualification")]
    QualifySyntheticChanges {
        count: u32,
        duration_ms: u32,
        sessions: u32,
    },
    /// Qualification only: add `sessions` synthetic fixture Sessions with
    /// `name_bytes`-long names (oversized-snapshot fixtures).
    #[cfg(feature = "qualification")]
    QualifyPopulate {
        sessions: u32,
        name_bytes: u32,
    },
    /// Qualification only: deliver a qualification command to hydrated views
    /// as a native intent (renderer fault injection, self-tests).
    #[cfg(feature = "qualification")]
    QualifyViewCommand {
        command: String,
        args: serde_json::Value,
    },
    /// Qualification only: arm a one-shot fault in the companion.
    #[cfg(feature = "qualification")]
    QualifyArmFault {
        fault: QualificationFault,
    },
    /// Qualification only: durably admit one fixture record by UUID and
    /// answer with its receipt after COMMIT (no attention, no notification).
    /// A repeat returns ALREADY_COMMITTED at the original cursor.
    #[cfg(feature = "qualification")]
    QualifyAdmit {
        observation_id: String,
        /// The record's capture time; a retry repeats it, as a real capture does.
        captured_wall_ms: i64,
    },
    /// Qualification only: remove this companion's own delivered and pending
    /// notifications from Notification Center.
    #[cfg(feature = "qualification")]
    QualifyClearNotifications,
    /// Qualification only: journaled identity and route observations after a
    /// cursor, for the evidence ledger.
    #[cfg(feature = "qualification")]
    QualifyExportObservations {
        after_cursor: String,
        limit: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum ControlResponseBody {
    HelloAck {
        protocol_version: u32,
        core_generation: String,
        store_generation: String,
        companion: ProcessIdentity,
    },
    ViewAttached {
        subscription_id: String,
        cursor: String,
        snapshot: FleetSnapshot,
    },
    Done,
    FleetPage {
        page: crate::ui::FleetPage,
    },
    AttentionPage {
        page: crate::ui::AttentionPage,
    },
    Diagnostics {
        report: Box<CompanionDiagnostics>,
    },
    IntegrationStatus {
        report: CompanionIntegration,
    },
    CommandReceipt {
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
    Routed {
        result: Box<RouteResult>,
    },
    EvidenceRefreshed {
        summary: DiscoverySummary,
    },
    #[cfg(feature = "qualification")]
    ObservationsExported {
        observations: Vec<serde_json::Value>,
    },
    MaintenancePrepared {
        report: Box<MaintenanceReport>,
    },
    #[cfg(feature = "qualification")]
    SyntheticChangesStarted {
        run_id: String,
        first_cursor: String,
    },
    #[cfg(feature = "qualification")]
    Populated {
        sessions: u32,
        cursor: String,
    },
    #[cfg(feature = "qualification")]
    Admitted {
        observation_id: String,
        status: crate::ui::ReceiptStatus,
        cursor: String,
    },
    #[cfg(feature = "qualification")]
    NotificationsCleared {
        removed: u32,
    },
    #[cfg(feature = "qualification")]
    ViewCommandQueued {
        intent_id: String,
        hydrated_views: u32,
    },
    #[cfg(feature = "qualification")]
    AttentionRaised {
        attention_id: String,
        session_id: String,
        notification_request_id: String,
        cursor: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum MaintenancePurpose {
    /// Stop Observation: prepare, then unregister; no migration.
    Stop,
    /// A staged update to `target_version` (M15 builds the full updater).
    Update { target_version: String },
}

/// Durable maintenance phase recorded by the single writer (SPEC §19.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MaintenancePhase {
    None,
    Preparing,
    Prepared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceReport {
    pub phase: MaintenancePhase,
    pub purpose: MaintenancePurpose,
    pub backup_file: String,
    pub backup_cursor: String,
    pub backup_sha256: String,
    pub backup_bytes: u64,
    pub prepared_at_ms: i64,
    pub companion: ProcessIdentity,
}

/// One-shot faults a qualification build can arm (never present in release).
#[cfg(feature = "qualification")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QualificationFault {
    /// The next `PrepareMaintenance` fails while creating its backup.
    FailNextMaintenanceBackup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlErrorCode {
    BadRequest,
    UnsupportedProtocol,
    StaleGeneration,
    RoleNotPermitted,
    HelloRequired,
    UnknownSubscription,
    NotFound,
    Conflict,
    Busy,
    Unavailable,
    /// Admission is gated by a prepared maintenance phase.
    MaintenanceGated,
    /// Observation is disabled; only control/inspection operations run.
    ObservationDisabled,
    /// This companion was not started by its login item (a notification
    /// cold start); it hands the store to the login item's companion.
    NotSupervised,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlError {
    pub code: ControlErrorCode,
    pub detail: String,
}

impl ControlError {
    pub fn new(code: ControlErrorCode, detail: impl Into<String>) -> Self {
        let detail: String = detail.into();
        Self {
            code,
            detail: detail
                .chars()
                .take(crate::limits::ERROR_DETAIL_MAX_CHARS)
                .collect(),
        }
    }
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ControlError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlOutcome {
    Ok(Box<ControlResponseBody>),
    Err(ControlError),
}

/// Everything the companion writes to a client connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum ControlMessage {
    Response {
        request_id: u64,
        outcome: ControlOutcome,
    },
    ViewPatch {
        subscription_id: String,
        cursor: String,
        patch: Box<ProjectionPatch>,
    },
    Intent {
        subscription_id: String,
        cursor: String,
        intent: NativeIntent,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let request = ControlRequest {
            request_id: 7,
            body: ControlRequestBody::AttachView {
                subscription_id: "s".into(),
            },
        };
        let json = serde_json::to_string(&request).expect("serializes");
        assert_eq!(
            json,
            r#"{"requestId":7,"body":{"kind":"AttachView","subscriptionId":"s"}}"#
        );
        let back: ControlRequest = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, request);
    }

    #[test]
    fn unknown_request_kinds_are_rejected() {
        let result: Result<ControlRequest, _> =
            serde_json::from_str(r#"{"requestId":1,"body":{"kind":"FocusTerminal"}}"#);
        assert!(result.is_err());
    }

    #[cfg(not(feature = "qualification"))]
    #[test]
    fn qualification_requests_do_not_parse_in_release_builds() {
        let result: Result<ControlRequest, _> = serde_json::from_str(
            r#"{"requestId":1,"body":{"kind":"QualifyRaiseAttention","label":"x"}}"#,
        );
        assert!(result.is_err());
    }
}
