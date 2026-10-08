//! The public read model (`Journal::snapshot`, the views the UI hydrates and
//! patches from) shows the canonical session state, whatever the delivery
//! order: every history is admitted through the real SQLite journal in
//! different valid orders and read back.

use serde_json::json;
use threadspace_contracts::projection::{ExecutionPresence, ObservationState, SessionView, TurnState};
use threadspace_journal::EnvelopeAdmission;
use threadspace_state_engine::synthetic::normalize_envelope;
use threadspace_synthetic::builder::{Builder, Scenario, Step, process, session};
use threadspace_synthetic::runner::run;
use threadspace_synthetic::scenarios::{CLAUDE_LIKE, latest_turn_outcome};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

/// The canonical session and its public view after admitting `order`.
fn admit(scenario: &Scenario, order: &[usize], native: &str) -> (TurnState, ExecutionPresence, ObservationState, SessionView) {
    let mut sqlite = SqliteRunner::open(TempStore::new("public-read"), 3).expect("open");
    let report = run(scenario, order, &mut sqlite);
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    let state = sqlite.journal.canonical_state();
    let canonical = state.sessions.values().find(|s| s.native_session_id == native).expect("canonical session");
    let (turn, presence, observation) = (canonical.turn_state.clone(), canonical.execution_presence.clone(), canonical.observation.clone());
    let (_, snapshot) = sqlite.journal.snapshot().expect("snapshot");
    let view = snapshot.sessions.into_iter().find(|v| v.native_session_id == native).expect("public view");
    (turn, presence, observation, view)
}

fn agree(scenario: &Scenario, orders: &[&[usize]], native: &str) -> Vec<SessionView> {
    orders
        .iter()
        .map(|order| {
            let (turn, presence, observation, view) = admit(scenario, order, native);
            assert_eq!(view.turn_state, turn, "{order:?}: public turn state is the canonical one");
            assert_eq!(view.execution_presence, presence, "{order:?}: public presence is the canonical one");
            assert_eq!(view.observation, observation, "{order:?}: public observation is the canonical one");
            view
        })
        .collect()
}

#[test]
fn the_public_turn_state_is_the_causally_latest_settled_turn_in_every_order() {
    // The reviewer's orders: t2 (interrupted, 4–5) before or after t1
    // (completed, 2–3).
    let views = agree(&latest_turn_outcome(), &[&[0, 1, 2, 3, 4], &[0, 3, 4, 1, 2]], "sess-latest");
    assert!(views.iter().all(|v| v.turn_state == TurnState::Interrupted));
}

#[test]
fn the_patch_that_settles_a_session_carries_its_canonical_turn_state() {
    // The UI applies patches after hydration: the one built from the last
    // admission must carry the same state as the snapshot.
    for order in [[0, 1, 2, 3, 4], [0, 3, 4, 1, 2]] {
        let scenario = latest_turn_outcome();
        let mut sqlite = SqliteRunner::open(TempStore::new("public-patch"), 3).expect("open");
        let (last, earlier) = order.split_last().expect("steps");
        run(&scenario, earlier, &mut sqlite);
        let Step::Observe(envelope) = &scenario.steps[*last] else { panic!("an observation") };
        let admission = EnvelopeAdmission { envelope, normalized: normalize_envelope(envelope) };
        let outcome = sqlite.journal.admit_batch(&[admission], sqlite.delivery, 1_000).expect("admit");
        let patch = sqlite.journal.patch_for(0, outcome.change.as_ref().expect("a change")).expect("patch");
        let view = patch.session_upserts.iter().find(|v| v.native_session_id == "sess-latest").expect("session upsert");
        assert_eq!(view.turn_state, TurnState::Interrupted, "{order:?}");
    }
}

#[test]
fn a_late_tool_callback_does_not_reopen_a_settled_public_turn() {
    let mut b = Builder::new("late-tool-public", "completion");
    let s = session(CLAUDE_LIKE, "sess-late-tool");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    b.obs(Some(&s), "turn.complete").turn("t1").sequence(4).payload(json!({ "reason": "answer" })).push();
    b.obs(Some(&s), "tool.call").turn("t1").occurrence("t1-tool").sequence(3).payload(json!({ "phase": "finished", "toolCategory": "bash", "result": "SUCCESS" })).push();
    let scenario = b.build(|_| Ok(()));
    let views = agree(&scenario, &[&[0, 1, 2, 3], &[0, 1, 3, 2]], "sess-late-tool");
    assert!(views.iter().all(|v| v.turn_state == TurnState::Completed));
}

#[test]
fn public_presence_is_the_sessions_not_the_newest_activations() {
    // An older activation still detached beside a newer one that ended: the
    // session is DETACHED. Activation numbers follow arrival.
    let mut b = Builder::new("presence-public", "activation-changes");
    let s = session(CLAUDE_LIKE, "sess-presence");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "execution.attach").activation("act-1").provider(process(901, 1000), "/opt/synthetic/bin/claude", Some(16_777_301))
        .payload(json!({ "mode": "terminal_embedded", "presence": "DETACHED", "device": 16_777_301 })).push();
    b.obs(Some(&s), "execution.attach").activation("act-2").provider(process(902, 1000), "/opt/synthetic/bin/claude", Some(16_777_302))
        .payload(json!({ "mode": "terminal_embedded", "presence": "LIVE", "device": 16_777_302 })).push();
    b.obs(Some(&s), "execution.end").activation("act-2").payload(json!({ "reason": "PROCESS_EXITED" })).push();
    let scenario = b.build(|_| Ok(()));
    let views = agree(&scenario, &[&[0, 1, 2, 3], &[0, 2, 3, 1]], "sess-presence");
    assert!(views.iter().all(|v| v.execution_presence == ExecutionPresence::Detached));
}

#[test]
fn public_observation_shows_a_disconnected_observer_link() {
    let mut b = Builder::new("link-public", "snapshot-live");
    let s = session(CLAUDE_LIKE, "sess-link");
    b.obs(Some(&s), "session.start").push();
    b.obs(Some(&s), "inventory.row").source("synthetic.inventory", "inv-epoch-1").sequence(1)
        .payload(json!({ "present": true, "row": null, "interval": { "startMs": 1, "endMs": 2 } })).push();
    b.obs(Some(&s), "observer.link").payload(json!({ "link": "DISCONNECTED" })).push();
    let scenario = b.build(|_| Ok(()));
    let views = agree(&scenario, &[&[0, 1, 2]], "sess-link");
    assert_eq!(views[0].observation, ObservationState::Disconnected, "not upgraded to CURRENT by inventory presence");
}
