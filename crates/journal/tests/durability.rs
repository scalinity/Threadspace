//! Admission receipts, crash points, backup, checkpoint and restore against
//! real on-disk SQLite files. The native crash matrix is
//! `tests/native/journal-crash`; these prove each primitive's contract.

use std::path::{Path, PathBuf};

use threadspace_contracts::ui::ReceiptStatus;
use threadspace_journal::{
    BackupError, Journal, JournalError, ObservationAdmission, SCHEMA_VERSION, WriterLock,
    backup_store_into, restore_backup, verify_backup,
};

const NOW: i64 = 1_790_000_000_000;
const SOURCE: &str = "test.durability";

struct TempStore(PathBuf);

impl TempStore {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("threadspace-durability-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp store");
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join("journal.sqlite3")
    }
    fn sub(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        std::fs::create_dir_all(&dir).expect("create subdir");
        dir
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Record {
    id: String,
    payload: serde_json::Value,
    captured: i64,
}

impl Record {
    fn new(index: i64) -> Self {
        Self {
            id: format!("00000000-0000-4000-8000-{index:012x}"),
            payload: serde_json::json!({ "index": index }),
            captured: NOW + index,
        }
    }
    fn admission(&self) -> ObservationAdmission<'_> {
        ObservationAdmission {
            observation_id: &self.id,
            source_id: SOURCE,
            source_epoch: "epoch-test",
            source_sequence: None,
            native_event: "TEST_RECORD",
            captured_wall_ms: self.captured,
            payload: &self.payload,
        }
    }
}

fn admit(journal: &mut Journal, from: i64, to: i64) -> Vec<i64> {
    (from..=to)
        .map(|index| {
            let receipt = journal
                .admit_observation(&Record::new(index).admission(), NOW + 100 + index)
                .expect("admit");
            assert_eq!(receipt.status, ReceiptStatus::Committed);
            receipt.cursor
        })
        .collect()
}

fn hash(path: &Path) -> Option<String> {
    path.exists().then(|| {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(std::fs::read(path).expect("read"));
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    })
}

fn side(path: &Path, suffix: &str) -> PathBuf {
    let mut text = path.as_os_str().to_owned();
    text.push(suffix);
    PathBuf::from(text)
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn admission_commits_once_and_replays_the_original_cursor() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let record = Record::new(1);
    let first = journal
        .admit_observation(&record.admission(), NOW + 1)
        .expect("admit");
    assert_eq!(first.status, ReceiptStatus::Committed);
    assert_eq!(first.observation_id, record.id);
    assert_eq!(journal.cursor().expect("cursor"), first.cursor);

    let retry = journal
        .admit_observation(&record.admission(), NOW + 2)
        .expect("retry");
    assert_eq!(retry.status, ReceiptStatus::AlreadyCommitted);
    assert_eq!(retry.cursor, first.cursor, "original cursor");
    assert_eq!(
        journal.cursor().expect("cursor"),
        first.cursor,
        "nothing written"
    );

    let upper = record.id.to_uppercase();
    let mut admission = record.admission();
    admission.observation_id = &upper;
    let canonical = journal
        .admit_observation(&admission, NOW + 3)
        .expect("uppercase");
    assert_eq!(canonical.status, ReceiptStatus::AlreadyCommitted);
    assert_eq!(canonical.observation_id, record.id);
    drop(journal);

    let mut reopened = Journal::open(&store.db(), "epoch-b", NOW + 10).expect("reopen");
    let replay = reopened
        .admit_observation(&record.admission(), NOW + 11)
        .expect("replay");
    assert_eq!(replay.status, ReceiptStatus::AlreadyCommitted);
    assert_eq!(replay.cursor, first.cursor);
}

#[test]
fn admission_rejects_reused_ids_and_non_uuids_without_writing() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let record = Record::new(1);
    journal
        .admit_observation(&record.admission(), NOW + 1)
        .expect("admit");
    let cursor = journal.cursor().expect("cursor");

    let other = serde_json::json!({ "index": 2 });
    let mut changed = record.admission();
    changed.payload = &other;
    assert!(matches!(
        journal.admit_observation(&changed, NOW + 2),
        Err(JournalError::Conflict { .. })
    ));
    let mut moved = record.admission();
    moved.native_event = "OTHER";
    assert!(matches!(
        journal.admit_observation(&moved, NOW + 3),
        Err(JournalError::Conflict { .. })
    ));
    let mut bad = record.admission();
    bad.observation_id = "not-a-uuid";
    assert!(matches!(
        journal.admit_observation(&bad, NOW + 4),
        Err(JournalError::Invalid { .. })
    ));
    assert_eq!(journal.cursor().expect("cursor"), cursor);
}

#[cfg(feature = "qualification")]
mod crash {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;

    use threadspace_journal::{CRASH_AT_ENV, CRASH_POINT_ENV, CrashPlan, CrashPoint, Journal};

    use super::{NOW, Record, SOURCE, TempStore};

    const CHILD_STORE_ENV: &str = "THREADSPACE_JOURNAL_TEST_CHILD_STORE";
    const CHILD_SETTER_ENV: &str = "THREADSPACE_JOURNAL_TEST_CHILD_SETTER";

    #[test]
    fn crash_point_names_round_trip() {
        for point in CrashPoint::ALL {
            assert_eq!(point.name().parse::<CrashPoint>().expect("parse"), point);
        }
        assert!("after-receipt".parse::<CrashPoint>().is_err());
    }

    /// Runs only as a re-executed child of the test below: admits three
    /// records and prints each receipt after the journal returns it.
    #[test]
    fn crash_child() {
        let Ok(store) = std::env::var(CHILD_STORE_ENV) else {
            return;
        };
        let db = std::path::Path::new(&store).join("journal.sqlite3");
        let mut journal = Journal::open(&db, "epoch-child", NOW).expect("open");
        if let Ok(point) = std::env::var(CHILD_SETTER_ENV) {
            journal.set_crash_plan(Some(CrashPlan {
                point: point.parse().expect("point"),
                at_admission: 2,
            }));
        }
        for index in 1..=3 {
            let receipt = journal
                .admit_observation(&Record::new(index).admission(), NOW + index)
                .expect("admit");
            println!("ACK {} {}", receipt.observation_id, receipt.cursor);
        }
    }

    #[test]
    fn crash_points_kill_the_process_at_their_boundary() {
        let exe = std::env::current_exe().expect("test binary");
        for point in CrashPoint::ALL {
            for via_env in [false, true] {
                let store = TempStore::new();
                let mut command = Command::new(&exe);
                command
                    .args(["--exact", "crash::crash_child", "--nocapture"])
                    .env(CHILD_STORE_ENV, &store.0)
                    .env_remove(CRASH_POINT_ENV)
                    .env_remove(CRASH_AT_ENV)
                    .env_remove(CHILD_SETTER_ENV);
                if via_env {
                    command
                        .env(CRASH_POINT_ENV, point.name())
                        .env(CRASH_AT_ENV, "2");
                } else {
                    command.env(CHILD_SETTER_ENV, point.name());
                }
                let output = command.output().expect("run child");
                let label = format!(
                    "{} via {}",
                    point.name(),
                    if via_env { "env" } else { "setter" }
                );
                assert_eq!(
                    output.status.signal(),
                    Some(libc::SIGKILL),
                    "{label}: child must die at the crash point"
                );
                let stdout = String::from_utf8_lossy(&output.stdout);
                let acks: Vec<&str> = stdout
                    .lines()
                    .filter(|line| line.starts_with("ACK "))
                    .collect();
                let first = Record::new(1);
                assert_eq!(acks.len(), 1, "{label}: only the first record is ACKed");
                assert!(
                    acks[0].starts_with(&format!("ACK {} ", first.id)),
                    "{label}"
                );

                let journal = Journal::open(&store.db(), "epoch-parent", NOW + 50).expect("reopen");
                let ids: Vec<String> = journal
                    .admitted_observations(SOURCE)
                    .expect("census")
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                let mut expected = vec![first.id];
                if point == CrashPoint::AfterCommitBeforeReceipt {
                    expected.push(Record::new(2).id);
                }
                assert_eq!(ids, expected, "{label}");
            }
        }
    }
}

#[test]
fn backup_into_publishes_a_verified_self_contained_snapshot() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    admit(&mut journal, 1, 12);
    let dir = store.sub("backups");
    let info = journal.backup_into(&dir, NOW + 500).expect("backup");

    assert_eq!(info.cursor, journal.cursor().expect("cursor"));
    assert_eq!(info.store_generation, journal.store_generation());
    assert_eq!(info.schema_version, SCHEMA_VERSION);
    assert_eq!(info.sqlite_version, "3.53.4");
    assert_eq!(info.created_at_ms, NOW + 500);
    assert_eq!(info.path.parent(), Some(dir.as_path()));
    assert_eq!(
        listing(&dir),
        vec![format!("backup-{}-{}.sqlite3", info.cursor, NOW + 500)],
        "no temp or side files remain"
    );
    assert_eq!(
        info.bytes,
        std::fs::metadata(&info.path).expect("meta").len()
    );
    assert_eq!(Some(info.sha256.clone()), hash(&info.path));
    let header = std::fs::read(&info.path).expect("read");
    assert_eq!((header[18], header[19]), (1, 1), "rollback-mode file");
    assert_eq!(verify_backup(&info.path).expect("verify"), info);
}

#[test]
fn backup_store_into_reads_beside_an_open_writer() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    admit(&mut journal, 1, 5);
    let info = backup_store_into(&store.db(), &store.sub("backups"), NOW + 1).expect("backup");
    assert_eq!(info.cursor, journal.cursor().expect("cursor"));
    admit(&mut journal, 6, 7);
    assert!(
        journal.cursor().expect("cursor") > info.cursor,
        "writer continues"
    );
}

#[test]
fn checkpoint_truncate_empties_the_wal() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    admit(&mut journal, 1, 20);
    let wal = side(&store.db(), "-wal");
    assert!(std::fs::metadata(&wal).expect("wal").len() > 0);
    let checkpoint = journal.checkpoint_truncate().expect("checkpoint");
    assert!(!checkpoint.busy);
    assert_eq!(std::fs::metadata(&wal).expect("wal").len(), 0);
    admit(&mut journal, 21, 21);
}

#[test]
fn restore_installs_the_backup_and_preserves_the_original() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let generation = journal.store_generation().to_owned();
    admit(&mut journal, 1, 6);
    let info = journal
        .backup_into(&store.sub("backups"), NOW + 1)
        .expect("backup");
    admit(&mut journal, 7, 9);
    drop(journal);
    let original = hash(&store.db());

    let lock = WriterLock::acquire(&store.0).expect("lock");
    let outcome = restore_backup(&lock, &store.db(), &info.path, NOW + 2).expect("restore");
    assert_eq!(outcome.installed.sha256, info.sha256);
    assert_eq!(hash(&store.db()), Some(info.sha256.clone()));
    assert_eq!(
        outcome.preserved_dir,
        store.0.join(format!("recovery-original-{}", NOW + 2))
    );
    assert_eq!(
        hash(&outcome.preserved_dir.join("journal.sqlite3")),
        original,
        "original preserved byte-identical"
    );
    assert!(!side(&store.db(), "-wal").exists());

    let mut restored = Journal::open(&store.db(), "epoch-b", NOW + 3).expect("reopen");
    assert_eq!(restored.store_generation(), generation);
    assert_eq!(restored.cursor().expect("cursor"), info.cursor);
    let diagnostics = restored.sqlite_diagnostics().expect("diagnostics");
    assert_eq!(diagnostics.journal_mode, "wal");
    let later = restored
        .admit_observation(&Record::new(7).admission(), NOW + 4)
        .expect("readmit");
    assert_eq!(
        later.status,
        ReceiptStatus::Committed,
        "post-backup record is gone"
    );
    let earlier = restored
        .admit_observation(&Record::new(6).admission(), NOW + 5)
        .expect("replay");
    assert_eq!(earlier.status, ReceiptStatus::AlreadyCommitted);
}

/// Writes a tampered copy of `backup` at `dir/name` through a plain SQLite
/// connection.
fn tampered(backup: &Path, dir: &Path, name: &str, sql: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::copy(backup, &path).expect("copy");
    let conn = rusqlite::Connection::open(&path).expect("open copy");
    conn.execute_batch(sql).expect("tamper");
    conn.close().expect("close");
    path
}

#[test]
fn restore_rejects_bad_backups_and_leaves_the_original_byte_identical() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    admit(&mut journal, 1, 30);
    let good = journal
        .backup_into(&store.sub("backups"), NOW + 1)
        .expect("backup");
    admit(&mut journal, 31, 32);
    let bad = store.sub("bad");
    let bytes = std::fs::read(&good.path).expect("read backup");
    let page = 4096;

    let mut cases: Vec<(&str, PathBuf)> = Vec::new();
    let mut raw = |name: &'static str, content: &[u8]| {
        let path = bad.join(name);
        std::fs::write(&path, content).expect("write");
        cases.push((name, path));
    };
    raw("empty", &[]);
    raw("not-sqlite", &[0x5a; 8192]);
    raw("truncated-mid-page", &bytes[..bytes.len() / 2 + 100]);
    raw("missing-last-page", &bytes[..bytes.len() - page]);
    let mut corrupt = bytes.clone();
    for offset in (page..corrupt.len()).step_by(page).skip(1) {
        corrupt[offset..offset + 8].fill(0xff);
    }
    raw("corrupt-pages", &corrupt);
    let mut one_page = bytes.clone();
    let last = one_page.len() - page;
    one_page[last + 1..last + 5].fill(0xff);
    raw("corrupt-last-page", &one_page);
    // A copy of the live main file while WAL is active.
    raw(
        "live-main-copy",
        &std::fs::read(store.db()).expect("read live main"),
    );
    cases.push((
        "future-schema",
        tampered(
            &good.path,
            &bad,
            "future-schema",
            "INSERT INTO schema_migrations VALUES (99, 'future', 'x', 0);",
        ),
    ));
    cases.push((
        "other-engine",
        tampered(
            &good.path,
            &bad,
            "other-engine",
            "UPDATE store_meta SET value = '3.53.2' WHERE key = 'backup_sqlite_version';",
        ),
    ));
    cases.push((
        "no-backup-record",
        tampered(
            &good.path,
            &bad,
            "no-backup-record",
            "DELETE FROM store_meta WHERE key LIKE 'backup_%';",
        ),
    ));
    cases.push((
        "cursor-mismatch",
        tampered(
            &good.path,
            &bad,
            "cursor-mismatch",
            "UPDATE store_meta SET value = '1' WHERE key = 'backup_cursor';",
        ),
    ));

    let watched: Vec<PathBuf> = ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| side(&store.db(), suffix))
        .collect();
    let lock = WriterLock::acquire(&store.0).expect("lock");
    let before_listing = listing(&store.0);
    for (name, path) in &cases {
        let before: Vec<Option<String>> = watched.iter().map(|path| hash(path)).collect();
        match restore_backup(&lock, &store.db(), path, NOW + 10) {
            Err(BackupError::Rejected { reason, .. }) => eprintln!("{name}: rejected: {reason}"),
            other => panic!("{name}: expected rejection, got {other:?}"),
        }
        let after: Vec<Option<String>> = watched.iter().map(|path| hash(path)).collect();
        assert_eq!(before, after, "{name}: original byte-identical");
        assert_eq!(
            listing(&store.0),
            before_listing,
            "{name}: no recovery dir or staged file"
        );
    }
    assert!(
        verify_backup(&good.path).is_ok(),
        "the untouched backup still verifies"
    );
    drop(journal);
}

#[test]
fn restore_refuses_a_lock_for_another_store() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    admit(&mut journal, 1, 3);
    let info = journal
        .backup_into(&store.sub("backups"), NOW + 1)
        .expect("backup");
    drop(journal);
    let before = hash(&store.db());
    let elsewhere = TempStore::new();
    let wrong = WriterLock::acquire(&elsewhere.0).expect("other lock");
    assert!(matches!(
        restore_backup(&wrong, &store.db(), &info.path, NOW + 2),
        Err(BackupError::LockNotHeld { .. })
    ));
    assert_eq!(hash(&store.db()), before);
}
