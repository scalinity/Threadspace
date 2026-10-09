//! Observer-mod records through the production path: `threadspace-hook
//! mod-batch`'s envelope builder, the local spool, the companion writer with
//! its adapter registry, and the journal (qualification fixture companion on
//! a disposable store). A session the engine identified completes natively
//! with a native observer tier; a reloaded load's host-read completion, whose
//! provider process nothing corroborates, is retained and not applied.

#![cfg(feature = "qualification")]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use threadspace_agent::fixture;
use threadspace_contracts::canonical::envelope::{ProcessRole, ProcessSample};
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_contracts::canonical::records::ObserverTier;
use threadspace_contracts::projection::{ObservationState, TurnState};
use threadspace_contracts::route::ProcessKey;
use threadspace_journal::Journal;
use threadspace_relay::modbatch::{BatchContext, envelope};
use threadspace_relay::spool::Spool;

const FIRST: &str = "6d1c7f0e-1111-4aaa-8bbb-000000000101";
const RELOAD: &str = "6d1c7f0e-1111-4aaa-8bbb-000000000102";
const SESSION: &str = "8a1f5e2c-0000-4000-8000-00000000abcd";
const PROFILE: &str = "claude-cli:/Users/u/.claude";

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ts-observer-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn context() -> BatchContext {
    BatchContext {
        profile_ref: PROFILE.into(),
        boot_id: Some("boot".into()),
        evidence: vec![ProcessSample {
            role: ProcessRole::Provider,
            key: ProcessKey {
                endpoint_id: String::new(),
                boot_id: "boot".into(),
                pid: 4242,
                start_seconds: "1791000000".into(),
                start_microseconds: 7,
            },
            parent_pid: None,
            executable: Some("/Users/u/.local/share/claude/versions/2.1.295".into()),
            controlling_device: Some(16_777_220),
        }],
    }
}

/// One mod record as `packages/provider-mod` writes it.
fn record(epoch: &str, n: u32, event: &str, phase: &str, identity: Option<&str>, turn: Option<&str>, detail: Value) -> Value {
    let settled = json!({ "links": 1, "endPlugin": "engine", "endTier": "core", "endOutcome": "returned", "coreSettled": true });
    let mut detail = detail;
    if phase != "entry" && detail.get("core").is_none() {
        detail["core"] = settled;
    }
    json!({
        "schemaVersion": 1, "observationId": format!("00000000-0000-4000-8{}{:02x}-{:012x}", &epoch[35..], phase.len(), n),
        "capturedAtMs": 1_791_000_000_000_i64 + i64::from(n), "adapterId": "threadspace-observer", "adapterVersion": "0.1.0", "sourceEpoch": epoch,
        "sequenceMeaning": "OBSERVER_CAPTURE",
        "callbackEntrySequence": n.to_string(),
        "callbackResultSequence": if phase == "entry" { Value::Null } else { Value::from((n + 1).to_string()) },
        "phase": phase, "nativeEvent": event,
        "dispatchOrigin": { "plugin": "engine", "tier": "core" }, "engineDispatch": true,
        "sessionId": identity.map(|_| SESSION), "sessionIdSource": identity, "sessionGeneration": 1,
        "actorNativeId": null, "nativeTurnId": turn, "nativeOccurrenceId": if event == "tool.call" { Value::from("toolu-1") } else { Value::Null },
        "payload": detail,
    })
}

fn publish(store: &Path, epoch: &str, records: Vec<Value>) {
    let spool = Spool::at(store);
    for record in &records {
        let built = envelope(record, epoch, &context()).expect("envelope");
        spool.publish(&built).expect("publish");
    }
}

#[test]
fn observer_records_drive_native_turns_and_hold_host_read_outcomes() {
    let store = temp();
    let companion = fixture::start(&store).expect("fixture companion");
    let engine = Some("classic.SessionStart");
    publish(&store, FIRST, vec![
        record(FIRST, 1, "session.start", "bootstrap", None, None, json!({ "hostSessionId": SESSION, "predecessorEpoch": null, "version": { "version": "2.1.295" } })),
        record(FIRST, 2, "classic.SessionStart", "entry", engine, None, json!({ "source": "startup" })),
        record(FIRST, 2, "classic.SessionStart", "result", engine, None, json!({})),
        record(FIRST, 4, "prompt.submit", "entry", engine, None, json!({ "origin": { "kind": "composer" }, "attachmentCount": 0 })),
        record(FIRST, 4, "prompt.submit", "result", engine, None, json!({ "outcome": "entered", "resultOrigin": { "kind": "composer" } })),
        record(FIRST, 6, "turn.start", "result", engine, Some("turn-1"), json!({ "echoedTurnId": "turn-1" })),
        record(FIRST, 8, "tool.call", "entry", engine, Some("turn-1"), json!({ "tool": "Bash" })),
        record(FIRST, 8, "tool.call", "result", engine, Some("turn-1"), json!({ "tool": "Bash", "resultKind": "result", "isReadOnly": false })),
        record(FIRST, 10, "turn.complete", "result", engine, Some("turn-1"), json!({ "reason": "answer", "isAborted": false })),
    ]);
    // A reload: a new load knows the session only from a host read.
    let host = Some("session.id");
    publish(&store, RELOAD, vec![
        record(RELOAD, 1, "session.start", "bootstrap", None, None, json!({ "hostSessionId": SESSION, "predecessorEpoch": FIRST, "version": { "version": "2.1.295" } })),
        record(RELOAD, 3, "turn.start", "result", host, Some("turn-2"), json!({ "echoedTurnId": "turn-2" })),
        record(RELOAD, 5, "turn.complete", "result", host, Some("turn-2"), json!({ "reason": "answer", "isAborted": false })),
    ]);
    assert_eq!(companion.drain_spool(), 12, "every record admitted");
    drop(companion);

    let journal = Journal::open(&store.join("journal.sqlite3"), "observer-test", 1).expect("reopen");
    let state = journal.canonical_state();
    let key = NativeSessionRef { provider: "claude".into(), profile_ref: PROFILE.into(), native_session_id: SESSION.into() };
    let session = state
        .sessions
        .values()
        .find(|s| s.native_session_id == key.native_session_id)
        .expect("the observed session");
    let turn = |native: &str| {
        state
            .turns
            .values()
            .find(|t| t.session_id == session.id && t.native_turn_id.as_deref() == Some(native))
            .expect("turn")
    };
    assert_eq!(turn("turn-1").state, TurnState::Completed, "an engine-identified settled completion is native");
    assert_eq!(turn("turn-2").state, TurnState::Working, "a host-read completion waits for corroboration");
    assert_eq!(turn("turn-2").pending_outcomes.len(), 1, "and is retained");
    assert_eq!(session.observation, ObservationState::Current);
    assert_eq!(session.observer_tier, Some(ObserverTier::LowerTier), "the reloaded load is unverified");
    assert_eq!(session.observer_version.as_deref(), Some("2.1.295"));
    let input = state.inputs.values().find(|i| i.session_id == session.id).expect("the submitted input");
    assert!(input.acceptances.iter().any(|p| p.qualified()), "accepted with verified provenance");
    assert!(state.activities.values().any(|a| a.session_id == session.id && a.tool_categories.contains("Bash")));
    let _ = std::fs::remove_dir_all(&store);
}
