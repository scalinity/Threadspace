//! SPEC §9.4: a store written by a newer schema, or holding a checkpoint
//! from a newer reducer, is refused before anything is written — the main
//! file, its header and every sidecar stay byte-identical and no file
//! appears. Each case is a disposable SQLite file in a temp directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use threadspace_journal::{Journal, JournalError, SCHEMA_VERSION};
use threadspace_state_engine::REDUCER_VERSION;

struct TempStore(PathBuf);

impl TempStore {
    fn new() -> Self {
        // URI-significant characters in the path: the preflight must still
        // open this file and no other.
        let dir = std::env::temp_dir().join(format!("threadspace-future %41?#-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp store");
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join("journal.sqlite3")
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_790_000_000_000;

/// Everything on disk in a store directory: each file's length and
/// SHA-256, and the main file's 100-byte header.
#[derive(Debug, PartialEq, Eq)]
struct Disk {
    files: BTreeMap<String, (u64, String)>,
    header: Vec<u8>,
}

fn disk(dir: &Path) -> Disk {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("list store") {
        let entry = entry.expect("entry");
        let bytes = std::fs::read(entry.path()).expect("read file");
        let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        files.insert(entry.file_name().to_string_lossy().into_owned(), (bytes.len() as u64, sha));
    }
    let main = std::fs::read(dir.join("journal.sqlite3")).expect("read main");
    Disk {
        files,
        header: main[..100].to_vec(),
    }
}

fn describe(label: &str, disk: &Disk) -> String {
    let files: Vec<String> = disk
        .files
        .iter()
        .map(|(name, (len, sha))| format!("{name} {len}B {}", &sha[..16]))
        .collect();
    format!(
        "{label}: header[18..20]={:02x}{:02x} {}",
        disk.header[18],
        disk.header[19],
        files.join(", ")
    )
}

#[derive(Clone, Copy)]
enum Shape {
    /// Rollback journal (`journal_mode=DELETE`), no sidecars.
    Rollback,
    /// WAL, closed cleanly: no sidecars.
    Wal,
    /// WAL left with its `-wal` and `-shm`; the tamper lives only in the WAL.
    WalSidecars,
}

/// A current store, then `sql` applied by a raw connection that leaves it
/// in `shape`.
fn store_in(shape: Shape, sql: &str) -> TempStore {
    let store = TempStore::new();
    drop(Journal::open(&store.db(), "epoch-a", NOW).expect("open"));
    let conn = Connection::open(store.db()).expect("raw open");
    match shape {
        Shape::Rollback => {
            let mode: String = conn
                .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
                .expect("rollback mode");
            assert_eq!(mode, "delete");
        }
        Shape::Wal => {}
        Shape::WalSidecars => {
            conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                .expect("no checkpoint on close");
            conn.pragma_update(None, "wal_autocheckpoint", 0)
                .expect("no autocheckpoint");
        }
    }
    conn.execute_batch(sql).expect("tamper");
    drop(conn);
    store
}

/// Rows `probe` counts in a copy of the main file alone, without its WAL.
fn main_file_only(db: &Path, probe: &str) -> i64 {
    let copy = TempStore::new();
    std::fs::copy(db, copy.db()).expect("copy main file");
    Connection::open(copy.db())
        .and_then(|conn| conn.query_row(probe, [], |row| row.get(0)))
        .expect("probe")
}

fn assert_refused_without_writing(label: &str, shape: Shape, sql: &str, probe: &str, found: u32) {
    let store = store_in(shape, sql);
    let sidecars: Vec<&str> = match shape {
        Shape::WalSidecars => {
            assert_eq!(main_file_only(&store.db(), probe), 0, "{label}: the tamper is only in the WAL");
            vec!["journal.sqlite3", "journal.sqlite3-shm", "journal.sqlite3-wal"]
        }
        Shape::Rollback | Shape::Wal => vec!["journal.sqlite3"],
    };
    let before = disk(&store.0);
    assert_eq!(before.files.keys().map(String::as_str).collect::<Vec<_>>(), sidecars, "{label}: setup");
    let expected_header = match shape {
        Shape::Rollback => [1, 1],
        Shape::Wal | Shape::WalSidecars => [2, 2],
    };
    assert_eq!(before.header[18..20], expected_header, "{label}: file-format bytes");

    let result = Journal::open(&store.db(), "epoch-b", NOW);
    let after = disk(&store.0);
    eprintln!("{}", describe(&format!("{label} before"), &before));
    eprintln!("{}", describe(&format!("{label} after "), &after));
    match result {
        Err(JournalError::SchemaTooNew { found: refused }) if refused == found => {}
        other => panic!("{label}: expected SchemaTooNew {{ found: {found} }}, got {other:?}"),
    }
    assert_eq!(before, after, "{label}: the refusal wrote to the store");
}

const FUTURE_SCHEMA: &str =
    "INSERT INTO schema_migrations (id, name, checksum, applied_at_ms) VALUES (99, 'future', 'x', 0);";
const FUTURE_SCHEMA_PROBE: &str = "SELECT COUNT(*) FROM schema_migrations WHERE id = 99";
const FUTURE_REDUCER_PROBE: &str = "SELECT COUNT(*) FROM projection_checkpoints WHERE origin = 'FUTURE'";

fn future_reducer() -> String {
    format!(
        "INSERT INTO projection_checkpoints (reducer_version, schema_version, through_cursor, state_json, state_sha256, origin, created_at_ms)
         VALUES ({}, {SCHEMA_VERSION}, 0, '{{}}', 'x', 'FUTURE', 0);",
        REDUCER_VERSION + 1
    )
}

#[test]
fn future_schema_rollback_store_is_refused_without_writing() {
    assert_refused_without_writing("schema99/rollback", Shape::Rollback, FUTURE_SCHEMA, FUTURE_SCHEMA_PROBE, 99);
}

#[test]
fn future_schema_clean_wal_store_is_refused_without_creating_sidecars() {
    assert_refused_without_writing("schema99/wal", Shape::Wal, FUTURE_SCHEMA, FUTURE_SCHEMA_PROBE, 99);
}

#[test]
fn future_schema_only_in_the_wal_is_seen_and_refused_without_writing() {
    assert_refused_without_writing("schema99/wal+sidecars", Shape::WalSidecars, FUTURE_SCHEMA, FUTURE_SCHEMA_PROBE, 99);
}

#[test]
fn newer_reducer_checkpoint_rollback_store_is_refused_without_writing() {
    assert_refused_without_writing(
        "reducer/rollback",
        Shape::Rollback,
        &future_reducer(),
        FUTURE_REDUCER_PROBE,
        REDUCER_VERSION + 1,
    );
}

#[test]
fn newer_reducer_checkpoint_only_in_the_wal_is_refused_without_writing() {
    assert_refused_without_writing(
        "reducer/wal+sidecars",
        Shape::WalSidecars,
        &future_reducer(),
        FUTURE_REDUCER_PROBE,
        REDUCER_VERSION + 1,
    );
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Leaves a rollback-mode `db` as a crash mid-transaction would: uncommitted
/// pages in the main file and the hot journal that undoes them. Returns the
/// committed main file.
fn leave_hot_journal(db: &Path) -> Vec<u8> {
    let committed = std::fs::read(db).expect("read committed");
    let conn = Connection::open(db).expect("raw open");
    // A one-page cache spills the uncommitted pages into the main file.
    conn.execute_batch(
        "PRAGMA cache_size=1; BEGIN; CREATE TABLE spill(x);
         INSERT INTO spill SELECT randomblob(4000) FROM (WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50) SELECT i FROM n);",
    )
    .expect("uncommitted write");
    let torn = std::fs::read(db).expect("read torn main");
    let hot = std::fs::read(sidecar(db, "-journal")).expect("read hot journal");
    conn.execute_batch("ROLLBACK").expect("rollback");
    drop(conn);
    assert_ne!(torn, committed, "the uncommitted pages reached the main file");
    std::fs::write(db, torn).expect("restore torn main");
    std::fs::write(sidecar(db, "-journal"), hot).expect("restore hot journal");
    committed
}

#[test]
fn future_schema_behind_a_hot_journal_is_refused_after_only_the_rollback() {
    let store = store_in(Shape::Rollback, FUTURE_SCHEMA);
    let committed = leave_hot_journal(&store.db());
    let before = disk(&store.0);
    let result = Journal::open(&store.db(), "epoch-b", NOW);
    let after = disk(&store.0);
    eprintln!("{}", describe("schema99/hot-journal before", &before));
    eprintln!("{}", describe("schema99/hot-journal after ", &after));
    match result {
        Err(JournalError::SchemaTooNew { found: 99 }) => {}
        other => panic!("expected SchemaTooNew {{ found: 99 }}, got {other:?}"),
    }
    assert_eq!(
        after.files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["journal.sqlite3"],
        "only the hot journal is consumed"
    );
    assert!(
        std::fs::read(store.db()).expect("read main") == committed,
        "the main file is exactly its last committed bytes"
    );
}

fn applied_schema(db: &Path) -> u32 {
    Connection::open(db)
        .and_then(|conn| conn.query_row("SELECT MAX(id) FROM schema_migrations", [], |row| row.get(0)))
        .expect("schema version")
}

#[test]
fn supported_stores_still_open_after_the_preflight() {
    // The committed M0 fixture is copied by VACUUM INTO from an immutable
    // read-only connection (its committed -wal is empty), so the fixture
    // itself is never written.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m1/m0-store-v2");
    let fixture_before = disk(&fixture);
    let store = TempStore::new();
    {
        let source = Connection::open_with_flags(
            format!("file:{}?mode=ro&immutable=1", fixture.join("journal.sqlite3").display()),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .expect("open fixture read-only");
        source
            .execute("VACUUM INTO ?1", [store.db().to_string_lossy()])
            .expect("copy fixture");
    }
    assert_eq!(disk(&fixture), fixture_before, "the fixture was written");
    assert_eq!(applied_schema(&store.db()), 2);

    drop(Journal::open(&store.db(), "epoch-a", NOW).expect("schema 2 migrates"));
    assert_eq!(applied_schema(&store.db()), SCHEMA_VERSION);
    drop(Journal::open(&store.db(), "epoch-b", NOW + 1).expect("schema 3 reopens"));
    assert_eq!(applied_schema(&store.db()), SCHEMA_VERSION);

    // A crash left a current store's newest rows only in its WAL.
    let crashed = TempStore::new();
    drop(Journal::open(&crashed.db(), "epoch-a", NOW).expect("open"));
    {
        let conn = Connection::open(crashed.db()).expect("raw open");
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .expect("no checkpoint on close");
        conn.execute("INSERT INTO store_meta (key, value) VALUES ('wal-only', 'kept')", [])
            .expect("insert");
    }
    assert!(sidecar(&crashed.db(), "-wal").exists() && sidecar(&crashed.db(), "-shm").exists());
    drop(Journal::open(&crashed.db(), "epoch-b", NOW).expect("a crashed current store reopens"));
    let kept: String = Connection::open(crashed.db())
        .and_then(|conn| conn.query_row("SELECT value FROM store_meta WHERE key = 'wal-only'", [], |row| row.get(0)))
        .expect("the WAL-only row survives");
    assert_eq!(kept, "kept");

    // A crash while a restored (rollback-mode) backup converted to WAL.
    let restored = store_in(Shape::Rollback, "SELECT 1;");
    leave_hot_journal(&restored.db());
    drop(Journal::open(&restored.db(), "epoch-b", NOW).expect("a hot journal is rolled back and the store opens"));
    assert_eq!(applied_schema(&restored.db()), SCHEMA_VERSION);

    let empty = TempStore::new();
    std::fs::write(empty.db(), b"").expect("empty store file");
    drop(Journal::open(&empty.db(), "epoch-a", NOW).expect("an empty file opens as a new store"));
    assert_eq!(applied_schema(&empty.db()), SCHEMA_VERSION);
}
