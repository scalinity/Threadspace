//! The SQLite fixture and expected state were written by 70eb355, not this reducer.
use threadspace_contracts::canonical::records::CanonicalState;
use threadspace_state_engine::hash::state_hash;
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

#[test]
fn pre_m3_journal_checkpoint_and_genesis_preserve_state() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/pre-m3");
    let old_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("state.json")).unwrap()).unwrap();
    assert!(old_json.get("activationCandidates").is_none());
    let expected: CanonicalState = serde_json::from_value(old_json).unwrap();
    assert_eq!(expected.executions.len(), 1);
    assert_eq!(expected.bindings.len(), 1);
    for use_checkpoint in [true, false] {
        let store = TempStore::new("pre-m3-journal");
        std::fs::copy(fixture.join("journal.sqlite3"), store.journal_path()).unwrap();
        if !use_checkpoint {
            rusqlite::Connection::open(store.journal_path())
                .unwrap()
                .execute(
                    "DELETE FROM projection_checkpoints WHERE origin = 'PRE_M3_70eb355'",
                    [],
                )
                .unwrap();
        }
        let mut runner = SqliteRunner::open(store, 42).unwrap();
        assert_eq!(
            runner.journal.canonical_state(),
            &expected,
            "checkpoint={use_checkpoint}"
        );
        assert_eq!(runner.journal.replay_from_genesis().unwrap(), expected);
        assert!(runner.journal.projection_differences().unwrap().is_empty());
        runner.journal.checkpoint("M3_COMPAT", 2000).unwrap();
        let runner = runner.restart(43).unwrap();
        assert_eq!(
            state_hash(runner.journal.canonical_state()),
            state_hash(&expected)
        );
    }
}
