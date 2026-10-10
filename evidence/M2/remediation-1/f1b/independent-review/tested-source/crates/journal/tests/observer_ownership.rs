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
#[path = "../../provider-claude/src/ownership_record.rs"]
mod ownership;
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

fn proof(session: &str, epoch: &str, generation: u64, token: &str, provider: ProcessSample) -> Capture {
    let request = ownership::ProbeRequest { protocol_version: 1, source_epoch: epoch.into(), session_generation: generation, session_id: session.into() };
    let envelope = ownership::envelope(&request, provider, PROFILE.into(), clock(), uuid::Uuid::from_u128(1000 + u128::from(generation)).to_string(), token.into(), (10, 11));
    let normalized = ownership::normalize(&envelope);
    assert!(normalized.unsupported.is_none(), "a qualified native proof envelope");
    Capture { envelope, normalized }
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

fn open(store: &TempStore) -> Journal {
    static SEEDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(99);
    let seed = SEEDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Journal::open_with(&store.db(), "f1b-test", clock().wall_time_ms, Box::new(SeededAllocator::new(seed)), false).expect("open disposable journal")
}

fn write_evidence(name: &str, value: &Value) {
    let Some(root) = std::env::var_os("THREADSPACE_F1B_EVIDENCE_DIR") else { return };
    let root = PathBuf::from(root);
    std::fs::create_dir_all(&root).expect("create explicit qualification output directory");
    std::fs::write(root.join(format!("{name}.json")), serde_json::to_vec_pretty(value).expect("evidence JSON")).expect("write portable qualification evidence");
}

fn run(captures: &[Capture], order: &[usize], check_steps: bool) -> CanonicalState {
    let store = TempStore::new();
    let mut journal = open(&store);
    let mut pure = Engine::empty();
    let mut cursor = 0;
    for (step, index) in order.iter().enumerate() {
        let c = &captures[*index];
        let admission = EnvelopeAdmission { envelope: &c.envelope, normalized: c.normalized.clone() };
        let receipt = journal.admit_batch(&[admission], Delivery::Live, clock().wall_time_ms + step as i64).expect("admit");
        assert!(receipt.records.iter().all(|r| matches!(r.status, RecordStatus::Committed | RecordStatus::AlreadyCommitted)), "{order:?} step {step}: {:?}", receipt.records);
        for entry in journal.journal_entries(cursor).expect("admitted facts") {
            cursor = cursor.max(entry.cursor);
            pure.apply(&entry);
        }
        if check_steps {
            assert_eq!(state_hash(journal.canonical_state()), state_hash(&pure.state), "pure/SQLite step {step} in {order:?}");
            assert!(journal.projection_differences().expect("materialized comparison").is_empty());
            if step % 2 == 0 {
                journal.checkpoint("F1B_FOCUSED", clock().wall_time_ms).expect("checkpoint");
                let before = state_hash(journal.canonical_state());
                drop(journal);
                journal = open(&store);
                assert_eq!(state_hash(journal.canonical_state()), before, "restart at step {step}");
            }
        }
    }
    let state = journal.canonical_state().clone();
    assert_eq!(state.reducer_version, REDUCER_VERSION);
    assert_eq!(semantic_hash(&state), semantic_hash(&pure.state));
    let (_, public) = journal.snapshot().expect("actual public view");
    for view in &public.sessions {
        let session = &state.sessions[&view.session_id];
        assert_eq!(view.turn_state, session.turn_state);
        assert_eq!(view.observer_tier, session.observer_tier);
    }
    assert_eq!(state_hash(&journal.replay_from_genesis().expect("genesis replay")), state_hash(&state));
    journal.checkpoint("F1B_FINAL", clock().wall_time_ms).expect("checkpoint");
    let digest = journal.replay_digest().expect("digest");
    assert_eq!(digest.state_sha256, state_hash(&state));
    assert_eq!(digest.projection_sha256, digest.tables_sha256);
    write_evidence(&format!("admission-{}", digest.state_sha256), &json!({
        "execution": "Linux portable actual observer/mod-batch adapters, SQLite and pure reducer",
        "deliveryOrder": order, "stepwiseSqlite": check_steps,
        "captures": captures.iter().map(|capture| &capture.envelope).collect::<Vec<_>>(),
        "entries": journal.journal_entries(0).expect("evidence canonical facts"),
        "state": state, "digest": digest,
    }));
    state
}

fn session<'a>(state: &'a CanonicalState, native: &str) -> &'a threadspace_contracts::canonical::records::SessionRecord {
    state.sessions.values().find(|s| s.native_session_id == native).expect("Session exists")
}
fn turn<'a>(state: &'a CanonicalState, session_name: &str, native: &str) -> &'a threadspace_contracts::canonical::records::TurnRecord {
    let owner = &session(state, session_name).id;
    state.turns.values().find(|t| &t.session_id == owner && t.native_turn_id.as_deref() == Some(native)).expect("Turn exists")
}
fn no_output(state: &CanonicalState, owner: &str) {
    let id = &session(state, owner).id;
    assert!(state.attention.values().all(|a| &a.session_id != id || a.resolved()), "no fabricated actionable attention for {owner}");
}

fn fresh() -> Vec<Capture> {
    vec![
        proof("A", EPOCH, 3, TOKEN, provider()),
        Capture::observer(record("ownership.seal", "A", None, 10, 3, Some(TOKEN))),
        Capture::observer(record("turn.start", "A", Some("new-turn"), 20, 3, Some(TOKEN))),
        Capture::observer(record("turn.complete", "A", Some("new-turn"), 30, 3, Some(TOKEN))),
    ]
}

fn permutations(values: &[usize]) -> Vec<Vec<usize>> {
    if values.is_empty() { return vec![Vec::new()] }
    let mut out = Vec::new();
    for (i, head) in values.iter().enumerate() {
        let rest: Vec<usize> = values.iter().enumerate().filter_map(|(j, n)| (i != j).then_some(*n)).collect();
        for mut tail in permutations(&rest) { tail.insert(0, *head); out.push(tail); }
    }
    out
}

#[test]
fn f1b_stale_a_p_attachment_cannot_restore_when_the_process_now_owns_b() {
    let captures = vec![
        history("A", 40), history("B", 42),
        Capture::observer(record("ownership.seal", "A", None, 10, 3, Some(TOKEN))),
        Capture::observer(record("turn.start", "A", Some("wrong-turn"), 20, 3, Some(TOKEN))),
        Capture::observer(record("turn.complete", "A", Some("wrong-turn"), 30, 3, Some(TOKEN))),
        proof("B", EPOCH, 3, TOKEN, provider()),
    ];
    for order in [&[0, 1, 5, 2, 3, 4][..], &[0, 1, 4, 3, 2, 5]] {
        let state = run(&captures, order, true);
        assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::LowerTier));
        let a = turn(&state, "A", "wrong-turn");
        assert_eq!(a.state, TurnState::Working);
        assert!(a.outcomes.is_empty());
        assert_eq!(a.pending_outcomes.len(), 1);
        no_output(&state, "A");
    }
}

#[test]
fn f1b_unchanged_id_new_turn_proof_seal_start_and_outcome_converge_in_every_delivery_order() {
    let captures = fresh();
    let mut expected = None;
    for (index, mut order) in permutations(&[0, 1, 2, 3]).into_iter().enumerate() {
        // Stable UUID retries, including an outcome delivered before its proof.
        order.push(3);
        let state = run(&captures, &order, index % 4 == 0);
        assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::Restored));
        let t = turn(&state, "A", "new-turn");
        assert_eq!(t.state, TurnState::Completed);
        assert_eq!(t.outcomes.len(), 1);
        assert_eq!(t.pending_outcomes.len(), 1);
        let hash = semantic_hash(&state);
        if let Some(expected) = &expected { assert_eq!(&hash, expected, "{order:?}"); } else { expected = Some(hash); }
    }
}

#[test]
fn f1b_wrong_incarnation_executable_epoch_generation_and_token_are_not_proofs() {
    for mismatch in ["birth", "image", "epoch", "generation", "token"] {
        let mut captures = fresh();
        let mut p = provider();
        if mismatch == "birth" { p.key.start_seconds = "1701".into(); }
        if mismatch == "image" { p.executable = Some(IMAGE.replace("#1:2", "#1:3")); }
        captures[0] = proof("A", if mismatch == "epoch" { "other-load" } else { EPOCH }, if mismatch == "generation" { 4 } else { 3 }, if mismatch == "token" { "other-token" } else { TOKEN }, p);
        for order in [[0, 1, 2, 3], [3, 2, 1, 0]] {
            let state = run(&captures, &order, true);
            assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::LowerTier), "{mismatch}");
            assert!(turn(&state, "A", "new-turn").outcomes.is_empty(), "{mismatch}");
            no_output(&state, "A");
        }
    }
}

#[test]
fn f1b_a_b_a_fresh_interval_never_revives_preseal_or_original_generation_outcomes() {
    let mut captures = fresh();
    captures.extend([
        Capture::observer(record("turn.start", "A", Some("original-a"), 4, 1, None)),
        Capture::observer(record("turn.complete", "A", Some("original-a"), 32, 1, None)),
        Capture::observer(record("turn.start", "B", Some("b-turn"), 6, 2, None)),
        Capture::observer(record("turn.complete", "B", Some("b-turn"), 34, 2, None)),
        Capture::observer(record("turn.start", "A", Some("pre-seal"), 8, 3, None)),
        Capture::observer(record("turn.complete", "A", Some("pre-seal"), 36, 3, None)),
    ]);
    for order in [(0..captures.len()).collect::<Vec<_>>(), (0..captures.len()).rev().collect()] {
        let state = run(&captures, &order, true);
        assert_eq!(turn(&state, "A", "new-turn").state, TurnState::Completed);
        for (s, t) in [("A", "original-a"), ("B", "b-turn"), ("A", "pre-seal")] {
            assert!(turn(&state, s, t).outcomes.is_empty(), "{s}/{t}");
            assert_eq!(turn(&state, s, t).pending_outcomes.len(), 1);
        }
    }
}

#[test]
fn f1b_a_token_in_an_intercepted_return_without_native_evidence_stays_pending() {
    let captures = fresh();
    let state = run(&captures, &[1, 2, 3], true);
    assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::LowerTier));
    assert!(turn(&state, "A", "new-turn").outcomes.is_empty());
    no_output(&state, "A");
}

#[test]
fn f1b_qualified_native_identity_does_not_borrow_host_read_authority() {
    let mut captures = fresh();
    let mut native = record("classic.SessionStart", "A", None, 40, 3, None);
    native["sessionIdSource"] = json!("classic.SessionStart");
    captures.push(Capture::observer(native));
    let mut completion = record("turn.complete", "A", Some("new-turn"), 50, 3, None);
    completion["sessionIdSource"] = json!("classic.SessionStart");
    captures.push(Capture::observer(completion));
    let state = run(&captures, &[1, 2, 3, 4, 5], true);
    assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::Native));
    let t = turn(&state, "A", "new-turn");
    assert_eq!(t.state, TurnState::Completed, "direct native evidence is independently authoritative");
    assert_eq!(t.pending_outcomes.len(), 1, "unproved HostRead report remains explicit");
    assert!(session(&state, "A").ownership_proofs.is_empty());
}

#[test]
fn f1b_semantic_oracle_distinguishes_authority_bearing_evidence() {
    let state = run(&fresh(), &[0, 1, 2, 3], true);
    let hash = semantic_hash(&state);
    for mutation in ["proof", "seal", "start", "pending-scope"] {
        let mut changed = state.clone();
        match mutation {
            "proof" => changed.sessions.values_mut().for_each(|s| s.ownership_proofs.clear()),
            "seal" => changed.sessions.values_mut().for_each(|s| s.links.clear()),
            "start" => changed.turns.values_mut().for_each(|t| t.ownership_starts.clear()),
            _ => changed.turns.values_mut().for_each(|t| t.pending_outcomes.clear()),
        }
        assert_ne!(semantic_hash(&changed), hash, "oracle cannot hide {mutation}");
    }
}

#[test]
fn f1a_actual_typescript_records_keep_original_session_through_adapter_sqlite_and_restart() {
    let default = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evidence/M2/remediation-1/f1a/observer-witnesses.json");
    let path = std::env::var_os("THREADSPACE_F1A_WITNESSES").map(PathBuf::from).unwrap_or(default);
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).expect("actual observer witness file")).expect("witness JSON");
    for case in fixture["results"].as_array().expect("cases") {
        let name = case["id"].as_str().expect("case id");
        let records = case["records"].as_array().expect("records");
        if records.is_empty() { continue; }
        let captures: Vec<Capture> = records.iter().cloned().map(Capture::observer).collect();
        let state = run(&captures, &(0..captures.len()).collect::<Vec<_>>(), true);
        match name {
            "F1A-1" | "F1A-2" => {
                assert_eq!(turn(&state, "Session-A", "Turn-A").state, TurnState::Completed);
                no_output(&state, "Session-B");
                let b = &session(&state, "Session-B").id;
                assert!(!state.turns.values().any(|t| &t.session_id == b && t.native_turn_id.as_deref() == Some("Turn-A")));
            }
            "F1A-3" => {
                for (s, t) in [("Session-A", "Turn-A-old"), ("Session-A", "Turn-A-new"), ("Session-B", "Turn-B"), ("Session-A", "Shared-child-Turn"), ("Session-B", "Shared-child-Turn")] {
                    assert_eq!(turn(&state, s, t).state, TurnState::Completed, "{name} {s}/{t}");
                }
            }
            "F1A-4" | "F1A-6" | "F1A-7" | "F1B-bridge-3" => {
                assert!(state.turns.values().all(|t| t.outcomes.is_empty()), "{name}: uncertain ownership creates no native outcome");
                assert!(state.attention.values().all(|a| a.resolved()), "{name}: no invented attention");
            }
            "F1A-5" => {
                assert_eq!(turn(&state, "Session-A", "Child-Turn").state, TurnState::Completed);
                no_output(&state, "Session-B");
            }
            "F1A-8" => {
                assert_eq!(turn(&state, "Session-A", "Real-Turn").state, TurnState::Completed);
                assert_eq!(state.turns.values().filter(|t| !t.outcomes.is_empty()).count(), 1);
            }
            "F1B-bridge-1" => {
                assert!(state.turns.values().all(|t| t.outcomes.is_empty()), "intercepted return token alone never proves reload");
                let seal = records.iter().find(|r| r["nativeEvent"] == "ownership.seal").expect("seal");
                let independent = proof("Session-A", seal["ownershipEpoch"].as_str().expect("epoch"), seal["ownershipGeneration"].as_u64().expect("generation"), seal["ownershipProofToken"].as_str().expect("token"), provider());
                let mut with_proof = captures.clone();
                with_proof.push(independent);
                for order in [(0..with_proof.len()).collect::<Vec<_>>(), std::iter::once(with_proof.len() - 1).chain(0..captures.len()).collect()] {
                    let qualified = run(&with_proof, &order, true);
                    assert!(turn(&qualified, "Session-A", "Pre-seal-Turn").outcomes.is_empty());
                    assert_eq!(turn(&qualified, "Session-A", "Post-seal-Turn").state, TurnState::Completed);
                    assert_eq!(session(&qualified, "Session-A").observer_tier, Some(ObserverTier::Restored));
                }
            }
            _ => {}
        }
    }
}

fn old_fixture() -> (PathBuf, Value) {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evidence/M2/remediation-1/f1b/reducer-3-store");
    let witness: Value = serde_json::from_slice(&std::fs::read(directory.join("witness.json")).expect("genuine reducer-3 witness")).expect("witness");
    assert_eq!(witness["source"], "2c4b5012f59189dcb38d61ced0b0b9fe96fb043e");
    assert_eq!(witness["reducerVersion"], 3);
    (directory.join("journal.sqlite3"), witness)
}

#[test]
fn f1b_genuine_reducer3_upgrade_preserves_owner_receipts_and_converges_at_three_checkpoints() {
    use threadspace_contracts::canonical::command::OwnerCommand;
    use threadspace_contracts::canonical::records::{OutboxState, ResolutionKind};
    use threadspace_contracts::ui::ReceiptStatus;
    let (fixture, witness) = old_fixture();
    let previous: CanonicalState = serde_json::from_value(witness["state"].clone()).expect("old state");
    let command: OwnerCommand = serde_json::from_value(witness["command"]["command"].clone()).expect("recorded owner command");
    let old_receipt = &witness["command"]["receipt"];
    let mut expected = None;
    for checkpoint in [11, 2, 0] {
        let store = TempStore::new();
        std::fs::copy(&fixture, store.db()).expect("copy genuine previous-version SQLite fixture");
        let conn = rusqlite::Connection::open(store.db()).expect("disposable fixture connection");
        conn.execute("DELETE FROM projection_checkpoints WHERE through_cursor > ?1", [checkpoint]).expect("select old checkpoint position");
        let (version, cursor): (u32, i64) = conn.query_row("SELECT reducer_version, through_cursor FROM projection_checkpoints ORDER BY id DESC LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?))).expect("actual previous checkpoint");
        assert_eq!((version, cursor), (3, checkpoint));
        drop(conn);
        let mut journal = open(&store);
        let state = journal.canonical_state().clone();
        assert_eq!(state.reducer_version, 4);
        assert_eq!(state.commands, previous.commands, "committed owner decision survives checkpoint {checkpoint}");
        assert_eq!(state.sessions.keys().collect::<Vec<_>>(), previous.sessions.keys().collect::<Vec<_>>(), "stable Session IDs");
        assert_eq!(state.turns.keys().collect::<Vec<_>>(), previous.turns.keys().collect::<Vec<_>>(), "stable Turn IDs");
        assert_eq!(state.attention.keys().collect::<Vec<_>>(), previous.attention.keys().collect::<Vec<_>>(), "durable attention history retained");
        assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::LowerTier));
        for native in ["owner-handled-stale", "unhandled-stale"] {
            let stale = turn(&state, "A", native);
            assert!(stale.outcomes.is_empty(), "withdraw stale HOST_READ completion");
            assert_eq!(stale.pending_outcomes.len(), 1);
            assert_eq!(stale.state, TurnState::Working);
            let item = state.attention.values().find(|item| item.turn_id.as_ref() == Some(&stale.id)).expect("retained attention");
            assert!(item.resolutions.iter().any(|cause| cause.kind == ResolutionKind::EvidenceUnavailable));
            assert!(state.outbox.values().filter(|intent| intent.attention_id == item.id).all(|intent| intent.state == OutboxState::Suppressed), "withdrawn authority cannot schedule live work");
        }
        let handled = &state.attention[&command.attention_id];
        assert!(handled.resolutions.iter().any(|cause| cause.kind == ResolutionKind::Owner && cause.detail == "owner decision retained through explicit upgrade"));
        assert_eq!(turn(&state, "C", "native-control").state, TurnState::Completed, "genuine independent outcome retained");
        for old in previous.outbox.values().filter(|intent| {
            let item = &previous.attention[&intent.attention_id];
            item.session_id == session(&previous, "C").id
        }) {
            assert_eq!(&state.outbox[&old.request_id], old, "already committed native outbox disposition is unchanged");
        }
        let retry = journal.admit_owner_command(&command, clock().wall_time_ms + 1000).expect("owner retry after upgrade");
        assert_eq!(retry.receipt.status, ReceiptStatus::AlreadyCommitted);
        assert_eq!(retry.receipt.cursor, old_receipt["cursor"]);
        assert_eq!(retry.receipt.target_revision, old_receipt["targetRevision"]);
        assert!(retry.change.is_none());
        assert_eq!(journal.journal_entries(0).expect("unchanged retained journal").len(), 11);
        assert!(journal.projection_differences().expect("materialized comparison").is_empty());
        let hash = state_hash(&state);
        let genesis = journal.replay_from_genesis().expect("versioned canonical genesis replay");
        write_evidence(&format!("migration-checkpoint-{checkpoint}"), &json!({ "oldCheckpoint": checkpoint, "state": state, "genesis": genesis, "digest": journal.replay_digest().expect("digest") }));
        assert_eq!(state_hash(&genesis), hash, "genesis independent of checkpoint {checkpoint}");
        if let Some(expected) = &expected { assert_eq!(&hash, expected, "checkpoint placement {checkpoint}"); } else { expected = Some(hash.clone()); }
        let digest = journal.replay_digest().expect("upgraded checkpoint digest");
        assert_eq!(digest.checkpoint_cursor, Some(11));
        assert_eq!(digest.checkpoint_sha256.as_deref(), Some(hash.as_str()));
        assert_eq!(digest.tables_sha256, digest.projection_sha256);
        drop(journal);
        let mut journal = open(&store);
        assert_eq!(state_hash(journal.canonical_state()), hash, "fixed-point restart");
        let mut captures = fresh();
        for capture in &mut captures {
            capture.envelope.observation_id = uuid::Uuid::new_v4().to_string();
        }
        for capture in &captures {
            journal.admit_batch(&[EnvelopeAdmission { envelope: &capture.envelope, normalized: capture.normalized.clone() }], Delivery::Live, clock().wall_time_ms + 2000).expect("new v2 facts after upgrade");
        }
        assert_eq!(turn(journal.canonical_state(), "A", "new-turn").state, TurnState::Completed);
        assert!(turn(journal.canonical_state(), "A", "unhandled-stale").outcomes.is_empty(), "fresh proof never revives previous interval");
        assert_eq!(state_hash(&journal.replay_from_genesis().expect("replay across v1/v2 boundary")), state_hash(journal.canonical_state()));
    }
}

#[test]
fn f1b_upgrade_refuses_missing_canonical_outcome_evidence() {
    let (fixture, _) = old_fixture();
    let store = TempStore::new();
    std::fs::copy(fixture, store.db()).expect("copy old fixture");
    let conn = rusqlite::Connection::open(store.db()).expect("disposable connection");
    conn.execute("DELETE FROM facts WHERE observation_id = ?1", [uuid::Uuid::from_u128(50).to_string()]).expect("explicit corrupt-fixture negative control");
    drop(conn);
    let result = Journal::open_with(&store.db(), "f1b-missing-fact", clock().wall_time_ms, Box::new(SeededAllocator::new(987)), false);
    assert!(matches!(result, Err(threadspace_journal::JournalError::Invalid { detail }) if detail.contains("ownership upgrade requires retained fact")), "never infer a missing canonical claim from a derived completion flag");
}
