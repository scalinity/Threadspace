//! Owner commands the M0 store committed stay idempotent through the
//! migration: retrying the original request with its original ID returns the
//! original receipt (cursors 7 and 9 in the accepted M0C fixture), across an
//! unrelated revision advance and a restart; reusing either ID for a
//! different request is a conflict with no effect.

use std::path::{Path, PathBuf};

use threadspace_contracts::ui::ReceiptStatus;
use threadspace_journal::{Journal, JournalError};

const ACKED: &str = "f14c9f36-e4e4-4bd0-acbd-525a886c0702";
const RESOLVED: &str = "2a78597e-9f5d-4ae5-a6e3-73ea0ff6b72b";

fn migrated_fixture() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("ts-m0-commands-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("journal.sqlite3");
    // `immutable=1`: reading the committed fixture must not touch its files.
    let fixture = repo.join("fixtures/m1/m0-store-v2/journal.sqlite3");
    rusqlite::Connection::open_with_flags(
        format!("file:{}?immutable=1", fixture.display()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .and_then(|source| source.execute("VACUUM INTO ?1", [path.to_str().expect("utf-8")]))
    .expect("copy");
    path
}

fn retry_both(journal: &mut Journal, now_ms: i64) {
    let ack = journal.acknowledge_attention("cmd-m0-ack", ACKED, None, now_ms).expect("the original acknowledgement");
    assert_eq!((ack.receipt.status, ack.receipt.cursor.as_str()), (ReceiptStatus::AlreadyCommitted, "7"));
    assert!(ack.change.is_none(), "nothing new");
    let resolve = journal
        .resolve_attention("cmd-m0-resolve", RESOLVED, None, "handled before M1", now_ms)
        .expect("the original resolution");
    assert_eq!((resolve.receipt.status, resolve.receipt.cursor.as_str()), (ReceiptStatus::AlreadyCommitted, "9"));
    assert!(resolve.change.is_none(), "nothing new");
}

fn observations(path: &Path) -> i64 {
    rusqlite::Connection::open(path)
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0)))
        .expect("count")
}

#[test]
fn original_m0_command_retries_return_their_original_receipts() {
    let path = migrated_fixture();
    let mut journal = Journal::open(&path, "test-core", 1_800_000_000_000).expect("migrate");
    retry_both(&mut journal, 1_800_000_000_001);

    // An unrelated revision advance, then a restart.
    journal.acknowledge_attention("cmd-m1-other", RESOLVED, None, 1_800_000_000_002).expect("unrelated");
    retry_both(&mut journal, 1_800_000_000_003);
    drop(journal);
    let mut journal = Journal::open(&path, "test-core", 1_800_000_000_004).expect("restart");
    retry_both(&mut journal, 1_800_000_000_005);

    // The same IDs for different requests: refused, nothing written.
    let before = observations(&path);
    let other_target = journal.acknowledge_attention("cmd-m0-ack", RESOLVED, None, 1_800_000_000_006);
    assert!(matches!(other_target, Err(JournalError::Conflict { .. })), "{other_target:?}");
    let other_reason = journal.resolve_attention("cmd-m0-resolve", RESOLVED, None, "another reason", 1_800_000_000_007);
    assert!(matches!(other_reason, Err(JournalError::Conflict { .. })), "{other_reason:?}");
    let other_action = journal.resolve_attention("cmd-m0-ack", ACKED, None, "handled", 1_800_000_000_008);
    assert!(matches!(other_action, Err(JournalError::Conflict { .. })), "{other_action:?}");
    assert_eq!(observations(&path), before, "a refused reuse writes nothing");
    let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
}
