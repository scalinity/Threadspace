//! An intent the M0 store recorded keeps governing its migrated item: the
//! reducer finds it by the item, not by the name-based ID it gives intents
//! it creates itself. The accepted M0C fixture is edited (in a copy) so its
//! acknowledged item is unacknowledged (column and command) with a PENDING
//! intent.

use std::path::{Path, PathBuf};

use threadspace_contracts::canonical::records::OutboxState;
use threadspace_journal::Journal;

const ITEM: &str = "f14c9f36-e4e4-4bd0-acbd-525a886c0702";
const M0_REQUEST: &str = "6612bb92-bb74-4d12-96bf-39f3fffd4fa9";

fn migrated_store_with_a_pending_intent() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::temp_dir().join(format!("ts-m0-outbox-{}", uuid::Uuid::new_v4()));
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
    rusqlite::Connection::open(&path)
        .and_then(|conn| {
            conn.execute_batch(&format!(
                "UPDATE attention_items SET acknowledged_at_ms = NULL, notification_state = 'PENDING' WHERE id = '{ITEM}';
                 DELETE FROM attention_commands WHERE attention_id = '{ITEM}';
                 UPDATE notification_outbox SET state = 'PENDING' WHERE request_id = '{M0_REQUEST}';"
            ))
        })
        .expect("edit");
    path
}

fn intents(journal: &Journal) -> Vec<(String, OutboxState)> {
    journal
        .canonical_state()
        .outbox
        .values()
        .filter(|o| o.attention_id == ITEM)
        .map(|o| (o.request_id.clone(), o.state))
        .collect()
}

#[test]
fn a_migrated_pending_intent_is_not_duplicated_and_is_suppressed_by_acknowledgement() {
    let path = migrated_store_with_a_pending_intent();
    let mut journal = Journal::open(&path, "test-core", 1_800_000_000_000).expect("open");
    assert_eq!(intents(&journal), vec![(M0_REQUEST.to_owned(), OutboxState::Pending)]);

    journal
        .snooze_attention("cmd-snooze", ITEM, None, 1_800_000_100_000, 1_800_000_000_001)
        .expect("snooze");
    assert_eq!(intents(&journal), vec![(M0_REQUEST.to_owned(), OutboxState::Pending)], "no second intent");

    journal.acknowledge_attention("cmd-ack", ITEM, None, 1_800_000_000_002).expect("acknowledge");
    assert_eq!(intents(&journal), vec![(M0_REQUEST.to_owned(), OutboxState::Suppressed)], "acknowledgement suppresses the M0 intent");
    let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
}
