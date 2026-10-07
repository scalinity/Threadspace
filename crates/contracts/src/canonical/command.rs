//! Owner commands as durable journal facts (SPEC §5.5, §7.2, §18.3). A
//! committed command stays retrievable and replayable by its command ID; a
//! retry with the same payload returns the original result even after the
//! target's revision advanced.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum OwnerAction {
    Acknowledge,
    /// "Mark handled": explicit resolution, always with a reason.
    Resolve { reason: String },
    /// Presentation/notification eligibility only; never provider state.
    Snooze { until_ms: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OwnerCommand {
    pub command_id: String,
    pub attention_id: String,
    /// The targeted item's revision when the owner acted; checked only for a
    /// new command, never for a retry of a committed one.
    pub expected_revision: Option<String>,
    pub action: OwnerAction,
}
