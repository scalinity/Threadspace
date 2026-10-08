//! Schema migration evidence: the M0-era store written by the accepted M0C
//! journal (`fixtures/m1/m0-store-v2/`) upgrades deterministically to the
//! canonical schema, preserves what it held, keeps the M0B route semantics,
//! and a newer schema or reducer checkpoint is refused without writing: in
//! rollback-journal, clean WAL, WAL-with-sidecars and WAL-without-`-shm`
//! stores, every file's bytes, the main header and the sidecar set are
//! compared across the refusal.

use std::path::{Path, PathBuf};

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OptionalExtension};
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
    /// WAL left with its `-wal` and no `-shm` (removed after the writer
    /// closed); the change lives only in the WAL.
    WalNoShm,
}

const STORE_FILES: [(&str, &str); 4] = [
    ("main", "journal.sqlite3"),
    ("wal", "journal.sqlite3-wal"),
    ("shm", "journal.sqlite3-shm"),
    ("journal", "journal.sqlite3-journal"),
];

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Every file in the store directory with its length and SHA-256, and
/// whether each of the store's own four files is present.
fn listing(disk: &Value) -> Value {
    let present: serde_json::Map<String, Value> = STORE_FILES
        .iter()
        .map(|(key, name)| ((*key).to_owned(), json!(disk["files"][*name].is_object())))
        .collect();
    json!({ "files": disk["files"], "present": present })
}

/// The main file's header fields SQLite versions the file by: the magic
/// string, the file-format write and read versions (bytes 18–19), the change
/// counter and the version-valid-for number that pairs with it (bytes 24–27,
/// 92–95), and the SQLITE_VERSION_NUMBER that last wrote it (bytes 96–99).
fn header_fields(path: &Path) -> Result<Value, String> {
    let main = std::fs::read(path).map_err(|e| e.to_string())?;
    let header = main.get(..100).ok_or("main file is shorter than its header")?;
    let be32 = |at: usize| u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]);
    Ok(json!({
        "magic": String::from_utf8_lossy(&header[..15]),
        "fileFormatWriteVersion": header[18],
        "fileFormatReadVersion": header[19],
        "fileChangeCounter": be32(24),
        "versionValidFor": be32(92),
        "sqliteVersionNumber": be32(96),
    }))
}

/// The applied schema and the newest checkpoint's reducer version in a copy
/// of the main file, with its `-wal` when `with_wal`, read in a scratch
/// directory so that the store itself is never opened.
fn versions_in_copy(path: &Path, with_wal: bool) -> Result<Value, String> {
    let dir = std::env::temp_dir().join(format!("threadspace-m1-migration-read-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let copy = dir.join("journal.sqlite3");
    let read = || -> Result<Value, String> {
        std::fs::copy(path, &copy).map_err(|e| e.to_string())?;
        if with_wal && sidecar(path, "-wal").exists() {
            std::fs::copy(sidecar(path, "-wal"), sidecar(&copy, "-wal")).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(&copy).map_err(|e| e.to_string())?;
        let schema: u32 = conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM schema_migrations", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let reducer: Option<u32> = conn
            .query_row("SELECT reducer_version FROM projection_checkpoints ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(json!({ "schemaVersion": schema, "reducerVersion": reducer }))
    };
    let versions = read();
    let _ = std::fs::remove_dir_all(&dir);
    versions
}

/// How the journal's `preflight` opens a store with these files: its open
/// flags, URI query and the file it opens. Every branch also sets
/// SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE.
fn preflight_open(present: &Value) -> Value {
    let read_only = "SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI | SQLITE_OPEN_NO_MUTEX";
    let (flags, query, opens) = if present["wal"] == true {
        if present["shm"] == true {
            (read_only, "?readonly_shm=1", "the store's main file")
        } else {
            (read_only, "", "a copy of the main file and -wal in a private 0700 temporary directory")
        }
    } else if present["journal"] == true {
        ("SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_URI | SQLITE_OPEN_NO_MUTEX", "", "the store's main file")
    } else {
        (read_only, "?immutable=1", "the store's main file")
    };
    json!({ "flags": flags, "uriQuery": query, "opens": opens, "dbConfig": "SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE" })
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
/// was and no file added. Each file's length and SHA-256 are recorded before
/// and after; they vary per run (a fresh store identity and WAL salts), and
/// the verdict is their equality.
fn refusal(repo: &Path, shape: Shape, sql: &str, found: u32) -> Result<Value, String> {
    let label = match shape {
        Shape::Rollback => "rollback",
        Shape::Wal => "wal",
        Shape::WalSidecars => "walSidecars",
        Shape::WalNoShm => "walNoShm",
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
            Shape::WalSidecars | Shape::WalNoShm => {
                conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                    .map_err(|e| e.to_string())?;
                conn.pragma_update(None, "wal_autocheckpoint", 0).map_err(|e| e.to_string())?;
            }
        }
        conn.execute_batch(sql).map_err(|e| e.to_string())?;
    }
    if let Shape::WalNoShm = shape {
        std::fs::remove_file(sidecar(&path, "-shm")).map_err(|e| e.to_string())?;
        if sidecar(&path, "-shm").exists() || !sidecar(&path, "-wal").exists() {
            return Err(format!("{label}: expected a -wal and no -shm"));
        }
    }
    let committed = versions_in_copy(&path, true)?;
    let main_file_only = versions_in_copy(&path, false)?;
    let before = store_disk(&path)?;
    let header_before = header_fields(&path)?;
    let result = Journal::open(&path, "migration", NOW);
    let refused = matches!(&result, Err(JournalError::SchemaTooNew { found: refused }) if *refused == found);
    let error = result.as_ref().err().map(ToString::to_string);
    drop(result);
    let after = store_disk(&path)?;
    let header_after = header_fields(&path)?;
    let header = before["header"].as_str().unwrap_or_default();
    let before_listing = listing(&before);
    Ok(json!({
        "shape": label,
        "files": before["files"].as_object().map(|files| files.keys().cloned().collect::<Vec<_>>()),
        "fileFormatBytes18To19": header.get(36..40),
        "refused": refused,
        "unchanged": before == after,
        "error": error,
        "sqliteVersion": rusqlite::version(),
        "preflightOpen": preflight_open(&before_listing["present"]),
        "committedVersions": committed,
        "mainFileOnlyVersions": main_file_only,
        "header": { "before": header_before, "after": header_after },
        "before": before_listing,
        "after": listing(&after),
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
    for shape in [Shape::Rollback, Shape::Wal, Shape::WalSidecars, Shape::WalNoShm] {
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
