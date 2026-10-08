//! Schema migration evidence: the M0-era store written by the accepted M0C
//! journal (`fixtures/m1/m0-store-v2/`) upgrades deterministically to the
//! canonical schema, preserves what it held, keeps the M0B route semantics,
//! and a newer schema or reducer checkpoint is refused without writing.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde_json::{Value, json};
use threadspace_contracts::canonical::records::ResolutionKind;
use threadspace_journal::{Journal, JournalError, RouteTargetRow, SCHEMA_VERSION, migration_catalog};
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::ids::{RandomAllocator, SeededAllocator};
use threadspace_state_engine::REDUCER_VERSION;

use crate::evidence::{Area, sha256_file};

const NOW: i64 = 1_791_100_000_000;

fn copy_fixture(repo: &Path, label: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("threadspace-m1-migration-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("journal.sqlite3");
    std::fs::copy(repo.join("fixtures/m1/m0-store-v2/journal.sqlite3"), &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn old_ids(path: &Path, table: &str) -> Result<Vec<String>, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let mut statement = conn.prepare(&format!("SELECT id FROM {table} ORDER BY id")).map_err(|e| e.to_string())?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

pub fn run(repo: &Path, root: &Path) -> Result<Value, String> {
    let area = Area::new(root, "migration")?;
    let fixture = repo.join("fixtures/m1/m0-store-v2/journal.sqlite3");
    let fixture_sha = sha256_file(&fixture);

    // Three independent upgrades, two allocators: identical state.
    let mut hashes = Vec::new();
    let mut digests = Vec::new();
    for (label, seeded) in [("a", false), ("b", false), ("c", true)] {
        let path = copy_fixture(repo, label)?;
        let m0_sessions = old_ids(&path, "sessions")?;
        let allocator: Box<dyn threadspace_state_engine::ids::Allocator + Send> = if seeded {
            Box::new(SeededAllocator::new(99))
        } else {
            Box::new(RandomAllocator)
        };
        let journal = Journal::open_with(&path, "migration", NOW, allocator, true).map_err(|e| e.to_string())?;
        let state = journal.canonical_state();
        let mut new_sessions: Vec<String> = state.sessions.keys().cloned().collect();
        new_sessions.sort();
        let digest = journal.replay_digest().map_err(|e| e.to_string())?;
        hashes.push(state_hash(state));
        digests.push(json!({
            "upgrade": label,
            "allocator": if seeded { "seeded" } else { "random" },
            "stateSha256": state_hash(state),
            "projectionMatchesTables": digest.projection_sha256 == digest.tables_sha256,
            "sessionIdsPreserved": new_sessions == m0_sessions,
        }));
    }
    let deterministic = hashes.windows(2).all(|w| w[0] == w[1]);

    // What the upgraded store holds, and that it keeps working.
    let path = copy_fixture(repo, "inspect")?;
    let mut journal = Journal::open(&path, "migration", NOW).map_err(|e| e.to_string())?;
    let state = journal.canonical_state().clone();
    let checkpoint_origin: String = Connection::open(&path)
        .and_then(|c| c.query_row("SELECT origin FROM projection_checkpoints ORDER BY id LIMIT 1", [], |r| r.get(0)))
        .map_err(|e| e.to_string())?;
    let session = |native: &str| state.sessions.values().find(|s| s.native_session_id == native).map(|s| s.id.clone());
    let a = session("A").ok_or("A")?;
    let b = session("B").ok_or("B")?;
    let route_a = journal.route_target(&a).map_err(|e| e.to_string())?;
    let route_b = journal.route_target(&b).map_err(|e| e.to_string())?;
    let a_bound = matches!(&route_a, RouteTargetRow::Bound { bindings, .. } if bindings.len() == 1 && bindings[0].start_seconds == 2000);
    let b_unbound = matches!(&route_b, RouteTargetRow::Unbound { reason, .. } if reason == "NO_MATCHING_TAB");
    let acknowledged = state.attention.values().filter(|a| a.acknowledged()).count();
    let resolved = state
        .attention
        .values()
        .filter(|a| a.resolutions.iter().any(|c| c.kind == ResolutionKind::Owner))
        .count();
    let b_stale = state.sessions.get(&b).is_some_and(|s| {
        serde_json::to_value(&s.observation).ok() == Some(json!("STALE"))
    });
    let outstanding = state.attention.values().find(|a| !a.resolved()).map(|a| a.id.clone());
    if let Some(item) = outstanding {
        journal.resolve_attention("cmd-after-upgrade", &item, None, "handled after upgrade", NOW + 1).map_err(|e| e.to_string())?;
    }
    let before_restart = state_hash(journal.canonical_state());
    drop(journal);
    let reopened = Journal::open(&path, "migration-2", NOW + 2).map_err(|e| e.to_string())?;
    let replay_ok = state_hash(reopened.canonical_state()) == before_restart;
    drop(reopened);

    // A store written by a newer schema, or a checkpoint from a newer
    // reducer, is refused before any write.
    let future = copy_fixture(repo, "future")?;
    drop(Journal::open(&future, "migration", NOW).map_err(|e| e.to_string())?);
    Connection::open(&future)
        .and_then(|c| c.execute("INSERT INTO schema_migrations (id, name, checksum, applied_at_ms) VALUES (99, 'future', 'x', 0)", []))
        .map_err(|e| e.to_string())?;
    let before = sha256_file(&future);
    let refused_schema = matches!(Journal::open(&future, "migration", NOW), Err(JournalError::SchemaTooNew { found: 99 }));
    let unchanged = sha256_file(&future) == before;

    let newer = copy_fixture(repo, "reducer")?;
    drop(Journal::open(&newer, "migration", NOW).map_err(|e| e.to_string())?);
    Connection::open(&newer)
        .and_then(|c| {
            c.execute(
                "INSERT INTO projection_checkpoints (reducer_version, schema_version, through_cursor, state_json, state_sha256, origin, created_at_ms)
                 VALUES (?1, ?2, 0, '{}', 'x', 'FUTURE', 0)",
                params![REDUCER_VERSION + 1, SCHEMA_VERSION],
            )
        })
        .map_err(|e| e.to_string())?;
    let refused_reducer = matches!(Journal::open(&newer, "migration", NOW), Err(JournalError::SchemaTooNew { .. }));

    let pass = deterministic
        && digests.iter().all(|d| d["projectionMatchesTables"] == true && d["sessionIdsPreserved"] == true)
        && checkpoint_origin == "M0_BASELINE"
        && a_bound
        && b_unbound
        && b_stale
        && acknowledged >= 1
        && resolved >= 1
        && replay_ok
        && refused_schema
        && unchanged
        && refused_reducer;
    let summary = json!({
        "area": "migration",
        "pass": pass,
        "fixture": {
            "path": "fixtures/m1/m0-store-v2/journal.sqlite3",
            "sha256": fixture_sha,
            "writtenBy": "accepted M0C journal at cd9e37645adf7e6b5f74ab7f0baa5197d8e08b54 (fixtures/m1/m0-store-v2/generator.rs)",
            "schemaVersion": 2,
        },
        "targetSchemaVersion": SCHEMA_VERSION,
        "reducerVersion": REDUCER_VERSION,
        "migrations": migration_catalog().into_iter().map(|(id, name, sum)| json!({ "id": id, "name": name, "sha256": sum })).collect::<Vec<_>>(),
        "deterministicUpgrade": { "pass": deterministic, "upgrades": digests },
        "baselineCheckpointOrigin": checkpoint_origin,
        "preserved": {
            "m0bRouteBoundAfterPidReuse": a_bound,
            "m0bUnprovenSurfaceKept": b_unbound,
            "inventoryAbsenceStale": b_stale,
            "acknowledgedItems": acknowledged,
            "ownerResolvedItems": resolved,
        },
        "operatesAfterUpgrade": { "ownerCommandThenRestartSameState": replay_ok },
        "futureSchemaRefused": { "pass": refused_schema, "storeUnchanged": unchanged },
        "newerReducerCheckpointRefused": refused_reducer,
    });
    area.json("summary.json", &summary)?;
    Ok(summary)
}
