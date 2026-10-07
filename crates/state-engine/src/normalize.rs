//! What a provider adapter's pure normalization yields for one envelope.

use serde_json::Value;
use threadspace_contracts::canonical::fact::NativeFactDraft;

/// Drafts, the payload admission may retain, and why nothing was drafted.
///
/// `retained` is the adapter's allowlisted payload: admission journals the
/// envelope with it in place of whatever the source sent, so a body a
/// provider supplied (a prompt, tool input or output) is never persisted
/// merely because it arrived (SPEC §8.5, §19.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub drafts: Vec<NativeFactDraft>,
    pub retained: Value,
    /// Set when the event or a discriminator is unknown or malformed; the
    /// observation is retained as unsupported and drives no state.
    pub unsupported: Option<String>,
}

/// Keeps only `allowed` top-level keys of an object payload.
pub fn retain(payload: &Value, allowed: &[&str]) -> Value {
    match payload {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| allowed.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
        _ => Value::Object(serde_json::Map::new()),
    }
}
