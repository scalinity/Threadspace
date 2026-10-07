//! The observation envelope (SPEC §5.1): one captured source occurrence with
//! its stable UUID, source/epoch/sequence namespaces, native identifiers and
//! sanitized, allowlisted metadata. Raw provider input never travels in it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::keys::NativeSessionRef;
use crate::route::ProcessKey;

/// The envelope contract version this build writes and accepts.
pub const OBSERVATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ClockQuality {
    LocalMonotonic,
    RemoteReported,
    ReceiptOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaptureClock {
    /// Empty for a local capture; admission fills in the local endpoint.
    pub endpoint_id: Option<String>,
    pub boot_id: Option<String>,
    /// Decimal nanoseconds on the capturing boot's monotonic clock.
    pub monotonic_ns: Option<String>,
    pub wall_time_ms: i64,
    pub clock_quality: ClockQuality,
}

/// Whose counter `sourceSequence` is: the provider's own, or one qualified
/// observer's capture order. A capture helper never numbers provider order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum SequenceMeaning {
    Native,
    ObserverCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ProcessRole {
    /// The capturing helper itself.
    Capture,
    /// A validated ancestor edge (SPEC §4.5).
    Ancestor,
    /// The qualified provider runtime selected by an adapter rule.
    Provider,
}

/// One kernel process sample carried as capture evidence. A controlling
/// device is a device number; a TTY path is never captured here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessSample {
    pub role: ProcessRole,
    pub key: ProcessKey,
    pub parent_pid: Option<u32>,
    pub executable: Option<String>,
    pub controlling_device: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ObservationEnvelope {
    pub schema_version: u32,
    /// Generated once at capture and retained through every retry and spool.
    pub observation_id: String,
    pub source_id: String,
    pub source_epoch: String,
    /// Only when the source really supplies it.
    pub source_sequence: Option<String>,
    pub sequence_meaning: Option<SequenceMeaning>,
    pub callback_entry_sequence: Option<String>,
    pub callback_result_sequence: Option<String>,
    pub adapter_id: String,
    pub adapter_version: String,
    pub provider_version: Option<String>,
    pub native_event: String,
    pub session_key: Option<NativeSessionRef>,
    pub actor_native_id: Option<String>,
    pub native_turn_id: Option<String>,
    pub native_prompt_id: Option<String>,
    pub native_occurrence_id: Option<String>,
    pub activation_ref: Option<String>,
    pub captured_at: CaptureClock,
    pub evidence: Vec<ProcessSample>,
    /// Allowlisted, sanitized provider metadata. Never prompt or tool bodies.
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
}
