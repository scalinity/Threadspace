//! Emits `fixtures/m1/reducer-1-store/journal.sqlite3`: a store written by
//! reducer 1 (candidate f7e9a6ce2ce034e04abff03bf8a358dc98a98e01), holding
//! the second review's wait owner-coverage histories and a reducer-1
//! checkpoint taken after them. It is compiled and run only inside a
//! checkout of that commit, as `tests/synthetic/tests/emit_reducer1_store.rs`:
//!
//! ```text
//! REDUCER1_OUT=<abs path>/journal.sqlite3 \
//!   cargo test -p threadspace-synthetic --test emit_reducer1_store -- --nocapture
//! ```
//!
//! With `REDUCER1_CHECKPOINT_AFTER=<n>` it also checkpoints after the first
//! `n` histories, writing the same history with one more checkpoint. With
//! `REDUCER1_HISTORIES=unordered-pair` it writes `wait-owner-unordered-pair`
//! instead: one owner decision over P3 and two positives without a causal
//! point, which reducer 1 records as a flag.
//!
//! The histories are built with exactly the builder calls of the current
//! catalog's `wait-owner-partial-coverage`, `wait-owner-merge-keeps-coverage`,
//! `wait-owner-partial-actions` and `wait-owner-unordered-coverage`, so the
//! same envelopes and owner commands reach both reducers. It prints how
//! reducer 1 derived each wait item.

use serde_json::json;
use threadspace_contracts::canonical::command::OwnerAction;
use threadspace_contracts::canonical::keys::NativeSessionRef;
use threadspace_contracts::canonical::records::{AttentionScope, OutboxState};
use threadspace_synthetic::builder::{Builder, DEFAULT_SOURCE, Scenario, Target, session};
use threadspace_synthetic::rng::VirtualClock;
use threadspace_synthetic::runner::{Admit, run};
use threadspace_synthetic::scenarios::CLAUDE_LIKE;
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

fn input_wait(b: &mut Builder, s: &NativeSessionRef, turn: Option<&str>, sequence: u64, signal: &str) -> usize {
    let obs = b.obs(Some(s), "wait").sequence(sequence).payload(json!({ "category": "INPUT", "signal": signal }));
    match turn {
        Some(turn) => obs.turn(turn).push(),
        None => obs.push(),
    }
}

fn other_epoch_wait(b: &mut Builder, s: &NativeSessionRef, turn: &str, sequence: u64, signal: &str) -> usize {
    b.obs(Some(s), "wait").turn(turn).source(DEFAULT_SOURCE, "mod-epoch-2").sequence(sequence)
        .payload(json!({ "category": "INPUT", "signal": signal })).push()
}

fn wait_target(s: &NativeSessionRef, turn: Option<&str>, witness: u64) -> Target {
    Target::Wait { session: s.clone(), turn: turn.map(str::to_owned), category: "INPUT".into(), witness }
}

fn wait_owner_partial_coverage() -> Scenario {
    let mut b = Builder::new("wait-owner-partial-coverage", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-partial");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(1).push();
    let p2 = input_wait(&mut b, &s, Some("t1"), 2, "POSITIVE");
    let resolve = b.owner("cmd-resolve-p", wait_target(&s, Some("t1"), 2), OwnerAction::Resolve { reason: "handled P".into() }, &[p2]);
    let c3 = input_wait(&mut b, &s, Some("t1"), 3, "CLEARED");
    let q1 = other_epoch_wait(&mut b, &s, "t1", 1, "POSITIVE");
    b.before(resolve, c3);
    b.before(resolve, q1);
    b.build(|_| Ok(()))
}

fn wait_owner_merge_keeps_coverage() -> Scenario {
    let mut b = Builder::new("wait-owner-merge-keeps-coverage", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-merge");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p10 = input_wait(&mut b, &s, Some("t1"), 10, "POSITIVE");
    let resolve = b.owner("cmd-resolve-p10", wait_target(&s, Some("t1"), 10), OwnerAction::Resolve { reason: "handled P10".into() }, &[p10]);
    input_wait(&mut b, &s, Some("t1"), 5, "CLEARED");
    let q2 = other_epoch_wait(&mut b, &s, "t1", 2, "POSITIVE");
    other_epoch_wait(&mut b, &s, "t1", 1, "CLEARED");
    b.before(resolve, q2);
    b.build(|_| Ok(()))
}

fn wait_owner_partial_actions() -> Scenario {
    let mut b = Builder::new("wait-owner-partial-actions", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-partial-actions");
    b.obs(Some(&s), "session.start").sequence(1).push();
    for (turn, p, c, q, action) in [
        ("t1", 2, 3, 1, OwnerAction::Acknowledge),
        ("t2", 12, 13, 11, OwnerAction::Snooze { until_ms: 4_102_444_800_000 }),
    ] {
        b.obs(Some(&s), "turn.start").turn(turn).sequence(1).push();
        let positive = input_wait(&mut b, &s, Some(turn), p, "POSITIVE");
        let owner = b.owner(&format!("cmd-{turn}"), wait_target(&s, Some(turn), p), action, &[positive]);
        let clear = input_wait(&mut b, &s, Some(turn), c, "CLEARED");
        let other = other_epoch_wait(&mut b, &s, turn, q, "POSITIVE");
        b.before(owner, clear);
        b.before(owner, other);
    }
    b.build(|_| Ok(()))
}

fn wait_owner_unordered_coverage() -> Scenario {
    let mut b = Builder::new("wait-owner-unordered-coverage", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-unordered-wait");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p3 = input_wait(&mut b, &s, Some("t1"), 3, "POSITIVE");
    let unordered = json!({ "category": "INPUT", "signal": "POSITIVE" });
    let u1 = b.obs(Some(&s), "wait").turn("t1").unordered().payload(unordered.clone()).push();
    let resolve = b.owner("cmd-resolve-unordered", wait_target(&s, Some("t1"), 3), OwnerAction::Resolve { reason: "handled".into() }, &[p3, u1]);
    let u2 = b.obs(Some(&s), "wait").turn("t1").unordered().payload(unordered).push();
    b.before(resolve, u2);
    b.build(|_| Ok(()))
}

fn wait_owner_unordered_pair() -> Scenario {
    let mut b = Builder::new("wait-owner-unordered-pair", "wait-events");
    let s = session(CLAUDE_LIKE, "sess-unordered-pair");
    b.obs(Some(&s), "session.start").sequence(1).push();
    b.obs(Some(&s), "turn.start").turn("t1").sequence(2).push();
    let p3 = input_wait(&mut b, &s, Some("t1"), 3, "POSITIVE");
    let unordered = json!({ "category": "INPUT", "signal": "POSITIVE" });
    let u1 = b.obs(Some(&s), "wait").turn("t1").unordered().payload(unordered.clone()).push();
    let u2 = b.obs(Some(&s), "wait").turn("t1").unordered().payload(unordered).push();
    b.owner("cmd-resolve-pair", wait_target(&s, Some("t1"), 3), OwnerAction::Resolve { reason: "handled all three".into() }, &[p3, u1, u2]);
    b.build(|_| Ok(()))
}

#[test]
fn emit() {
    let out = std::env::var("REDUCER1_OUT").expect("REDUCER1_OUT");
    let after: Option<usize> = std::env::var("REDUCER1_CHECKPOINT_AFTER").ok().map(|n| n.parse().expect("count"));
    let mut sqlite = SqliteRunner::open(TempStore::new("reducer1-emit"), 31).expect("open");
    let histories = match std::env::var("REDUCER1_HISTORIES").as_deref() {
        Ok("unordered-pair") => vec![wait_owner_unordered_pair()],
        _ => vec![
            wait_owner_partial_coverage(),
            wait_owner_merge_keeps_coverage(),
            wait_owner_partial_actions(),
            wait_owner_unordered_coverage(),
        ],
    };
    for (done, scenario) in histories.into_iter().enumerate() {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let report = run(&scenario, &order, &mut sqlite);
        assert!(report.owner_failures.is_empty(), "{}: {:?}", scenario.name, report.owner_failures);
        if after == Some(done + 1) {
            sqlite.journal.checkpoint("FIXTURE", VirtualClock::EPOCH_MS + 500_000).expect("checkpoint");
        }
    }
    sqlite.journal.checkpoint("FIXTURE", VirtualClock::EPOCH_MS + 1_000_000).expect("checkpoint");
    let state = sqlite.state().clone();
    let mut items = Vec::new();
    for item in state.attention.values().filter(|a| matches!(a.scope, AttentionScope::SessionWaitCategory { .. })) {
        let session = &state.sessions[&item.session_id].native_session_id;
        let eligible = state
            .outbox
            .values()
            .any(|o| o.attention_id == item.id && matches!(o.state, OutboxState::Pending | OutboxState::Held));
        items.push(json!({
            "session": session,
            "resolutions": item.resolutions,
            "acknowledged": item.acknowledged(),
            "snoozedUntilMs": item.snoozed_until_ms,
            "eligible": eligible,
        }));
    }
    items.sort_by_key(|i| i.to_string());
    println!("{}", serde_json::to_string_pretty(&json!({ "reducerVersion": state.reducer_version, "waitItems": items })).expect("json"));
    rusqlite::Connection::open(sqlite.store.journal_path())
        .and_then(|c| c.execute("VACUUM INTO ?1", [out.as_str()]))
        .expect("export");
}
