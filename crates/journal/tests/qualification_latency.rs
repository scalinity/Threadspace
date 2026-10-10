//! Portable execution of the exact metadata sink and a real SQLite admission
//! with its qualification COMMIT bracket. This does not run macOS clocks,
//! native helper ancestry, a provider, or a UI.
#![cfg(feature = "qualification")]

#[allow(dead_code)]
#[path = "../../relay/src/latency.rs"]
mod helper_measurements;

use threadspace_contracts::canonical::envelope::{CaptureClock, ClockQuality, ObservationEnvelope};
use threadspace_contracts::canonical::fact::Delivery;
use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_journal::{EnvelopeAdmission, Journal};
use threadspace_state_engine::normalize::Normalized;

#[test]
fn actual_sqlite_admission_is_committed_before_its_timing_receipt_is_returned() {
    let dir = std::env::temp_dir().join(format!("threadspace-f4-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).expect("acquire new disposable fixture");
    let path = dir.join("journal.sqlite3");
    let mut journal = Journal::open(&path, "f4-portable", 1000).expect("journal");
    let envelope = ObservationEnvelope {
        schema_version: 1, observation_id: uuid::Uuid::new_v4().to_string(),
        source_id: "f4.fixture".into(), source_epoch: uuid::Uuid::new_v4().to_string(),
        source_sequence: None, sequence_meaning: None, callback_entry_sequence: None,
        callback_result_sequence: None, adapter_id: "f4.fixture".into(), adapter_version: "1".into(),
        provider_version: None, native_event: "F4Fixture".into(), session_key: None,
        actor_native_id: None, native_turn_id: None, native_prompt_id: None,
        native_occurrence_id: None, activation_ref: None,
        captured_at: CaptureClock { endpoint_id: None, boot_id: Some("portable-boot".into()), monotonic_ns: None,
            wall_time_ms: 1000, clock_quality: ClockQuality::ReceiptOnly },
        evidence: vec![], payload: serde_json::json!({}),
    };
    let admission = || EnvelopeAdmission { envelope: &envelope, normalized: Normalized {
        drafts: vec![], retained: serde_json::json!({}), unsupported: None,
    }};
    let result = journal.admit_batch(&[admission()], Delivery::Live, 1001).expect("actual admission");
    assert_eq!(result.records[0].status, RecordStatus::Committed);
    let timing = result.commit_timing.as_ref().expect("actual COMMIT bracket available");
    assert_eq!(timing.boundary, "SQLITE_COMMIT_CALL");
    assert_eq!(timing.clock, if cfg!(target_os = "macos") { "CLOCK_UPTIME_RAW" } else { "CLOCK_MONOTONIC" });
    assert!(timing.begin_ns.parse::<u64>().expect("begin") <= timing.end_ns.parse::<u64>().expect("end"));
    let independent = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).expect("independent connection");
    let count: i64 = independent.query_row("SELECT count(*) FROM observations WHERE observation_id=?1", [&envelope.observation_id], |row| row.get(0)).expect("durable independent readback");
    assert_eq!(count, 1);
    let retried = journal.admit_batch(&[admission()], Delivery::Live, 1002).expect("stable UUID retry");
    assert_eq!(retried.records[0].status, RecordStatus::AlreadyCommitted);
    assert_eq!(retried.records[0].cursor, result.records[0].cursor);
    drop(independent);
    drop(journal);
    std::fs::remove_dir_all(dir).expect("remove only acquired fixture");
}
