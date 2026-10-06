//! Consistent backup, verification and restore of the journal store
//! (SPEC §9.4), and an explicit TRUNCATE checkpoint (§19.5).
//!
//! A backup is taken with SQLite's online backup API inside one read
//! transaction, never by copying the live main file while WAL is active. The
//! copy is converted to a self-contained rollback-journal file and carries a
//! backup record in its own `store_meta`: the cursor, schema version and the
//! engine that wrote it. It is verified before it is published by an atomic
//! rename, and again before a restore installs it.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::schema::{self, SCHEMA_VERSION};
use crate::{
    Journal, JournalError, REQUIRED_SQLITE_SOURCE_ID, REQUIRED_SQLITE_VERSION, WriterLock,
    verify_engine,
};

const KEY_GENERATION: &str = "store_generation";
const KEY_CURSOR: &str = "backup_cursor";
const KEY_SCHEMA: &str = "backup_schema_version";
const KEY_SQLITE_VERSION: &str = "backup_sqlite_version";
const KEY_SQLITE_SOURCE_ID: &str = "backup_sqlite_source_id";
const KEY_CREATED: &str = "backup_created_at_ms";

/// Files SQLite may keep beside a database; a restore preserves all of them.
const SIDE_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

#[derive(Debug)]
pub enum BackupError {
    /// The caller's writer lock is not the lock of the store being restored.
    LockNotHeld {
        store_dir: PathBuf,
    },
    /// The file failed verification; nothing was published or installed.
    Rejected {
        path: PathBuf,
        reason: String,
    },
    Io {
        path: PathBuf,
        error: io::Error,
    },
    Journal(JournalError),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LockNotHeld { store_dir } => write!(
                f,
                "restore requires the writer lock of {}",
                store_dir.display()
            ),
            Self::Rejected { path, reason } => {
                write!(f, "backup {} rejected: {reason}", path.display())
            }
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Journal(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BackupError {}

impl From<JournalError> for BackupError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}

impl From<rusqlite::Error> for BackupError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Journal(JournalError::Sqlite(error))
    }
}

/// A verified backup file and the snapshot it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: PathBuf,
    pub cursor: i64,
    pub store_generation: String,
    pub schema_version: u32,
    pub sqlite_version: String,
    pub sqlite_source_id: String,
    pub created_at_ms: i64,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    /// The verified copy now at the database path.
    pub installed: BackupInfo,
    /// `recovery-original-<now_ms>/` beside the database.
    pub preserved_dir: PathBuf,
    /// The original database and side files, byte-identical, as preserved.
    pub preserved_files: Vec<PathBuf>,
}

/// `PRAGMA wal_checkpoint(TRUNCATE)` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalCheckpoint {
    /// The checkpoint could not finish because a reader or writer held it off.
    pub busy: bool,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
}

impl Journal {
    /// Writes a verified, consistent backup of this store into `dir` as
    /// `backup-<cursor>-<now_ms>.sqlite3` and returns what it holds.
    pub fn backup_into(&self, dir: &Path, now_ms: i64) -> Result<BackupInfo, BackupError> {
        backup_from(&self.conn, dir, now_ms)
    }

    /// Copies every WAL frame into the main file and truncates the WAL to
    /// zero bytes, unless `busy` reports that it could not.
    pub fn checkpoint_truncate(&mut self) -> Result<WalCheckpoint, JournalError> {
        Ok(self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok(WalCheckpoint {
                    busy: row.get::<_, i64>(0)? != 0,
                    log_frames: row.get(1)?,
                    checkpointed_frames: row.get(2)?,
                })
            })?)
    }
}

/// Backs up the store at `db_path` through a separate read-only connection,
/// for a process that is not the writer. The snapshot is the one committed
/// state visible when its read transaction began; concurrent commits by the
/// writer neither block it nor appear in it.
pub fn backup_store_into(
    db_path: &Path,
    dir: &Path,
    now_ms: i64,
) -> Result<BackupInfo, BackupError> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    verify_engine(&conn)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    backup_from(&conn, dir, now_ms)
}

fn backup_from(src: &Connection, dir: &Path, now_ms: i64) -> Result<BackupInfo, BackupError> {
    let staged = dir.join(format!(".backup-{}.tmp", Uuid::new_v4()));
    let result = write_backup(src, &staged, dir, now_ms);
    if result.is_err() {
        remove_with_side_files(&staged);
    }
    result
}

fn write_backup(
    src: &Connection,
    staged: &Path,
    dir: &Path,
    now_ms: i64,
) -> Result<BackupInfo, BackupError> {
    // One read transaction pins the snapshot that is both described and copied.
    let snapshot = src.unchecked_transaction()?;
    let (cursor, generation): (i64, String) = snapshot.query_row(
        "SELECT (SELECT COALESCE(MAX(ingest_seq), 0) FROM observations),
                (SELECT value FROM store_meta WHERE key = 'store_generation')",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut copy = Connection::open_with_flags(
        staged,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    {
        let backup = Backup::new(&snapshot, &mut copy)?;
        let mut waits = 0;
        loop {
            match backup.step(-1)? {
                StepResult::Done => break,
                StepResult::More => {}
                _ => {
                    waits += 1;
                    if waits > 500 {
                        return Err(JournalError::Conflict {
                            detail: "backup source stayed locked".to_owned(),
                        }
                        .into());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    snapshot.finish()?;

    // The copy inherits the source's WAL header; make it a single
    // self-contained file before recording what it holds.
    let mode: String = copy.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
    if mode != "delete" {
        return Err(BackupError::Rejected {
            path: staged.to_path_buf(),
            reason: format!("backup copy stayed in journal_mode {mode}"),
        });
    }
    let (version, source_id) = verify_engine(&copy)?;
    let tx = copy.transaction()?;
    for (key, value) in [
        (KEY_CURSOR, cursor.to_string()),
        (KEY_SCHEMA, SCHEMA_VERSION.to_string()),
        (KEY_SQLITE_VERSION, version),
        (KEY_SQLITE_SOURCE_ID, source_id),
        (KEY_CREATED, now_ms.to_string()),
    ] {
        tx.execute(
            "INSERT OR REPLACE INTO store_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    tx.commit()?;
    copy.close().map_err(|(_, error)| error)?;

    let info = verify_backup(staged)?;
    if info.cursor != cursor || info.store_generation != generation {
        return Err(BackupError::Rejected {
            path: staged.to_path_buf(),
            reason: format!(
                "copy holds cursor {} of {}, source snapshot was {cursor} of {generation}",
                info.cursor, info.store_generation
            ),
        });
    }
    sync_path(staged)?;
    let target = dir.join(format!("backup-{cursor}-{now_ms}.sqlite3"));
    fs::rename(staged, &target).map_err(|error| BackupError::Io {
        path: target.clone(),
        error,
    })?;
    sync_path(dir)?;
    Ok(BackupInfo {
        path: target,
        ..info
    })
}

/// Verifies a backup file without changing it: a complete rollback-journal
/// SQLite file, `integrity_check` ok, read by the qualified engine, this
/// binary's exact schema, a store generation, and a backup record written by
/// the qualified engine whose cursor equals the highest journal position it
/// holds.
pub fn verify_backup(path: &Path) -> Result<BackupInfo, BackupError> {
    let reject = |reason: String| BackupError::Rejected {
        path: path.to_path_buf(),
        reason,
    };
    let bytes = fs::metadata(path)
        .map_err(|error| BackupError::Io {
            path: path.to_path_buf(),
            error,
        })?
        .len();
    check_header(path, bytes).map_err(reject)?;

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| reject(format!("cannot open: {error}")))?;
    let (sqlite_version, sqlite_source_id) =
        verify_engine(&conn).map_err(|error| reject(error.to_string()))?;
    let problems = integrity_problems(&conn).map_err(|error| reject(error.to_string()))?;
    if !problems.is_empty() {
        return Err(reject(format!("integrity_check: {}", problems.join("; "))));
    }
    let schema_version = schema::verify_applied(&conn).map_err(reject)?;
    let meta = backup_meta(&conn).map_err(|error| reject(error.to_string()))?;
    let field = |key: &str| {
        meta.get(key)
            .cloned()
            .ok_or_else(|| reject(format!("no {key} in store_meta; not a journal backup")))
    };
    let store_generation = field(KEY_GENERATION)?;
    let recorded_version = field(KEY_SQLITE_VERSION)?;
    let recorded_source_id = field(KEY_SQLITE_SOURCE_ID)?;
    if recorded_version != REQUIRED_SQLITE_VERSION
        || recorded_source_id != REQUIRED_SQLITE_SOURCE_ID
    {
        return Err(reject(format!(
            "written by SQLite {recorded_version} ({recorded_source_id}), not the qualified {REQUIRED_SQLITE_VERSION}"
        )));
    }
    let number = |key: &str| {
        let text = field(key)?;
        text.parse::<i64>()
            .map_err(|_| reject(format!("{key} {text:?} is not a number")))
    };
    if number(KEY_SCHEMA)? != i64::from(SCHEMA_VERSION) {
        return Err(reject(format!(
            "backup record names schema {}, not {SCHEMA_VERSION}",
            field(KEY_SCHEMA)?
        )));
    }
    let cursor = number(KEY_CURSOR)?;
    let created_at_ms = number(KEY_CREATED)?;
    let held: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) FROM observations",
            [],
            |row| row.get(0),
        )
        .map_err(|error| reject(error.to_string()))?;
    if held != cursor {
        return Err(reject(format!(
            "holds cursor {held} but its backup record says {cursor}"
        )));
    }
    drop(conn);

    let sha256 = file_sha256(path).map_err(|error| BackupError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    Ok(BackupInfo {
        path: path.to_path_buf(),
        cursor,
        store_generation,
        schema_version,
        sqlite_version,
        sqlite_source_id,
        created_at_ms,
        sha256,
        bytes,
    })
}

/// Replaces the store's database with a verified backup.
///
/// The caller holds `lock` for the store directory and has closed every
/// `Journal` on `db_path`. The backup is copied beside the database and
/// verified first; any failure up to that point leaves the original files
/// untouched. The original database and its `-wal`, `-shm` and `-journal`
/// files are then preserved byte-identical in `recovery-original-<now_ms>/`,
/// the side files are removed (a stale WAL would otherwise be replayed onto
/// the restored database), and the verified copy is renamed into place.
pub fn restore_backup(
    lock: &WriterLock,
    db_path: &Path,
    backup: &Path,
    now_ms: i64,
) -> Result<RestoreOutcome, BackupError> {
    let store_dir = db_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_lock_covers(lock, store_dir)?;
    let Some(name) = db_path.file_name() else {
        return Err(BackupError::Io {
            path: db_path.to_path_buf(),
            error: io::Error::from(io::ErrorKind::InvalidInput),
        });
    };
    let staged = store_dir.join(format!(
        ".{}.restore-{}.tmp",
        name.to_string_lossy(),
        Uuid::new_v4()
    ));
    let installed = match stage_restore(backup, &staged) {
        Ok(info) => info,
        Err(error) => {
            remove_with_side_files(&staged);
            return Err(error);
        }
    };
    match swap_in(db_path, store_dir, &staged, now_ms) {
        Ok((preserved_dir, preserved_files)) => Ok(RestoreOutcome {
            installed: BackupInfo {
                path: db_path.to_path_buf(),
                ..installed
            },
            preserved_dir,
            preserved_files,
        }),
        Err(error) => {
            remove_with_side_files(&staged);
            Err(error)
        }
    }
}

fn ensure_lock_covers(lock: &WriterLock, store_dir: &Path) -> Result<(), BackupError> {
    let covered = lock.is_held()
        && lock.path().parent().is_some_and(|lock_dir| {
            matches!(
                (fs::canonicalize(lock_dir), fs::canonicalize(store_dir)),
                (Ok(a), Ok(b)) if a == b
            )
        });
    if covered {
        Ok(())
    } else {
        Err(BackupError::LockNotHeld {
            store_dir: store_dir.to_path_buf(),
        })
    }
}

/// Copies the backup beside the database and verifies the exact bytes that
/// would be installed.
fn stage_restore(backup: &Path, staged: &Path) -> Result<BackupInfo, BackupError> {
    fs::copy(backup, staged).map_err(|error| BackupError::Io {
        path: backup.to_path_buf(),
        error,
    })?;
    sync_path(staged)?;
    verify_backup(staged).map_err(|error| match error {
        BackupError::Rejected { reason, .. } => BackupError::Rejected {
            path: backup.to_path_buf(),
            reason,
        },
        other => other,
    })
}

fn swap_in(
    db_path: &Path,
    store_dir: &Path,
    staged: &Path,
    now_ms: i64,
) -> Result<(PathBuf, Vec<PathBuf>), BackupError> {
    let io_at = |path: &Path| {
        let path = path.to_path_buf();
        move |error| BackupError::Io { path, error }
    };
    let preserved_dir = store_dir.join(format!("recovery-original-{now_ms}"));
    fs::create_dir(&preserved_dir).map_err(io_at(&preserved_dir))?;

    let originals: Vec<PathBuf> = std::iter::once(db_path.to_path_buf())
        .chain(
            SIDE_SUFFIXES
                .iter()
                .map(|suffix| with_suffix(db_path, suffix)),
        )
        .filter(|path| path.symlink_metadata().is_ok())
        .collect();
    let mut preserved = Vec::with_capacity(originals.len());
    for original in &originals {
        let kept = preserved_dir.join(original.file_name().unwrap_or_default());
        if let Err(error) = fs::hard_link(original, &kept) {
            let _ = fs::remove_dir_all(&preserved_dir);
            return Err(io_at(original)(error));
        }
        preserved.push(kept);
    }
    if let Err(error) = sync_path(&preserved_dir) {
        let _ = fs::remove_dir_all(&preserved_dir);
        return Err(error);
    }

    // Side files go before the main file is replaced: SQLite opens and
    // replays any `-wal` it finds beside a database.
    for original in originals.iter().filter(|path| path.as_path() != db_path) {
        if let Err(error) = fs::remove_file(original) {
            relink(&originals, &preserved);
            return Err(io_at(original)(error));
        }
    }
    if let Err(error) = fs::rename(staged, db_path) {
        relink(&originals, &preserved);
        return Err(io_at(db_path)(error));
    }
    sync_path(store_dir)?;
    Ok((preserved_dir, preserved))
}

/// Best effort: puts preserved originals back where they are missing.
fn relink(originals: &[PathBuf], preserved: &[PathBuf]) {
    for (original, kept) in originals.iter().zip(preserved) {
        if original.symlink_metadata().is_err() {
            let _ = fs::hard_link(kept, original);
        }
    }
}

/// The 100-byte database header must describe a complete rollback-journal
/// file: the right magic, page size, version bytes 18/19 = 1 (not WAL) and a
/// valid in-header page count that matches the file length.
fn check_header(path: &Path, bytes: u64) -> Result<(), String> {
    let mut header = [0_u8; 100];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| format!("{bytes}-byte file has no SQLite header ({error})"))?;
    if &header[..16] != SQLITE_MAGIC {
        return Err("not a SQLite database file".to_owned());
    }
    let page_size = match u16::from_be_bytes([header[16], header[17]]) {
        1 => 65_536,
        size => u64::from(size),
    };
    if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
        return Err(format!("invalid page size {page_size}"));
    }
    if header[18] != 1 || header[19] != 1 {
        return Err(format!(
            "header version bytes {}/{} are not rollback mode; a WAL-mode file is not a self-contained backup",
            header[18], header[19]
        ));
    }
    let word = |at: usize| {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let pages = u64::from(word(28));
    if pages == 0 || word(24) != word(92) {
        return Err("in-header page count is not valid".to_owned());
    }
    if bytes != pages * page_size {
        return Err(format!(
            "incomplete: {bytes} bytes, header declares {pages} pages of {page_size}"
        ));
    }
    Ok(())
}

fn integrity_problems(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut statement = conn.prepare("PRAGMA integrity_check")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if rows == ["ok"] {
        return Ok(Vec::new());
    }
    let detail: Vec<String> = rows
        .iter()
        .flat_map(|row| row.lines())
        .filter(|line| !line.starts_with("*** in database"))
        .take(5)
        .map(str::to_owned)
        .collect();
    Ok(if detail.is_empty() { rows } else { detail })
}

fn backup_meta(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    let mut statement =
        conn.prepare("SELECT key, value FROM store_meta WHERE key IN (?1, ?2, ?3, ?4, ?5, ?6)")?;
    statement
        .query_map(
            params![
                KEY_GENERATION,
                KEY_CURSOR,
                KEY_SCHEMA,
                KEY_SQLITE_VERSION,
                KEY_SQLITE_SOURCE_ID,
                KEY_CREATED
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?
        .collect()
}

/// Lowercase hex SHA-256 of a file's bytes.
pub fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// `fsync`s a file or directory (std issues `F_FULLFSYNC` on Apple targets).
fn sync_path(path: &Path) -> Result<(), BackupError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| BackupError::Io {
            path: path.to_path_buf(),
            error,
        })
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut text = path.as_os_str().to_owned();
    text.push(suffix);
    PathBuf::from(text)
}

fn remove_with_side_files(path: &Path) {
    let _ = fs::remove_file(path);
    for suffix in SIDE_SUFFIXES {
        let _ = fs::remove_file(with_suffix(path, suffix));
    }
}
