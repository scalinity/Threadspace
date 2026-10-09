//! A store last checkpointed by reducer 2 opens under reducer 3 through an
//! explicit upgrade (D-0010): its attach, link and submission reports are
//! read back from its own journal into the evidence sets, every record is
//! re-derived, and the result is checkpointed once, to the same result
//! wherever the old checkpoint sits. The fixtures
//! (`fixtures/m2/reducer-2-store`) were written by accepted M1 af9b285 from
//! the histories of `attachment-evidence`, `followup-frontier-evidence` and
//! `observer-link-evidence`; reducer 2 kept each field as the latest arrival,
//! so it left frontiers that a rejection or a disagreeing origin withdraws.

use std::path::{Path, PathBuf};

use threadspace_contracts::canonical::records::{CanonicalState, OutboxState};
use threadspace_contracts::projection::{ExecutionPresence, ObservationState};
use threadspace_state_engine::REDUCER_VERSION;
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_synthetic::runner::{Admit, PureRunner, run};
use threadspace_synthetic::scenarios::{attachment_evidence, followup_frontier_evidence, observer_link_evidence};
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};
use threadspace_synthetic::view::View;

/// The same history, upgraded from a reducer-2 checkpoint at its end (as
/// committed), at its start, after its first scenario, and three steps into
/// its last scenario: each variant but the first removes the final
/// checkpoint, so the upgrade replays every entry after the earlier one.
const VARIANTS: &[(&str, bool)] = &[
    ("journal.sqlite3", false),
    ("journal.sqlite3", true),
    ("journal-checkpoint-after-1.sqlite3", true),
    ("journal-checkpoint-step-3.sqlite3", true),
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m2/reducer-2-store").join(name)
}

/// A disposable copy; `immutable=1` keeps the committed fixture untouched.
fn copy(name: &str) -> TempStore {
    let store = TempStore::new("reducer-2-upgrade");
    rusqlite::Connection::open_with_flags(
        format!("file:{}?immutable=1", fixture(name).display()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .and_then(|source| source.execute("VACUUM INTO ?1", [store.journal_path().to_str().expect("utf-8")]))
    .expect("copy");
    store
}

/// Removes the newest reducer-2 checkpoint, leaving every other row.
fn without_final_checkpoint(store: &TempStore) -> i64 {
    let conn = rusqlite::Connection::open(store.journal_path()).expect("open");
    let removed = conn
        .execute(
            "DELETE FROM projection_checkpoints WHERE id = (SELECT MAX(id) FROM projection_checkpoints) AND reducer_version = 2",
            [],
        )
        .expect("remove checkpoint");
    assert_eq!(removed, 1, "exactly the final checkpoint is removed");
    conn.query_row("SELECT MAX(through_cursor) FROM projection_checkpoints", [], |r| r.get(0))
        .expect("remaining checkpoint")
}

/// Every JSON path at which two states differ.
fn differing_paths(a: &serde_json::Value, b: &serde_json::Value, path: String, out: &mut Vec<String>) {
    match (a, b) {
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            for key in x.keys().chain(y.keys()).collect::<std::collections::BTreeSet<_>>() {
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => differing_paths(p, q, format!("{path}/{key}"), out),
                    _ => out.push(format!("{path}/{key}")),
                }
            }
        }
        (serde_json::Value::Array(x), serde_json::Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                differing_paths(p, q, format!("{path}/{i}"), out);
            }
        }
        _ if a != b => out.push(path),
        _ => {}
    }
}

fn query<T: rusqlite::types::FromSql>(path: &Path, sql: &str) -> T {
    rusqlite::Connection::open(path)
        .and_then(|c| c.query_row(sql, [], |r| r.get(0)))
        .expect(sql)
}

/// The same histories admitted by this reducer from the start.
fn current_reducer() -> CanonicalState {
    let mut pure = PureRunner::new(31);
    for scenario in [attachment_evidence(), followup_frontier_evidence(), observer_link_evidence()] {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let report = run(&scenario, &order, &mut pure);
        assert!(report.violations.is_empty() && report.owner_failures.is_empty(), "{}", scenario.name);
    }
    pure.engine.state
}

#[test]
fn a_reducer_2_store_upgrades_to_one_result_wherever_its_checkpoint_sits() {
    let expected = semantic_hash(&current_reducer());
    let mut results = Vec::new();
    for (name, strip) in VARIANTS {
        let store = copy(name);
        let path = store.journal_path();
        let total: i64 = query(&path, "SELECT MAX(ingest_seq) FROM observations");
        let from = if *strip { without_final_checkpoint(&store) } else { total };
        let variant = format!("{name} from cursor {from} of {total}");
        let newest = "SELECT reducer_version FROM projection_checkpoints ORDER BY id DESC LIMIT 1";
        assert_eq!(query::<u32>(&path, newest), 2, "{variant}: the fixture's newest checkpoint is reducer 2's");
        assert_eq!(query::<i64>(&path, "SELECT COUNT(*) FROM projection_checkpoints WHERE origin = 'REDUCER_UPGRADE'"), 0);
        let pending = "SELECT COUNT(*) FROM notification_outbox WHERE state = 'PENDING'";
        let pending_before: i64 = query(&path, pending);

        let sqlite = SqliteRunner::open(store, 32).expect("open and upgrade");
        let state = sqlite.state().clone();
        assert_eq!(state.reducer_version, REDUCER_VERSION);
        assert_eq!(query::<u32>(&path, newest), REDUCER_VERSION, "{variant}: the upgrade is checkpointed");
        assert_eq!(semantic_hash(&state), expected, "{variant}: the upgraded state is this reducer's");

        let v = View::new(&state);
        let unordered = v.execution("sess-attachment-evidence", "act-unordered").expect("execution");
        assert!(unordered.attachment_conflict && unordered.presence == ExecutionPresence::Detached, "{variant}");
        let ordered = v.execution("sess-attachment-evidence", "act-ordered").expect("execution");
        assert_eq!((ordered.presence.clone(), ordered.controlling_device), (ExecutionPresence::Live, Some(9)), "{variant}");
        let link = v.session("sess-link-unordered").expect("session");
        assert!(link.link_conflict && link.observation == ObservationState::Disconnected, "{variant}");
        // Reducer 2 left three frontiers; the rejection and the disagreeing
        // origin withdraw two, so only the control's stands.
        assert_eq!(state.frontiers.len(), 1, "{variant}");
        let rejected = v.session("sess-frontier-rejected").expect("session").id.clone();
        let item = state
            .attention
            .values()
            .find(|a| a.session_id == rejected)
            .expect("rejected session item");
        assert!(!item.resolved(), "{variant}: a rejected follow-up no longer resolves the output");
        let intent = state.outbox.values().find(|o| o.attention_id == item.id).expect("intent");
        assert_eq!(intent.state, OutboxState::Held, "{variant}: the re-armed intent is held, never live");
        assert_eq!(query::<i64>(&path, pending), pending_before, "{variant}: the upgrade creates no live work");

        assert!(sqlite.journal.projection_differences().expect("differences").is_empty(), "{variant}: tables equal the state");
        let digest = sqlite.journal.replay_digest().expect("digest");
        assert_eq!(digest.state_sha256, state_hash(&state), "{variant}: checkpoint + replay reproduce it");

        let restarted = sqlite.restart(33).expect("restart");
        assert_eq!(state_hash(restarted.state()), state_hash(&state), "{variant}: a restart reads the upgrade");
        assert_eq!(query::<i64>(&path, "SELECT COUNT(*) FROM projection_checkpoints WHERE origin = 'REDUCER_UPGRADE'"), 1);
        results.push((variant, state_hash(&state), digest.tables_sha256, serde_json::to_value(&state).expect("json")));
    }
    for (variant, state, tables, json) in &results[1..] {
        let mut paths = Vec::new();
        differing_paths(&results[0].3, json, String::new(), &mut paths);
        assert!(paths.is_empty(), "{variant}: differs from {} at {paths:?}", results[0].0);
        assert_eq!(state, &results[0].1, "{variant}: the same state as {}", results[0].0);
        assert_eq!(tables, &results[0].2, "{variant}: the same tables as {}", results[0].0);
    }
}
