//! M0C journal behaviour: bounded initial views and pages (SPEC §18.4),
//! explicit "Mark handled" (SPEC §7.2), the observation preference and
//! maintenance phase records (SPEC §19.5), and qualification fixtures.

use std::path::PathBuf;

use threadspace_contracts::frames::snapshot_fits;
use threadspace_contracts::ui::ReceiptStatus;
use threadspace_journal::{Journal, JournalError};

struct TempStore(PathBuf);

impl TempStore {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("threadspace-m0c-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp store");
        Self(dir)
    }
    fn open(&self) -> Journal {
        Journal::open(&self.0.join("journal.sqlite3"), "epoch", NOW).expect("open")
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_790_000_000_000;

#[test]
fn small_store_gets_a_complete_view() {
    let store = TempStore::new();
    let mut journal = store.open();
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    assert!(snapshot.complete);
    assert_eq!(snapshot.total_sessions as usize, snapshot.sessions.len());
    assert_eq!(snapshot.total_attention as usize, snapshot.attention.len());
    assert!(snapshot.sessions_after.is_none() && snapshot.attention_after.is_none());
}

#[test]
fn oversized_store_gets_a_bounded_view_and_pages_cover_every_row_once() {
    let store = TempStore::new();
    let mut journal = store.open();
    journal
        .populate_synthetic(3000, 400, NOW)
        .expect("populate");
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    assert!(!snapshot.complete, "a 3000-session view cannot fit 512 KiB");
    let json = serde_json::to_string(&snapshot).expect("json");
    assert!(
        snapshot_fits(&json),
        "the bounded view itself fits the bound"
    );
    assert_eq!(
        snapshot.total_sessions, 3001,
        "fixture plus populated sessions"
    );
    let mut seen: Vec<String> = snapshot
        .sessions
        .iter()
        .map(|row| row.session_id.clone())
        .collect();
    let mut after = snapshot.sessions_after.clone();
    while let Some(position) = after {
        let (_, page) = journal.session_page(Some(&position), 500).expect("page");
        let bytes = serde_json::to_vec(&page.rows).expect("json").len();
        assert!(bytes < 64 * 1024, "page fits a 64 KiB reply");
        assert_eq!(page.total, 3001);
        seen.extend(page.rows.iter().map(|row| row.session_id.clone()));
        after = page.next_after;
    }
    let count = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), count, "no row twice");
    assert_eq!(count, 3001, "every row once");
}

#[test]
fn mark_handled_is_idempotent_reasoned_and_conflict_checked() {
    let store = TempStore::new();
    let mut journal = store.open();
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let item = snapshot.attention[0].attention_id.clone();
    let command = uuid::Uuid::new_v4().to_string();
    let first = journal
        .resolve_attention(&command, &item, None, "handled in terminal", NOW)
        .expect("resolve");
    assert_eq!(first.receipt.status, ReceiptStatus::Committed);
    let retry = journal
        .resolve_attention(&command, &item, None, "handled in terminal", NOW)
        .expect("retry");
    assert_eq!(retry.receipt.status, ReceiptStatus::AlreadyCommitted);
    assert!(retry.change.is_none());
    let conflict = journal.resolve_attention(&command, &item, None, "other reason", NOW);
    assert!(matches!(conflict, Err(JournalError::Conflict { .. })));
    let (_, after) = journal.snapshot().expect("snapshot");
    assert!(after.attention.iter().all(|row| row.attention_id != item));
    assert_eq!(after.counts.needs_attention, 0);
}

#[test]
fn observation_preference_and_maintenance_phase_survive_reopen() {
    let store = TempStore::new();
    {
        let mut journal = store.open();
        assert!(journal.observation_enabled().expect("read"));
        journal
            .set_observation_enabled(false, NOW)
            .expect("disable");
        journal
            .record_maintenance_phase(
                "PREPARED",
                Some("{\"kind\":\"Stop\"}"),
                serde_json::json!({}),
                NOW,
            )
            .expect("phase");
    }
    let journal = store.open();
    assert!(!journal.observation_enabled().expect("read"));
    let (phase, purpose) = journal.maintenance_phase().expect("phase");
    assert_eq!(phase, "PREPARED");
    assert_eq!(purpose.as_deref(), Some("{\"kind\":\"Stop\"}"));
}

#[test]
fn synthetic_changes_update_one_session_per_slot() {
    let store = TempStore::new();
    let mut journal = store.open();
    let mut last = 0;
    for sequence in 0..20 {
        let change = journal
            .synthetic_change("run", (sequence % 4) as u32, sequence, NOW)
            .expect("change");
        assert!(change.cursor > last, "each change is its own commit");
        last = change.cursor;
    }
    let (cursor, snapshot) = journal.snapshot().expect("snapshot");
    assert_eq!(cursor, last);
    let streams: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|row| row.native_session_id.starts_with("m0c-stream-"))
        .collect();
    assert_eq!(streams.len(), 4);
    assert!(
        streams
            .iter()
            .any(|row| row.display_name.contains("change 19"))
    );
}

#[test]
fn attention_can_be_raised_on_an_existing_session() {
    let store = TempStore::new();
    let mut journal = store.open();
    journal.populate_synthetic(1, 8, NOW).expect("populate");
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let target = snapshot
        .sessions
        .iter()
        .find(|row| row.native_session_id.starts_with("m0c-populate-"))
        .expect("populated session")
        .session_id
        .clone();
    let raised = journal
        .raise_attention_on("probe", Some(&target), NOW)
        .expect("raise");
    assert_eq!(raised.intent.session_id, target);
    let missing = journal.raise_attention_on("probe", Some(&uuid::Uuid::new_v4().to_string()), NOW);
    assert!(matches!(missing, Err(JournalError::NotFound { .. })));
}
