//! Schema migration evidence: the M0-era store written by the accepted M0C
//! journal (`fixtures/m1/m0-store-v2/`) upgrades deterministically to the
//! canonical schema, preserves what it held, keeps the M0B route semantics,
//! and a newer schema or reducer checkpoint is refused without writing: in
//! rollback-journal, clean WAL and WAL-with-sidecars stores, every file's
//! bytes, the main header and the sidecar set are compared across the
//! refusal.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rusqlite::config::DbConfig;
use serde_json::{Value, json};
use threadspace_contracts::canonical::records::ResolutionKind;
use threadspace_journal::{Journal, JournalError, RouteTargetRow, SCHEMA_VERSION, migration_catalog};
use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::ids::{RandomAllocator, SeededAllocator};
use threadspace_state_engine::REDUCER_VERSION;

use crate::evidence::{Area, sha256, sha256_file};

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

#[derive(Clone, Copy)]
enum Shape {
    /// Rollback journal (`journal_mode=DELETE`), no sidecars.
    Rollback,
    /// WAL, closed cleanly: no sidecars.
    Wal,
    /// WAL left with its `-wal` and `-shm`; the change lives only in the WAL.
    WalSidecars,
}

/// Everything on disk beside the store: each file's length and SHA-256, and
/// the main file's 100-byte header (file-format bytes 18–19 included).
fn store_disk(path: &Path) -> Result<Value, String> {
    let dir = path.parent().ok_or("store has no directory")?;
    let mut files = serde_json::Map::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let bytes = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
        files.insert(
            entry.file_name().to_string_lossy().into_owned(),
            json!({ "bytes": bytes.len(), "sha256": sha256(&bytes) }),
        );
    }
    let main = std::fs::read(path).map_err(|e| e.to_string())?;
    let header: String = main.iter().take(100).map(|b| format!("{b:02x}")).collect();
    Ok(json!({ "files": files, "header": header }))
}

/// The upgraded fixture with `sql` applied and left in `shape`, then opened:
/// it must be refused as `found` with every file, header and sidecar as it
/// was and no file added. Hashes vary per run, so only the verdicts and the
/// shape are reported.
fn refusal(repo: &Path, shape: Shape, sql: &str, found: u32) -> Result<Value, String> {
    let label = match shape {
        Shape::Rollback => "rollback",
        Shape::Wal => "wal",
        Shape::WalSidecars => "walSidecars",
    };
    let path = copy_fixture(repo, &format!("refuse-{label}"))?;
    drop(Journal::open(&path, "migration", NOW).map_err(|e| e.to_string())?);
    {
        let conn = Connection::open(&path).map_err(|e| e.to_string())?;
        match shape {
            Shape::Rollback => {
                let mode: String = conn
                    .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                if mode != "delete" {
                    return Err(format!("store stayed in journal_mode {mode}"));
                }
            }
            Shape::Wal => {}
            Shape::WalSidecars => {
                conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                    .map_err(|e| e.to_string())?;
                conn.pragma_update(None, "wal_autocheckpoint", 0).map_err(|e| e.to_string())?;
            }
        }
        conn.execute_batch(sql).map_err(|e| e.to_string())?;
    }
    let before = store_disk(&path)?;
    let refused = matches!(
        Journal::open(&path, "migration", NOW),
        Err(JournalError::SchemaTooNew { found: refused }) if refused == found
    );
    let after = store_disk(&path)?;
    let header = before["header"].as_str().unwrap_or_default();
    Ok(json!({
        "shape": label,
        "files": before["files"].as_object().map(|files| files.keys().cloned().collect::<Vec<_>>()),
        "fileFormatBytes18To19": header.get(36..40),
        "refused": refused,
        "unchanged": before == after,
    }))
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
    // reducer, is refused before any write, in every shape a store is left.
    let future_schema = "INSERT INTO schema_migrations (id, name, checksum, applied_at_ms) VALUES (99, 'future', 'x', 0);";
    let future_reducer = format!(
        "INSERT INTO projection_checkpoints (reducer_version, schema_version, through_cursor, state_json, state_sha256, origin, created_at_ms)
         VALUES ({}, {SCHEMA_VERSION}, 0, '{{}}', 'x', 'FUTURE', 0);",
        REDUCER_VERSION + 1
    );
    let mut schema_stores = Vec::new();
    let mut reducer_stores = Vec::new();
    for shape in [Shape::Rollback, Shape::Wal, Shape::WalSidecars] {
        schema_stores.push(refusal(repo, shape, future_schema, 99)?);
        reducer_stores.push(refusal(repo, shape, &future_reducer, REDUCER_VERSION + 1)?);
    }
    let all = |stores: &[Value], key: &str| stores.iter().all(|s| s[key] == true);
    let refused_schema = all(&schema_stores, "refused");
    let unchanged = all(&schema_stores, "unchanged");
    let refused_reducer = all(&reducer_stores, "refused");
    let reducer_unchanged = all(&reducer_stores, "unchanged");

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
        && refused_reducer
        && reducer_unchanged;
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
        "futureSchemaRefused": { "pass": refused_schema, "storeUnchanged": unchanged, "stores": schema_stores },
        "newerReducerCheckpointRefused": { "pass": refused_reducer, "storeUnchanged": reducer_unchanged, "stores": reducer_stores },
    });
    area.json("summary.json", &summary)?;
    Ok(summary)
}
