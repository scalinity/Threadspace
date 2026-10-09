//! `threadspace-hook mod-batch` (SPEC §8.3): the observer mod's bounded batch
//! becomes observation envelopes, delivered to the companion within the
//! caller's budget, spooled when they cannot be, and answered with exactly
//! one typed result per submitted record. Exit status is never acceptance.
//!
//! The mod's records already carry only allowlisted metadata; each is still
//! checked here (identifiers, codes, bounded scalars) before it can become
//! an envelope, and one that fails is answered NOT_ACCEPTED without
//! touching the rest. Each envelope names the provider process this helper's
//! own kernel ancestry found, never what the mod claims.

use std::collections::BTreeSet;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use threadspace_contracts::canonical::capture::{
    MOD_BATCH_RECEIPT_VERSION, ModBatchReceipt, RecordReceipt, RecordStatus,
};
use threadspace_contracts::canonical::envelope::{
    CaptureClock, OBSERVATION_SCHEMA_VERSION, ObservationEnvelope, ProcessSample, SequenceMeaning,
};
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_contracts::limits::capture::{BATCH_MAX_RECORDS, FRAME_MAX_BYTES, OBSERVATION_MAX_BYTES};

pub const OBSERVER_SOURCE: &str = "claude.observer";
pub const OBSERVER_ADAPTER: &str = "threadspace-observer";
/// The mod's own envelope kind and version (packages/provider-mod).
pub const BATCH_KIND: &str = "mod-batch";
/// The synthetic event recording records the mod's bounded queue evicted.
pub const QUEUE_DROPPED_EVENT: &str = "observer.queue-dropped";

const EVENTS: &[&str] = &[
    "session.start",
    "classic.SessionStart",
    "session.end",
    "session.attach",
    "session.detach",
    "prompt.submit",
    "turn.start",
    "turn.step",
    "turn.complete",
    "tool.call",
    "tool.check",
    "agent.spawn",
];
const PHASES: &[&str] = &["entry", "result", "bootstrap", "provider-error", "abandoned"];
const MAX_TEXT: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModBatchRequest {
    pub receipt_version: u32,
    pub kind: String,
    pub source_epoch: String,
    pub dropped_records: u64,
    pub records: Vec<Value>,
}

/// What the helper itself knows about this invocation.
pub struct BatchContext {
    /// `claude-cli:<home>/.claude`, the namespace inventory uses.
    pub profile_ref: String,
    pub clock: CaptureClock,
    /// The helper's own kernel sample and its parent's, the process that
    /// ran it (`$.process.run`), as the provider. The adapter reads the
    /// provider's version from that executable's path.
    pub evidence: Vec<ProcessSample>,
}

fn uuid_like(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// An opaque identifier: bounded, printable, no separators that could carry
/// text.
fn identifier(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    let ok = !text.is_empty()
        && text.len() <= MAX_TEXT
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':' | b'@'));
    ok.then(|| text.to_owned())
}

fn decimal(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (!text.is_empty() && text.len() <= 20 && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.to_owned())
}

/// A bounded scalar: a short printable string, a number, a flag or null.
fn scalar(value: &Value) -> bool {
    match value {
        Value::String(text) => text.len() <= MAX_TEXT && !text.chars().any(char::is_control),
        Value::Number(_) | Value::Bool(_) | Value::Null => true,
        _ => false,
    }
}

/// The mod's payload shape: scalars, or objects of scalars (SPEC §8.5).
fn bounded_payload(value: &Value) -> Option<Map<String, Value>> {
    let map = value.as_object()?;
    let ok = map.iter().all(|(key, value)| {
        key.len() <= 64
            && match value {
                Value::Object(inner) => inner.iter().all(|(k, v)| k.len() <= 64 && scalar(v)),
                other => scalar(other),
            }
    });
    ok.then(|| map.clone())
}

fn optional_identifier(record: &Value, key: &str) -> Result<Option<String>, &'static str> {
    match record.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => identifier(value).map(Some).ok_or("UNSAFE_IDENTIFIER"),
    }
}

/// One record's envelope, or the reason it is not accepted.
pub fn envelope(record: &Value, batch_epoch: &str, context: &BatchContext) -> Result<ObservationEnvelope, &'static str> {
    let text = |key: &str| record.get(key).and_then(Value::as_str);
    if record.get("schemaVersion").and_then(Value::as_u64) != Some(1) || text("adapterId") != Some(OBSERVER_ADAPTER) {
        return Err("UNSUPPORTED_RECORD");
    }
    let observation_id = text("observationId").filter(|id| uuid_like(id)).ok_or("BAD_OBSERVATION_ID")?;
    let source_epoch = text("sourceEpoch").filter(|e| *e == batch_epoch && uuid_like(e)).ok_or("BAD_SOURCE_EPOCH")?;
    let entry = record.get("callbackEntrySequence").and_then(decimal).ok_or("BAD_SEQUENCE")?;
    let result = match record.get("callbackResultSequence") {
        None | Some(Value::Null) => None,
        Some(value) => Some(decimal(value).ok_or("BAD_SEQUENCE")?),
    };
    let phase = text("phase").filter(|p| PHASES.contains(p)).ok_or("BAD_PHASE")?;
    let event = text("nativeEvent").filter(|e| EVENTS.contains(e)).ok_or("BAD_EVENT")?;
    let origin = record.get("dispatchOrigin").ok_or("BAD_ORIGIN")?;
    let plugin = origin.get("plugin").and_then(identifier);
    let tier = origin.get("tier").and_then(identifier);
    let (Some(plugin), Some(tier)) = (plugin, tier) else {
        return Err("BAD_ORIGIN");
    };
    let engine_dispatch = record.get("engineDispatch").and_then(Value::as_bool).ok_or("BAD_ORIGIN")?;
    let detail = bounded_payload(record.get("payload").unwrap_or(&Value::Null)).ok_or("UNBOUNDED_PAYLOAD")?;
    let session_source = optional_identifier(record, "sessionIdSource")?;
    let generation = record.get("sessionGeneration").and_then(Value::as_u64).ok_or("BAD_GENERATION")?;
    // The frozen entry context names the session; a fresh load's bootstrap
    // has none yet and names the host read it made (a lower tier).
    let session = match optional_identifier(record, "sessionId")? {
        Some(session) => Some(session),
        None if phase == "bootstrap" => detail.get("hostSessionId").and_then(identifier),
        None => None,
    };
    let payload = Value::Object(Map::from_iter([
        ("phase".to_owned(), Value::from(phase)),
        ("dispatchPlugin".to_owned(), Value::from(plugin)),
        ("dispatchTier".to_owned(), Value::from(tier)),
        ("engineDispatch".to_owned(), Value::from(engine_dispatch)),
        ("sessionIdSource".to_owned(), session_source.map_or(Value::Null, Value::from)),
        ("sessionGeneration".to_owned(), Value::from(generation)),
        ("detail".to_owned(), Value::Object(detail)),
    ]));
    let envelope = ObservationEnvelope {
        schema_version: OBSERVATION_SCHEMA_VERSION,
        observation_id: observation_id.to_owned(),
        source_id: OBSERVER_SOURCE.to_owned(),
        source_epoch: source_epoch.to_owned(),
        source_sequence: Some(result.clone().unwrap_or_else(|| entry.clone())),
        sequence_meaning: Some(SequenceMeaning::ObserverCapture),
        callback_entry_sequence: Some(entry.clone()),
        callback_result_sequence: result,
        adapter_id: OBSERVER_ADAPTER.to_owned(),
        adapter_version: optional_identifier(record, "adapterVersion")?.unwrap_or_default(),
        provider_version: None,
        native_event: event.to_owned(),
        session_key: session.map(|native_session_id| NativeSessionRef {
            provider: "claude".to_owned(),
            profile_ref: context.profile_ref.clone(),
            native_session_id,
        }),
        actor_native_id: optional_identifier(record, "actorNativeId")?,
        native_turn_id: optional_identifier(record, "nativeTurnId")?,
        // A submission has no native input ID: its callback entry names it,
        // and the result record shares that entry.
        native_prompt_id: (event == "prompt.submit").then(|| format!("observer:{source_epoch}:{entry}")),
        native_occurrence_id: optional_identifier(record, "nativeOccurrenceId")?,
        activation_ref: None,
        captured_at: context.clock.clone(),
        evidence: context.evidence.clone(),
        payload,
    };
    let size = serde_json::to_vec(&envelope).map_or(usize::MAX, |bytes| bytes.len());
    if size > OBSERVATION_MAX_BYTES {
        return Err("TOO_LARGE");
    }
    Ok(envelope)
}

/// A canonical lowercase UUID derived from a name, so a retried batch's
/// marker is the same observation.
fn derived_uuid(name: &str) -> String {
    let digest = Sha256::digest(name.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// The observation recording how many records the mod's queue evicted
/// before this batch (SPEC §8.3: loss is recorded, never hidden).
pub fn queue_dropped(source_epoch: &str, dropped: u64, context: &BatchContext) -> ObservationEnvelope {
    ObservationEnvelope {
        schema_version: OBSERVATION_SCHEMA_VERSION,
        observation_id: derived_uuid(&format!("threadspace-observer|queue-dropped|{source_epoch}|{dropped}")),
        source_id: OBSERVER_SOURCE.to_owned(),
        source_epoch: source_epoch.to_owned(),
        source_sequence: None,
        sequence_meaning: None,
        callback_entry_sequence: None,
        callback_result_sequence: None,
        adapter_id: OBSERVER_ADAPTER.to_owned(),
        adapter_version: String::new(),
        provider_version: None,
        native_event: QUEUE_DROPPED_EVENT.to_owned(),
        session_key: None,
        actor_native_id: None,
        native_turn_id: None,
        native_prompt_id: None,
        native_occurrence_id: None,
        activation_ref: None,
        captured_at: context.clock.clone(),
        evidence: context.evidence.clone(),
        payload: Value::Object(Map::from_iter([("droppedRecords".to_owned(), Value::from(dropped))])),
    }
}

/// Splits envelopes into delivery batches within the record and frame
/// bounds, keeping their order.
fn frames(envelopes: &[ObservationEnvelope]) -> Vec<Vec<ObservationEnvelope>> {
    let mut out: Vec<Vec<ObservationEnvelope>> = Vec::new();
    let mut bytes = 0usize;
    for envelope in envelopes {
        let size = serde_json::to_vec(envelope).map_or(OBSERVATION_MAX_BYTES, |b| b.len()) + 1;
        let full = out
            .last()
            .is_none_or(|frame| frame.len() >= BATCH_MAX_RECORDS || bytes + size > FRAME_MAX_BYTES - 1024);
        if full {
            out.push(Vec::new());
            bytes = 0;
        }
        bytes += size;
        if let Some(frame) = out.last_mut() {
            frame.push(envelope.clone());
        }
    }
    out
}

/// Where the batch goes: the companion and, failing that, the local spool.
pub trait Sink {
    /// Delivers one frame before `deadline`; `None` when no receipt arrived.
    fn deliver(&mut self, frame: &[ObservationEnvelope], deadline: Instant) -> Option<Vec<RecordReceipt>>;
    /// Publishes one record to the local spool.
    fn spool(&mut self, envelope: &ObservationEnvelope) -> bool;
}

/// Answers one batch. `delivery` bounds the companion round trips and
/// `deadline` the whole answer: what is neither committed nor spooled by
/// then is NOT_ACCEPTED, so the mod keeps it and retries with the same UUID.
pub fn answer(stdin: &[u8], context: &BatchContext, sink: &mut dyn Sink, delivery: Instant, deadline: Instant) -> ModBatchReceipt {
    let mut receipt = ModBatchReceipt {
        receipt_version: MOD_BATCH_RECEIPT_VERSION,
        results: Vec::new(),
    };
    let Ok(request) = serde_json::from_slice::<ModBatchRequest>(stdin) else {
        return receipt;
    };
    if request.receipt_version != MOD_BATCH_RECEIPT_VERSION || request.kind != BATCH_KIND {
        return receipt;
    }
    let mut seen = BTreeSet::new();
    let mut prepared: Vec<(String, Result<ObservationEnvelope, &'static str>)> = Vec::new();
    for record in &request.records {
        let Some(id) = record.get("observationId").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let built = if request.records.len() > BATCH_MAX_RECORDS {
            Err("BATCH_TOO_LARGE")
        } else {
            envelope(record, &request.source_epoch, context)
        };
        prepared.push((id, built));
    }
    let mut envelopes: Vec<ObservationEnvelope> =
        prepared.iter().filter_map(|(_, built)| built.as_ref().ok().cloned()).collect();
    let marker = (request.dropped_records > 0 && uuid_like(&request.source_epoch))
        .then(|| queue_dropped(&request.source_epoch, request.dropped_records, context));
    envelopes.extend(marker.clone());
    let mut receipts: Vec<RecordReceipt> = Vec::new();
    for frame in frames(&envelopes) {
        if Instant::now() >= delivery {
            break;
        }
        match sink.deliver(&frame, delivery) {
            Some(answered) => receipts.extend(answered),
            None => break,
        }
    }
    for (id, built) in prepared {
        let (status, reason) = match built {
            Err(reason) => (RecordStatus::NotAccepted, Some(reason.to_owned())),
            Ok(envelope) => {
                let committed = receipts.iter().find(|r| r.observation_id == id).map(|r| (r.status, r.reason.clone()));
                match committed {
                    Some((status @ (RecordStatus::Committed | RecordStatus::AlreadyCommitted), _)) => (status, None),
                    Some((RecordStatus::NotAccepted, reason)) => (RecordStatus::NotAccepted, reason),
                    _ if Instant::now() < deadline && sink.spool(&envelope) => (RecordStatus::LocalSpooled, None),
                    _ if Instant::now() >= deadline => (RecordStatus::NotAccepted, Some("BUDGET".to_owned())),
                    _ => (RecordStatus::NotAccepted, Some("SPOOL_UNAVAILABLE".to_owned())),
                }
            }
        };
        receipt.results.push(RecordReceipt {
            observation_id: id,
            status,
            reason,
        });
    }
    // The loss marker is the helper's own record: kept like any other, but
    // not one the mod submitted, so it has no result.
    if let Some(marker) = marker {
        let committed = receipts.iter().any(|r| {
            r.observation_id == marker.observation_id
                && matches!(r.status, RecordStatus::Committed | RecordStatus::AlreadyCommitted)
        });
        if !committed && Instant::now() < deadline {
            sink.spool(&marker);
        }
    }
    receipt
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use threadspace_contracts::canonical::envelope::{ClockQuality, ProcessRole};
    use threadspace_contracts::limits::capture::RECEIPT_MAX_BYTES;
    use threadspace_contracts::route::ProcessKey;

    use super::*;

    const EPOCH: &str = "6d1c7f0e-1111-4aaa-8bbb-000000000001";

    fn context() -> BatchContext {
        let sample = |role, pid| ProcessSample {
            role,
            key: ProcessKey {
                endpoint_id: String::new(),
                boot_id: "boot".into(),
                pid,
                start_seconds: "1791000000".into(),
                start_microseconds: 7,
            },
            parent_pid: None,
            executable: Some("/Users/u/.local/share/claude/versions/2.1.295".into()),
            controlling_device: Some(16_777_220),
        };
        BatchContext {
            profile_ref: "claude-cli:/Users/u/.claude".into(),
            clock: CaptureClock {
                endpoint_id: None,
                boot_id: Some("boot".into()),
                monotonic_ns: Some("1".into()),
                wall_time_ms: 1,
                clock_quality: ClockQuality::LocalMonotonic,
            },
            evidence: vec![sample(ProcessRole::Capture, 101), sample(ProcessRole::Provider, 100)],
        }
    }

    fn id(n: u32) -> String {
        format!("00000000-0000-4000-8000-{n:012x}")
    }

    fn record(n: u32, event: &str, phase: &str) -> Value {
        json!({
            "schemaVersion": 1, "observationId": id(n), "adapterId": OBSERVER_ADAPTER, "adapterVersion": "0.1.0",
            "sourceEpoch": EPOCH, "sequenceMeaning": "OBSERVER_CAPTURE",
            "callbackEntrySequence": n.to_string(), "callbackResultSequence": if phase == "entry" { Value::Null } else { Value::from((n + 1).to_string()) },
            "phase": phase, "nativeEvent": event,
            "dispatchOrigin": { "plugin": "engine", "tier": "core" }, "engineDispatch": true,
            "sessionId": "8a1f5e2c-0000-4000-8000-00000000abcd", "sessionIdSource": "classic.SessionStart",
            "sessionGeneration": 1, "actorNativeId": null, "nativeTurnId": "turn-1", "nativeOccurrenceId": null,
            "payload": { "reason": "answer", "core": { "links": 1, "endPlugin": "engine", "endTier": "core", "endOutcome": "returned", "coreSettled": true } },
        })
    }

    fn batch(records: Vec<Value>, dropped: u64) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "receiptVersion": 1, "kind": "mod-batch", "sourceEpoch": EPOCH, "droppedRecords": dropped, "records": records,
        }))
        .expect("json")
    }

    /// A companion that commits the first `commit` records it sees (and
    /// says ALREADY_COMMITTED for any it committed before), then stops
    /// answering; and a spool that accepts or refuses.
    struct Fake {
        commit: usize,
        committed: BTreeSet<String>,
        delivered: Vec<ObservationEnvelope>,
        spooled: Vec<String>,
        spool_works: bool,
    }

    impl Fake {
        fn new(commit: usize, spool_works: bool) -> Self {
            Self { commit, committed: BTreeSet::new(), delivered: Vec::new(), spooled: Vec::new(), spool_works }
        }
    }

    impl Sink for Fake {
        fn deliver(&mut self, frame: &[ObservationEnvelope], _: Instant) -> Option<Vec<RecordReceipt>> {
            if self.commit == 0 {
                return None;
            }
            self.delivered.extend(frame.iter().cloned());
            let receipts = frame
                .iter()
                .take(self.commit)
                .map(|e| RecordReceipt {
                    observation_id: e.observation_id.clone(),
                    status: if self.committed.insert(e.observation_id.clone()) {
                        RecordStatus::Committed
                    } else {
                        RecordStatus::AlreadyCommitted
                    },
                    reason: None,
                })
                .collect::<Vec<_>>();
            self.commit = self.commit.saturating_sub(receipts.len());
            Some(receipts)
        }

        fn spool(&mut self, envelope: &ObservationEnvelope) -> bool {
            self.spooled.push(envelope.observation_id.clone());
            self.spool_works
        }
    }

    fn later() -> (Instant, Instant) {
        let now = Instant::now();
        (now + Duration::from_secs(5), now + Duration::from_secs(10))
    }

    fn statuses(receipt: &ModBatchReceipt) -> Vec<(String, RecordStatus)> {
        receipt.results.iter().map(|r| (r.observation_id.clone(), r.status)).collect()
    }

    #[test]
    fn a_committed_batch_has_one_result_per_record() {
        let mut sink = Fake::new(usize::MAX, true);
        let (delivery, deadline) = later();
        let receipt = answer(&batch(vec![record(1, "turn.complete", "result"), record(3, "tool.call", "entry")], 0), &context(), &mut sink, delivery, deadline);
        assert_eq!(receipt.receipt_version, MOD_BATCH_RECEIPT_VERSION);
        assert_eq!(statuses(&receipt), vec![(id(1), RecordStatus::Committed), (id(3), RecordStatus::Committed)]);
        assert!(sink.spooled.is_empty());
    }

    #[test]
    fn without_a_companion_records_are_spooled_and_without_a_spool_not_accepted() {
        let (delivery, deadline) = later();
        let mut sink = Fake::new(0, true);
        let receipt = answer(&batch(vec![record(1, "turn.start", "result")], 0), &context(), &mut sink, delivery, deadline);
        assert_eq!(statuses(&receipt), vec![(id(1), RecordStatus::LocalSpooled)]);
        let mut sink = Fake::new(0, false);
        let receipt = answer(&batch(vec![record(1, "turn.start", "result")], 0), &context(), &mut sink, delivery, deadline);
        assert_eq!(receipt.results[0].status, RecordStatus::NotAccepted);
        assert_eq!(receipt.results[0].reason.as_deref(), Some("SPOOL_UNAVAILABLE"));
    }

    #[test]
    fn a_bad_record_is_not_accepted_and_the_rest_proceed() {
        let mut bad_id = record(1, "turn.step", "result");
        bad_id["observationId"] = json!("NOT-A-UUID-0000-0000-0000-000000000001");
        let mut text = record(5, "tool.call", "entry");
        text["payload"] = json!({ "tool": "Bash\nrm -rf /" });
        let mut nested = record(7, "tool.call", "entry");
        nested["payload"] = json!({ "core": { "deep": { "x": 1 } } });
        let mut unknown = record(9, "worktree.create", "entry");
        unknown["nativeEvent"] = json!("WorktreeCreate");
        let mut sink = Fake::new(usize::MAX, true);
        let (delivery, deadline) = later();
        let receipt = answer(&batch(vec![bad_id, record(3, "turn.step", "result"), text, nested, unknown], 0), &context(), &mut sink, delivery, deadline);
        let by_id = |n: u32| receipt.results.iter().find(|r| r.observation_id == id(n)).map(|r| (r.status, r.reason.clone()));
        assert_eq!(by_id(3), Some((RecordStatus::Committed, None)));
        assert_eq!(by_id(5), Some((RecordStatus::NotAccepted, Some("UNBOUNDED_PAYLOAD".into()))));
        assert_eq!(by_id(7), Some((RecordStatus::NotAccepted, Some("UNBOUNDED_PAYLOAD".into()))));
        assert_eq!(by_id(9), Some((RecordStatus::NotAccepted, Some("BAD_EVENT".into()))));
        assert_eq!(receipt.results.len(), 5, "every submitted ID has exactly one result");
        assert_eq!(sink.delivered.len(), 1, "only the valid record reached the companion");
    }

    #[test]
    fn a_duplicate_uuid_in_one_batch_has_one_result() {
        let mut sink = Fake::new(usize::MAX, true);
        let (delivery, deadline) = later();
        let receipt = answer(&batch(vec![record(1, "turn.start", "result"), record(1, "turn.start", "result")], 0), &context(), &mut sink, delivery, deadline);
        assert_eq!(receipt.results.len(), 1);
    }

    #[test]
    fn partial_commit_then_retry_is_idempotent() {
        let records: Vec<Value> = (0..6).map(|n| record(n * 2 + 1, "tool.call", "result")).collect();
        let (delivery, deadline) = later();
        // The companion commits three, then stops answering: the rest spool.
        let mut sink = Fake::new(3, true);
        let first = answer(&batch(records.clone(), 0), &context(), &mut sink, delivery, deadline);
        let committed = first.results.iter().filter(|r| r.status == RecordStatus::Committed).count();
        assert_eq!(committed, 3);
        assert_eq!(first.results.iter().filter(|r| r.status == RecordStatus::LocalSpooled).count(), 3);
        let first_ids: Vec<String> = sink.delivered.iter().map(|e| e.observation_id.clone()).collect();
        // The mod retries the whole batch with the same UUIDs.
        sink.commit = usize::MAX;
        sink.delivered.clear();
        let second = answer(&batch(records, 0), &context(), &mut sink, delivery, deadline);
        let retried: Vec<String> = sink.delivered.iter().map(|e| e.observation_id.clone()).collect();
        assert_eq!(&retried[..first_ids.len()], &first_ids[..], "the same UUIDs, in the same order");
        assert_eq!(second.results.iter().filter(|r| r.status == RecordStatus::AlreadyCommitted).count(), 3);
        assert_eq!(second.results.iter().filter(|r| r.status == RecordStatus::Committed).count(), 3);
    }

    #[test]
    fn a_full_batch_receipt_fits_its_bound() {
        let records: Vec<Value> = (0..BATCH_MAX_RECORDS as u32).map(|n| record(n * 2 + 1, "tool.call", "entry")).collect();
        let mut sink = Fake::new(0, false);
        let (delivery, deadline) = later();
        let receipt = answer(&batch(records, 0), &context(), &mut sink, delivery, deadline);
        assert_eq!(receipt.results.len(), BATCH_MAX_RECORDS);
        assert!(serde_json::to_vec(&receipt).expect("json").len() <= RECEIPT_MAX_BYTES);
    }

    #[test]
    fn past_the_budget_nothing_is_claimed() {
        let mut sink = Fake::new(0, true);
        let now = Instant::now();
        let receipt = answer(&batch(vec![record(1, "turn.start", "result")], 0), &context(), &mut sink, now, now);
        assert_eq!(receipt.results[0].status, RecordStatus::NotAccepted);
        assert_eq!(receipt.results[0].reason.as_deref(), Some("BUDGET"));
        assert!(sink.spooled.is_empty());
    }

    #[test]
    fn dropped_records_are_recorded_once_under_a_stable_id_and_never_answered() {
        let (delivery, deadline) = later();
        let mut sink = Fake::new(usize::MAX, true);
        let receipt = answer(&batch(vec![record(1, "turn.start", "result")], 152), &context(), &mut sink, delivery, deadline);
        assert_eq!(receipt.results.len(), 1, "the marker is not a submitted record");
        let marker = sink.delivered.iter().find(|e| e.native_event == QUEUE_DROPPED_EVENT).expect("marker delivered");
        assert_eq!(marker.payload["droppedRecords"], 152);
        assert_eq!(marker.observation_id, queue_dropped(EPOCH, 152, &context()).observation_id, "stable across retries");
        assert!(uuid_like(&marker.observation_id));
    }

    #[test]
    fn envelopes_keep_the_frozen_session_and_name_their_submission() {
        let built = envelope(&record(1, "prompt.submit", "entry"), EPOCH, &context()).expect("envelope");
        assert_eq!(built.source_id, OBSERVER_SOURCE);
        assert_eq!(built.source_sequence.as_deref(), Some("1"));
        assert_eq!(built.native_prompt_id.as_deref(), Some(format!("observer:{EPOCH}:1").as_str()));
        assert_eq!(built.session_key.as_ref().map(|s| s.profile_ref.as_str()), Some("claude-cli:/Users/u/.claude"));
        assert_eq!(built.evidence.iter().filter(|s| s.role == ProcessRole::Provider).count(), 1);
        assert_eq!(built.payload["phase"], "entry");
        assert_eq!(built.payload["dispatchPlugin"], "engine");
        let result = envelope(&record(1, "prompt.submit", "result"), EPOCH, &context()).expect("envelope");
        assert_eq!(result.native_prompt_id, built.native_prompt_id, "the result names the same submission");
        assert_eq!(result.source_sequence.as_deref(), Some("2"), "a result is ordered by its result sequence");
        let mut bootstrap = record(3, "session.start", "bootstrap");
        bootstrap["sessionId"] = Value::Null;
        bootstrap["payload"] = json!({ "hostSessionId": "host-read-session", "predecessorEpoch": null });
        let built = envelope(&bootstrap, EPOCH, &context()).expect("envelope");
        assert_eq!(built.session_key.map(|s| s.native_session_id).as_deref(), Some("host-read-session"));
        let mut other_epoch = record(5, "turn.start", "result");
        other_epoch["sourceEpoch"] = json!("6d1c7f0e-1111-4aaa-8bbb-000000000002");
        assert_eq!(envelope(&other_epoch, EPOCH, &context()).err(), Some("BAD_SOURCE_EPOCH"));
    }
}
