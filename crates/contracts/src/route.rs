//! Return-to-Agent results and their evidence (SPEC §4.10, §13.1–§13.3).
//!
//! A route result keeps three independent axes: where focus landed
//! (`SurfaceResult`), whether the provider conversation in that surface was
//! freshly proven current (`SessionVerification`), and whether owner input
//! belongs there now (`InputReadiness`). An `osascript` exit status alone is
//! never a result; every passing route carries the chain of samples, lookups
//! and readbacks that produced it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum SurfaceResult {
    ExactNativeSurface,
    ExactWindow,
    AppOnly,
    UrlDispatched,
    ProjectOnly,
    InspectorOnly,
    Ambiguous,
    Unavailable,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum SessionVerification {
    CurrentNativeRevalidated,
    NativeBoundLastKnown,
    UserAttested,
    Unbound,
    Conflict,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum InputReadiness {
    ForegroundCompatible,
    BackgroundJob,
    Unknown,
}

/// SPEC §3.2 `ProcessKey`: endpoint, boot, PID and kernel birth.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessKey {
    pub endpoint_id: String,
    pub boot_id: String,
    pub pid: u32,
    pub start_seconds: String,
    pub start_microseconds: u32,
}

/// One kernel sample of a process, as recorded in evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessEvidence {
    pub pid: u32,
    pub ppid: u32,
    pub start_seconds: String,
    pub start_microseconds: u32,
    /// Canonical executable identity (`path#dev:ino`).
    pub executable: String,
    pub comm: String,
    /// `e_tdev`; `None` for no controlling terminal (NODEV).
    pub controlling_device: Option<u32>,
    pub pgid: u32,
    pub tpgid: u32,
    pub status: u32,
    pub sampled_at_ms: i64,
}

/// A process sample that failed, with its stable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum SampleEvidence {
    Sampled {
        sample: ProcessEvidence,
    },
    Failed {
        pid: u32,
        code: String,
        sampled_at_ms: i64,
    },
}

/// The fields of one provider inventory row that identity depends on.
/// Labels such as cwd and name are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InventoryRowEvidence {
    pub pid: Option<i64>,
    pub session_id: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
}

/// One bounded provider inventory request and what it said about the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InventoryEvidence {
    pub request_started_ms: i64,
    pub request_ended_ms: i64,
    pub binary: String,
    pub row_count: u32,
    /// Rows naming the target PID (normally exactly one).
    pub pid_rows: Vec<InventoryRowEvidence>,
    /// Rows naming the target session (more than one means multiple attachments).
    pub session_rows: Vec<InventoryRowEvidence>,
    pub error: Option<String>,
}

/// A native application incarnation (PID + kernel birth), e.g. Terminal.app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppGeneration {
    pub pid: u32,
    pub start_seconds: String,
    pub start_microseconds: u32,
}

impl AppGeneration {
    pub fn canonical(&self) -> String {
        format!(
            "{}@{}.{:06}",
            self.pid, self.start_seconds, self.start_microseconds
        )
    }
}

/// A Terminal tab whose TTY's character device matched (or was examined).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TerminalTabEvidence {
    pub window_id: i64,
    pub window_index: i64,
    pub tab_index: i64,
    pub selected: bool,
    pub tty: String,
    /// Normalized `st_rdev`, or `None` when the path did not stat as a
    /// character device.
    pub rdev: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TerminalEvidence {
    pub generation_before: Option<AppGeneration>,
    pub generation_after: Option<AppGeneration>,
    pub started_ms: i64,
    pub ended_ms: i64,
    /// PID of the `osascript` worker that sent the Apple events.
    pub sender_pid: Option<u32>,
    pub window_count: u32,
    pub tab_count: u32,
    /// Tabs whose `st_rdev` equals the provider's `e_tdev`.
    pub matches: Vec<TerminalTabEvidence>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FrontmostApplication {
    pub bundle_identifier: Option<String>,
    pub pid: i32,
}

/// The focus operation and its native readback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FocusEvidence {
    /// `FOCUSED`, `GONE`, `AMBIGUOUS`, `CHANGED` or `FAILED`.
    pub outcome: String,
    pub sender_pid: Option<u32>,
    pub started_ms: i64,
    pub elapsed_ms: u32,
    pub target_window_id: Option<i64>,
    pub target_tab_index: Option<i64>,
    pub front_window_id: Option<i64>,
    /// `tty` of the front window's selected tab, read back after focus.
    pub readback_tty: Option<String>,
    /// `st_rdev` of the read-back TTY.
    pub readback_rdev: Option<u32>,
    pub target_window_frontmost: Option<bool>,
    pub target_tab_selected: Option<bool>,
    /// Native frontmost application after focus (NSWorkspace).
    pub frontmost_application: Option<FrontmostApplication>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhaseTiming {
    pub phase: String,
    pub elapsed_ms: u32,
}

/// The reconstructible proof chain of one route (MILESTONES M0B "Evidence"):
/// Session → native ID → inventory → ProcessKey → executable → e_tdev → tty →
/// st_rdev → binding revision → readback → post-focus revalidation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteEvidence {
    pub native_session_id: Option<String>,
    pub binding_id: Option<String>,
    pub binding_revision_loaded: Option<String>,
    pub binding_revision_before_focus: Option<String>,
    pub binding_revision_after_focus: Option<String>,
    pub process_key: Option<ProcessKey>,
    /// Executable identity recorded when the binding was proven.
    pub bound_executable: Option<String>,
    /// Controlling device recorded when the binding was proven.
    pub bound_device: Option<u32>,
    pub pre_lookup_sample: Option<SampleEvidence>,
    pub lookup: Option<InventoryEvidence>,
    pub post_lookup_sample: Option<SampleEvidence>,
    pub terminal: Option<TerminalEvidence>,
    pub focus: Option<FocusEvidence>,
    pub post_focus_sample: Option<SampleEvidence>,
    pub post_focus_lookup: Option<InventoryEvidence>,
    pub phases: Vec<PhaseTiming>,
}

/// A live attachment offered when one Session has several (SPEC §4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BindingChoice {
    pub binding_id: String,
    pub revision: String,
    pub pid: u32,
    pub tty: String,
}

/// SPEC §13.1 `NativeRouteRequest`. The only fallback M0B supports is NONE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct RouteRequest {
    pub request_id: String,
    pub session_id: String,
    pub chosen_binding_id: Option<String>,
    pub expected_binding_revision: Option<String>,
}

/// SPEC §13.1 `RouteResult` plus its evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteResult {
    pub request_id: String,
    pub session_id: String,
    pub surface_result: SurfaceResult,
    pub session_verification: SessionVerification,
    pub input_readiness: InputReadiness,
    pub binding_id: Option<String>,
    pub validated_binding_revision: Option<String>,
    /// `OK`, `AUTOMATION_DENIED`, `TARGET_GONE`, `SESSION_CHANGED`,
    /// `MULTIPLE_ATTACHMENTS`, `NO_LIVE_MAPPING`, `READBACK_FAILED`, …
    pub reason_code: String,
    /// Whether any focus-changing Apple event was sent.
    pub focus_performed: bool,
    pub started_at_ms: i64,
    pub latency_ms: u32,
    /// Offered when the session has several live attachments.
    pub choices: Vec<BindingChoice>,
    pub evidence: RouteEvidence,
}

/// Compact last-route projection shown beside a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteSummary {
    pub request_id: String,
    pub surface_result: SurfaceResult,
    pub session_verification: SessionVerification,
    pub input_readiness: InputReadiness,
    pub reason_code: String,
    pub focus_performed: bool,
    pub latency_ms: u32,
    pub recorded_at_ms: i64,
}

/// Outcome of one discovery pass (SPEC §4.4), returned by `RefreshEvidence`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiscoverySummary {
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub inventory_rows: u32,
    pub joined: u32,
    pub provisional: Vec<ProvisionalCandidate>,
    pub executions_started: u32,
    pub executions_ended: u32,
    pub bindings_recorded: u32,
    pub bindings_invalidated: u32,
    pub surface_unbound: u32,
    pub committed_cursor: Option<String>,
    pub error: Option<String>,
}

/// An inventory row that did not earn a deterministic join, with why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProvisionalCandidate {
    pub pid: Option<i64>,
    pub session_id: Option<String>,
    pub kind: Option<String>,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_serialize_as_spec_names() {
        assert_eq!(
            serde_json::to_value(SurfaceResult::ExactNativeSurface).expect("json"),
            "EXACT_NATIVE_SURFACE"
        );
        assert_eq!(
            serde_json::to_value(SessionVerification::CurrentNativeRevalidated).expect("json"),
            "CURRENT_NATIVE_REVALIDATED"
        );
        assert_eq!(
            serde_json::to_value(InputReadiness::ForegroundCompatible).expect("json"),
            "FOREGROUND_COMPATIBLE"
        );
    }

    #[test]
    fn route_request_rejects_unknown_fields() {
        let parsed: Result<RouteRequest, _> = serde_json::from_value(serde_json::json!({
            "requestId": "r", "sessionId": "s", "chosenBindingId": null,
            "expectedBindingRevision": null, "allowedFallback": "APP"
        }));
        assert!(parsed.is_err(), "fallback beyond NONE is not accepted");
    }
}
