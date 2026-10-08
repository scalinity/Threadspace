//! The second independent review's wait owner-coverage witnesses (A–G),
//! through every layer: the pure reducer, the real SQLite journal, its
//! materialized attention and outbox, the public snapshot and patch, a
//! checkpoint, restart and replay from genesis, and the semantic oracle. An
//! owner decision applies only to the evidence it covered: evidence the
//! owner never handled keeps its item actionable, and decisions made on
//! different evidence are different semantic states.

use threadspace_contracts::canonical::command::OwnerAction;
use threadspace_contracts::canonical::records::{
    AttentionRecord, AttentionScope, CanonicalState, OutboxState, ResolutionKind, WaitScopeRecord,
};
use threadspace_journal::EnvelopeAdmission;
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_state_engine::synthetic::normalize_envelope;
use threadspace_state_engine::wait;
use threadspace_synthetic::builder::{OwnerStep, Scenario, Step, Target, session};
use threadspace_synthetic::permute::linear_extension;
use threadspace_synthetic::runner::{Admit, PureRunner, run};
use threadspace_synthetic::scenarios::{
    CLAUDE_LIKE, wait_new_after_resolution, wait_owner_covered_p, wait_owner_covered_q, wait_owner_covers_both,
    wait_owner_merge_keeps_coverage, wait_owner_partial_actions, wait_owner_partial_coverage, wait_owner_scopes,
};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};
use threadspace_synthetic::view::View;

struct Admitted {
    state: CanonicalState,
    semantic: String,
}

/// Admits `order` through the pure reducer and the SQLite journal, checks
/// every layer agrees, and returns the canonical state.
fn admit(scenario: &Scenario, order: &[usize]) -> Admitted {
    let mut pure = PureRunner::new(21);
    let report = run(scenario, order, &mut pure);
    assert!(report.violations.is_empty(), "{order:?}: {:?}", report.violations);
    assert!(report.owner_failures.is_empty(), "{order:?}: {:?}", report.owner_failures);

    let mut sqlite = SqliteRunner::open(TempStore::new("wait-owner-coverage"), 21).expect("open");
    let sqlite_report = run(scenario, order, &mut sqlite);
    assert!(sqlite_report.violations.is_empty(), "{order:?}: {:?}", sqlite_report.violations);
    assert_eq!(sqlite_report.semantic_hash, report.semantic_hash, "{order:?}: SQLite and pure semantics");

    // Materialized tables, the outbox rows and the public snapshot.
    let state = sqlite.journal.canonical_state().clone();
    let digest = sqlite.journal.replay_digest().expect("digest");
    assert_eq!(digest.projection_sha256, digest.tables_sha256, "{order:?}: tables equal the state");
    let eligible: i64 = rusqlite::Connection::open(sqlite.store.journal_path())
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM notification_outbox WHERE state IN ('PENDING', 'HELD')", [], |r| r.get(0)))
        .expect("outbox");
    assert_eq!(usize::try_from(eligible).ok(), Some(eligible_items(&state).len()), "{order:?}: materialized eligibility");
    let (_, snapshot) = sqlite.journal.snapshot().expect("snapshot");
    // The snapshot's attention is the public open-attention view.
    for item in state.attention.values().filter(|a| matches!(a.scope, AttentionScope::SessionWaitCategory { .. })) {
        let view = snapshot.attention.iter().find(|v| v.attention_id == item.id);
        assert_eq!(view.is_some(), !item.resolved(), "{order:?}: public open attention");
        if let Some(view) = view {
            assert_eq!(view.acknowledged_at_ms.is_some(), item.acknowledged(), "{order:?}: public acknowledgement");
        }
    }

    // A checkpoint, a restart and replay from genesis reproduce it.
    let current = state_hash(&state);
    sqlite.journal.checkpoint("TEST", 2).expect("checkpoint");
    assert_eq!(state_hash(&sqlite.journal.replay_from_genesis().expect("genesis")), current, "{order:?}: genesis replay");
    let restarted = sqlite.restart(22).expect("restart");
    assert_eq!(state_hash(restarted.state()), current, "{order:?}: restart");
    assert_eq!(semantic_hash(&state), report.semantic_hash);
    Admitted { state, semantic: report.semantic_hash }
}

/// Wait items with a currently eligible (PENDING or HELD) intent.
fn eligible_items(state: &CanonicalState) -> Vec<String> {
    let mut ids: Vec<String> = state
        .outbox
        .values()
        .filter(|o| matches!(o.state, OutboxState::Pending | OutboxState::Held))
        .map(|o| o.attention_id.clone())
        .collect();
    ids.sort();
    ids
}

fn scope<'a>(state: &'a CanonicalState, session: &str) -> &'a WaitScopeRecord {
    let id = &View::new(state).session(session).expect("session").id;
    let mut scopes = state.waits.values().filter(|w| &w.session_id == id);
    let scope = scopes.next().expect("a wait scope");
    assert!(scopes.next().is_none(), "one wait scope");
    scope
}

/// The item of the episode `index` of the session's one wait scope.
fn episode_item<'a>(state: &'a CanonicalState, session: &str, index: u32) -> &'a AttentionRecord {
    let scope = scope(state, session);
    let id = scope.episodes.iter().find(|e| e.index == index).and_then(|e| e.attention_id.as_ref()).expect("episode");
    &state.attention[id]
}

fn covered(state: &CanonicalState, session: &str) -> Vec<Vec<String>> {
    scope(state, session)
        .owner_decisions
        .iter()
        .map(|d| d.positives.iter().filter_map(|p| p.sequence.clone()).collect())
        .collect()
}

fn reference(scenario: &Scenario) -> Vec<usize> {
    (0..scenario.steps.len()).collect()
}

/// Valid orders other than the reference converge on its semantics.
fn converges(scenario: &Scenario, expected: &str) {
    for seed in 0..8u64 {
        let order = linear_extension(scenario.steps.len(), &scenario.constraints, 9_000 + seed);
        let mut pure = PureRunner::new(23);
        let report = run(scenario, &order, &mut pure);
        assert!(report.violations.is_empty(), "{order:?}: {:?}", report.violations);
        assert_eq!(report.semantic_hash, expected, "{}: {order:?} diverges", scenario.name);
        (scenario.expect)(&pure.engine.state).unwrap_or_else(|e| panic!("{}: {order:?}: {e}", scenario.name));
    }
}

#[test]
fn a_p2_resolve_c3_q1_keeps_the_unhandled_q_actionable() {
    // Steps: 0 session, 1 t1, 2 P2 (A), 3 owner Resolve, 4 C3 (A), 5 Q1 (B).
    let scenario = wait_owner_partial_coverage();
    let admitted = admit(&scenario, &[0, 1, 2, 3, 4, 5]);
    let state = &admitted.state;
    assert_eq!(covered(state, "sess-partial"), vec![vec!["2".to_owned()]], "the resolution covered P2 alone");
    let episodes = wait::partition(scope(state, "sess-partial"));
    let episode = &episodes[&0];
    assert_eq!(episode.positives.len(), 2, "P2 and Q1 share episode 0");
    assert_eq!(episode.active.iter().map(|p| p.source_epoch.as_str()).collect::<Vec<_>>(), ["mod-epoch-2"], "C3 cleared P2; Q1 stays active");
    let item = episode_item(state, "sess-partial", 0);
    assert!(!item.resolved() && !item.acknowledged(), "Q keeps the item open");
    assert!(!item.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner), "P's resolution does not reach Q");
    assert_eq!(eligible_items(state), vec![item.id.clone()], "Q's intent is eligible");
    converges(&scenario, &admitted.semantic);
}

#[test]
fn b_a_late_d1_merges_q_with_p_without_transferring_p_s_resolution() {
    // Steps: 0 session, 1 t1, 2 P10 (A), 3 owner Resolve, 4 C5 (A), 5 Q2 (B),
    // 6 D1 (B, before Q2).
    let scenario = wait_owner_merge_keeps_coverage();

    // Before D1: P10 alone in episode 1 (resolved), Q2 alone in episode 0 (open).
    let before = admit(&scenario, &[0, 1, 2, 3, 4, 5]);
    let p = episode_item(&before.state, "sess-merge", 1);
    let q = episode_item(&before.state, "sess-merge", 0);
    assert!(p.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner), "P10 handled");
    assert!(!q.resolved(), "Q2 actionable before D1");
    assert_eq!(eligible_items(&before.state), vec![q.id.clone()]);

    // D1 merges Q2 into episode 1: the item stays open for Q2.
    let after = admit(&scenario, &[0, 1, 2, 3, 4, 5, 6]);
    let state = &after.state;
    let episodes = wait::partition(scope(state, "sess-merge"));
    assert_eq!(episodes.keys().copied().collect::<Vec<_>>(), [1], "one episode after the merge");
    assert_eq!(episodes[&1].active.len(), 2, "P10 and Q2 both active (uncertain)");
    let merged = episode_item(state, "sess-merge", 1);
    assert!(!merged.resolved(), "P10's resolution is not transferred to Q2");
    assert_eq!(eligible_items(state), vec![merged.id.clone()], "Q2 notifiable");
    assert_eq!(covered(state, "sess-merge"), vec![vec!["10".to_owned()]], "the decision still covers P10 alone");
    assert_eq!(state.commands.len(), 1, "no owner action manufactured");
    converges(&scenario, &after.semantic);

    // The patch the UI applies for D1's admission carries the reopened item.
    let mut sqlite = SqliteRunner::open(TempStore::new("wait-merge-patch"), 21).expect("open");
    run(&scenario, &[0, 1, 2, 3, 4, 5], &mut sqlite);
    let Step::Observe(envelope) = &scenario.steps[6] else { panic!("D1 is an observation") };
    let admission = EnvelopeAdmission { envelope, normalized: normalize_envelope(envelope) };
    let outcome = sqlite.journal.admit_batch(&[admission], sqlite.delivery, 5_000).expect("admit D1");
    let patch = sqlite.journal.patch_for(0, outcome.change.as_ref().expect("a change")).expect("patch");
    let view = patch.attention_upserts.iter().find(|v| v.attention_id == merged.id).expect("merged item upsert");
    assert!(view.resolved_at_ms.is_none() && view.acknowledged_at_ms.is_none(), "public patch: needs attention");
}

#[test]
fn c_decisions_on_different_evidence_are_different_semantic_states() {
    // History A: P2, Resolve episode 0, Q10, C5. History B: Q10, Resolve
    // episode 0, P2, C5. Same native positives, same command ID and action;
    // the owner was shown P2 in A and Q10 in B.
    let p = wait_owner_covered_p();
    let q = wait_owner_covered_q();
    let a = admit(&p, &[0, 1, 2, 3, 4]);
    let b = admit(&q, &[0, 1, 2, 3, 4]);
    assert_eq!(scope(&a.state, "sess-coverage").positives, scope(&b.state, "sess-coverage").positives);
    assert_eq!(covered(&a.state, "sess-coverage"), vec![vec!["2".to_owned()]]);
    assert_eq!(covered(&b.state, "sess-coverage"), vec![vec!["10".to_owned()]]);
    assert_eq!(eligible_items(&a.state).len(), 1, "A: Q10 unhandled");
    assert_eq!(eligible_items(&b.state).len(), 1, "B: P2 unhandled");
    assert_ne!(a.semantic, b.semantic, "covered-P and covered-Q decisions differ");

    // C5 reveals the difference: it clears P2 and moves Q10 to episode 1.
    let a = admit(&p, &reference(&p));
    let b = admit(&q, &reference(&q));
    assert_eq!(eligible_items(&a.state).len(), 1, "A: Q10 still open");
    assert!(eligible_items(&b.state).is_empty(), "B: Q10 handled");
    assert_ne!(a.semantic, b.semantic);
    converges(&p, &a.semantic);
    converges(&q, &b.semantic);
}

#[test]
fn d_the_same_coverage_in_another_admissible_order_converges() {
    // Steps: 0 session, 1 t1, 2 P2, 3 Q10, 4 owner Resolve (after both), 5 C5.
    let both = wait_owner_covers_both();
    let a = admit(&both, &[0, 1, 2, 3, 4, 5]);
    let b = admit(&both, &[0, 1, 3, 2, 4, 5]);
    assert_eq!(a.semantic, b.semantic, "equal coverage converges");
    assert_eq!(covered(&a.state, "sess-coverage"), vec![vec!["10".to_owned(), "2".to_owned()]]);
    assert!(eligible_items(&a.state).is_empty());
    converges(&both, &a.semantic);
    // And it differs from a decision covering one of them.
    for one in [wait_owner_covered_p(), wait_owner_covered_q()] {
        assert_ne!(a.semantic, admit(&one, &reference(&one)).semantic, "{}", one.name);
    }
}

#[test]
fn e_a_new_wait_after_a_clear_is_not_resolved_by_the_old_decision() {
    // Steps: 0 session, 1 t1, 2 P10, 3 owner Resolve, 4 C12, 5 P15.
    let scenario = wait_new_after_resolution();
    let admitted = admit(&scenario, &reference(&scenario));
    let state = &admitted.state;
    let old = episode_item(state, "sess-new-wait", 0);
    assert!(old.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner), "P10's item keeps its resolution");
    assert!(old.resolutions.iter().any(|c| c.kind == ResolutionKind::WaitEnded), "and its native clear");
    let new = episode_item(state, "sess-new-wait", 1);
    assert!(!new.resolved(), "P15 is not owner-resolved automatically");
    assert_eq!(eligible_items(state), vec![new.id.clone()]);
    converges(&scenario, &admitted.semantic);
}

#[test]
fn f_owner_decisions_stay_on_their_turn_or_session_scope() {
    let scenario = wait_owner_scopes();
    let admitted = admit(&scenario, &reference(&scenario));
    (scenario.expect)(&admitted.state).expect("scopes");
    converges(&scenario, &admitted.semantic);
}

/// `scenario` plus one more owner command on `turn`'s wait witnessed by
/// `witness`, delivered after step `after`.
fn with_owner(mut scenario: Scenario, command: &str, turn: &str, witness: u64, action: OwnerAction, after: usize) -> Scenario {
    let index = scenario.steps.len();
    let previous: Vec<usize> = (0..index).filter(|&i| matches!(scenario.steps[i], Step::Owner(_))).collect();
    scenario.steps.push(Step::Owner(OwnerStep {
        command_id: command.into(),
        target: Target::Wait {
            session: session(CLAUDE_LIKE, "sess-partial-actions"),
            turn: Some(turn.into()),
            category: "INPUT".into(),
            witness,
        },
        action,
        at_ms: 1_900_000_000_000,
    }));
    scenario.constraints.push((after, index));
    scenario.constraints.extend(previous.into_iter().map(|p| (p, index)));
    scenario.name.push_str("-again");
    scenario
}

#[test]
fn g_acknowledge_and_snooze_apply_only_to_the_evidence_they_covered() {
    // Steps: 0 session; t1: 1 start, 2 P2, 3 Acknowledge, 4 C3, 5 Q1 (B);
    // t2: 6 start, 7 P12, 8 Snooze, 9 C13, 10 Q11 (B).
    let scenario = wait_owner_partial_actions();
    let item = |state: &CanonicalState, turn: &str| -> AttentionRecord {
        let v = View::new(state);
        let turn = v.turn("sess-partial-actions", None, turn).expect("turn").id.clone();
        v.wait_items("sess-partial-actions").into_iter().find(|i| i.turn_id.as_deref() == Some(turn.as_str())).expect("item").clone()
    };

    // While P is the only evidence, each action covers it.
    let early = admit(&scenario, &[0, 1, 2, 3, 6, 7, 8]);
    assert!(item(&early.state, "t1").acknowledged(), "t1 acknowledged");
    assert!(item(&early.state, "t2").snoozed_until_ms.is_some(), "t2 snoozed");

    // Q joins each episode: neither action reaches it.
    let full = admit(&scenario, &reference(&scenario));
    for turn in ["t1", "t2"] {
        let i = item(&full.state, turn);
        assert!(!i.acknowledged() && i.snoozed_until_ms.is_none() && !i.resolved(), "{turn}: Q keeps the item open");
    }
    assert_eq!(eligible_items(&full.state).len(), 2);
    converges(&scenario, &full.semantic);

    // Acting again on the item now showing P and Q covers both.
    let again = with_owner(scenario, "cmd-t1-again", "t1", 1, OwnerAction::Acknowledge, 5);
    let again = with_owner(again, "cmd-t2-again", "t2", 11, OwnerAction::Snooze { until_ms: 4_102_444_800_000 }, 10);
    let state = admit(&again, &reference(&again)).state;
    assert!(item(&state, "t1").acknowledged(), "t1 acknowledged once P and Q are covered");
    assert_eq!(item(&state, "t2").snoozed_until_ms, Some(4_102_444_800_000), "t2 snoozed once P and Q are covered");
    assert_eq!(eligible_items(&state).len(), 1, "the acknowledged item leaves the outbox; a snooze does not");
}

#[test]
fn the_oracle_compares_covered_witnesses_not_only_episode_ordinals() {
    // The two decisions of witness C, rewritten to cover the other positive:
    // the hash follows the coverage.
    let a = admit(&wait_owner_covered_p(), &[0, 1, 2, 3, 4]).state;
    let b = admit(&wait_owner_covered_q(), &[0, 1, 2, 3, 4]).state;
    let mut swapped = a.clone();
    for (wait, other) in swapped.waits.values_mut().zip(b.waits.values()) {
        wait.owner_decisions = other.owner_decisions.clone();
    }
    assert_eq!(semantic_hash(&swapped), semantic_hash(&b), "only the covered witness differed");
    let mut unordered = a.clone();
    unordered.waits.values_mut().flat_map(|w| &mut w.owner_decisions).for_each(|d| d.unordered += 1);
    assert_ne!(semantic_hash(&unordered), semantic_hash(&a), "the count of covered unordered positives");
}
