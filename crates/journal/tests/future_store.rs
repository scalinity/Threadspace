//! SPEC §9.4: a store written by a newer schema, or holding a checkpoint
//! from a newer reducer, is refused before anything is written — the main
//! file, its header and every sidecar stay byte-identical and no file
//! appears. Each case is a disposable SQLite file in a temp directory.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    /// WAL left with its `-wal` and no `-shm` (removed after the writer
    /// closed); the tamper lives only in the WAL.
    WalNoShm,
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
        Shape::WalSidecars | Shape::WalNoShm => {
            conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                .expect("no checkpoint on close");
            conn.pragma_update(None, "wal_autocheckpoint", 0)
                .expect("no autocheckpoint");
        }
    }
    conn.execute_batch(sql).expect("tamper");
    drop(conn);
    if let Shape::WalNoShm = shape {
        std::fs::remove_file(sidecar(&store.db(), "-shm")).expect("remove -shm");
        assert!(!sidecar(&store.db(), "-shm").exists(), "the -shm is gone");
        assert!(sidecar(&store.db(), "-wal").exists(), "the -wal stays");
    }
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
        Shape::WalNoShm => {
            assert_eq!(main_file_only(&store.db(), probe), 0, "{label}: the tamper is only in the WAL");
            vec!["journal.sqlite3", "journal.sqlite3-wal"]
        }
        Shape::Rollback | Shape::Wal => vec!["journal.sqlite3"],
    };
    let before = disk(&store.0);
    assert_eq!(before.files.keys().map(String::as_str).collect::<Vec<_>>(), sidecars, "{label}: setup");
    let expected_header = match shape {
        Shape::Rollback => [1, 1],
        Shape::Wal | Shape::WalSidecars | Shape::WalNoShm => [2, 2],
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

#[test]
fn future_schema_only_in_a_wal_without_shm_is_seen_and_refused_without_writing() {
    assert_refused_without_writing("schema99/wal-no-shm", Shape::WalNoShm, FUTURE_SCHEMA, FUTURE_SCHEMA_PROBE, 99);
}

#[test]
fn newer_reducer_checkpoint_only_in_a_wal_without_shm_is_refused_without_writing() {
    assert_refused_without_writing(
        "reducer/wal-no-shm",
        Shape::WalNoShm,
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

#[test]
fn a_supported_store_with_rows_only_in_a_wal_without_shm_opens_and_keeps_them() {
    let probe = "SELECT COUNT(*) FROM store_meta WHERE key = 'wal-only'";
    let store = store_in(Shape::WalNoShm, "INSERT INTO store_meta (key, value) VALUES ('wal-only', 'kept');");
    assert_eq!(main_file_only(&store.db(), probe), 0, "the row is only in the WAL");
    drop(Journal::open(&store.db(), "epoch-b", NOW).expect("a current store with a WAL and no -shm opens"));
    let kept: String = Connection::open(store.db())
        .and_then(|conn| conn.query_row("SELECT value FROM store_meta WHERE key = 'wal-only'", [], |row| row.get(0)))
        .expect("the WAL-only row survives");
    assert_eq!(kept, "kept");
    assert_eq!(applied_schema(&store.db()), SCHEMA_VERSION);
}

/// The URI a read-only connection opens `db` by, with URI-significant
/// characters escaped.
fn file_uri(db: &Path) -> String {
    let mut uri = String::from("file:");
    for ch in db.to_string_lossy().chars() {
        match ch {
            '%' | '?' | '#' => uri.push_str(&format!("%{:02X}", u32::from(ch))),
            _ => uri.push(ch),
        }
    }
    uri
}

/// Negative control: a plain read-only connection must build a wal-index to
/// read a `-wal`, so with no `-shm` beside it SQLite creates one in the store
/// directory. This is the open a preflight must not make.
#[test]
fn a_plain_read_only_open_creates_a_shm_beside_a_wal_without_one() {
    let store = store_in(Shape::WalNoShm, FUTURE_SCHEMA);
    let before = disk(&store.0);
    {
        let conn = Connection::open_with_flags(
            file_uri(&store.db()),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .expect("read-only open");
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .expect("no checkpoint on close");
        let schema: u32 = conn
            .query_row("SELECT MAX(id) FROM schema_migrations", [], |row| row.get(0))
            .expect("read schema");
        assert_eq!(schema, 99, "the read-only connection sees the WAL-only commit");
    }
    let after = disk(&store.0);
    eprintln!("{}", describe("control/plain-read-only before", &before));
    eprintln!("{}", describe("control/plain-read-only after ", &after));
    assert_eq!(
        after.files.get("journal.sqlite3-shm").map(|(len, _)| *len),
        Some(32_768),
        "a new 32,768-byte -shm appears"
    );
    assert_eq!(before.files.get("journal.sqlite3"), after.files.get("journal.sqlite3"));
    assert_eq!(before.files.get("journal.sqlite3-wal"), after.files.get("journal.sqlite3-wal"));
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("set mode");
}

/// Makes a file unreadable until dropped.
struct Unreadable(PathBuf, u32);

impl Unreadable {
    fn new(path: PathBuf) -> Self {
        let mode = std::fs::metadata(&path).expect("mode").permissions().mode();
        set_mode(&path, 0o000);
        Self(path, mode)
    }
}

impl Drop for Unreadable {
    fn drop(&mut self) {
        set_mode(&self.0, self.1);
    }
}

/// A WAL-only future store whose `suffix` file cannot be read: the open
/// fails, and once the mode is restored every file is as it was.
fn assert_unreadable_fails_without_writing(label: &str, suffix: &str) {
    let store = store_in(Shape::WalNoShm, FUTURE_SCHEMA);
    let before = disk(&store.0);
    let unreadable = Unreadable::new(sidecar(&store.db(), suffix));
    let result = Journal::open(&store.db(), "epoch-b", NOW);
    drop(unreadable);
    let after = disk(&store.0);
    eprintln!("{label}: {result:?}");
    assert!(result.is_err(), "{label}: an unreadable store must not open");
    assert_eq!(before, after, "{label}: the failed open wrote to the store");
}

#[test]
fn an_unreadable_wal_fails_without_writing() {
    assert_unreadable_fails_without_writing("unreadable -wal", "-wal");
}

#[test]
fn an_unreadable_main_file_fails_without_writing() {
    assert_unreadable_fails_without_writing("unreadable main", "");
}

fn garbage(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i.wrapping_mul(31) + 7) as u8).collect()
}

/// Opens `store` and returns the result, failing if any file changed.
fn open_without_writing(label: &str, store: &TempStore) -> Result<Journal, JournalError> {
    let before = disk(&store.0);
    let result = Journal::open(&store.db(), "epoch-b", NOW);
    let after = disk(&store.0);
    eprintln!("{}", describe(&format!("{label} before"), &before));
    eprintln!("{}", describe(&format!("{label} after "), &after));
    eprintln!("{label}: {result:?}");
    assert_eq!(before, after, "{label}: the open wrote to the store");
    result
}

/// A `-wal` of garbage and no `-shm` beside a store whose main file holds
/// schema 99.
#[test]
fn a_garbage_wal_beside_a_future_store_is_refused_without_writing() {
    let store = store_in(Shape::Wal, FUTURE_SCHEMA);
    std::fs::write(sidecar(&store.db(), "-wal"), garbage(8192)).expect("garbage -wal");
    let result = open_without_writing("garbage -wal", &store);
    assert!(result.is_err(), "garbage -wal: a future store must not open");
}

/// A `-shm` of garbage beside a WAL holding schema 99.
#[test]
fn a_garbage_shm_beside_a_future_wal_is_refused_without_writing() {
    let store = store_in(Shape::WalSidecars, FUTURE_SCHEMA);
    let shm = sidecar(&store.db(), "-shm");
    let len = std::fs::metadata(&shm).expect("shm").len();
    std::fs::write(&shm, garbage(len as usize)).expect("garbage -shm");
    let result = open_without_writing("garbage -shm", &store);
    assert!(result.is_err(), "garbage -shm: a future store must not open");
}

const CHILD_STORE_ENV: &str = "THREADSPACE_FUTURE_STORE_CHILD_DB";

/// Runs only as a re-executed child of the tests below, whose TMPDIR is a
/// directory of their own: opens the store and prints the result.
#[test]
fn preflight_child() {
    let Ok(db) = std::env::var(CHILD_STORE_ENV) else {
        return;
    };
    let result = Journal::open(Path::new(&db), "epoch-child", NOW).map(drop);
    println!("RESULT {result:?}");
}

/// Opens `store` in a child test process whose TMPDIR alone is `tmp`, so no
/// other test's temp directory moves, and returns the child's result.
fn open_in_child(store: &TempStore, tmp: &Path) -> String {
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", "preflight_child", "--nocapture"])
        .env(CHILD_STORE_ENV, store.db())
        .env("TMPDIR", tmp)
        .output()
        .expect("run child");
    assert!(output.status.success(), "child: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("RESULT "))
        .expect("child result")
        .to_owned()
}

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).expect("list").count()
}

#[test]
fn the_inspection_copy_is_removed_after_a_refusal() {
    let store = store_in(Shape::WalNoShm, FUTURE_SCHEMA);
    let tmp = TempStore::new();
    let before = disk(&store.0);
    let result = open_in_child(&store, &tmp.0);
    eprintln!("inspection copy removed: {result}");
    assert_eq!(result, "Err(SchemaTooNew { found: 99 })");
    assert_eq!(disk(&store.0), before, "the refusal wrote to the store");
    assert_eq!(entries(&tmp.0), 0, "the inspection copy is left behind");
}

#[test]
fn an_inspection_copy_that_cannot_be_made_fails_without_writing() {
    let store = store_in(Shape::WalNoShm, FUTURE_SCHEMA);
    let tmp = TempStore::new();
    set_mode(&tmp.0, 0o500);
    let before = disk(&store.0);
    let result = open_in_child(&store, &tmp.0);
    set_mode(&tmp.0, 0o700);
    eprintln!("inspection copy not writable: {result}");
    assert!(result.starts_with("Err("), "a store the preflight cannot inspect must not open: {result}");
    assert_eq!(disk(&store.0), before, "the failed open wrote to the store");
    assert_eq!(entries(&tmp.0), 0);
}
