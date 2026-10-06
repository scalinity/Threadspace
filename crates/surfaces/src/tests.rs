//! Synthetic route races (SPEC §21.3). No scenario may focus an unrelated
//! surface: every negative case asserts that no focus event was sent.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use threadspace_provider_claude::inventory::InventoryRow;
use threadspace_surfaces_macos::process::{ExecutableIdentity, ProcessSample};
use threadspace_surfaces_macos::terminal::{FocusReadback, TerminalTab};

use super::*;

const PID: i32 = 501;
const DEVICE: u32 = 0x1000005;
const TTY: &str = "/dev/ttys005";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";
const EXE: &str = "/h/.local/share/claude/versions/2.1.291";

fn incarnation(pid: i32, birth: u64, device: Option<u32>) -> Incarnation {
    Incarnation {
        sample: ProcessSample {
            pid,
            ppid: 400,
            uid: 501,
            start_seconds: birth,
            start_microseconds: 7,
            controlling_device: device,
            pgid: pid as u32,
            tpgid: pid as u32,
            status: 3,
            comm: "claude".into(),
        },
        executable: ExecutableIdentity {
            path: EXE.into(),
            file_id: Some("1:2".into()),
        },
    }
}

fn row(pid: i64, session: &str) -> InventoryRow {
    InventoryRow {
        kind: Some("interactive".into()),
        pid: Some(pid),
        session_id: Some(session.into()),
        status: Some("idle".into()),
        ..InventoryRow::default()
    }
}

fn snapshot(rows: Vec<InventoryRow>) -> InventorySnapshot {
    InventorySnapshot {
        rows,
        request_started_ms: 10,
        request_ended_ms: 20,
        binary: EXE.into(),
    }
}

fn generation(pid: u32) -> AppGeneration {
    AppGeneration {
        pid,
        start_seconds: "50".into(),
        start_microseconds: 1,
    }
}

fn tab(window: i64, index: i64, tty: &str) -> TerminalTab {
    TerminalTab {
        window_id: window,
        window_index: 1,
        window_miniaturized: false,
        tab_index: index,
        selected: false,
        tty: tty.into(),
    }
}

fn tabs(list: Vec<TerminalTab>) -> TerminalTabs {
    TerminalTabs {
        windows: 1,
        tabs: list,
        sender_pid: 9001,
        elapsed_ms: 5,
    }
}

/// Pops scripted values in order; the last one repeats.
struct Seq<T: Clone>(Mutex<VecDeque<T>>);

impl<T: Clone> Seq<T> {
    fn of(values: Vec<T>) -> Self {
        Self(Mutex::new(values.into()))
    }

    fn next(&self) -> T {
        let mut queue = self.0.lock().expect("lock");
        if queue.len() > 1 {
            queue.pop_front().expect("value")
        } else {
            queue.front().cloned().expect("scripted value")
        }
    }
}

struct Mock {
    clock: AtomicI64,
    step_ms: i64,
    samples: Mutex<HashMap<i32, Seq<Result<Incarnation, ProcessError>>>>,
    inventory: Seq<Result<InventorySnapshot, InventoryError>>,
    generations: Seq<Result<Option<AppGeneration>, String>>,
    authorized: Result<bool, String>,
    tabs: Seq<Result<TerminalTabs, String>>,
    devices: HashMap<String, u32>,
    focus_result: Option<Result<(FocusOutcome, u32, u32), String>>,
    focus_calls: Mutex<Vec<String>>,
    frontmost: Option<FrontmostApplication>,
    revisions: Seq<Option<i64>>,
}

impl Mock {
    /// A healthy session in one Terminal tab.
    fn healthy() -> Self {
        Self {
            clock: AtomicI64::new(1_000),
            step_ms: 1,
            samples: Mutex::new(HashMap::from([(
                PID,
                Seq::of(vec![Ok(incarnation(PID, 1000, Some(DEVICE)))]),
            )])),
            inventory: Seq::of(vec![Ok(snapshot(vec![row(i64::from(PID), SESSION)]))]),
            generations: Seq::of(vec![Ok(Some(generation(300)))]),
            authorized: Ok(true),
            tabs: Seq::of(vec![Ok(tabs(vec![
                tab(77, 1, "/dev/ttys004"),
                tab(77, 2, TTY),
            ]))]),
            devices: HashMap::from([
                ("/dev/ttys004".to_owned(), 0x1000004),
                (TTY.to_owned(), DEVICE),
            ]),
            focus_result: None,
            focus_calls: Mutex::new(Vec::new()),
            frontmost: Some(FrontmostApplication {
                bundle_identifier: Some(TERMINAL_BUNDLE_ID.into()),
                pid: 300,
            }),
            revisions: Seq::of(vec![Some(42)]),
        }
    }

    fn with_samples(self, pid: i32, samples: Vec<Result<Incarnation, ProcessError>>) -> Self {
        self.samples
            .lock()
            .expect("lock")
            .insert(pid, Seq::of(samples));
        self
    }

    fn focus_count(&self) -> usize {
        self.focus_calls.lock().expect("lock").len()
    }
}

impl RouteNative for Mock {
    fn now_ms(&self) -> i64 {
        self.clock.fetch_add(self.step_ms, Ordering::SeqCst)
    }
    fn sample(&self, pid: i32) -> Result<Incarnation, ProcessError> {
        self.samples
            .lock()
            .expect("lock")
            .get(&pid)
            .map_or(Err(ProcessError::Vanished { pid }), Seq::next)
    }
    fn inventory(&self) -> Result<InventorySnapshot, InventoryError> {
        self.inventory.next()
    }
    fn terminal_generation(&self) -> Result<Option<AppGeneration>, String> {
        self.generations.next()
    }
    fn automation_authorized(&self) -> Result<bool, String> {
        self.authorized.clone()
    }
    fn enumerate(&self) -> Result<TerminalTabs, String> {
        self.tabs.next()
    }
    fn device_of(&self, tty: &str) -> Result<u32, TtyError> {
        self.devices
            .get(tty)
            .copied()
            .ok_or(TtyError::Stat { errno: 2 })
    }
    fn focus(&self, tty: &str) -> Result<(FocusOutcome, u32, u32), String> {
        self.focus_calls.lock().expect("lock").push(tty.to_owned());
        self.focus_result.clone().unwrap_or_else(|| {
            let window = self
                .tabs
                .next()
                .ok()
                .and_then(|t| t.tabs.into_iter().find(|tab| tab.tty == tty))
                .map_or(0, |tab| tab.window_id);
            Ok((
                FocusOutcome::Focused(FocusReadback {
                    window_id: window,
                    tab_index: 2,
                    front_window_id: window,
                    front_selected_tty: tty.to_owned(),
                    target_window_frontmost: true,
                    target_tab_selected: true,
                }),
                9002,
                40,
            ))
        })
    }
    fn frontmost(&self) -> Option<FrontmostApplication> {
        self.frontmost.clone()
    }
    fn binding_revision(&self, _binding_id: &str) -> Option<i64> {
        self.revisions.next()
    }
}

fn bound(pid: i32, binding: &str) -> BoundTarget {
    BoundTarget {
        binding_id: binding.into(),
        revision: 42,
        execution_id: "exec-1".into(),
        native_session_id: SESSION.into(),
        process_key: ProcessKey {
            endpoint_id: "endpoint".into(),
            boot_id: "boot".into(),
            pid: pid as u32,
            start_seconds: "1000".into(),
            start_microseconds: 7,
        },
        executable: incarnation(pid, 1000, None).executable.canonical(),
        device: DEVICE,
        tty_hint: "/dev/ttys999".into(),
        terminal_generation: generation(300).canonical(),
    }
}

fn target(bindings: Vec<BoundTarget>) -> SessionTarget {
    SessionTarget::Bound {
        native_session_id: SESSION.into(),
        bindings,
    }
}

fn request() -> RouteRequest {
    RouteRequest {
        request_id: "req-1".into(),
        session_id: "session-1".into(),
        chosen_binding_id: None,
        expected_binding_revision: None,
    }
}

fn go(mock: &Mock, target: &SessionTarget) -> RouteResult {
    route(mock, &request(), target, 1_000)
}

fn refused(result: &RouteResult, mock: &Mock, reason: &str) {
    assert_eq!(result.reason_code, reason, "{result:#?}");
    assert!(
        !result.focus_performed,
        "{reason}: no focus may be performed"
    );
    assert_eq!(
        mock.focus_count(),
        0,
        "{reason}: no focus event may be sent"
    );
    assert_ne!(result.surface_result, SurfaceResult::ExactNativeSurface);
}

#[test]
fn a_proven_target_is_focused_exactly_once_and_reads_back() {
    let mock = Mock::healthy();
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "OK", "{result:#?}");
    assert_eq!(result.surface_result, SurfaceResult::ExactNativeSurface);
    assert_eq!(
        result.session_verification,
        SessionVerification::CurrentNativeRevalidated
    );
    assert_eq!(result.input_readiness, InputReadiness::ForegroundCompatible);
    assert_eq!(result.validated_binding_revision.as_deref(), Some("42"));
    // Focus aimed at the freshly enumerated tab, not the stored hint.
    assert_eq!(
        *mock.focus_calls.lock().expect("lock"),
        vec![TTY.to_owned()]
    );
    let evidence = &result.evidence;
    assert!(evidence.lookup.is_some() && evidence.post_focus_lookup.is_some());
    assert_eq!(evidence.terminal.as_ref().map(|t| t.matches.len()), Some(1));
    let focus = evidence.focus.as_ref().expect("focus evidence");
    assert_eq!(focus.readback_rdev, Some(DEVICE));
    assert_eq!(focus.sender_pid, Some(9002));
}

#[test]
fn a_moved_tab_is_found_by_fresh_enumeration() {
    let mut mock = Mock::healthy();
    // The tab moved to another window and position; hints are stale.
    mock.tabs = Seq::of(vec![Ok(tabs(vec![
        tab(88, 3, TTY),
        tab(77, 1, "/dev/ttys004"),
    ]))]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "OK");
    let matched = &result.evidence.terminal.as_ref().expect("terminal").matches[0];
    assert_eq!((matched.window_id, matched.tab_index), (88, 3));
}

#[test]
fn same_pid_with_a_different_birth_cannot_use_the_old_binding() {
    let mock = Mock::healthy().with_samples(PID, vec![Ok(incarnation(PID, 2000, Some(DEVICE)))]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "TARGET_GONE");
    assert_eq!(result.session_verification, SessionVerification::Unbound);
}

#[test]
fn a_reused_tty_path_cannot_revive_a_binding_whose_process_ended() {
    // The bound process is gone, while a new tab now has the same TTY path
    // and therefore the same st_rdev.
    let mock = Mock::healthy().with_samples(PID, vec![Err(ProcessError::Vanished { pid: PID })]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "TARGET_GONE");
}

#[test]
fn executable_replacement_is_refused() {
    let mut replaced = incarnation(PID, 1000, Some(DEVICE));
    replaced.executable.path = "/bin/zsh".into();
    let mock = Mock::healthy().with_samples(PID, vec![Ok(replaced)]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "EXECUTABLE_CHANGED");
    assert_eq!(result.session_verification, SessionVerification::Conflict);
}

#[test]
fn a_process_replaced_between_bracket_samples_is_refused() {
    let mock = Mock::healthy().with_samples(
        PID,
        vec![
            Ok(incarnation(PID, 1000, Some(DEVICE))),
            Ok(incarnation(PID, 1000, Some(0x1000099))),
        ],
    );
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "DEVICE_CHANGED");
}

#[test]
fn a_provider_session_change_is_a_conflict_not_a_route() {
    let mut mock = Mock::healthy();
    mock.inventory = Seq::of(vec![Ok(snapshot(vec![row(
        i64::from(PID),
        "another-session",
    )]))]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "SESSION_CHANGED");
    assert_eq!(result.session_verification, SessionVerification::Conflict);
}

#[test]
fn a_missing_or_failed_provider_lookup_never_becomes_current() {
    let mut mock = Mock::healthy();
    mock.inventory = Seq::of(vec![Ok(snapshot(vec![]))]);
    let missing = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&missing, &mock, "NO_LIVE_MAPPING");
    assert_eq!(missing.surface_result, SurfaceResult::InspectorOnly);
    assert_eq!(
        missing.session_verification,
        SessionVerification::NativeBoundLastKnown
    );

    let mut mock = Mock::healthy();
    mock.inventory = Seq::of(vec![Err(InventoryError::Failed {
        status: None,
        timed_out: true,
        stderr: String::new(),
    })]);
    let failed = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&failed, &mock, "INVENTORY_TIMEOUT");
}

#[test]
fn several_eligible_tabs_are_ambiguous() {
    let mut mock = Mock::healthy();
    mock.tabs = Seq::of(vec![Ok(tabs(vec![
        tab(77, 1, TTY),
        tab(78, 1, "/dev/ttys005b"),
    ]))]);
    mock.devices.insert("/dev/ttys005b".into(), DEVICE);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "MULTIPLE_MATCHING_TABS");
    assert_eq!(result.surface_result, SurfaceResult::Ambiguous);
}

#[test]
fn no_matching_tab_is_unavailable() {
    let mut mock = Mock::healthy();
    mock.tabs = Seq::of(vec![Ok(tabs(vec![tab(77, 1, "/dev/ttys004")]))]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "NO_MATCHING_TAB");
}

#[test]
fn a_stale_terminal_generation_is_refused() {
    let mut mock = Mock::healthy();
    mock.generations = Seq::of(vec![Ok(Some(generation(301)))]);
    let restarted = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&restarted, &mock, "TERMINAL_GENERATION_CHANGED");

    let mut mock = Mock::healthy();
    mock.generations = Seq::of(vec![Ok(Some(generation(300))), Ok(Some(generation(302)))]);
    let during = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&during, &mock, "TERMINAL_GENERATION_CHANGED");
}

#[test]
fn a_binding_revised_before_focus_is_refused() {
    let mut mock = Mock::healthy();
    mock.revisions = Seq::of(vec![Some(43)]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&result, &mock, "BINDING_STALE");

    let mut invalidated = Mock::healthy();
    invalidated.revisions = Seq::of(vec![None]);
    let result = go(&invalidated, &target(vec![bound(PID, "b1")]));
    refused(&result, &invalidated, "BINDING_STALE");
}

#[test]
fn an_old_queued_request_never_moves_focus() {
    // The owner looked at revision 41; the binding is now 42.
    let mock = Mock::healthy();
    let mut stale = request();
    stale.expected_binding_revision = Some("41".into());
    let result = route(&mock, &stale, &target(vec![bound(PID, "b1")]), 1_000);
    refused(&result, &mock, "BINDING_STALE");

    // The request waited past its two-second budget before reaching focus.
    let mock = Mock::healthy();
    let result = route(&mock, &request(), &target(vec![bound(PID, "b1")]), -5_000);
    refused(&result, &mock, "TIMEOUT");
}

#[test]
fn multiple_attachments_require_an_explicit_choice() {
    let mock = Mock::healthy();
    let both = target(vec![bound(PID, "b1"), bound(PID + 1, "b2")]);
    let result = go(&mock, &both);
    refused(&result, &mock, "MULTIPLE_ATTACHMENTS");
    assert_eq!(result.surface_result, SurfaceResult::Ambiguous);
    assert_eq!(result.choices.len(), 2);

    let mock = Mock::healthy();
    let mut chosen = request();
    chosen.chosen_binding_id = Some("b1".into());
    let routed = route(&mock, &chosen, &both, 1_000);
    assert_eq!(routed.reason_code, "OK");
    assert_eq!(routed.binding_id.as_deref(), Some("b1"));
}

#[test]
fn readback_must_name_the_target_device() {
    let mut mock = Mock::healthy();
    mock.focus_result = Some(Ok((
        FocusOutcome::Focused(FocusReadback {
            window_id: 77,
            tab_index: 2,
            front_window_id: 77,
            front_selected_tty: "/dev/ttys004".into(),
            target_window_frontmost: true,
            target_tab_selected: true,
        }),
        9002,
        40,
    )));
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "READBACK_FAILED");
    assert!(
        result.focus_performed,
        "focus was attempted and is reported"
    );
    assert_ne!(result.surface_result, SurfaceResult::ExactNativeSurface);
}

#[test]
fn a_target_window_terminal_does_not_call_frontmost_is_not_exact() {
    // Observed natively: the target was AppleScript's front window and its
    // tab was selected, but Terminal reported it was not the frontmost
    // window while the owner clicked elsewhere.
    let mut mock = Mock::healthy();
    mock.focus_result = Some(Ok((
        FocusOutcome::Focused(FocusReadback {
            window_id: 77,
            tab_index: 2,
            front_window_id: 77,
            front_selected_tty: TTY.into(),
            target_window_frontmost: false,
            target_tab_selected: true,
        }),
        9002,
        40,
    )));
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "READBACK_FAILED");
    assert!(result.focus_performed);
    assert_ne!(result.surface_result, SurfaceResult::ExactNativeSurface);
}

#[test]
fn terminal_not_frontmost_after_activation_is_not_exact() {
    let mut mock = Mock::healthy();
    mock.frontmost = Some(FrontmostApplication {
        bundle_identifier: Some("ai.scalinity.threadspace".into()),
        pid: 1,
    });
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "ACTIVATION_REFUSED");
    assert_ne!(result.surface_result, SurfaceResult::ExactNativeSurface);
}

#[test]
fn a_frontmost_process_other_than_the_terminal_incarnation_is_not_exact() {
    let mut mock = Mock::healthy();
    // Something else carrying Terminal's bundle identifier is frontmost.
    mock.frontmost = Some(FrontmostApplication {
        bundle_identifier: Some(TERMINAL_BUNDLE_ID.into()),
        pid: 35621,
    });
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "ACTIVATION_REFUSED");
    assert_ne!(result.surface_result, SurfaceResult::ExactNativeSurface);
}

#[test]
fn a_session_change_seen_after_focus_downgrades_verification() {
    let mut mock = Mock::healthy();
    mock.inventory = Seq::of(vec![
        Ok(snapshot(vec![row(i64::from(PID), SESSION)])),
        Ok(snapshot(vec![row(i64::from(PID), "switched")])),
    ]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.surface_result, SurfaceResult::ExactNativeSurface);
    assert_eq!(result.session_verification, SessionVerification::Conflict);
    assert_eq!(result.reason_code, "SESSION_CHANGED");
    assert_eq!(result.input_readiness, InputReadiness::Unknown);
}

#[test]
fn a_stopped_or_backgrounded_provider_is_not_foreground_compatible() {
    let mut stopped = incarnation(PID, 1000, Some(DEVICE));
    stopped.sample.status = threadspace_surfaces_macos::process::SSTOP;
    let mock = Mock::healthy().with_samples(PID, vec![Ok(stopped)]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.reason_code, "OK");
    assert_eq!(result.input_readiness, InputReadiness::BackgroundJob);

    let mut background = incarnation(PID, 1000, Some(DEVICE));
    background.sample.tpgid = 999;
    let mock = Mock::healthy().with_samples(PID, vec![Ok(background)]);
    let result = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(result.input_readiness, InputReadiness::BackgroundJob);
}

#[test]
fn automation_denial_and_script_errors_are_explicit() {
    let mut mock = Mock::healthy();
    mock.authorized = Ok(false);
    let denied = go(&mock, &target(vec![bound(PID, "b1")]));
    refused(&denied, &mock, "AUTOMATION_DENIED");

    let mut mock = Mock::healthy();
    mock.focus_result = Some(Err("execution error: Not authorized (-1743)".into()));
    let refused_at_focus = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(refused_at_focus.reason_code, "AUTOMATION_DENIED");
    assert!(!refused_at_focus.focus_performed);

    let mut mock = Mock::healthy();
    mock.focus_result = Some(Ok((FocusOutcome::Gone, 9002, 10)));
    let gone = go(&mock, &target(vec![bound(PID, "b1")]));
    assert_eq!(gone.reason_code, "TARGET_GONE");
    assert!(!gone.focus_performed);
}

#[test]
fn unbound_fixture_and_unknown_sessions_open_the_inspector_only() {
    let mock = Mock::healthy();
    for (target, reason, surface) in [
        (
            SessionTarget::Fixture,
            "NO_NATIVE_SURFACE",
            SurfaceResult::InspectorOnly,
        ),
        (
            SessionTarget::Unbound {
                native_session_id: "bg".into(),
                reason: "UNSUPPORTED_KIND".into(),
            },
            "UNSUPPORTED_KIND",
            SurfaceResult::InspectorOnly,
        ),
        (
            SessionTarget::NotFound,
            "SESSION_NOT_FOUND",
            SurfaceResult::Unavailable,
        ),
    ] {
        let result = go(&mock, &target);
        refused(&result, &mock, reason);
        assert_eq!(result.surface_result, surface);
    }
}
