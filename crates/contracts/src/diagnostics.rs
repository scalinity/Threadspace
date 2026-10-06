//! Diagnostics and integration-status records (SPEC §20.4). They describe
//! build identity, native permissions and engine state; they never carry
//! prompt text, payloads or credentials.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A process incarnation: PID alone is never identity (SPEC §4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub boot_id: String,
    pub start_seconds: String,
    pub start_microseconds: u32,
    pub executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SqliteDiagnostics {
    pub version: String,
    pub source_id: String,
    pub compile_options: Vec<String>,
    pub journal_mode: String,
    /// `PRAGMA synchronous`: 0 OFF, 1 NORMAL, 2 FULL, 3 EXTRA.
    pub synchronous: u8,
    pub foreign_keys: bool,
    pub fullfsync: bool,
    pub database_path: String,
    pub cursor: String,
    pub schema_version: u32,
}

/// Actual `UNUserNotificationCenter` settings as reported by the system for the
/// companion's own identity. Values are Apple's enum case names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotificationSettings {
    pub authorization_status: String,
    pub alert_setting: String,
    pub sound_setting: String,
    pub badge_setting: String,
    pub notification_center_setting: String,
    pub lock_screen_setting: String,
    pub alert_style: String,
}

/// AppKit accessibility display preferences read through the native bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccessibilityPreferences {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub reduce_transparency: bool,
    pub differentiate_without_color: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CompanionDiagnostics {
    pub bundle_identifier: String,
    pub process: ProcessIdentity,
    pub core_generation: String,
    pub store_generation: String,
    pub started_at_ms: i64,
    pub writer_lock_held: bool,
    pub sqlite: SqliteDiagnostics,
    pub notification_settings: Option<NotificationSettings>,
    pub accessibility_preferences: Option<AccessibilityPreferences>,
    pub qualification_build: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DesktopDiagnostics {
    pub app_identifier: String,
    pub app_version: String,
    pub tauri_version: String,
    pub os_product_version: String,
    pub os_build_version: String,
    pub webkit_version: Option<String>,
    pub executable_path: String,
    pub qualification_build: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TerminalDictionary {
    pub sha256: String,
    pub byte_length: u32,
    pub has_tab_class: bool,
    pub has_tty_property: bool,
    pub has_selected_tab_property: bool,
    pub has_frontmost_property: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TerminalInventorySummary {
    pub window_count: u32,
    pub tab_count: u32,
    pub tty_paths: Vec<String>,
    pub elapsed_ms: u32,
}

/// Apple-event automation consent for the companion's own identity, from
/// `AEDeterminePermissionToAutomateTarget`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum AutomationPermission {
    Authorized,
    Denied,
    RequiresConsent,
    TargetNotRunning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TerminalIntegration {
    pub application_path: Option<String>,
    pub application_version: Option<String>,
    pub running: bool,
    pub dictionary: Option<TerminalDictionary>,
    pub automation: AutomationPermission,
    pub automation_status_code: i32,
    pub inventory: Option<TerminalInventorySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CompanionIntegration {
    pub notification_settings: Option<NotificationSettings>,
    pub terminal: TerminalIntegration,
}

/// `SMAppService.Status` for the companion login item, as reported to the
/// outer application's native bootstrap path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ServiceStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServiceReport {
    pub agent_identifier: String,
    pub status: ServiceStatus,
}
