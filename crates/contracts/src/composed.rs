//! Schema 3 qualification messages. These never acknowledge stream delivery.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub run_id: String,
    pub dom_runtime_id: String,
    pub native_incarnation: String,
    pub subscription_id: String,
    pub view_epoch: String,
    pub core_generation: String,
    pub store_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revision {
    pub entity: String,
    pub id: String,
    pub revision: String,
    /// Absence is the applied visible state for removed/resolved rows.
    pub absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Witness {
    pub schema_version: u32,
    pub witness_id: String,
    pub context: Context,
    pub from_cursor: String,
    pub cursor: String,
    pub from_stream_seq: u32,
    pub stream_seq: u32,
    pub revisions: Vec<Revision>,
    pub pending_hydration: u32,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Request {
    Start { context: Context, cursor: String, stream_seq: u32 },
    Witness { witness: Witness },
    Close { context: Context, total_witnesses: u32, failures: u32 },
}

impl Request {
    pub fn context(&self) -> &Context {
        match self {
            Self::Start { context, .. } | Self::Close { context, .. } => context,
            Self::Witness { witness } => &witness.context,
        }
    }
}

pub fn valid_context(context: &Context) -> bool {
    [&context.run_id, &context.dom_runtime_id, &context.native_incarnation,
        &context.subscription_id, &context.view_epoch, &context.core_generation, &context.store_generation]
        .iter().all(|value| uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == **value))
}

pub fn valid_witness(witness: &Witness) -> bool {
    let mut ids = std::collections::BTreeSet::new();
    witness.schema_version == 3 && valid_context(&witness.context)
        && uuid::Uuid::parse_str(&witness.witness_id).is_ok_and(|id| id.to_string() == witness.witness_id)
        && witness.visible && witness.pending_hydration == 0
        && witness.from_stream_seq < witness.stream_seq
        && crate::cursor::parse_cursor(&witness.from_cursor).zip(crate::cursor::parse_cursor(&witness.cursor)).is_some_and(|(a, b)| a < b)
        && witness.revisions.len() <= 1024
        && witness.revisions.iter().all(|row| {
            matches!(row.entity.as_str(), "SESSION" | "ATTENTION")
                && uuid::Uuid::parse_str(&row.id).is_ok_and(|id| id.to_string() == row.id)
                && ids.insert((&row.entity, &row.id))
                && crate::cursor::parse_cursor(&row.revision).is_some()
        })
}
