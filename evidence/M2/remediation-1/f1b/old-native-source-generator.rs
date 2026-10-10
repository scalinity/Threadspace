//! M2 F1A/F1B through the actual portable production modules: observer
//! records -> mod-batch envelopes -> Claude adapter -> SQLite -> public
//! projection. Native inventory/kernel IO is qualified separately on macOS;
//! its independently captured proof envelope is exercised here as evidence.

#[allow(dead_code)]
#[path = "../../provider-claude/src/profiles.rs"]
mod profiles;
#[allow(dead_code)]
#[path = "../../provider-claude/src/observer.rs"]
mod observer;

#[allow(dead_code)]
#[path = "../../relay/src/modbatch.rs"]
mod modbatch;

use std::path::PathBuf;
use serde_json::{Value, json};
use threadspace_contracts::canonical::capture::RecordStatus;
use threadspace_contracts::canonical::envelope::{CaptureClock, ClockQuality, ObservationEnvelope, ProcessRole, ProcessSample};
use threadspace_contracts::canonical::fact::{AttachedPresence, Delivery, EvidenceClass, ExecutionMode, FactPayload, NativeFactDraft, NativeRefs};
use threadspace_contracts::canonical::keys::{NativeExecutionRef, NativeSessionRef};
use threadspace_contracts::canonical::records::{CanonicalState, ObserverTier};
use threadspace_contracts::projection::TurnState;
use threadspace_contracts::route::ProcessKey;
use threadspace_journal::{EnvelopeAdmission, Journal};
use threadspace_state_engine::{Engine, REDUCER_VERSION};
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::ids::SeededAllocator;
use threadspace_state_engine::normalize::Normalized;
use threadspace_state_engine::semantic::semantic_hash;

const EPOCH: &str = "22222222-2222-4222-8222-222222222222";
const TOKEN: &str = "33333333-3333-4333-8333-333333333333";
const PROFILE: &str = "claude-cli:/f1b/.claude";
const IMAGE: &str = "/f1b/.local/share/claude/versions/2.1.295#1:2";

struct TempStore(PathBuf);
impl TempStore {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("threadspace-f1b-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).expect("acquire unique disposable directory");
        Self(dir)
    }
    fn db(&self) -> PathBuf { self.0.join("journal.sqlite3") }
}
impl Drop for TempStore {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn provider() -> ProcessSample {
    ProcessSample {
        role: ProcessRole::Provider,
        key: ProcessKey { endpoint_id: String::new(), boot_id: "f1b-boot".into(), pid: 710, start_seconds: "1700".into(), start_microseconds: 7 },
        parent_pid: Some(100), executable: Some(IMAGE.into()), controlling_device: Some(12),
    }
}

fn context() -> modbatch::BatchContext {
    modbatch::BatchContext { profile_ref: PROFILE.into(), boot_id: Some("f1b-boot".into()), evidence: vec![provider()] }
}

fn clock() -> CaptureClock {
    CaptureClock { endpoint_id: None, boot_id: Some("f1b-boot".into()), monotonic_ns: Some("1000000000".into()), wall_time_ms: 1_791_000_000_000, clock_quality: ClockQuality::LocalMonotonic }
}

#[derive(Clone)]
struct Capture { envelope: ObservationEnvelope, normalized: Normalized }
impl Capture {
    fn observer(record: Value) -> Self {
        let epoch = record["sourceEpoch"].as_str().expect("epoch");
        let envelope = modbatch::envelope(&record, epoch, &context()).expect("production mod-batch validation");
        let normalized = observer::normalize(&envelope);
        Self { envelope, normalized }
    }
}

fn record(event: &str, session: &str, turn: Option<&str>, seq: u64, generation: u64, token: Option<&str>) -> Value {
    json!({
        "schemaVersion": 1, "adapterId": "threadspace-observer", "adapterVersion": "1",
        "observationId": uuid::Uuid::from_u128(u128::from(seq)).to_string(),
        "sourceEpoch": EPOCH, "callbackEntrySequence": (seq - 1).to_string(), "callbackResultSequence": seq.to_string(),
        "capturedAtMs": clock().wall_time_ms + seq as i64,
        "phase": "result", "nativeEvent": event,
        "dispatchOrigin": { "plugin": "engine", "tier": "core" }, "engineDispatch": true,
        "sessionId": session, "sessionIdSource": "session.id", "sessionGeneration": generation,
        "nativeTurnId": turn, "ownershipEpoch": EPOCH, "ownershipGeneration": generation,
        "ownershipStatus": "HOST_READ", "ownershipProofToken": token,
        "payload": { "reason": "answer", "core": { "coreSettled": true } }
    })
}

fn history(session: &str, id: u128) -> Capture {
    let mut envelope = Capture::observer(record("session.start", session, None, id as u64, 1, None)).envelope;
    envelope.native_event = "historical-attachment".into();
    let refs = NativeRefs {
        session: Some(NativeSessionRef { provider: "claude".into(), profile_ref: PROFILE.into(), native_session_id: session.into() }),
        process: Some(provider().key),
        execution: Some(NativeExecutionRef::Activation { activation_ref: format!("historical-{session}") }),
        ..NativeRefs::default()
    };
    let normalized = Normalized {
        drafts: vec![NativeFactDraft {
            refs,
            provenance: EvidenceClass::ProviderSnapshot,
            causal: None,
            payload: FactPayload::ExecutionAttached { mode: ExecutionMode::TerminalEmbedded, presence: AttachedPresence::Live, native_runtime_id: None, controlling_device: Some(12) },
        }], retained: json!({}), unsupported: None,
    };
    Capture { envelope, normalized }
}

#[test]
fn f1b_rejected_native_only_store() {
    use threadspace_contracts::canonical::command::{OwnerAction, OwnerCommand};
    assert_eq!(REDUCER_VERSION,3,"actual rejected source required");
    let out=PathBuf::from(std::env::var_os("THREADSPACE_F1B_NATIVE_OUTPUT").expect("explicit disposable output"));
    std::fs::create_dir(&out).expect("acquire own fixture directory");
    let mut journal=Journal::open_with(&out.join("journal.sqlite3"),"f1b-rejected-native-only",clock().wall_time_ms,Box::new(SeededAllocator::new(301)),false).expect("old journal");
    let mut attached=history("A",100);
    let mut process=attached.normalized.drafts[0].clone();
    process.provenance=EvidenceClass::Kernel;
    process.payload=FactPayload::ProcessObserved{executable_identity:IMAGE.into()};
    attached.normalized.drafts.insert(0,process);
    let mut captures=vec![attached];
    for (event,turn,seq) in [("classic.SessionStart",None,10),("turn.start",Some("native-handled"),20),("turn.complete",Some("native-handled"),30),("turn.start",Some("native-unhandled"),40),("turn.complete",Some("native-unhandled"),50)] {
        let mut r=record(event,"A",turn,seq,1,None);
        r["sessionIdSource"]=json!("classic.SessionStart");
        captures.push(Capture::observer(r));
    }
    let mut owner=None;
    for (i,c) in captures.iter().enumerate() {
        let outcome=journal.admit_batch(&[EnvelopeAdmission{envelope:&c.envelope,normalized:c.normalized.clone()}],Delivery::Live,clock().wall_time_ms+i as i64).expect("actual rejected admission");
        assert_eq!(outcome.records[0].status,RecordStatus::Committed);
        if i==1 {journal.checkpoint("F1B_NATIVE_BEFORE_OUTCOME",clock().wall_time_ms).expect("old checkpoint");}
        if i==3 {
            let item=journal.canonical_state().attention.values().next().expect("native attention");
            let command=OwnerCommand{command_id:uuid::Uuid::from_u128(77777).to_string(),attention_id:item.id.clone(),expected_revision:Some(item.revision.to_string()),action:OwnerAction::Resolve{reason:"native owner decision survives version transition".into()}};
            let receipt=journal.admit_owner_command(&command,clock().wall_time_ms+100).expect("old owner command");
            owner=Some(json!({"command":command,"receipt":receipt.receipt}));
        }
    }
    journal.checkpoint("F1B_NATIVE_FINAL",clock().wall_time_ms+200).expect("final checkpoint");
    let entries=journal.journal_entries(0).expect("retained facts");
    assert!(entries.iter().flat_map(|e| &e.facts).all(|f| f.provenance!=EvidenceClass::HostRead),"native-only branch excludes any HOST_READ fact");
    let state=journal.canonical_state().clone();
    assert_eq!(state.processes.values().next().expect("process").revision,1);
    assert_eq!(state.executions.values().next().expect("execution").revision,1);
    assert_eq!(state.through_cursor,7);
    std::fs::write(out.join("witness.json"),serde_json::to_vec_pretty(&json!({"source":"2c4b5012f59189dcb38d61ced0b0b9fe96fb043e","reducerVersion":3,"purpose":"genuine native-only reducer3 history requiring the explicit version transition without HOST_READ outcomes","state":state,"entries":entries,"command":owner,"digest":journal.replay_digest().expect("digest")})).expect("JSON")).expect("evidence");
    println!("GENUINE_REDUCER3_NATIVE_ONLY_SOURCE_STORE cursor7 process_revision1 execution_revision1");
}
