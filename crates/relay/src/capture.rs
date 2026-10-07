//! Bounded, sanitizing capture (SPEC §8.2, §8.5, §19.4). Provider input is
//! parsed with a byte cap and a nesting-depth cap checked before any JSON
//! tree is built; only allowlisted identifiers and short codes are copied
//! into the envelope. Prompt bodies, tool inputs and results, assistant
//! text, transcripts, paths and messages are never copied.

use serde_json::{Map, Value, json};
use threadspace_contracts::canonical::envelope::{
    CaptureClock, ClockQuality, OBSERVATION_SCHEMA_VERSION, ObservationEnvelope, ProcessSample,
};
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_contracts::limits::capture::{OBSERVATION_MAX_BYTES, RAW_DEPTH_MAX, RAW_INPUT_MAX_BYTES};

pub const CLAUDE_HOOK_ADAPTER: &str = "claude.hook";
pub const CLAUDE_HOOK_ADAPTER_VERSION: &str = "1";
pub const CLAUDE_HOOK_SOURCE: &str = "claude.hook";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    TooLarge,
    TooDeep,
    Malformed,
    /// Required identity is missing or not a safe identifier.
    Unsafe(&'static str),
}

impl CaptureError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "too-large",
            Self::TooDeep => "too-deep",
            Self::Malformed => "malformed",
            Self::Unsafe(_) => "unsafe",
        }
    }
}

/// The maximum container nesting of a JSON text, ignoring brackets inside
/// strings. A scan, not a parse: it allocates nothing.
pub fn nesting_depth(bytes: &[u8]) -> usize {
    let (mut depth, mut max, mut in_string, mut escaped) = (0usize, 0usize, false, false);
    for &byte in bytes {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                max = max.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

/// Parses provider input within the frozen byte and depth bounds.
pub fn parse_bounded(bytes: &[u8]) -> Result<Value, CaptureError> {
    if bytes.len() > RAW_INPUT_MAX_BYTES {
        return Err(CaptureError::TooLarge);
    }
    if nesting_depth(bytes) > RAW_DEPTH_MAX {
        return Err(CaptureError::TooDeep);
    }
    serde_json::from_slice(bytes).map_err(|_| CaptureError::Malformed)
}

/// An opaque provider identifier: bounded, printable, no separators that
/// could smuggle text (`/`, whitespace, quotes).
fn identifier(value: &Value, max: usize) -> Option<String> {
    let text = value.as_str()?;
    let ok = !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':' | b'@'));
    ok.then(|| text.to_owned())
}

/// A short category code (hook event names, sources, tool names).
fn code(value: &Value, max: usize) -> Option<String> {
    identifier(value, max)
}

fn flag(value: &Value) -> Option<bool> {
    value.as_bool()
}

/// The sanitized metadata of one Claude conventional hook invocation.
/// Every field here is an identifier, a code or a flag.
fn claude_metadata(input: &Value) -> Map<String, Value> {
    let read = |field: &str| input.get(field);
    let fields = [
        ("source", read("source").and_then(|v| code(v, 32)).map(Value::from)),
        ("reason", read("reason").and_then(|v| code(v, 48)).map(Value::from)),
        ("stopHookActive", read("stop_hook_active").and_then(flag).map(Value::from)),
        ("agentType", read("agent_type").and_then(|v| code(v, 64)).map(Value::from)),
        ("toolCategory", read("tool_name").and_then(|v| code(v, 64)).map(Value::from)),
        ("notificationType", read("notification_type").and_then(|v| code(v, 64)).map(Value::from)),
        ("permissionMode", read("permission_mode").and_then(|v| code(v, 32)).map(Value::from)),
        ("isInterrupt", read("is_interrupt").and_then(flag).map(Value::from)),
    ];
    fields
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| (name.to_owned(), v)))
        .collect()
}

/// Inputs to one Claude hook envelope besides the provider's own JSON.
pub struct HookCapture {
    pub observation_id: String,
    pub profile_ref: String,
    pub clock: CaptureClock,
    pub evidence: Vec<ProcessSample>,
}

/// Builds the sanitized envelope for a Claude conventional hook. Each hook
/// process is independent: no source sequence is manufactured (SPEC §5.4).
pub fn claude_hook_envelope(input: &Value, capture: HookCapture) -> Result<ObservationEnvelope, CaptureError> {
    let event = input
        .get("hook_event_name")
        .and_then(|v| code(v, 64))
        .ok_or(CaptureError::Unsafe("hook_event_name"))?;
    let session = input
        .get("session_id")
        .and_then(|v| identifier(v, 128))
        .ok_or(CaptureError::Unsafe("session_id"))?;
    let envelope = ObservationEnvelope {
        schema_version: OBSERVATION_SCHEMA_VERSION,
        observation_id: capture.observation_id,
        source_id: CLAUDE_HOOK_SOURCE.into(),
        source_epoch: "hook".into(),
        source_sequence: None,
        sequence_meaning: None,
        callback_entry_sequence: None,
        callback_result_sequence: None,
        adapter_id: CLAUDE_HOOK_ADAPTER.into(),
        adapter_version: CLAUDE_HOOK_ADAPTER_VERSION.into(),
        provider_version: None,
        native_event: event,
        session_key: Some(NativeSessionRef {
            provider: "claude".into(),
            profile_ref: capture.profile_ref,
            native_session_id: session,
        }),
        actor_native_id: input.get("agent_id").and_then(|v| identifier(v, 128)),
        native_turn_id: None,
        native_prompt_id: input.get("prompt_id").and_then(|v| identifier(v, 128)),
        native_occurrence_id: input.get("tool_use_id").and_then(|v| identifier(v, 128)),
        activation_ref: None,
        captured_at: capture.clock,
        evidence: capture.evidence,
        payload: Value::Object(claude_metadata(input)),
    };
    let size = serde_json::to_vec(&envelope).map_or(usize::MAX, |b| b.len());
    if size > OBSERVATION_MAX_BYTES {
        return Err(CaptureError::TooLarge);
    }
    Ok(envelope)
}

/// A capture clock from this process's own reading.
pub fn local_clock(boot_id: Option<String>, wall_time_ms: i64, monotonic_ns: Option<u64>) -> CaptureClock {
    CaptureClock {
        endpoint_id: None,
        boot_id,
        monotonic_ns: monotonic_ns.map(|ns| ns.to_string()),
        wall_time_ms,
        clock_quality: if monotonic_ns.is_some() {
            ClockQuality::LocalMonotonic
        } else {
            ClockQuality::ReceiptOnly
        },
    }
}

/// A representative hook input for tests and benchmarks (no real content).
pub fn sample_hook_input(event: &str, session: &str) -> Value {
    json!({ "hook_event_name": event, "session_id": session, "transcript_path": "/x", "cwd": "/x" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> HookCapture {
        HookCapture {
            observation_id: "00000000-0000-4000-8000-000000000001".into(),
            profile_ref: "claude-cli:~/.claude".into(),
            clock: local_clock(None, 1, None),
            evidence: Vec::new(),
        }
    }

    #[test]
    fn depth_is_scanned_without_counting_brackets_inside_strings() {
        assert_eq!(nesting_depth(br#"{"a":[1,{"b":"[[[{{{"}]}"#), 3);
        let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
        assert_eq!(parse_bounded(deep.as_bytes()), Err(CaptureError::TooDeep));
        assert!(parse_bounded(br#"{"ok":true}"#).is_ok());
        assert_eq!(parse_bounded(b"{nope"), Err(CaptureError::Malformed));
    }

    #[test]
    fn bodies_and_paths_are_never_copied() {
        let input = json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "8a1f5e2c-0000-4000-8000-00000000abcd",
            "prompt": "SECRET prompt body",
            "prompt_id": "p-1",
            "tool_input": { "command": "cat ~/.ssh/id_ed25519" },
            "tool_response": "SECRET output",
            "last_assistant_message": "SECRET answer",
            "transcript_path": "/Users/someone/.claude/projects/x.jsonl",
            "cwd": "/Users/someone/private",
            "tool_name": "Bash",
            "message": "SECRET notification text",
        });
        let envelope = claude_hook_envelope(&input, capture()).expect("envelope");
        let text = serde_json::to_string(&envelope).expect("json");
        for secret in ["SECRET", "id_ed25519", "/Users/someone", ".jsonl"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert_eq!(envelope.native_prompt_id.as_deref(), Some("p-1"));
        assert_eq!(envelope.payload["toolCategory"], "Bash");
    }

    #[test]
    fn identifiers_that_could_carry_text_are_refused() {
        let input = json!({ "hook_event_name": "Stop", "session_id": "has spaces / slash" });
        assert_eq!(claude_hook_envelope(&input, capture()).err(), Some(CaptureError::Unsafe("session_id")));
        let input = json!({ "hook_event_name": "Stop", "session_id": "ok-1", "tool_name": "rm -rf /" });
        let envelope = claude_hook_envelope(&input, capture()).expect("envelope");
        assert!(envelope.payload.get("toolCategory").is_none(), "unsafe code dropped, not truncated");
    }
}
