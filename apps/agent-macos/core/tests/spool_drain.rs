//! The spool drainer through the real writer on a disposable store
//! (qualification fixture companion). A record the journal can never admit
//! is quarantined and the records around it still commit, so one bad record
//! cannot stall capture. The poison is a trigger in this test's own store.

#![cfg(feature = "qualification")]

use std::path::PathBuf;

use threadspace_agent::fixture;
use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_relay::capture::{HookCapture, claude_hook_envelope, local_clock, sample_hook_input};
use threadspace_relay::spool::Spool;

const POISON: &str = "00000000-0000-4000-8000-000000000002";

fn envelope(index: u32) -> ObservationEnvelope {
    claude_hook_envelope(
        &sample_hook_input("SessionStart", &format!("session-{index}")),
        HookCapture {
            observation_id: format!("00000000-0000-4000-8000-{index:012}"),
            profile_ref: "claude-cli:~/.claude".into(),
            clock: local_clock(None, 1, None),
            evidence: Vec::new(),
        },
    )
    .expect("envelope")
}

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ts-drain-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

#[test]
fn a_record_the_journal_refuses_is_quarantined_and_the_rest_commit() {
    let store = temp();
    let companion = fixture::start(&store).expect("fixture companion");
    rusqlite::Connection::open(store.join("journal.sqlite3"))
        .and_then(|conn| {
            conn.execute_batch(&format!(
                "CREATE TRIGGER poison BEFORE INSERT ON observations
                 WHEN NEW.observation_id = '{POISON}'
                 BEGIN SELECT RAISE(ABORT, 'poison'); END;"
            ))
        })
        .expect("poison trigger");
    let spool = Spool::at(&store);
    for index in 1..=3 {
        spool.publish(&envelope(index)).expect("publish");
    }

    let committed = companion.drain_spool();

    let stats = spool.stats();
    assert_eq!(committed, 2, "the records beside the poison commit");
    assert_eq!(stats.ready_records, 0, "nothing is left to stall the next pass");
    assert_eq!(stats.quarantined, 1, "the refused record is kept aside");
    let _ = std::fs::remove_dir_all(&store);
}
