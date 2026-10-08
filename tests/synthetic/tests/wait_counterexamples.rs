//! The independent review's wait counterexamples, in their exact delivery
//! orders, through the pure path and the real SQLite journal: both orders
//! must agree on turn state, attention, owner resolution, currently
//! eligible intents and the semantic hash. Then the semantic oracle must
//! notice an owner decision being removed.

use threadspace_contracts::canonical::records::{CanonicalState, OutboxState, ResolutionKind};
use threadspace_contracts::projection::TurnState;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_synthetic::builder::Scenario;
use threadspace_synthetic::runner::{PureRunner, run};
use threadspace_synthetic::scenarios::{
    wait_owner_resolution_kept, wait_reappears, wait_turn_ownership, wait_uncertain_reopens,
};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};
use threadspace_synthetic::view::View;

fn eligible(state: &CanonicalState) -> usize {
    state
        .outbox
        .values()
        .filter(|o| matches!(o.state, OutboxState::Pending | OutboxState::Held))
        .count()
}

/// Admits `order` through both paths; returns the pure state after checking
/// the SQLite path reached the same semantics.
fn admit(scenario: &Scenario, order: &[usize]) -> CanonicalState {
    let mut pure = PureRunner::new(11);
    let report = run(scenario, order, &mut pure);
    assert!(report.violations.is_empty(), "{order:?}: {:?}", report.violations);
    let mut sqlite = SqliteRunner::open(TempStore::new("wait-counterexample"), 11).expect("open");
    let sqlite_report = run(scenario, order, &mut sqlite);
    assert_eq!(sqlite_report.semantic_hash, report.semantic_hash, "{order:?}: SQLite and pure paths agree");
    pure.engine.state
}

fn converge(scenario: &Scenario, orders: [&[usize]; 2]) -> CanonicalState {
    let a = admit(scenario, orders[0]);
    let b = admit(scenario, orders[1]);
    assert_eq!(eligible(&a), eligible(&b), "currently eligible intents");
    assert_eq!(semantic_hash(&a), semantic_hash(&b), "semantic hash");
    a
}

#[test]
fn p10_c5_q_keeps_both_waits_notifiable_in_either_order() {
    // Steps: 0 session, 1 t1, 2 P10, 3 C5, 4 Q (another source epoch).
    let state = converge(&wait_reappears(), [&[0, 1, 2, 3, 4], &[0, 1, 3, 2, 4]]);
    assert_eq!(eligible(&state), 2, "P10's and Q's waits");
}

#[test]
fn p2_c3_q_keeps_the_uncertain_wait_notifiable_in_either_order() {
    // Steps: 0 session, 1 t1, 2 P2, 3 C3, 4 Q.
    let state = converge(&wait_uncertain_reopens(), [&[0, 1, 2, 3, 4], &[0, 1, 4, 2, 3]]);
    assert_eq!(eligible(&state), 1, "never zero intents for an unresolved wait");
}

#[test]
fn an_owner_resolution_survives_a_late_earlier_clear() {
    // Steps: 0 session, 1 t1, 2 P10, 3 owner Resolve, 4 C5.
    let state = converge(&wait_owner_resolution_kept(), [&[0, 1, 2, 3, 4], &[0, 1, 4, 2, 3]]);
    let v = View::new(&state);
    let items = v.wait_items("sess-owner-wait");
    assert!(items.iter().all(|i| i.resolved()), "no item reopens");
    assert!(items.iter().any(|i| i.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner)));
    assert_eq!(eligible(&state), 0);
}

#[test]
fn a_waiting_turn_owns_its_wait_whichever_turn_arrives_first() {
    // Steps: 0 session, 1 t1 start, 2 t1 P3, 3 t1 C4, 4 t1 complete,
    // 5 t2 start, 6 t2 P7.
    let state = converge(&wait_turn_ownership(), [&[0, 1, 2, 3, 4, 5, 6], &[0, 6, 5, 1, 2, 3, 4]]);
    let v = View::new(&state);
    assert_eq!(v.turn("sess-turn-wait", None, "t1").map(|t| t.state.clone()), Some(TurnState::Completed));
    assert_eq!(v.turn("sess-turn-wait", None, "t2").map(|t| t.state.clone()), Some(TurnState::Waiting));
}

#[test]
fn the_semantic_oracle_sees_an_owner_decision_removed_or_changed() {
    let scenario = wait_owner_resolution_kept();
    let order: Vec<usize> = (0..scenario.steps.len()).collect();
    let state = admit(&scenario, &order);
    let original = semantic_hash(&state);

    let mut dropped = state.clone();
    dropped.waits.values_mut().for_each(|w| w.owner_decisions.clear());
    assert_ne!(semantic_hash(&dropped), original, "a removed owner decision");

    let mut uncovered = state.clone();
    uncovered.waits.values_mut().flat_map(|w| &mut w.owner_decisions).for_each(|d| d.positives.clear());
    assert_ne!(semantic_hash(&uncovered), original, "a decision that no longer governs its evidence");

    let mut effect = state.clone();
    effect.attention.values_mut().for_each(|a| a.resolutions.retain(|c| c.kind != ResolutionKind::Owner));
    assert_ne!(semantic_hash(&effect), original, "an owner resolution removed from its item");
}
