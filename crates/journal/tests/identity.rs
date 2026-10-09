//! M0B persistence of provider identity, activations, bindings and routes,
//! against real on-disk SQLite. Synthetic process/TTY permutations prove the
//! store never lets a reused PID or TTY path inherit an old binding.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use threadspace_contracts::canonical::causal::CausalPoint;
use threadspace_contracts::canonical::fact::{FactPayload, SnapshotRow, WaitCategory, WaitSignal};
use threadspace_contracts::canonical::records::{AttentionScope, ResolutionKind};
use threadspace_contracts::projection::AttentionCategory;
use threadspace_contracts::route::{
    InputReadiness, RouteEvidence, RouteResult, SessionVerification, SurfaceResult,
};
use threadspace_journal::{
    ActivationChange, DiscoveryApplication, Journal, ObservedSessionRecord, ProcessRecord,
    RouteTargetRow, SCHEMA_VERSION, SessionWait, SurfaceRecord,
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
        state: None,
    }
}

/// Passes of this test process, strictly increasing like a companion's.
static PASSES: AtomicU64 = AtomicU64::new(0);

/// A provider-shaped wait mapping: an interactive `waiting` row waits for
/// approval when `waitingFor` names a permission and for input otherwise;
/// a background `blocked` row is a blocked job.
fn wait_of(row: &SnapshotRow) -> Option<SessionWait> {
    let subtype = row.waiting_for.clone().unwrap_or_default();
    let category = match (row.kind.as_deref(), row.status.as_deref(), row.state.as_deref()) {
        (Some("interactive"), Some("waiting"), _) if subtype.contains("permission") => {
            WaitCategory::Approval
        }
        (Some("interactive"), Some("waiting"), _) => WaitCategory::Input,
        (Some("background"), _, Some("blocked")) => WaitCategory::JobBlocked,
        _ => return None,
    };
    Some(SessionWait { category, subtype })
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
        pass: PASSES.fetch_add(1, Ordering::SeqCst) + 1,
        wait_of,
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
fn the_m1_schema_keeps_the_frozen_engine_and_m0a_fixture() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    let diagnostics = journal.sqlite_diagnostics().expect("diagnostics");
    assert_eq!(diagnostics.version, "3.53.4", "D-0001 engine unchanged");
    assert_eq!(diagnostics.schema_version, SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 3, "M1 canonical schema");
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

// ------------------------------------------------- inventory points and waits

fn reporting(id: &str, status: &str, waiting_for: Option<&str>) -> ObservedSessionRecord {
    ObservedSessionRecord {
        status: Some(status.into()),
        waiting_for: waiting_for.map(str::to_owned),
        ..observed(id)
    }
}

fn job(id: &str, state: &str) -> ObservedSessionRecord {
    ObservedSessionRecord {
        kind: Some("background".into()),
        status: None,
        state: Some(state.into()),
        ..observed(id)
    }
}

fn inventory_point(epoch: &str, pass: u64) -> CausalPoint {
    CausalPoint {
        source_id: "claude.inventory".into(),
        source_epoch: epoch.into(),
        order_domain: "inventory".into(),
        sequence: Some(pass.to_string()),
        native_key: None,
        native_predecessor_keys: Vec::new(),
    }
}

/// Applies one pass and returns its pass number.
fn pass(journal: &mut Journal, sessions: Vec<ObservedSessionRecord>, changes: Vec<ActivationChange>) -> u64 {
    let application = application(sessions, changes);
    journal.apply_discovery(&application, NOW).expect("apply");
    application.pass
}

/// Every wait fact journaled so far as (category, signal, subtype, pass),
/// after checking it is session-scoped and ordered.
fn wait_facts(journal: &Journal) -> Vec<(WaitCategory, WaitSignal, Option<String>, String)> {
    let mut out = Vec::new();
    for fact in journal.journal_entries(0).expect("entries").into_iter().flat_map(|e| e.facts) {
        let FactPayload::WaitStateObserved { category, signal, subtype, generation } = fact.payload else {
            continue;
        };
        assert_eq!(generation, None);
        assert!(fact.refs.session_id.is_some(), "the session's wait");
        assert!(
            fact.refs.turn_id.is_none() && fact.refs.actor_id.is_none() && fact.refs.execution_id.is_none(),
            "an inventory wait names no turn, actor or execution: {:?}",
            fact.refs
        );
        let point = fact.causal.expect("an ordered inventory point");
        assert_eq!((point.source_id.as_str(), point.order_domain.as_str()), ("claude.inventory", "inventory"));
        out.push((category, signal, subtype, point.sequence.expect("sequence")));
    }
    out
}

fn input(signal: WaitSignal, subtype: Option<&str>, pass: u64) -> (WaitCategory, WaitSignal, Option<String>, String) {
    (WaitCategory::Input, signal, subtype.map(str::to_owned), pass.to_string())
}

/// The session's wait items as (category, resolved natively, open).
fn wait_items(journal: &Journal, session: &str) -> Vec<(AttentionCategory, bool, bool)> {
    journal
        .canonical_state()
        .attention
        .values()
        .filter(|a| a.session_id == session && matches!(a.scope, AttentionScope::SessionWaitCategory { .. }))
        .map(|a| {
            assert_eq!(a.turn_id, None, "never a turn's item");
            let ended = a.resolutions.iter().any(|c| c.kind == ResolutionKind::WaitEnded);
            (a.category.clone(), ended, !a.resolved())
        })
        .collect()
}

#[test]
fn inventory_facts_carry_the_pass_point_and_kernel_facts_do_not() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "core-generation-1", NOW).expect("open");
    let first = pass(&mut journal, vec![observed("A")], vec![start("A", 11, 1000, "/dev/ttys011", 5)]);
    // B is started before any listing names it; its snapshot is drafted too.
    let second = pass(&mut journal, vec![reporting("A", "busy", None)], vec![start("B", 12, 1001, "/dev/ttys012", 6)]);
    let third = pass(&mut journal, vec![], vec![]);
    let mut snapshots = Vec::new();
    for fact in journal.journal_entries(0).expect("entries").into_iter().flat_map(|e| e.facts) {
        match &fact.payload {
            FactPayload::ProviderSnapshotObserved { present, .. } => {
                let point = fact.causal.clone().expect("a snapshot is ordered");
                snapshots.push((*present, point));
            }
            FactPayload::ProcessObserved { .. }
            | FactPayload::ExecutionAttached { .. }
            | FactPayload::SurfaceBindingRecorded { .. }
            | FactPayload::SurfaceBindingUnproven { .. }
            | FactPayload::ExecutionEnded { .. } => {
                assert_eq!(fact.causal, None, "kernel and binding drafts are unchanged");
            }
            _ => {}
        }
    }
    let epoch = "core-generation-1";
    assert_eq!(
        snapshots,
        vec![
            (true, inventory_point(epoch, first)),
            (true, inventory_point(epoch, second)),
            (true, inventory_point(epoch, second)),
            (false, inventory_point(epoch, third)),
            (false, inventory_point(epoch, third)),
        ]
    );
    let a = session_id(&mut journal, "A");
    let inventory = journal.canonical_state().sessions[&a].inventory.clone().expect("inventory");
    assert_eq!(inventory.point, Some(inventory_point(epoch, third)));
    assert_eq!(inventory.row.and_then(|r| r.status).as_deref(), Some("busy"));
    assert_eq!(journal.live_executions("claude").expect("live").len(), 2, "absence never ends an execution");
}

#[test]
fn entering_waiting_raises_a_session_wait_and_leaving_it_clears() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    pass(&mut journal, vec![observed("A")], vec![]);
    assert!(wait_facts(&journal).is_empty(), "an idle row reports no wait");
    let raised = pass(&mut journal, vec![reporting("A", "waiting", Some("input needed"))], vec![]);
    assert_eq!(wait_facts(&journal), vec![input(WaitSignal::Positive, Some("input needed"), raised)]);
    let a = session_id(&mut journal, "A");
    assert_eq!(wait_items(&journal, &a), vec![(AttentionCategory::InputRequired, false, true)]);
    let scope = journal.canonical_state().waits.values().find(|w| w.session_id == a).expect("scope");
    assert!(scope.turn_id.is_none() && scope.actor_id.is_none() && scope.execution_id.is_none());

    // The same report again changes nothing.
    let again = application(vec![reporting("A", "waiting", Some("input needed"))], vec![]);
    assert!(journal.apply_discovery(&again, NOW).expect("apply").change.is_none());

    let cleared = pass(&mut journal, vec![reporting("A", "busy", None)], vec![]);
    assert_eq!(
        wait_facts(&journal),
        vec![
            input(WaitSignal::Positive, Some("input needed"), raised),
            input(WaitSignal::Cleared, None, cleared),
        ]
    );
    assert_eq!(wait_items(&journal, &a), vec![(AttentionCategory::InputRequired, true, false)]);
}

#[test]
fn absence_from_the_inventory_is_never_a_clear() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    let raised = pass(
        &mut journal,
        vec![reporting("A", "waiting", Some("input needed"))],
        vec![start("A", 11, 1000, "/dev/ttys011", 5)],
    );
    let a = session_id(&mut journal, "A");
    pass(&mut journal, vec![], vec![]);
    assert_eq!(wait_facts(&journal).len(), 1, "absence adds no wait evidence");
    assert_eq!(wait_items(&journal, &a), vec![(AttentionCategory::InputRequired, false, true)]);
    assert_eq!(journal.live_executions("claude").expect("live").len(), 1, "nor ends the execution");
    // Reappearing with the same wait is the same wait.
    pass(&mut journal, vec![reporting("A", "waiting", Some("input needed"))], vec![]);
    assert_eq!(wait_facts(&journal).len(), 1);
    // Only a present row that stopped waiting clears it.
    pass(&mut journal, vec![], vec![]);
    let cleared = pass(&mut journal, vec![reporting("A", "idle", None)], vec![]);
    assert_eq!(
        wait_facts(&journal),
        vec![
            input(WaitSignal::Positive, Some("input needed"), raised),
            input(WaitSignal::Cleared, None, cleared),
        ]
    );
    assert_eq!(wait_items(&journal, &a), vec![(AttentionCategory::InputRequired, true, false)]);
}

#[test]
fn a_changed_wait_clears_its_category_and_background_jobs_map_to_job_blocked() {
    let store = TempStore::new();
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    let a_waits = |text: &str| reporting("A", "waiting", Some(text));
    let p1 = pass(&mut journal, vec![a_waits("input needed"), job("J", "working")], vec![]);
    let p2 = pass(&mut journal, vec![a_waits("permission prompt"), job("J", "blocked")], vec![]);
    // A new detail in the same category is a further witness, not a clear.
    let p3 = pass(&mut journal, vec![a_waits("permission prompt (Bash)"), job("J", "working")], vec![]);
    // A job state after the block is display metadata only.
    pass(&mut journal, vec![a_waits("permission prompt (Bash)"), job("J", "done")], vec![]);
    use WaitCategory::{Approval, Input, JobBlocked};
    use WaitSignal::{Cleared, Positive};
    let text = |s: &str| Some(s.to_owned());
    assert_eq!(
        wait_facts(&journal),
        vec![
            (Input, Positive, text("input needed"), p1.to_string()),
            (Input, Cleared, None, p2.to_string()),
            (Approval, Positive, text("permission prompt"), p2.to_string()),
            (JobBlocked, Positive, None, p2.to_string()),
            (Approval, Positive, text("permission prompt (Bash)"), p3.to_string()),
            (JobBlocked, Cleared, None, p3.to_string()),
        ]
    );
    let a = session_id(&mut journal, "A");
    let j = session_id(&mut journal, "J");
    let mut items = wait_items(&journal, &a);
    items.sort_by_key(|(category, ..)| format!("{category:?}"));
    assert_eq!(
        items,
        vec![(AttentionCategory::ApprovalRequired, false, true), (AttentionCategory::InputRequired, true, false)]
    );
    assert_eq!(wait_items(&journal, &j), vec![(AttentionCategory::Blocked, true, false)]);
    let row = journal.canonical_state().sessions[&j].inventory.clone().and_then(|i| i.row).expect("row");
    assert_eq!(row.state.as_deref(), Some("done"), "the job state stays visible");
    assert!(
        journal.canonical_state().turns.values().all(|t| t.session_id != j && t.session_id != a),
        "no turn, and so no turn outcome, from an inventory row"
    );
}

#[test]
fn rows_stored_before_wait_mapping_are_observed_once() {
    // The M0 fixture's sessions carry inventory rows without a causal point.
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/m1/m0-store-v2/journal.sqlite3");
    let store = TempStore::new();
    rusqlite::Connection::open_with_flags(
        format!("file:{}?mode=ro&immutable=1", fixture.display()),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .expect("open fixture read-only")
    .execute("VACUUM INTO ?1", [store.db().to_string_lossy()])
    .expect("copy fixture");
    let mut journal = Journal::open(&store.db(), "epoch", NOW).expect("open");
    let a = session_id(&mut journal, "A");
    let stored = journal.canonical_state().sessions[&a].inventory.clone().expect("inventory");
    assert_eq!(stored.point, None);
    let fixture_row = stored.row.expect("row");
    let record = ObservedSessionRecord {
        native_session_id: "A".into(),
        kind: fixture_row.kind.clone(),
        display_name: fixture_row.display_name.clone(),
        status: fixture_row.status.clone(),
        waiting_for: fixture_row.waiting_for.clone(),
        state: None,
    };
    let first = pass(&mut journal, vec![record.clone()], vec![]);
    let inventory = journal.canonical_state().sessions[&a].inventory.clone().expect("inventory");
    assert_eq!(inventory.point, Some(inventory_point("epoch", first)), "re-observed with a point");
    assert_eq!(inventory.row, Some(fixture_row));
    let again = application(vec![record], vec![]);
    assert!(journal.apply_discovery(&again, NOW).expect("apply").change.is_none(), "then only on change");
    assert!(wait_facts(&journal).is_empty());
}
