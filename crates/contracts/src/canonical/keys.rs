//! Native identity keys as a provider or native source supplies them (SPEC
//! §3.2, §3.3, §4.1). These never carry canonical UUIDs, except the explicit
//! `Canonical` references a reconciler uses for records it already holds.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A provider conversation as the provider names it, within one profile on
/// the local endpoint. The namespace UUID is resolved at admission.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeSessionRef {
    pub provider: String,
    /// The provider profile/store authority (for example a config home).
    pub profile_ref: String,
    /// Opaque, even when it looks like a UUID.
    pub native_session_id: String,
}

/// An Actor within its owning Session: the principal worker, or a
/// subordinate named by its native agent ID. `agent_type` is never a key.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum NativeActorRef {
    Principal,
    Agent { native_agent_id: String },
}

/// One logical activation of a Session/Actor in a runtime (SPEC §4.2). The
/// activation is keyed by what the native source proves, never by PID alone.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum NativeExecutionRef {
    /// A native activation reference (a runtime/thread activation ID, or the
    /// observation that proved a new activation of a process).
    Activation { activation_ref: String },
    /// A record a reconciler already holds; admission verifies it exists.
    Canonical { execution_id: String },
}

/// A native surface (terminal tab, pane, window). The locator (a TTY path)
/// is not identity: the application generation and the adapter's surface
/// generation are part of the key, so a reused path is a new surface.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeSurfaceRef {
    pub surface_kind: String,
    pub app_generation: String,
    pub locator: String,
    pub device_number: Option<u32>,
    pub surface_generation: String,
}
