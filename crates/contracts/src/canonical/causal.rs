//! Scoped causal points (SPEC §5.4). Two points compare only within one
//! source, epoch and order domain, or through an explicit native predecessor
//! relation; everything else is incomparable. No vector clock is implied.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CausalPoint {
    pub source_id: String,
    pub source_epoch: String,
    /// The actual native or qualified observer counter scope.
    pub order_domain: String,
    /// Validated nonnegative decimal; compared numerically, never lexically.
    pub sequence: Option<String>,
    /// This point's own native object key, when predecessor relations name it.
    pub native_key: Option<String>,
    /// Native keys this point is known to follow.
    pub native_predecessor_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum CausalOrder {
    Before,
    After,
    Equal,
    Incomparable,
}
