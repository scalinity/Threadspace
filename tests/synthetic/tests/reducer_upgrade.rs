//! A store last checkpointed by reducer 1 opens under this reducer through
//! an explicit upgrade: its checkpoint is read through reducer 1's
//! representation, every record is re-derived under the current rules, and
//! the result is materialized and checkpointed at this reducer in one
//! transaction. The fixture (`fixtures/m1/reducer-1-store`) was written by
//! candidate f7e9a6c from the second review's wait owner-coverage
//! histories; reducer 1 derived each of those wait items as handled.

use std::path::{Path, PathBuf};

use threadspace_contracts::canonical::records::{AttentionScope, CanonicalState, OutboxState};
use threadspace_state_engine::REDUCER_VERSION;
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_synthetic::runner::{Admit, PureRunner, run};
use threadspace_synthetic::scenarios::{
    wait_owner_merge_keeps_coverage, wait_owner_partial_actions, wait_owner_partial_coverage,
    wait_owner_unordered_coverage,
};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m1/reducer-1-store/journal.sqlite3")
}

/// A disposable copy of the fixture; `immutable=1` keeps the committed
/// fixture's files untouched.
fn copy() -> TempStore {
    let store = TempStore::new("reducer-1-upgrade");
    rusqlite::Connection::open_with_flags(
        format!("file:{}?immutable=1", fixture().display()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .and_then(|source| source.execute("VACUUM INTO ?1", [store.journal_path().to_str().expect("utf-8")]))
    .expect("copy");
    store
}

fn newest_checkpoint(path: &Path) -> (u32, String, String) {
    rusqlite::Connection::open(path)
        .and_then(|c| {
            c.query_row(
                "SELECT reducer_version, origin, state_json FROM projection_checkpoints ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .expect("checkpoint")
}

fn outbox_rows(path: &Path, state: &str) -> i64 {
    rusqlite::Connection::open(path)
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM notification_outbox WHERE state = ?1", [state], |r| r.get(0)))
        .expect("outbox")
}

/// The same histories admitted by this reducer from the start.
fn current_reducer() -> CanonicalState {
    let mut pure = PureRunner::new(31);
    for scenario in [
        wait_owner_partial_coverage(),
        wait_owner_merge_keeps_coverage(),
        wait_owner_partial_actions(),
        wait_owner_unordered_coverage(),
    ] {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let report = run(&scenario, &order, &mut pure);
        assert!(report.violations.is_empty() && report.owner_failures.is_empty(), "{}", scenario.name);
    }
    pure.engine.state
}

#[test]
fn a_reducer_1_store_is_upgraded_not_reinterpreted() {
    let store = copy();
    let path = store.journal_path();
    let (version, origin, json) = newest_checkpoint(&path);
    assert_eq!((version, origin.as_str()), (1, "FIXTURE"), "the fixture's newest checkpoint is reducer 1's");
    assert!(
        serde_json::from_str::<CanonicalState>(&json).is_err(),
        "reducer 1's checkpoint is not this reducer's representation"
    );
    assert_eq!(outbox_rows(&path, "PENDING") + outbox_rows(&path, "HELD"), 1, "reducer 1 left one wait notifiable");

    let sqlite = SqliteRunner::open(store, 32).expect("open and upgrade");
    let state = sqlite.state().clone();
    assert_eq!(state.reducer_version, REDUCER_VERSION);
    let (version, origin, _) = newest_checkpoint(&path);
    assert_eq!((version, origin.as_str()), (REDUCER_VERSION, "REDUCER_UPGRADE"), "the upgrade is checkpointed");

    // Exactly the semantics this reducer reaches admitting the histories.
    assert_eq!(semantic_hash(&state), semantic_hash(&current_reducer()), "upgraded state is the current reducer's");
    let open = state
        .attention
        .values()
        .filter(|a| matches!(a.scope, AttentionScope::SessionWaitCategory { .. }) && !a.resolved() && !a.acknowledged())
        .count();
    assert_eq!(open, 5, "partial, merged, unordered, and both partial-action items are open");
    // Reducer 1 suppressed four of those intents; they re-arm HELD (an
    // upgrade is not a live event), and the materialized rows say so.
    let held = state.outbox.values().filter(|o| o.state == OutboxState::Held).count();
    assert_eq!(held, 4);
    assert_eq!((outbox_rows(&path, "PENDING"), outbox_rows(&path, "HELD")), (1, 4));
    assert!(sqlite.journal.projection_differences().expect("differences").is_empty(), "tables equal the state");
    let digest = sqlite.journal.replay_digest().expect("digest");
    assert_eq!(digest.state_sha256, state_hash(&state), "checkpoint + replay reproduce the upgraded state");

    // A restart reads the upgraded checkpoint; nothing upgrades twice.
    let restarted = sqlite.restart(33).expect("restart");
    assert_eq!(state_hash(restarted.state()), state_hash(&state));
    let checkpoints: i64 = rusqlite::Connection::open(&path)
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM projection_checkpoints WHERE origin = 'REDUCER_UPGRADE'", [], |r| r.get(0)))
        .expect("count");
    assert_eq!(checkpoints, 1);
}
