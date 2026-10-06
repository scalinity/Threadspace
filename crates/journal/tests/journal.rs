//! Domain-layer tests against real on-disk SQLite files. These prove the
//! journal's contract; native G10 evidence comes from the system-started
//! companion (tests/native).

use std::path::PathBuf;

use threadspace_contracts::ui::ReceiptStatus;
use threadspace_journal::{
    Journal, JournalError, LockError, REQUIRED_SQLITE_SOURCE_ID, REQUIRED_SQLITE_VERSION, WriterLock,
};

struct TempStore(PathBuf);

impl TempStore {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("threadspace-journal-{}", uuid::Uuid::new_v4()));
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

#[test]
fn linked_engine_is_exactly_the_frozen_candidate() {
    let conn = rusqlite::Connection::open_in_memory().expect("open");
    let (version, source_id) = threadspace_journal::verify_engine(&conn).expect("engine matches");
    assert_eq!(version, REQUIRED_SQLITE_VERSION);
    assert_eq!(source_id, REQUIRED_SQLITE_SOURCE_ID);
    assert_eq!(rusqlite::version(), "3.53.4");
}

#[test]
fn open_applies_wal_full_foreign_keys_and_seeds_one_fixture() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let diagnostics = journal.sqlite_diagnostics().expect("diagnostics");
    assert_eq!(diagnostics.journal_mode, "wal");
    assert_eq!(diagnostics.synchronous, 2, "synchronous=FULL");
    assert!(diagnostics.foreign_keys);

    let (cursor, snapshot) = journal.snapshot().expect("snapshot");
    assert!(cursor > 0);
    assert_eq!(snapshot.sessions.len(), 1);
    let session = &snapshot.sessions[0];
    assert!(session.fixture);
    assert!(session.process.is_some(), "fixture ProcessKey");
    assert!(session.binding.is_some(), "fixture SurfaceBinding");
    assert_eq!(session.activation.as_deref(), Some("1"));
    assert_eq!(snapshot.attention.len(), 1);
    assert_eq!(snapshot.counts.needs_attention, 1);
}

#[test]
fn close_and_reopen_recovers_the_same_stored_fixture() {
    let store = TempStore::new();
    let (generation, before) = {
        let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
        (journal.store_generation().to_owned(), journal.snapshot().expect("snapshot"))
    };
    let mut reopened = Journal::open(&store.db(), "epoch-b", NOW + 1).expect("reopen");
    assert_eq!(reopened.store_generation(), generation, "store generation survives restart");
    assert_eq!(reopened.snapshot().expect("snapshot"), before, "identical fixture, no reseed");
}

#[test]
fn receipts_are_commit_only_and_idempotent() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let item = &snapshot.attention[0];
    let revision: i64 = item.revision.parse().expect("revision");
    let command = uuid::Uuid::new_v4().to_string();

    let first = journal
        .acknowledge_attention(&command, &item.attention_id, Some(revision), NOW + 5)
        .expect("ack");
    assert_eq!(first.receipt.status, ReceiptStatus::Committed);
    assert!(first.change.is_some());

    let retry = journal
        .acknowledge_attention(&command, &item.attention_id, Some(revision), NOW + 9)
        .expect("retry");
    assert_eq!(retry.receipt.status, ReceiptStatus::AlreadyCommitted);
    assert_eq!(retry.receipt.cursor, first.receipt.cursor, "original result returned");
    assert!(retry.change.is_none(), "no second state change");

    let conflicting = journal.acknowledge_attention(&command, &item.attention_id, None, NOW + 10);
    assert!(matches!(conflicting, Err(JournalError::Conflict { .. })), "payload reuse rejected");

    let stale = journal.acknowledge_attention(
        &uuid::Uuid::new_v4().to_string(),
        &item.attention_id,
        Some(revision),
        NOW + 11,
    );
    assert!(matches!(stale, Err(JournalError::Conflict { .. })), "stale revision rejected");

    let (_, after) = journal.snapshot().expect("snapshot");
    assert_eq!(after.counts.needs_attention, 0);
    assert_eq!(after.counts.awaiting_action, 1, "acknowledged is not resolved");
    drop(journal);

    let mut reopened = Journal::open(&store.db(), "epoch-b", NOW + 20).expect("reopen");
    let (_, recovered) = reopened.snapshot().expect("snapshot");
    assert_eq!(recovered.attention[0].acknowledged_at_ms, Some(NOW + 5));
    let replay = reopened
        .acknowledge_attention(&command, &item.attention_id, Some(revision), NOW + 21)
        .expect("retry after restart");
    assert_eq!(replay.receipt.status, ReceiptStatus::AlreadyCommitted);
}

#[test]
fn patch_carries_full_upserts_for_changed_entities() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let (cursor, snapshot) = journal.snapshot().expect("snapshot");
    let item = &snapshot.attention[0];
    let outcome = journal
        .acknowledge_attention(&uuid::Uuid::new_v4().to_string(), &item.attention_id, None, NOW + 1)
        .expect("ack");
    let change = outcome.change.expect("change");
    let patch = journal.patch_for(cursor, &change).expect("patch");
    assert_eq!(patch.from_cursor, cursor.to_string());
    assert_eq!(patch.to_cursor, change.cursor.to_string());
    assert_eq!(patch.attention_upserts.len(), 1);
    assert_eq!(patch.attention_upserts[0].acknowledged_at_ms, Some(NOW + 1));
    assert_eq!(patch.session_upserts.len(), 1);
}

#[test]
fn only_one_writer_can_hold_the_store() {
    let store = TempStore::new();
    let first = WriterLock::acquire(&store.0).expect("first writer");
    match WriterLock::acquire(&store.0) {
        Err(LockError::Held { .. }) => {}
        other => panic!("second writer must be refused, got {other:?}"),
    }
    drop(first);
    WriterLock::acquire(&store.0).expect("lock is released with its holder");
}

#[test]
fn newer_schema_is_refused() {
    let store = TempStore::new();
    drop(Journal::open(&store.db(), "epoch-a", NOW).expect("open"));
    {
        let conn = rusqlite::Connection::open(store.db()).expect("raw open");
        conn.execute(
            "INSERT INTO schema_migrations (id, name, checksum, applied_at_ms) VALUES (99, 'future', 'x', 0)",
            [],
        )
        .expect("insert future migration");
    }
    match Journal::open(&store.db(), "epoch-b", NOW) {
        Err(JournalError::SchemaTooNew { found: 99 }) => {}
        other => panic!("expected SchemaTooNew, got {other:?}"),
    }
}

#[cfg(feature = "qualification")]
#[test]
fn qualification_attention_has_outbox_intent_in_the_same_commit() {
    use threadspace_contracts::projection::NotificationState;

    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch-a", NOW).expect("open");
    let raised = journal.raise_qualification_attention("smoke", NOW + 1).expect("raise");
    let target = journal.attention_target(&raised.intent.attention_id).expect("target");
    assert!(target.outstanding);
    let patch = journal.patch_for(0, &raised.change).expect("patch");
    assert_eq!(patch.attention_upserts[0].notification_state, NotificationState::Pending);

    let change = journal
        .record_notification_state(&raised.intent.request_id, &NotificationState::Submitted, "accepted", NOW + 2)
        .expect("record");
    let patch = journal.patch_for(raised.change.cursor, &change).expect("patch");
    assert_eq!(patch.attention_upserts[0].notification_state, NotificationState::Submitted);
}
