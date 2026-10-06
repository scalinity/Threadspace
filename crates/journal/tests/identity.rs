//! M0B persistence of provider identity, activations, bindings and routes,
//! against real on-disk SQLite. Synthetic process/TTY permutations prove the
//! store never lets a reused PID or TTY path inherit an old binding.

use std::path::PathBuf;

use threadspace_contracts::route::{
    InputReadiness, RouteEvidence, RouteResult, SessionVerification, SurfaceResult,
};
use threadspace_journal::{
    ActivationChange, DiscoveryApplication, Journal, ObservedSessionRecord, ProcessRecord,
    RouteTargetRow, SCHEMA_VERSION, SurfaceRecord,
};

struct TempStore(PathBuf);

impl TempStore {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("threadspace-identity-{}", uuid::Uuid::new_v4()));
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

const NOW: i64 = 1_791_000_000_000;
const EXE: &str = "/h/.local/share/claude/versions/2.1.291#16777234:42";
const TERMINAL: &str = "95390@1791000000.000001";

fn process(pid: u32, birth: u64) -> ProcessRecord {
    ProcessRecord {
        boot_id: "boot-1".into(),
        pid,
        start_seconds: birth,
        start_microseconds: 7,
        executable: EXE.into(),
    }
}

fn surface(tty: &str, device: u32) -> SurfaceRecord {
    SurfaceRecord {
        tty: tty.into(),
        device,
        terminal_generation: TERMINAL.into(),
        window_hint: 7508,
        tab_hint: 1,
        proof: serde_json::json!({ "matches": 1 }),
    }
}

fn observed(id: &str) -> ObservedSessionRecord {
    ObservedSessionRecord {
        native_session_id: id.into(),
        kind: Some("interactive".into()),
        display_name: Some(format!("label {id}")),
        status: Some("idle".into()),
        waiting_for: None,
    }
}

fn application(
    sessions: Vec<ObservedSessionRecord>,
    changes: Vec<ActivationChange>,
) -> DiscoveryApplication {
    DiscoveryApplication {
        provider: "claude".into(),
        profile_ref: "claude-cli:~/.claude".into(),
        sessions,
        changes,
        payload: serde_json::json!({ "test": true }),
    }
}

fn start(id: &str, pid: u32, birth: u64, tty: &str, device: u32) -> ActivationChange {
    ActivationChange::Start {
        native_session_id: id.into(),
        process: process(pid, birth),
        device,
        surface: Ok(surface(tty, device)),
    }
}

fn session_id(journal: &mut Journal, native: &str) -> String {
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    snapshot
        .sessions
        .iter()
        .find(|s| s.native_session_id == native)
        .map(|s| s.session_id.clone())
        .expect("session present")
}

fn bindings(journal: &mut Journal, session: &str) -> Vec<threadspace_journal::BindingRow> {
    match journal.route_target(session).expect("target") {
        RouteTargetRow::Bound { bindings, .. } => bindings,
        _ => Vec::new(),
    }
}

#[test]
fn migration_two_keeps_the_frozen_engine_and_m0a_fixture() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    let diagnostics = journal.sqlite_diagnostics().expect("diagnostics");
    assert_eq!(diagnostics.version, "3.53.4", "D-0001 engine unchanged");
    assert_eq!(diagnostics.schema_version, SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 2);
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    assert_eq!(snapshot.sessions.len(), 1, "M0A fixture retained");
    let fixture = snapshot.sessions[0].session_id.clone();
    assert_eq!(
        journal.route_target(&fixture).expect("target"),
        RouteTargetRow::Fixture
    );
}

#[test]
fn three_same_cwd_sessions_are_three_sessions_with_independent_bindings() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    let outcome = journal
        .apply_discovery(
            &application(
                vec![observed("A"), observed("B"), observed("C")],
                vec![
                    start("A", 11, 1000, "/dev/ttys011", 0x1000011),
                    start("B", 12, 1001, "/dev/ttys012", 0x1000012),
                    start("C", 13, 1002, "/dev/ttys013", 0x1000013),
                ],
            ),
            NOW,
        )
        .expect("apply");
    assert_eq!(outcome.executions_started, 3);
    assert_eq!(outcome.bindings_recorded, 3);
    for (native, device) in [("A", 0x1000011), ("B", 0x1000012), ("C", 0x1000013)] {
        let id = session_id(&mut journal, native);
        let list = bindings(&mut journal, &id);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].device, device);
        assert_eq!(
            journal.binding_revision(&list[0].binding_id).expect("rev"),
            Some(list[0].revision)
        );
    }
    // Re-applying the same pass changes nothing and commits nothing.
    let again = journal
        .apply_discovery(
            &application(
                vec![observed("A"), observed("B"), observed("C")],
                vec![start("A", 11, 1000, "/dev/ttys011", 0x1000011)],
            ),
            NOW + 1,
        )
        .expect("apply again");
    assert!(again.change.is_none(), "idempotent: {again:?}");
}

#[test]
fn same_pid_with_a_new_birth_cannot_inherit_the_old_binding() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![start("A", 11, 1000, "/dev/ttys011", 5)],
            ),
            NOW,
        )
        .expect("apply");
    let a = session_id(&mut journal, "A");
    let old = bindings(&mut journal, &a);
    let live = journal.live_executions("claude").expect("live");
    assert_eq!(live.len(), 1);

    // PID 11 exited and was reused by a new process hosting session B.
    journal
        .apply_discovery(
            &application(
                vec![observed("B")],
                vec![
                    ActivationChange::End {
                        execution_id: live[0].execution_id.clone(),
                        reason: "PROCESS_EXITED".into(),
                    },
                    start("B", 11, 2000, "/dev/ttys011", 5),
                ],
            ),
            NOW + 10,
        )
        .expect("apply");
    assert!(
        bindings(&mut journal, &a).is_empty(),
        "A keeps no live binding"
    );
    assert_eq!(
        journal.binding_revision(&old[0].binding_id).expect("rev"),
        None
    );
    let b = session_id(&mut journal, "B");
    let new = bindings(&mut journal, &b);
    assert_eq!(new.len(), 1);
    assert_ne!(new[0].binding_id, old[0].binding_id);
    assert_eq!(new[0].start_seconds, 2000);
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let view = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id == a)
        .expect("A persists");
    assert_eq!(view.last_invalidation.as_deref(), Some("PROCESS_EXITED"));
    assert_eq!(view.live_bindings, 0);
}

#[test]
fn a_reused_tty_path_does_not_resurrect_an_invalidated_binding() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![start("A", 11, 1000, "/dev/ttys011", 5)],
            ),
            NOW,
        )
        .expect("apply");
    let live = journal.live_executions("claude").expect("live");
    journal
        .apply_discovery(
            &application(
                vec![],
                vec![ActivationChange::End {
                    execution_id: live[0].execution_id.clone(),
                    reason: "PROCESS_EXITED".into(),
                }],
            ),
            NOW + 1,
        )
        .expect("end");
    // A later surface outcome naming the same TTY for the ended activation
    // is ignored: only live activations can be bound.
    let ignored = journal
        .apply_discovery(
            &application(
                vec![],
                vec![ActivationChange::Surface {
                    execution_id: live[0].execution_id.clone(),
                    surface: Ok(surface("/dev/ttys011", 5)),
                }],
            ),
            NOW + 2,
        )
        .expect("apply");
    assert_eq!(ignored.bindings_recorded, 0);
    let a = session_id(&mut journal, "A");
    assert!(matches!(
        journal.route_target(&a).expect("target"),
        RouteTargetRow::Unbound { .. }
    ));
}

#[test]
fn a_delayed_old_end_closes_only_its_own_activation_across_a_to_b_to_a() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    // A in process 11.
    journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![start("A", 11, 1000, "/dev/ttys011", 5)],
            ),
            NOW,
        )
        .expect("A1");
    let a1 = journal.live_executions("claude").expect("live")[0]
        .execution_id
        .clone();
    // A→B in place.
    journal
        .apply_discovery(
            &application(
                vec![observed("B")],
                vec![
                    ActivationChange::End {
                        execution_id: a1.clone(),
                        reason: "SESSION_SWITCHED".into(),
                    },
                    start("B", 11, 1000, "/dev/ttys011", 5),
                ],
            ),
            NOW + 1,
        )
        .expect("A→B");
    let b1 = journal.live_executions("claude").expect("live")[0]
        .execution_id
        .clone();
    // B→A in place: a second activation of A.
    journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![
                    ActivationChange::End {
                        execution_id: b1,
                        reason: "SESSION_SWITCHED".into(),
                    },
                    start("A", 11, 1000, "/dev/ttys011", 5),
                ],
            ),
            NOW + 2,
        )
        .expect("B→A");
    let live = journal.live_executions("claude").expect("live");
    assert_eq!(live.len(), 1);
    let a2 = live[0].execution_id.clone();
    assert_ne!(a1, a2);
    let a = session_id(&mut journal, "A");
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let view = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id == a)
        .expect("A");
    assert_eq!(
        view.activation.as_deref(),
        Some("2"),
        "A's second activation"
    );

    // A delayed end for A's first activation arrives late: it must not close
    // the second one.
    let late = journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![ActivationChange::End {
                    execution_id: a1,
                    reason: "PROCESS_EXITED".into(),
                }],
            ),
            NOW + 3,
        )
        .expect("late end");
    assert_eq!(late.executions_ended, 0);
    assert_eq!(
        bindings(&mut journal, &a).len(),
        1,
        "A's live binding survives"
    );
}

#[test]
fn one_session_with_two_live_attachments_keeps_both_bindings() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![
                    start("A", 11, 1000, "/dev/ttys011", 5),
                    start("A", 12, 1001, "/dev/ttys012", 6),
                ],
            ),
            NOW,
        )
        .expect("apply");
    let a = session_id(&mut journal, "A");
    assert_eq!(bindings(&mut journal, &a).len(), 2);
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let view = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id == a)
        .expect("A");
    assert_eq!(view.live_bindings, 2, "multiple-attachments badge");
}

#[test]
fn sessions_leaving_the_inventory_persist_as_stale() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    journal
        .apply_discovery(&application(vec![observed("A")], vec![]), NOW)
        .expect("apply");
    let a = session_id(&mut journal, "A");
    journal
        .apply_discovery(&application(vec![], vec![]), NOW + 1)
        .expect("apply");
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let view = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id == a)
        .expect("A persists");
    assert_eq!(
        serde_json::to_value(&view.observation).expect("json"),
        "STALE"
    );
}

#[test]
fn route_results_are_journaled_with_their_evidence() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    journal
        .apply_discovery(
            &application(
                vec![observed("A")],
                vec![start("A", 11, 1000, "/dev/ttys011", 5)],
            ),
            NOW,
        )
        .expect("apply");
    let a = session_id(&mut journal, "A");
    let result = RouteResult {
        request_id: "6f1b3c2e-4a5d-4e6f-8a9b-0c1d2e3f4a5b".into(),
        session_id: a.clone(),
        surface_result: SurfaceResult::ExactNativeSurface,
        session_verification: SessionVerification::CurrentNativeRevalidated,
        input_readiness: InputReadiness::ForegroundCompatible,
        binding_id: None,
        validated_binding_revision: None,
        reason_code: "OK".into(),
        focus_performed: true,
        started_at_ms: NOW,
        latency_ms: 512,
        choices: vec![],
        evidence: RouteEvidence {
            native_session_id: Some("A".into()),
            ..RouteEvidence::default()
        },
    };
    let change = journal.record_route(&result, NOW + 600).expect("record");
    assert_eq!(change.session_ids, vec![a.clone()]);
    let (_, snapshot) = journal.snapshot().expect("snapshot");
    let view = snapshot
        .sessions
        .iter()
        .find(|s| s.session_id == a)
        .expect("A");
    let last = view.last_route.as_ref().expect("last route");
    assert_eq!(last.surface_result, SurfaceResult::ExactNativeSurface);
    assert_eq!(last.latency_ms, 512);
    let stored = journal
        .route_evidence(&result.request_id)
        .expect("evidence")
        .expect("present");
    assert!(stored.contains("\"nativeSessionId\":\"A\""));
}
