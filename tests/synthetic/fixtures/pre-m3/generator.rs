use serde_json::json;
use threadspace_journal::{
    ActivationChange, DiscoveryApplication, Journal, ProcessRecord, SurfaceRecord,
};
fn main() {
    let output = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&output).unwrap();
    let mut journal = Journal::open_with(
        std::path::Path::new(&output)
            .join("journal.sqlite3")
            .as_path(),
        "pre-m3",
        1000,
        Box::new(threadspace_state_engine::ids::SeededAllocator::new(42)),
        false,
    )
    .unwrap();
    let app = DiscoveryApplication {
        provider: "claude".into(),
        profile_ref: "claude-cli:/qualification/.claude".into(),
        sessions: vec![],
        payload: json!({}),
        pass: 1,
        wait_of: |_| None,
        changes: vec![ActivationChange::Start {
            native_session_id: "A".into(),
            process: ProcessRecord {
                boot_id: "boot-A".into(),
                pid: 4100,
                start_seconds: 1000,
                start_microseconds: 0,
                executable: "/opt/claude/versions/2.1.295#file-id".into(),
            },
            device: 77,
            surface: Ok(SurfaceRecord {
                surface_kind: "terminal.app".into(),
                locator: "/dev/ttys077".into(),
                tty: "/dev/ttys077".into(),
                device: 77,
                terminal_generation: "terminal-test".into(),
                window_hint: Some(1),
                tab_hint: Some(1),
                proof: json!({}),
            }),
        }],
    };
    journal.apply_discovery(&app, 1020).unwrap();
    journal.checkpoint("PRE_M3_70eb355", 1030).unwrap();
    std::fs::write(
        format!("{output}/state.json"),
        serde_json::to_vec_pretty(journal.canonical_state()).unwrap(),
    )
    .unwrap();
    assert!(journal.projection_differences().unwrap().is_empty());
    assert_eq!(
        threadspace_state_engine::hash::state_hash(journal.canonical_state()),
        threadspace_state_engine::hash::state_hash(&journal.replay_from_genesis().unwrap())
    );
    drop(journal);
    let conn = rusqlite::Connection::open(format!("{output}/journal.sqlite3")).unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}
