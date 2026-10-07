//! The capture relay protocol on the private event socket (SPEC §8.1–§8.4)
//! and the mod-batch receipt (SPEC §8.3). The event socket accepts
//! observations and returns per-record receipts; it can execute nothing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::envelope::ObservationEnvelope;

pub const CAPTURE_PROTOCOL_VERSION: u32 = 1;
pub const MOD_BATCH_RECEIPT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaptureBatch {
    pub protocol_version: u32,
    pub records: Vec<ObservationEnvelope>,
}

/// One record's durability. `LocalSpooled` means published in the local
/// spool's durability domain, not committed to the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum RecordStatus {
    Committed,
    AlreadyCommitted,
    LocalSpooled,
    NotAccepted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordReceipt {
    pub observation_id: String,
    pub status: RecordStatus,
    /// A bounded reason code for `NotAccepted`.
    pub reason: Option<String>,
}

/// Why the companion refused a whole batch without reading its records. The
/// sender keeps every record and spools it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum CaptureRefusal {
    AdmissionClosed,
    Busy,
    ProtocolMismatch,
    Malformed,
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum CaptureReply {
    /// Sent only after the journal transaction committed.
    Receipts {
        protocol_version: u32,
        receipts: Vec<RecordReceipt>,
    },
    Refused {
        protocol_version: u32,
        code: CaptureRefusal,
    },
}

/// The typed receipt `threadspace-hook mod-batch` prints for the calling mod.
/// Exit status, a missing receipt and a timeout are never acceptance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModBatchReceipt {
    pub receipt_version: u32,
    pub receipts: Vec<RecordReceipt>,
}
