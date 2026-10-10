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
        proof("A", EPOCH, 1, "original-proven-token", provider()),
        Capture::observer(record("ownership.seal", "A", None, 2, 1, Some("original-proven-token"))),
        Capture::observer(record("turn.start", "A", Some("proven-original-a"), 3, 1, Some("original-proven-token"))),
        Capture::observer(record("turn.complete", "A", Some("proven-original-a"), 38, 1, Some("original-proven-token"))),
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
        assert_eq!(turn(&state, "A", "proven-original-a").state, TurnState::Completed,
            "a delayed callback whose original Turn was independently proven retains that original ownership; no mutable-current-generation gate");
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
        if std::env::var_os("THREADSPACE_F1B_NEGATIVE_SKIP_UPGRADE").is_some() {
            // Negative control: the exact prohibited alternative of merely
            // relabeling a v3 checkpoint, without retained-fact requalification.
            // It must fail the same authority assertions below.
            let raw: String = conn.query_row("SELECT state_json FROM projection_checkpoints ORDER BY id DESC LIMIT 1", [], |row| row.get(0)).expect("negative-control checkpoint");
            let mut value: Value = serde_json::from_str(&raw).expect("old checkpoint JSON");
            value["reducerVersion"] = json!(4);
            let text = serde_json::to_string(&value).expect("negative-control JSON");
            conn.execute("UPDATE projection_checkpoints SET reducer_version = 4, state_json = ?1, state_sha256 = ?2 WHERE id = (SELECT MAX(id) FROM projection_checkpoints)", rusqlite::params![text, threadspace_state_engine::hash::sha256_hex(text.as_bytes())]).expect("test-only skipped upgrade");
            println!("NEGATIVE CONTROL: relabeled genuine reducer-3 checkpoint without requalifying canonical facts");
        }
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
        // A late legacy-shaped report after the v2 boundary is still
        // interpreted by the new authority predicate. Only this disposable
        // fixture's fact version is changed, to exercise mixed-version
        // import/replay defensively; its claim and native IDs are unchanged.
        let late = Capture::observer(record("turn.complete", "A", Some("unhandled-stale"), 90, 3, None));
        journal.admit_batch(&[EnvelopeAdmission { envelope: &late.envelope, normalized: late.normalized }], Delivery::Live, clock().wall_time_ms + 3000).expect("late legacy-shaped claim");
        let before = state_hash(journal.canonical_state());
        drop(journal);
        let conn = rusqlite::Connection::open(store.db()).expect("disposable mixed-version fixture");
        let updated = conn.execute("UPDATE facts SET fact_json = json_set(fact_json, '$.payloadVersion', 1) WHERE observation_id = ?1", [&late.envelope.observation_id]).expect("select retained v1 payload representation");
        assert_eq!(updated, 1);
        conn.execute("UPDATE observations SET payload_version = 1 WHERE observation_id = ?1", [&late.envelope.observation_id]).expect("late retained legacy admission header");
        drop(conn);
        let journal = open(&store);
        assert_eq!(state_hash(journal.canonical_state()), before);
        assert!(turn(journal.canonical_state(), "A", "unhandled-stale").outcomes.is_empty(), "late v1 must never reinstate legacy attachment authority");
        assert_eq!(state_hash(&journal.replay_from_genesis().expect("v1 after the permanent v2 transition")), before);
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

#[test]
fn f1b_native_only_genuine_reducer3_prefix_still_records_the_version_transition() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evidence/M2/remediation-1/f1b/reducer-3-native-only-store");
    let witness: Value = serde_json::from_slice(&std::fs::read(root.join("witness.json")).expect("genuine native-only fixture witness")).expect("JSON");
    assert_eq!(witness["source"], "2c4b5012f59189dcb38d61ced0b0b9fe96fb043e");
    assert_eq!(witness["reducerVersion"], 3);
    let old: CanonicalState = serde_json::from_value(witness["state"].clone()).expect("old canonical state");
    assert!(old.turns.values().all(|turn| turn.pending_outcomes.is_empty()), "this is the native-only branch");
    let mut expected = None;
    for checkpoint in [7, 2, 0] {
        let store = TempStore::new();
        std::fs::copy(root.join("journal.sqlite3"), store.db()).expect("copy actual rejected native-only store");
        let conn = rusqlite::Connection::open(store.db()).expect("owned fixture");
        conn.execute("DELETE FROM projection_checkpoints WHERE through_cursor > ?1", [checkpoint]).expect("select old checkpoint");
        let version: u32 = conn.query_row("SELECT reducer_version FROM projection_checkpoints ORDER BY id DESC LIMIT 1", [], |row| row.get(0)).expect("old version");
        assert_eq!(version, 3);
        drop(conn);
        let mut journal = open(&store);
        let state = journal.canonical_state().clone();
        assert_eq!(state.commands, old.commands);
        assert_eq!(state.attention, old.attention, "native attention and owner state are unaffected");
        assert_eq!(state.outbox, old.outbox, "native outbox dispositions are unaffected");
        assert_eq!(session(&state, "A").observer_tier, Some(ObserverTier::Native));
        assert!(state.turns.values().all(|turn| turn.state == TurnState::Completed));
        assert!(state.processes.values().all(|process| process.revision == 7));
        assert!(state.executions.values().all(|execution| execution.revision == 7));
        let genesis = journal.replay_from_genesis().expect("native-only genesis replay");
        write_evidence(&format!("native-migration-checkpoint-{checkpoint}"), &json!({ "oldCheckpoint": checkpoint, "state": state, "genesis": genesis, "digest": journal.replay_digest().expect("digest") }));
        assert_eq!(state_hash(&genesis), state_hash(&state), "native-only v1 prefix must still perform its explicit version transition");
        if let Some(hash) = &expected { assert_eq!(&state_hash(&state), hash); } else { expected = Some(state_hash(&state)); }
        let native = Capture::observer({
            let mut value = record("turn.step", "A", Some("native-unhandled"), 70, 1, None);
            value["sessionIdSource"] = json!("classic.SessionStart");
            value
        });
        journal.admit_batch(&[EnvelopeAdmission { envelope: &native.envelope, normalized: native.normalized }], Delivery::Live, clock().wall_time_ms + 2000).expect("new payload-version-2 native fact");
        assert_eq!(state_hash(&journal.replay_from_genesis().expect("native-only first-v2 transition")), state_hash(journal.canonical_state()));
        journal.checkpoint("F1B_NATIVE_ONLY_FINAL", clock().wall_time_ms).expect("new checkpoint");
        let hash = state_hash(journal.canonical_state());
        drop(journal);
        assert_eq!(state_hash(open(&store).canonical_state()), hash, "restart retains the explicit transition");
    }
}

#[test]
fn f1b_metadata_only_admissions_cannot_move_an_already_committed_upgrade_boundary() {
    fn restore_old_checkpoint(store: &TempStore, row: &[rusqlite::types::Value]) {
        let conn = rusqlite::Connection::open(store.db()).expect("owned checkpoint recovery fixture");
        conn.execute("DELETE FROM projection_checkpoints", []).expect("simulate current checkpoint unavailable");
        conn.execute("INSERT INTO projection_checkpoints (reducer_version, schema_version, through_cursor, state_json, state_sha256, origin, created_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", rusqlite::params_from_iter(row)).expect("retain the genuine verified older checkpoint");
    }
    for (name, old_end) in [("reducer-3-store", 11), ("reducer-3-native-only-store", 7)] {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../evidence/M2/remediation-1/f1b/{name}/journal.sqlite3"));
        for checkpoint in [old_end, 2, 0] {
            let store = TempStore::new();
            std::fs::copy(&fixture, store.db()).expect("copy genuine reducer-3 fixture");
            let conn = rusqlite::Connection::open(store.db()).expect("owned fixture");
            conn.execute("DELETE FROM projection_checkpoints WHERE through_cursor > ?1", [checkpoint]).expect("select old checkpoint");
            let old_checkpoint: Vec<rusqlite::types::Value> = conn.query_row("SELECT reducer_version, schema_version, through_cursor, state_json, state_sha256, origin, created_at_ms FROM projection_checkpoints ORDER BY id DESC LIMIT 1", [], |row| (0..7).map(|index| row.get(index)).collect()).expect("genuine old checkpoint row");
            drop(conn);
            let mut journal = open(&store);
            let transition = journal.canonical_state().clone();
            assert_eq!(transition.through_cursor, old_end);
            for (count, event) in ["session.attach", "session.detach"].into_iter().enumerate() {
                let metadata = Capture::observer({
                    let mut value = record(event, "A", None, 100 + count as u64, 3, None);
                    value["observationId"] = json!(uuid::Uuid::new_v4().to_string());
                    value
                });
                assert!(metadata.normalized.drafts.is_empty(), "actual adapter metadata-only path");
                let receipt = journal.admit_batch(&[EnvelopeAdmission { envelope: &metadata.envelope, normalized: metadata.normalized }], Delivery::Live, clock().wall_time_ms + 4000).expect("metadata-only admission");
                assert_eq!(receipt.records[0].status, RecordStatus::Committed, "{:?}", receipt.records[0]);
                let entries = journal.journal_entries(old_end).expect("retained metadata rows");
                assert_eq!(entries.len(), count + 1);
                assert!(entries.iter().all(|entry| entry.facts.is_empty()), "no fact can serve as the boundary witness");
                let state = journal.canonical_state().clone();
                assert_eq!(state.processes, transition.processes, "metadata must not shift process revisions");
                assert_eq!(state.executions, transition.executions, "metadata must not shift execution revisions");
                let genesis = journal.replay_from_genesis().expect("replay with metadata-only v4 tail");
                write_evidence(&format!("metadata-migration-{name}-checkpoint-{checkpoint}-tail-{count}"), &json!({ "fixture": name, "oldCheckpoint": checkpoint, "oldEndpoint": old_end, "newMetadataCount": count + 1, "entries": entries, "state": state, "genesis": genesis }));
                assert_eq!(state_hash(&genesis), state_hash(&state), "a metadata-only tail cannot relocate the committed upgrade boundary");
                journal.checkpoint("F1B_METADATA_ONLY_TAIL", clock().wall_time_ms).expect("checkpoint without any new canonical fact");
                let hash = state_hash(&state);
                drop(journal);
                journal = open(&store);
                assert_eq!(state_hash(journal.canonical_state()), hash, "metadata-only restart");
                assert_eq!(state_hash(&journal.replay_from_genesis().expect("metadata-only restart genesis")), hash);
                drop(journal);
                restore_old_checkpoint(&store, &old_checkpoint);
                journal = open(&store);
                write_evidence(&format!("metadata-checkpoint-recovery-{name}-{checkpoint}-tail-{count}"), &json!({ "expected": state, "recovered": journal.canonical_state(), "genesis": journal.replay_from_genesis().expect("recovered metadata genesis") }));
                assert_eq!(state_hash(journal.canonical_state()), hash, "an older verified checkpoint plus v2 metadata suffix must retain the original transition");
            }
            let native = Capture::observer({
                let mut value = record("turn.step", "A", Some("metadata-tail-native"), 200, 1, None);
                value["sessionIdSource"] = json!("classic.SessionStart");
                value["observationId"] = json!(uuid::Uuid::new_v4().to_string());
                value
            });
            assert!(!native.normalized.drafts.is_empty());
            journal.admit_batch(&[EnvelopeAdmission { envelope: &native.envelope, normalized: native.normalized }], Delivery::Live, clock().wall_time_ms + 5000).expect("first v2 fact after metadata-only tail");
            assert_eq!(state_hash(&journal.replay_from_genesis().expect("first fact after metadata boundary")), state_hash(journal.canonical_state()));
            // Genuine post-upgrade durable effects in the suffix. The
            // notification record is a controlled fixture through the
            // production admission API; no OS notification is sent.
            let attention_id = journal.canonical_state().attention.values().find(|item| !item.resolved()).expect("unhandled native control").id.clone();
            let request_id = journal.canonical_state().outbox.values().find(|record| record.attention_id == attention_id).expect("native control intent").request_id.clone();
            journal.record_notification_state(&request_id, &threadspace_contracts::projection::NotificationState::Submitted, "F1B controlled fixture submission receipt", clock().wall_time_ms + 6000).expect("post-upgrade notification disposition fact");
            let owner = threadspace_contracts::canonical::command::OwnerCommand {
                command_id: uuid::Uuid::new_v4().to_string(),
                attention_id: attention_id.clone(),
                expected_revision: Some(journal.canonical_state().attention[&attention_id].revision.to_string()),
                action: threadspace_contracts::canonical::command::OwnerAction::Resolve { reason: "F1B post-upgrade owner decision".into() },
            };
            let owner_receipt = journal.admit_owner_command(&owner, clock().wall_time_ms + 9000).expect("post-upgrade owner decision").receipt;
            assert_eq!(owner_receipt.status, threadspace_contracts::ui::ReceiptStatus::Committed);
            assert_eq!(state_hash(&journal.replay_from_genesis().expect("post-upgrade owner and notification genesis")), state_hash(journal.canonical_state()));
            journal.checkpoint("F1B_METADATA_THEN_FACT", clock().wall_time_ms).expect("final checkpoint");
            let expected = journal.canonical_state().clone();
            let hash = state_hash(journal.canonical_state());
            drop(journal);
            assert_eq!(state_hash(open(&store).canonical_state()), hash);
            restore_old_checkpoint(&store, &old_checkpoint);
            let mut recovered = open(&store);
            write_evidence(&format!("mixed-suffix-owner-notification-{name}-checkpoint-{checkpoint}"), &json!({ "expected": expected, "recovered": recovered.canonical_state(), "genesis": recovered.replay_from_genesis().expect("mixed owner suffix genesis"), "owner": owner, "receipt": owner_receipt, "entries": recovered.journal_entries(old_end).expect("post-upgrade suffix facts") }));
            assert_eq!(state_hash(recovered.canonical_state()), hash, "old checkpoint plus metadata, first v2 fact, notification disposition and owner command");
            assert_eq!(state_hash(&recovered.replay_from_genesis().expect("mixed suffix genesis")), hash);
            assert!(recovered.projection_differences().expect("mixed suffix tables").is_empty());
            let retry = recovered.admit_owner_command(&owner, clock().wall_time_ms + 10_000).expect("retained post-upgrade command receipt");
            assert_eq!(retry.receipt.status, threadspace_contracts::ui::ReceiptStatus::AlreadyCommitted);
            assert_eq!(retry.receipt.cursor, owner_receipt.cursor);
            assert_eq!(retry.receipt.target_revision, owner_receipt.target_revision);
            assert!(retry.change.is_none());
        }
    }
}

#[test]
fn f1b_unrecognized_journal_header_versions_refuse_recovery() {
    for version in [0, 3] {
        let store = TempStore::new();
        let mut journal = open(&store);
        let metadata = Capture::observer(record("session.attach", "A", None, 400, 1, None));
        assert!(metadata.normalized.drafts.is_empty());
        journal.admit_batch(&[EnvelopeAdmission { envelope: &metadata.envelope, normalized: metadata.normalized }], Delivery::Live, clock().wall_time_ms).expect("metadata admission");
        drop(journal);
        let conn = rusqlite::Connection::open(store.db()).expect("owned invalid-header fixture");
        conn.execute("UPDATE observations SET payload_version = ?1 WHERE canonical = 1", [version]).expect("explicit invalid/future header control");
        drop(conn);
        let result = Journal::open_with(&store.db(), "f1b-unsupported-header", clock().wall_time_ms, Box::new(SeededAllocator::new(909)), false);
        assert!(matches!(result, Err(threadspace_journal::JournalError::Invalid { detail }) if detail.contains("unsupported journal payload version")));
    }
}
