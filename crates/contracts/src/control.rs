//! The private companion control protocol on `control.sock` (SPEC §8.1).
//! Length-prefixed versioned JSON frames between the companion and its
//! registered native clients. The renderer never speaks this protocol.

use serde::{Deserialize, Serialize};

use crate::diagnostics::{
    AutomationPermission, CompanionDiagnostics, CompanionIntegration, NotificationSettings,
    ProcessIdentity,
};
use crate::projection::{FleetSnapshot, NativeIntent, ProjectionPatch};
use crate::ui::CommandReceipt;

pub const CONTROL_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClientRole {
    /// The desktop shell's native bridge.
    Ui,
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
    Diagnostics,
    IntegrationStatus,
    AcknowledgeAttention {
        command_id: String,
        attention_id: String,
        expected_revision: Option<String>,
    },
    RequestNotificationAuthorization,
    RequestTerminalAutomation,
    /// Qualification only: commit a synthetic fixture turn plus a completed-turn
    /// attention item and its notification intent.
    #[cfg(feature = "qualification")]
    QualifyRaiseAttention {
        label: String,
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
    Diagnostics {
        report: CompanionDiagnostics,
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
    #[cfg(feature = "qualification")]
    AttentionRaised {
        attention_id: String,
        session_id: String,
        notification_request_id: String,
        cursor: String,
    },
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
