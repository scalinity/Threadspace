//! M3 fixtures through the actual conventional hook and observer adapters.
//! Inventory fixtures stand for the bracketed attach drafted by Journal;
//! the journal integration test separately exercises apply_discovery.
use crate::builder::{Scenario, Step, process};
use serde_json::{Value, json};
use threadspace_contracts::canonical::envelope::{
    CaptureClock, ClockQuality, ObservationEnvelope, ProcessRole, ProcessSample,
};
use threadspace_contracts::canonical::fact::{
    ActivationEvidenceKind, AttachedPresence, EvidenceClass, ExecutionMode, FactPayload,
    NativeActivation, NativeFactDraft, NativeRefs,
};
use threadspace_contracts::canonical::keys::NativeExecutionRef;
use threadspace_contracts::canonical::records::CanonicalState;
use threadspace_contracts::projection::{ExecutionPresence, TurnState};
use threadspace_provider_claude::{hooks, observer};
use threadspace_relay::modbatch::{self, BatchContext};
use threadspace_state_engine::normalize::Normalized;

pub const EPOCH: &str = "00000000-0000-4000-8000-000000000001";
pub const IMAGE: &str = "/opt/claude/versions/2.1.295#file-id";
fn context() -> BatchContext {
    BatchContext {
        profile_ref: "claude-cli:/qualification/.claude".into(),
        boot_id: Some("boot-A".into()),
        evidence: vec![ProcessSample {
            role: ProcessRole::Provider,
            key: process(4100, 1000),
            parent_pid: Some(1),
            executable: Some(IMAGE.into()),
            controlling_device: Some(77),
        }],
    }
}
fn clock(at: i64) -> CaptureClock {
    CaptureClock {
        endpoint_id: None,
        boot_id: Some("boot-A".into()),
        monotonic_ns: None,
        wall_time_ms: 1000 + at,
        clock_quality: ClockQuality::RemoteReported,
    }
}
pub fn observer_record(
    event: &str,
    session: &str,
    turn: Option<&str>,
    seq: u64,
    generation: u64,
    phase: &str,
    detail: Value,
) -> ObservationEnvelope {
    let record = json!({ "schemaVersion":1,"adapterId":"threadspace-observer","adapterVersion":"1",
        "observationId":threadspace_state_engine::ids::derived_id(&format!("m3:{event}:{session}:{seq}:{phase}")),
        "sourceEpoch":EPOCH,"callbackEntrySequence":seq.to_string(),"callbackResultSequence":(seq+1).to_string(),
        "phase":phase,"nativeEvent":event,"capturedAtMs":1000+seq,
        "dispatchOrigin":{"plugin":"engine","tier":"core"},"engineDispatch":true,
        "sessionId":session,"sessionIdSource":"classic.SessionStart","sessionGeneration":generation,
        "ownershipEpoch":EPOCH,"ownershipGeneration":generation,"ownershipStatus":"KNOWN",
        "nativeTurnId":turn,"payload":detail });
    modbatch::envelope(&record, EPOCH, &context()).expect("real mod-batch envelope")
}
pub fn hook(event: &str, session: &str, seq: u64, source: &str) -> ObservationEnvelope {
    use threadspace_relay::capture::{claude_hook_envelope, HookCapture};
    let mut provider = context().evidence.remove(0);
    provider.role = ProcessRole::Ancestor;
    let capture = ProcessSample {
        role: ProcessRole::Capture,
        key: process(4102, 1002),
        parent_pid: Some(4101),
        executable: Some("/qualification/threadspace-hook#1:2".into()),
        controlling_device: Some(77),
    };
    let shell = ProcessSample {
        role: ProcessRole::Ancestor,
        key: process(4101, 1001),
        parent_pid: Some(4100),
        executable: Some("/bin/zsh#1:3".into()),
        controlling_device: Some(77),
    };
    claude_hook_envelope(&json!({"hook_event_name":event,"session_id":session,"source":source}), HookCapture {
        observation_id: threadspace_state_engine::ids::derived_id(&format!("m3-hook:{event}:{session}:{seq}")),
        profile_ref: context().profile_ref,
        clock: clock(seq as i64),
        evidence: vec![capture, shell, provider],
    }).expect("production conventional-hook envelope")
}

pub fn inventory(session: &str, seq: u64) -> ObservationEnvelope {
    let mut envelope = hook("SessionStart", session, seq, "startup");
    envelope.evidence = context().evidence;
    envelope.adapter_id = "m3.inventory-fixture".into();
    envelope.native_event = "inventory.join".into();
    envelope.source_id = "claude.inventory".into();
    envelope.activation_ref = Some(format!("inventory:{seq}"));
    envelope
}
pub fn normalize(envelope: &ObservationEnvelope) -> Normalized {
    match envelope.adapter_id.as_str() {
        hooks::ADAPTER_ID => hooks::normalize(envelope),
        observer::ADAPTER_ID => observer::normalize(envelope),
        "m3.inventory-end" => Normalized {
            retained: envelope.payload.clone(), unsupported: None,
            drafts: vec![NativeFactDraft {
                refs: NativeRefs { session: envelope.session_key.clone(), process: Some(envelope.evidence[0].key.clone()),
                    execution: envelope.activation_ref.clone().map(|activation_ref| NativeExecutionRef::Activation { activation_ref }),
                    ..NativeRefs::default() },
                provenance: EvidenceClass::Kernel, causal: None,
                payload: FactPayload::ExecutionEnded { reason: "SESSION_SWITCHED".into() },
            }],
        },
        "m3.inventory-fixture" => {
            let process = envelope.evidence[0].clone();
            Normalized {
                retained: envelope.payload.clone(),
                unsupported: None,
                drafts: vec![
                    NativeFactDraft {
                        refs: NativeRefs {
                            process: Some(process.key.clone()),
                            ..NativeRefs::default()
                        },
                        provenance: EvidenceClass::Kernel,
                        causal: None,
                        payload: FactPayload::ProcessObserved {
                            executable_identity: IMAGE.into(),
                        },
                    },
                    NativeFactDraft {
                        refs: NativeRefs {
                            session: envelope.session_key.clone(),
                            process: Some(process.key),
                            execution: envelope.activation_ref.clone().map(|activation_ref| {
                                NativeExecutionRef::Activation { activation_ref }
                            }),
                            activation: (!envelope.payload["legacy"].as_bool().unwrap_or(false)).then_some(NativeActivation {
                                kind: ActivationEvidenceKind::Inventory,
                                entry_sequence: None,
                                start_source: None,
                                generation: None,
                            }),
                            ..NativeRefs::default()
                        },
                        provenance: EvidenceClass::ProviderSnapshot,
                        causal: None,
                        payload: FactPayload::ExecutionAttached {
                            mode: ExecutionMode::TerminalEmbedded,
                            presence: AttachedPresence::Live,
                            native_runtime_id: None,
                            controlling_device: Some(77),
                        },
                    },
                ],
            }
        }
        _ => threadspace_state_engine::synthetic::normalize_envelope(envelope),
    }
}

fn legacy_inventory() -> ObservationEnvelope {
    let mut e = inventory("A", 2);
    e.activation_ref = Some("process:4100:1000.0#old-join".into());
    e.payload = json!({"legacy":true});
    e
}
fn inventory_end() -> ObservationEnvelope {
    let mut e = legacy_inventory();
    e.adapter_id = "m3.inventory-end".into();
    e.observation_id = threadspace_state_engine::ids::derived_id("m3:exact-inventory-end");
    e
}
fn inventory_resume_scenario(name: &str, end_first: bool) -> Scenario {
    let b = start("B", 30, 3, "clear");
    let mut s = scenario(name, vec![legacy_inventory(),
        if end_first { inventory_end() } else { b.clone() },
        if end_first { b } else { inventory_end() },
        start("A", 50, 5, "resume"), turn("A", 60, 5, "resumed")],
        |s| {
            count(s, 3, 1)?;
            let t = s.turns.values().find(|t|t.native_turn_id.as_deref()==Some("resumed")).ok_or("resumed turn")?;
            if t.execution_ids.len()!=1 || s.executions[t.execution_ids.first().unwrap()].presence!=ExecutionPresence::Live {
                return Err("ended inventory poisoned resumed A".into());
            }
            Ok(())
        });
    // The old attach is already committed when the M3 evidence is admitted.
    s.constraints = vec![(0,1),(0,2),(0,3),(0,4)];
    s
}

fn settled() -> Value {
    json!({ "core":{"coreSettled":true} })
}
fn start(session: &str, seq: u64, generation: u64, source: &str) -> ObservationEnvelope {
    observer_record(
        "classic.SessionStart",
        session,
        None,
        seq,
        generation,
        "entry",
        json!({"source":source}),
    )
}
fn turn(session: &str, seq: u64, generation: u64, id: &str) -> ObservationEnvelope {
    observer_record(
        "turn.start",
        session,
        Some(id),
        seq,
        generation,
        "result",
        settled(),
    )
}
fn scenario(
    name: &str,
    events: Vec<ObservationEnvelope>,
    expect: crate::builder::Expect,
) -> Scenario {
    Scenario {
        name: name.into(),
        family: "native-activation-ownership",
        steps: events
            .into_iter()
            .map(|e| Step::Observe(Box::new(e)))
            .collect(),
        constraints: vec![],
        expect,
    }
}
fn count(state: &CanonicalState, total: usize, live: usize) -> Result<(), String> {
    if state.executions.len() != total
        || state
            .executions
            .values()
            .filter(|e| e.presence == ExecutionPresence::Live)
            .count()
            != live
    {
        return Err(format!(
            "expected {total} executions / {live} live; got {:?}",
            state.executions
        ));
    }
    Ok(())
}
fn aba_expected(state: &CanonicalState) -> Result<(), String> {
    count(state, 3, 1)?;
    let old = state
        .turns
        .values()
        .find(|t| t.native_turn_id.as_deref() == Some("a1"))
        .ok_or("missing a1")?;
    let new = state
        .turns
        .values()
        .find(|t| t.native_turn_id.as_deref() == Some("a2"))
        .ok_or("missing a2")?;
    if old.execution_ids.len() != 1
        || new.execution_ids.len() != 1
        || old.execution_ids == new.execution_ids
    {
        return Err("turns lost originating activation".into());
    }
    if old.state != TurnState::Completed {
        return Err("delayed old completion missing".into());
    }
    if state.executions[old.execution_ids.first().expect("one")].presence
        != ExecutionPresence::Ended
    {
        return Err("old activation still live".into());
    }
    if state.executions[new.execution_ids.first().expect("one")].presence != ExecutionPresence::Live
    {
        return Err("legacy end closed newer A".into());
    }
    Ok(())
}
pub fn catalog() -> Vec<Scenario> {
    let mut completion = observer_record(
        "turn.complete",
        "A",
        Some("a1"),
        22,
        1,
        "result",
        json!({"reason":"answer","core":{"coreSettled":true}}),
    );
    completion.source_sequence = Some("100".into());
    completion.callback_result_sequence = Some("100".into());
    completion.captured_at = clock(100);
    let mut reload = observer_record(
        "session.start",
        "A",
        None,
        90,
        1,
        "bootstrap",
        json!({"hostSessionId":"A","predecessorEpoch":EPOCH}),
    );
    reload.source_epoch = "00000000-0000-4000-8000-000000000002".into();
    reload.payload["sessionIdSource"] = json!("session.id");
    let mut delayed_start = turn("A", 20, 1, "a1");
    delayed_start.source_sequence = Some("99".into());
    delayed_start.callback_result_sequence = Some("99".into());
    delayed_start.captured_at = clock(99);
    let mut engine_start = observer_record("session.start", "A", None, 12, 1, "result", settled());
    engine_start.payload["sessionIdSource"] = json!("classic.SessionStart");
    vec![
        scenario(
            "claude-native-a-b-a",
            vec![
                hook("SessionStart", "A", 10, "startup"),
                start("A", 11, 1, "startup"),
                delayed_start,
                hook("SessionStart", "B", 30, "clear"),
                start("B", 31, 3, "clear"),
                turn("B", 40, 3, "b1"),
                hook("SessionStart", "A", 50, "resume"),
                start("A", 51, 5, "resume"),
                turn("A", 60, 5, "a2"),
                inventory("A", 70),
                observer_record(
                    "session.end",
                    "A",
                    None,
                    25,
                    1,
                    "entry",
                    json!({"reason":"resume"}),
                ),
                hook("SessionEnd", "A", 95, ""),
                completion,
                hook("SessionStart", "A", 80, "compact"),
                start("A", 81, 5, "compact"),
                reload,
            ],
            aba_expected,
        ),
        scenario(
            "claude-inventory-native-convergence",
            vec![
                hook("SessionStart", "A", 10, "startup"),
                start("A", 11, 1, "startup"),
                inventory("A", 20),
                engine_start,
                turn("A", 30, 1, "one"),
            ],
            |s| {
                count(s, 1, 1)?;
                if s.turns.values().any(|t| t.execution_ids.len() != 1) {
                    return Err("turn lacks execution".into());
                }
                Ok(())
            },
        ),
        scenario(
            "claude-exact-native-end",
            vec![
                start("A", 10, 1, "startup"),
                turn("A", 20, 1, "old"),
                observer_record(
                    "session.end",
                    "A",
                    None,
                    25,
                    1,
                    "entry",
                    json!({"reason":"resume"}),
                ),
                start("A", 30, 3, "resume"),
                turn("A", 40, 3, "new"),
            ],
            |s| count(s, 2, 1),
        ),
        scenario(
            "claude-compaction-reload",
            vec![
                start("A", 10, 1, "startup"),
                turn("A", 20, 1, "one"),
                hook("SessionStart", "A", 30, "compact"),
                start("A", 31, 1, "compact"),
                {
                    let mut reload = observer_record(
                        "session.start",
                        "A",
                        None,
                        40,
                        1,
                        "bootstrap",
                        json!({"hostSessionId":"A","predecessorEpoch":EPOCH}),
                    );
                    reload.source_epoch = "00000000-0000-4000-8000-000000000002".into();
                    reload.payload["sessionIdSource"] = json!("session.id");
                    reload
                },
            ],
            |s| count(s, 1, 1),
        ),
        scenario(
            "claude-same-generation-reopen",
            vec![
                start("A", 10, 1, "startup"),
                turn("A", 20, 1, "old"),
                observer_record(
                    "session.end",
                    "A",
                    None,
                    25,
                    1,
                    "entry",
                    json!({"reason":"resume"}),
                ),
                start("A", 30, 1, "resume"),
                turn("A", 40, 1, "new"),
                hook("SessionEnd", "A", 50, ""),
            ],
            |s| {
                count(s, 2, 1)?;
                let old = s
                    .turns
                    .values()
                    .find(|t| t.native_turn_id.as_deref() == Some("old"))
                    .ok_or("old")?;
                let new = s
                    .turns
                    .values()
                    .find(|t| t.native_turn_id.as_deref() == Some("new"))
                    .ok_or("new")?;
                if old.execution_ids.len() != 1
                    || new.execution_ids.len() != 1
                    || old.execution_ids == new.execution_ids
                {
                    return Err("reopen collapsed into one generation".into());
                }
                if !s.executions[old.execution_ids.first().expect("old")]
                    .end_reasons
                    .contains("SESSION_END")
                {
                    return Err("exact end missing".into());
                }
                if s.executions[new.execution_ids.first().expect("new")].presence
                    != ExecutionPresence::Live
                {
                    return Err("old end closed new opening".into());
                }
                Ok(())
            },
        ),
        inventory_resume_scenario("claude-inventory-a-b-resume-a", false),
        inventory_resume_scenario("claude-legacy-end-before-native", true),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Admit, PureRunner, run};
    use crate::sqlite::{SqliteRunner, TempStore};
    #[test]
    fn hook_uses_nearest_validated_claude_ancestor() {
        let mut e = hook("SessionStart", "A", 10, "startup");
        let bytes = serde_json::to_vec(&e).unwrap();
        let mut outer = e.evidence[2].clone();
        outer.key = process(4000, 900);
        e.evidence[2].parent_pid = Some(4000);
        e.evidence.push(outer);
        let facts = hooks::normalize(&e).drafts;
        let attach = facts.iter().find(|f|matches!(f.payload, FactPayload::ExecutionAttached {..})).expect("production hook opening");
        assert_eq!(attach.refs.process.as_ref(), Some(&process(4100, 1000)));
        let end = hook("SessionEnd", "A", 20, "");
        assert!(hooks::normalize(&end).drafts.iter().any(|f|matches!(f.payload, FactPayload::ExecutionEnded {..})));
        let original = hook("SessionStart", "A", 10, "startup");
        let _ = hooks::normalize(&original);
        assert_eq!(serde_json::to_vec(&original).unwrap(), bytes, "normalization preserves stored bytes");
        e.evidence.truncate(2);
        assert!(!hooks::normalize(&e).drafts.iter().any(|f|matches!(f.payload, FactPayload::ExecutionAttached {..})));
    }
    #[test]
    fn hook_and_engine_only_opening_corroborate_and_legacy_end_matches() {
        let events = [hook("SessionStart", "A", 10, "startup"),
            observer_record("session.start", "A", None, 12, 1, "result", settled()),
            hook("SessionEnd", "A", 30, "")];
        for order in [[0,1,2], [1,0,2], [2,1,0]] {
            let mut runner = PureRunner::new(1);
            for i in order { runner.observe(&events[i]); }
            count(runner.state(), 1, 0).expect("hook and engine opening are one ended activation");
            assert!(runner.state().executions.values().next().unwrap().end_reasons.contains("SESSION_END"));
        }
    }
    fn discovery(pass: u64, session: &str) -> threadspace_journal::DiscoveryApplication {
        use threadspace_journal::{ActivationChange, DiscoveryApplication, ProcessRecord};
        DiscoveryApplication {
            provider: "claude".into(), profile_ref: context().profile_ref, sessions: vec![], payload: json!({}), pass,
            wait_of: |_|None,
            changes: vec![ActivationChange::Start { native_session_id: session.into(),
                process: ProcessRecord { boot_id: "boot-A".into(), pid:4100,start_seconds:1000,start_microseconds:0,executable:IMAGE.into() },
                device:77, surface:Err("QUALIFICATION_UNBOUND".into()) }],
        }
    }
    fn end_live(runner: &mut SqliteRunner, pass: u64, session: &str) {
        use threadspace_journal::ActivationChange;
        let session_id = runner.state().sessions.values().find(|s|s.native_session_id==session).unwrap().id.clone();
        let id = runner.state().executions.values().find(|e|e.session_id==session_id && e.presence==ExecutionPresence::Live).unwrap().id.clone();
        let mut app = discovery(pass, session);
        app.changes = vec![ActivationChange::End {execution_id:id,reason:"SESSION_SWITCHED".into()}];
        runner.journal.apply_discovery(&app, 1000+pass as i64).unwrap();
    }
    fn assert_journal(runner: &SqliteRunner) {
        assert!(runner.journal.projection_differences().unwrap().is_empty());
        assert_eq!(threadspace_state_engine::hash::state_hash(runner.state()),
            threadspace_state_engine::hash::state_hash(&runner.journal.replay_from_genesis().unwrap()));
    }
    #[test]
    fn ended_inventory_does_not_poison_resume_or_depend_on_admission_order() {
        let mut failures = Vec::new();
        for first in [false, true] {
            let scenario = inventory_resume_scenario("regression", first);
            let order: Vec<_> = (0..scenario.steps.len()).collect();
            let mut runner = PureRunner::new(1);
            run(&scenario, &order, &mut runner);
            if let Err(e) = (scenario.expect)(runner.state()) { failures.push((first,e)); }
        }
        assert!(failures.is_empty(), "both R3 orders: {failures:?}");
        let mut runner = SqliteRunner::open(TempStore::new("m3-resume-polls"),42).unwrap();
        runner.journal.apply_discovery(&discovery(1,"A"),1001).unwrap();
        runner.observe(&start("B",30,3,"clear"));
        end_live(&mut runner,2,"A");
        runner.observe(&start("A",50,5,"resume"));
        count(runner.state(),3,1).expect("resumed A is live");
        let candidates = runner.state().activation_candidates.len();
        for pass in 3..6 {
            let outcome = runner.journal.apply_discovery(&discovery(pass,"A"),1000+pass as i64).unwrap();
            assert_eq!(outcome.executions_started,0);
            assert_eq!(runner.state().activation_candidates.len(),candidates);
            count(runner.state(),3,1).unwrap();
        }
        assert_journal(&runner);
    }
    #[test]
    fn late_native_opening_is_ended_by_next_reconcile() {
        let mut runner = SqliteRunner::open(TempStore::new("m3-late-native"),42).unwrap();
        runner.journal.apply_discovery(&discovery(1,"A"),1001).unwrap();
        end_live(&mut runner,2,"A");
        runner.observe(&start("A",10,1,"startup"));
        count(runner.state(),2,1).unwrap();
        end_live(&mut runner,3,"A");
        count(runner.state(),2,0).unwrap();
        assert_journal(&runner);
    }
    #[test]
    fn lagging_inventory_switch_does_not_poison_later_positive_join() {
        let mut runner = SqliteRunner::open(TempStore::new("m3-lagging-row"),42).unwrap();
        runner.observe(&start("A",50,5,"resume"));
        // The reconcile planner accepts the lagging B join as a switch and
        // addresses its end to the fresh A ID. That end stays authoritative.
        end_live(&mut runner,2,"A");
        count(runner.state(),1,0).unwrap();
        runner.journal.apply_discovery(&discovery(3,"A"),1003).unwrap();
        count(runner.state(),2,1).expect("later positive inventory is a separate live attachment");
        let candidates = runner.state().activation_candidates.len();
        for pass in 4..7 {
            assert_eq!(runner.journal.apply_discovery(&discovery(pass,"A"),1000+pass as i64).unwrap().executions_started,0);
            assert_eq!(runner.state().activation_candidates.len(),candidates);
        }
        assert_journal(&runner);
    }
    #[test]
    fn activation_work_is_scoped_to_touched_runtime() {
        use threadspace_state_engine::engine::{Engine, Index};
        let mut runner = PureRunner::new(1);
        runner.observe(&start("A",10,1,"startup"));
        let mut other = start("other",10,1,"startup");
        other.evidence[0].key = process(5100,2000);
        runner.observe(&other);
        for i in 0..100 {
            let mut e = turn("other",20+i,1,&format!("other-{i}"));
            e.evidence[0].key = process(5100,2000);
            runner.observe(&e);
        }
        let mut engine = Engine::new(runner.engine.state.clone());
        runner.observe(&turn("A",20,1,"current"));
        let output = engine.apply(runner.entries.last().unwrap());
        assert_eq!(output.activation_facts_visited,2,"one A opening and the new A turn; unrelated history is untouched");
        assert_eq!(engine.index(), &Index::build(&engine.state));
        runner.observe(&start("A",30,1,"resume"));
        let output = engine.apply(runner.entries.last().unwrap());
        assert_eq!(output.activation_facts_visited,3,"two A openings and its one turn");
        assert_eq!(engine.index(), &Index::build(&engine.state));
    }
    #[test]
    #[ignore = "explicit 2000-turn debug/release measurement"]
    fn activation_apply_scaling() {
        use std::time::Instant;
        let mut prep = PureRunner::new(1);
        prep.observe(&start("A", 1, 1, "startup"));
        let mut engine = threadspace_state_engine::Engine::empty();
        engine.apply(&prep.entries[0]);
        let mut samples = Vec::new();
        for i in 0..2000 {
            prep.observe(&turn("A", 10+i, 1, &format!("turn-{i}")));
            let entry = prep.entries.last().unwrap();
            let began = Instant::now();
            engine.apply(entry);
            samples.push(began.elapsed().as_secs_f64()*1000.0);
        }
        let mean = |slice: &[f64]|slice.iter().sum::<f64>()/slice.len() as f64;
        println!("R4_MEASUREMENT turns=2000 first100_ms={:.6} last100_ms={:.6}", mean(&samples[..100]),mean(&samples[1900..]));
        assert_eq!(engine.state.turns.len(), 2000);
    }
    #[test]
    fn pre_change_helper_retry_is_already_committed() {
        use threadspace_contracts::canonical::{capture::RecordStatus, fact::Delivery};
        use threadspace_journal::EnvelopeAdmission;
        let current = start("A",10,1,"startup");
        let mut old = current.clone();
        old.activation_ref = None; // Pre-M3 modbatch::envelope wire representation.
        let mut runner = SqliteRunner::open(TempStore::new("m3-helper-retry"),42).unwrap();
        runner.journal.admit_batch(&[EnvelopeAdmission {envelope:&old,normalized:observer::normalize(&old)}],Delivery::Live,1010).unwrap();
        let receipt = runner.observe(&current);
        assert_eq!(receipt.status,RecordStatus::AlreadyCommitted,"helper upgrade preserves the stored envelope bytes");
        count(runner.state(),1,1).unwrap();
        assert_journal(&runner);
    }
    #[test]
    fn each_real_opening_source_establishes_an_activation() {
        for source in ["startup", "resume", "clear"] {
            for event in [hook("SessionStart", "A", 10, source), start("A", 10, 1, source)] {
                let mut runner = PureRunner::new(1);
                runner.observe(&event);
                count(&runner.engine.state, 1, 1).expect(source);
                let mut without_process = event.clone();
                without_process.evidence.clear();
                let mut runner = PureRunner::new(1);
                runner.observe(&without_process);
                count(&runner.engine.state, 0, 0).expect("no kernel process proof");
            }
        }
        let mut runner = PureRunner::new(1);
        runner.observe(&observer_record("session.start", "A", None, 12, 1, "result", settled()));
        count(&runner.engine.state, 1, 1).expect("engine opening without classic callback delivery");
    }
    #[test]
    fn native_ownership_ignores_wall_time_and_duplicate_deliveries() {
        let mut scenario = catalog().remove(0);
        let order: Vec<_> = (0..scenario.steps.len()).collect();
        let mut baseline = PureRunner::new(1);
        let expected = run(&scenario, &order, &mut baseline).semantic_hash;
        for (i, step) in scenario.steps.iter_mut().enumerate() {
            if let Step::Observe(e) = step {
                e.captured_at.wall_time_ms = if i % 2 == 0 { 0 } else { i64::MAX - i as i64 };
            }
        }
        let mut shuffled = order.clone();
        shuffled.reverse();
        shuffled.extend(order);
        let mut runner = PureRunner::new(2);
        let result = run(&scenario, &shuffled, &mut runner);
        assert_eq!(result.semantic_hash, expected);
        assert!(result.violations.is_empty(), "{:?}", result.violations);
        (scenario.expect)(&runner.engine.state).expect("original ownership");
    }
    #[test]
    fn existing_inventory_journal_converges_without_rewriting_old_facts() {
        use threadspace_contracts::canonical::fact::Delivery;
        use threadspace_journal::EnvelopeAdmission;
        let mut runner =
            SqliteRunner::open(TempStore::new("m3-legacy-inventory"), 42).expect("journal");
        let mut join = inventory("A", 20);
        join.activation_ref = Some("process:4100:1000.0#old-join".into());
        let mut old = normalize(&join);
        for fact in &mut old.drafts {
            fact.refs.activation = None;
        }
        runner
            .journal
            .admit_batch(
                &[EnvelopeAdmission {
                    envelope: &join,
                    normalized: old,
                }],
                Delivery::Live,
                1020,
            )
            .expect("old inventory representation");
        assert!(runner.state().activation_candidates.is_empty());
        let start = hook("SessionStart", "A", 10, "startup");
        runner.observe(&start);
        count(runner.state(), 1, 1).expect("legacy/native convergence");
        assert!(
            runner
                .journal
                .projection_differences()
                .expect("readback")
                .is_empty()
        );
        assert_eq!(
            threadspace_state_engine::hash::state_hash(
                &runner
                    .journal
                    .replay_from_genesis()
                    .expect("old and new facts")
            ),
            threadspace_state_engine::hash::state_hash(runner.state())
        );
    }
    #[test]
    fn journal_inventory_join_converges_with_native_start_and_binding() {
        use threadspace_contracts::canonical::fact::Delivery;
        use threadspace_journal::EnvelopeAdmission;
        use threadspace_journal::{
            ActivationChange, DiscoveryApplication, ProcessRecord, SurfaceRecord,
        };
        for (native_first, reason) in [false, true].into_iter().flat_map(|first|
            ["PROCESS_EXITED", "EXECUTABLE_CHANGED", "SESSION_SWITCHED"].map(|reason| (first, reason))) {
            let mut runner =
                SqliteRunner::open(TempStore::new("m3-discovery"), 42).expect("journal");
            let capture = hook("SessionStart", "A", 10, "startup");
            let admit = |runner: &mut SqliteRunner| {
                runner
                    .journal
                    .admit_batch(
                        &[EnvelopeAdmission {
                            envelope: &capture,
                            normalized: hooks::normalize(&capture),
                        }],
                        Delivery::Live,
                        1030,
                    )
                    .expect("real hook admission");
            };
            if native_first {
                admit(&mut runner);
            }
            let app = DiscoveryApplication {
                provider: "claude".into(),
                profile_ref: context().profile_ref,
                sessions: vec![],
                payload: json!({}),
                pass: 1,
                wait_of: |_| None,
                changes: vec![ActivationChange::Start {
                    native_session_id: "A".into(),
                    process: ProcessRecord {
                        boot_id: "boot-A".into(),
                        pid: 4100,
                        start_seconds: 1000,
                        start_microseconds: 0,
                        executable: IMAGE.into(),
                    },
                    device: 77,
                    surface: Ok(SurfaceRecord {
                        surface_kind: "terminal.app".into(),
                        locator: "/dev/ttys077".into(),
                        tty: "/dev/ttys077".into(),
                        device: 77,
                        terminal_generation: "terminal-test".into(),
                        window_hint: Some(1),
                        tab_hint: Some(1),
                        proof: json!({}),
                    }),
                }],
            };
            runner
                .journal
                .apply_discovery(&app, 1020)
                .expect("real inventory admission");
            if !native_first {
                admit(&mut runner);
            }
            count(runner.state(), 1, 1).expect("one activation");
            if !native_first {
                let binding = runner
                    .state()
                    .bindings
                    .values()
                    .next()
                    .expect("retained inventory binding");
                assert!(binding.valid);
                assert!(
                    runner
                        .state()
                        .executions
                        .contains_key(&binding.execution_id)
                );
            }
            assert!(
                runner
                    .journal
                    .projection_differences()
                    .expect("readback")
                    .is_empty()
            );
            let id = runner
                .state()
                .executions
                .keys()
                .next()
                .expect("execution")
                .clone();
            let end = DiscoveryApplication {
                pass: 2,
                changes: vec![ActivationChange::End {
                    execution_id: id,
                    reason: reason.into(),
                }],
                ..app
            };
            runner
                .journal
                .apply_discovery(&end, 1040)
                .expect("reconcile still ends by exact ID");
            count(runner.state(), 1, 0).expect("ended");
            assert!(
                runner
                    .journal
                    .projection_differences()
                    .expect("ended readback")
                    .is_empty()
            );
            let hash = threadspace_state_engine::hash::state_hash(runner.state());
            assert_eq!(
                threadspace_state_engine::hash::state_hash(
                    &runner.journal.replay_from_genesis().expect("genesis")
                ),
                hash
            );
        }
    }
    #[test]
    fn real_claude_activation_orders_replay_and_sqlite() {
        for scenario in catalog() {
            let order: Vec<_> = (0..scenario.steps.len()).collect();
            let mut reference = PureRunner::new(1);
            let reference_report = run(&scenario, &order, &mut reference);
            (scenario.expect)(&reference.engine.state)
                .unwrap_or_else(|e| panic!("{}: {e}", scenario.name));
            for order in [order.clone(), order.iter().copied().rev().collect()] {
                let mut runner = SqliteRunner::open(TempStore::new("m3-native"), 7).expect("open");
                let report = run(&scenario, &order, &mut runner);
                assert_eq!(
                    report.semantic_hash, reference_report.semantic_hash,
                    "{} receipts {:?}",
                    scenario.name, report.receipts
                );
                (scenario.expect)(runner.state()).expect("expectations");
                assert!(
                    runner
                        .journal
                        .projection_differences()
                        .expect("table readback")
                        .is_empty()
                );
                let hash = threadspace_state_engine::hash::state_hash(runner.state());
                assert_eq!(
                    threadspace_state_engine::hash::state_hash(
                        &runner.journal.replay_from_genesis().expect("genesis")
                    ),
                    hash
                );
                runner
                    .journal
                    .checkpoint("M3_NATIVE", 2000)
                    .expect("checkpoint");
                let runner = runner.restart(9).expect("restart");
                assert_eq!(
                    threadspace_state_engine::hash::state_hash(runner.state()),
                    hash
                );
            }
        }
    }
}
