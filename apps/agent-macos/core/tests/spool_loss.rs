//! Spool loss through the real writer on a disposable store (qualification
//! fixture companion): a drop marker is removed only after the coverage gap
//! it stands for is journaled, so a failed recording loses no evidence. The
//! failure is a trigger in this test's own store. (Its own test binary: the
//! fixture companion is one per process.)

#![cfg(feature = "qualification")]

use std::path::{Path, PathBuf};

use threadspace_agent::fixture;
use threadspace_relay::spool::Spool;

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ts-loss-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn sql(store: &Path, batch: &str) {
    rusqlite::Connection::open(store.join("journal.sqlite3"))
        .and_then(|conn| conn.execute_batch(batch))
        .expect("sql");
}

fn losses(store: &Path) -> i64 {
    rusqlite::Connection::open(store.join("journal.sqlite3"))
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM observations WHERE native_event = 'CAPTURE_LOSS_RECORDED'",
                [],
                |row| row.get(0),
            )
        })
        .expect("count")
}

#[test]
fn a_drop_marker_outlives_a_failed_loss_recording() {
    let store = temp();
    let companion = fixture::start(&store).expect("fixture companion");
    let spool = Spool::at(&store);
    spool.record_drop("00000000-0000-4000-8000-000000000001", "saturated");
    sql(
        &store,
        "CREATE TRIGGER refuse_loss BEFORE INSERT ON observations
         WHEN NEW.native_event = 'CAPTURE_LOSS_RECORDED'
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    );

    companion.drain_spool();
    assert_eq!(losses(&store), 0, "the recording failed");
    assert_eq!(spool.stats().dropped_markers, 1, "the marker is kept until the loss is recorded");

    sql(&store, "DROP TRIGGER refuse_loss;");
    companion.drain_spool();
    assert_eq!(losses(&store), 1, "the loss is recorded once");
    assert_eq!(spool.stats().dropped_markers, 0, "then its marker is removed");
    let _ = std::fs::remove_dir_all(&store);
}
