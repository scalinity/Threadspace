//! Return-to-Agent route model (SPEC §13.1–§13.4) for direct interactive
//! Claude in Ghostty or Terminal.app.
//!
//! `route` revalidates a stored binding against fresh native evidence and
//! focuses only a target proven on every axis:
//!
//! 1. the provider process still has the bound PID, kernel birth, executable
//!    and controlling device (sampled before and after the provider lookup);
//! 2. a fresh provider inventory maps that PID to the requested full session
//!    ID as a direct interactive client;
//! 3. a fresh enumeration of the bound application, bracketed by an
//!    unchanged incarnation (and, for Ghostty, an unchanged executable image
//!    whose dictionary is known), has exactly one tab or terminal whose TTY
//!    `st_rdev` equals `e_tdev` (for Ghostty, the bound native terminal ID);
//! 4. the binding revision is unchanged and the request is still within its
//!    two-second budget immediately before focus.
//!
//! It then selects that tab, reads back the selected TTY and the frontmost
//! application, and repeats the process and provider checks. A failed check
//! never falls back to cwd, recency, frontmost or "only candidate" choices:
//! the default fallback is NONE and the result says why.
//!
//! One [`RouteDeadline`] covers the whole attempt: every bounded native call
//! takes its timeout from what remains of it, and a result reached after it
//! never claims an exact surface or current verification.

use std::time::{Duration, Instant};

use threadspace_contracts::route::{
    AppGeneration, BindingChoice, FocusEvidence, FrontmostApplication, GhosttyApplication,
    GhosttyEvidence, GhosttyFocusEvidence, GhosttyTerminalEvidence, InputReadiness,
    InventoryEvidence, PhaseTiming, ProcessEvidence, ProcessKey, RouteEvidence, RouteRequest,
    RouteResult, SampleEvidence, SessionVerification, SurfaceResult, TerminalEvidence,
    TerminalTabEvidence,
};
use threadspace_provider_claude::inventory::{InventoryError, InventorySnapshot};
use threadspace_surfaces_macos::ghostty::{self, GhosttyInventory, GhosttyTarget};
use threadspace_surfaces_macos::process::{Incarnation, ProcessError};
use threadspace_surfaces_macos::terminal::{FocusOutcome, TerminalTabs};
use threadspace_surfaces_macos::tty::TtyError;

pub const TERMINAL_BUNDLE_ID: &str = "com.apple.Terminal";
pub use threadspace_surfaces_macos::ghostty::GHOSTTY_BUNDLE_ID;
/// Surface kinds of stored bindings.
pub const SURFACE_TERMINAL: &str = "terminal.app";
pub const SURFACE_GHOSTTY: &str = "ghostty";
/// SPEC §13.2: hard per-attempt budget.
pub const ROUTE_BUDGET: Duration = Duration::from_secs(2);
/// Reason code of an attempt that ran out of its budget.
pub const TIMEOUT: &str = "TIMEOUT";

/// The one hard budget of a Return attempt (SPEC §13.2), on the monotonic
/// clock. It is fixed when the request reaches the companion and can only
/// run down: every bounded phase takes its timeout from [`remaining`], so no
/// phase can start an allowance of its own.
///
/// [`remaining`]: RouteDeadline::remaining
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDeadline(Instant);

impl RouteDeadline {
    /// The budget of a request that reached the companion at `received`.
    pub fn for_request(received: Instant) -> Self {
        Self(received + ROUTE_BUDGET)
    }

    /// What is left of the budget; zero once it has run out.
    pub fn remaining(&self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }

    pub fn expired(&self) -> bool {
        self.remaining().is_zero()
    }
}

/// Named points of an attempt where a qualification build can hold a route
/// (SPEC §21.4 race and deadline witnesses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePoint {
    /// The one tab is proven and the binding is current; the budget check
    /// and the focus script have not run.
    BeforeFocus,
    /// The focus script returned; frontmost readback and post-focus
    /// revalidation have not run.
    BeforeReadback,
    /// Post-focus revalidation has its evidence; the final decision has not
    /// been made.
    BeforeDecision,
}

impl RoutePoint {
    pub fn code(self) -> &'static str {
        match self {
            Self::BeforeFocus => "BEFORE_FOCUS",
            Self::BeforeReadback => "BEFORE_READBACK",
            Self::BeforeDecision => "BEFORE_DECISION",
        }
    }
}

/// A stored, valid binding of one live activation to a native surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundTarget {
    /// [`SURFACE_TERMINAL`] or [`SURFACE_GHOSTTY`].
    pub surface_kind: String,
    /// The TTY path for Terminal.app; the native terminal ID for Ghostty.
    pub locator: String,
    pub binding_id: String,
    pub revision: i64,
    pub execution_id: String,
    pub native_session_id: String,
    pub process_key: ProcessKey,
    /// Canonical executable identity proven with the binding.
    pub executable: String,
    /// `e_tdev` proven with the binding.
    pub device: u32,
    /// TTY path when bound: a hint, never what focus is aimed at.
    pub tty_hint: String,
    /// The surface application's incarnation when bound.
    pub terminal_generation: String,
}

/// The same Ghostty process still running the same mapped image.
fn same_image(now: &GhosttyApplication, then: &GhosttyApplication) -> bool {
    now.generation == then.generation && now.image.is_some() && now.image == then.image
}

/// What the store knows about the requested session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    NotFound,
    /// The M0 fixture: a synthetic record with no native surface.
    Fixture,
    /// A known session with no valid live binding (background job, unproven
    /// surface, ended activation).
    Unbound {
        native_session_id: String,
        reason: String,
    },
    Bound {
        native_session_id: String,
        bindings: Vec<BoundTarget>,
    },
}

/// Every native effect and observation the route needs. The companion
/// implements it with kernel calls, the Claude CLI, the Terminal scripts and
/// the AppKit bridge; tests implement it with scripted races. Calls that can
/// wait take the attempt's deadline and wait no longer than what remains.
pub trait RouteNative: Sync {
    /// Wall-clock milliseconds, for evidence only; the budget is `RouteDeadline`.
    fn now_ms(&self) -> i64;
    fn sample(&self, pid: i32) -> Result<Incarnation, ProcessError>;
    fn inventory(&self, deadline: &RouteDeadline) -> Result<InventorySnapshot, InventoryError>;
    /// Terminal.app's incarnation, `None` when it is not running.
    fn terminal_generation(&self) -> Result<Option<AppGeneration>, String>;
    /// Apple-event authorization for Terminal, checked without prompting.
    fn automation_authorized(&self, deadline: &RouteDeadline) -> Result<bool, String>;
    fn enumerate(&self, deadline: &RouteDeadline) -> Result<TerminalTabs, String>;
    fn device_of(&self, tty: &str) -> Result<u32, TtyError>;
    /// Runs the focus script: (outcome, sender PID, elapsed ms). A script
    /// stopped at the deadline is an error and may already have acted.
    fn focus(
        &self,
        tty: &str,
        deadline: &RouteDeadline,
    ) -> Result<(FocusOutcome, u32, u32), String>;
    /// The native frontmost application, given time to settle on `bundle`.
    fn frontmost(&self, bundle: &str, deadline: &RouteDeadline) -> Option<FrontmostApplication>;
    /// The running Ghostty process with this canonical generation; its
    /// `limitation` is `None` only when its dictionary is known and sufficient.
    fn ghostty_app(&self, generation: &str) -> Result<Option<GhosttyApplication>, String>;
    /// Apple-event authorization for Ghostty, checked without prompting.
    fn ghostty_authorized(&self, deadline: &RouteDeadline) -> Result<bool, String>;
    /// Read-only inventory of exactly `app`'s process.
    fn ghostty_enumerate(
        &self,
        app: &GhosttyApplication,
        deadline: &RouteDeadline,
    ) -> Result<GhosttyInventory, String>;
    /// Activates exactly `app`'s process and focuses `target`, with readback:
    /// (outcome, sender PID, elapsed ms). An error may follow a performed focus.
    fn ghostty_focus(
        &self,
        app: &GhosttyApplication,
        target: &GhosttyTarget,
        deadline: &RouteDeadline,
    ) -> Result<(ghostty::FocusOutcome, u32, u32), String>;
    /// The binding's current revision if it is still valid.
    fn binding_revision(&self, binding_id: &str, deadline: &RouteDeadline) -> Option<i64>;
    /// The attempt reached `point`; only qualification builds act on it.
    fn reached(&self, _point: RoutePoint, _tty: &str, _deadline: &RouteDeadline) {}
}

fn evidence_of(incarnation: &Incarnation, at_ms: i64) -> ProcessEvidence {
    let sample = &incarnation.sample;
    ProcessEvidence {
        pid: sample.pid as u32,
        ppid: sample.ppid.max(0) as u32,
        start_seconds: sample.start_seconds.to_string(),
        start_microseconds: sample.start_microseconds,
        executable: incarnation.executable.canonical(),
        comm: sample.comm.clone(),
        controlling_device: sample.controlling_device,
        pgid: sample.pgid,
        tpgid: sample.tpgid,
        status: sample.status,
        sampled_at_ms: at_ms,
    }
}

fn sample_evidence(
    pid: i32,
    result: &Result<Incarnation, ProcessError>,
    at_ms: i64,
) -> SampleEvidence {
    match result {
        Ok(incarnation) => SampleEvidence::Sampled {
            sample: evidence_of(incarnation, at_ms),
        },
        Err(error) => SampleEvidence::Failed {
            pid: pid.max(0) as u32,
            code: error.code().to_owned(),
            sampled_at_ms: at_ms,
        },
    }
}

/// Input readiness from kernel state (SPEC §13.3): the provider's process
/// group must own its terminal's foreground and not be stopped.
pub fn readiness(incarnation: &Incarnation) -> InputReadiness {
    let sample = &incarnation.sample;
    if sample.is_stopped() {
        InputReadiness::BackgroundJob
    } else if sample.is_terminal_foreground() {
        InputReadiness::ForegroundCompatible
    } else if sample.controlling_device.is_some() {
        InputReadiness::BackgroundJob
    } else {
        InputReadiness::Unknown
    }
}

/// Why a sampled process no longer matches its binding, if it does not.
fn process_mismatch(
    target: &BoundTarget,
    result: &Result<Incarnation, ProcessError>,
) -> Option<(&'static str, SessionVerification)> {
    match result {
        Err(ProcessError::Vanished { .. }) => Some(("TARGET_GONE", SessionVerification::Unbound)),
        Err(_) => Some((
            "PROCESS_UNREADABLE",
            SessionVerification::NativeBoundLastKnown,
        )),
        Ok(now) => {
            let key = &target.process_key;
            if now.sample.pid as u32 != key.pid
                || now.sample.start_seconds.to_string() != key.start_seconds
                || now.sample.start_microseconds != key.start_microseconds
            {
                Some(("TARGET_GONE", SessionVerification::Unbound))
            } else if now.executable.canonical() != target.executable {
                Some(("EXECUTABLE_CHANGED", SessionVerification::Conflict))
            } else if now.sample.controlling_device != Some(target.device) {
                Some(("DEVICE_CHANGED", SessionVerification::Conflict))
            } else {
                None
            }
        }
    }
}

/// Whether the inventory currently maps the PID to exactly this session as a
/// direct interactive client.
fn provider_mismatch(
    snapshot: &InventorySnapshot,
    pid: i32,
    session: &str,
) -> Option<(&'static str, SessionVerification)> {
    let rows = snapshot.rows_for_pid(pid);
    match rows.as_slice() {
        [] => Some(("NO_LIVE_MAPPING", SessionVerification::NativeBoundLastKnown)),
        [row] if !row.is_interactive() => Some(("SESSION_CHANGED", SessionVerification::Conflict)),
        [row] if row.full_session_id() == Some(session) => None,
        [_] => Some(("SESSION_CHANGED", SessionVerification::Conflict)),
        _ => Some(("PROVIDER_CONFLICT", SessionVerification::Conflict)),
    }
}

struct Run<'a, N: RouteNative> {
    native: &'a N,
    started_ms: i64,
    deadline: RouteDeadline,
    last_ms: i64,
    evidence: RouteEvidence,
    focus_performed: bool,
}

impl<N: RouteNative> Run<'_, N> {
    fn phase(&mut self, name: &str) {
        let now = self.native.now_ms();
        self.evidence.phases.push(PhaseTiming {
            phase: name.to_owned(),
            elapsed_ms: (now - self.last_ms).max(0) as u32,
        });
        self.last_ms = now;
    }

    /// Every result leaves through here, so the deadline is checked last,
    /// after all evidence is in: a result reached after the deadline claims
    /// neither an exact surface nor current verification, however good its
    /// late evidence is. Effects already issued stay recorded in
    /// `focus_performed` and the focus evidence.
    fn finish(
        self,
        request: &RouteRequest,
        (surface, verification, readiness): (SurfaceResult, SessionVerification, InputReadiness),
        reason: &str,
        target: Option<&BoundTarget>,
        choices: Vec<BindingChoice>,
    ) -> RouteResult {
        let claims = surface == SurfaceResult::ExactNativeSurface
            || verification == SessionVerification::CurrentNativeRevalidated;
        let (surface, verification, readiness, reason) = if claims && self.deadline.expired() {
            Self::timed_out()
        } else {
            (surface, verification, readiness, reason)
        };
        let ended = self.native.now_ms();
        RouteResult {
            request_id: request.request_id.clone(),
            session_id: request.session_id.clone(),
            surface_result: surface,
            session_verification: verification,
            input_readiness: readiness,
            binding_id: target.map(|t| t.binding_id.clone()),
            validated_binding_revision: (surface == SurfaceResult::ExactNativeSurface)
                .then(|| target.map(|t| t.revision.to_string()))
                .flatten(),
            reason_code: reason.to_owned(),
            focus_performed: self.focus_performed,
            started_at_ms: self.started_ms,
            latency_ms: (ended - self.started_ms).max(0) as u32,
            choices,
            evidence: self.evidence,
        }
    }

    /// The typed result of a spent budget: nothing is exact or current, and
    /// readiness is unknown.
    fn timed_out() -> (
        SurfaceResult,
        SessionVerification,
        InputReadiness,
        &'static str,
    ) {
        (
            SurfaceResult::Unavailable,
            SessionVerification::NativeBoundLastKnown,
            InputReadiness::Unknown,
            TIMEOUT,
        )
    }

    fn finish_timed_out(self, request: &RouteRequest, target: &BoundTarget) -> RouteResult {
        let (surface, verification, readiness, reason) = Self::timed_out();
        self.finish(
            request,
            (surface, verification, readiness),
            reason,
            Some(target),
            vec![],
        )
    }
}

fn choices(bindings: &[BoundTarget]) -> Vec<BindingChoice> {
    bindings
        .iter()
        .map(|binding| BindingChoice {
            binding_id: binding.binding_id.clone(),
            revision: binding.revision.to_string(),
            pid: binding.process_key.pid,
            tty: binding.tty_hint.clone(),
        })
        .collect()
}

/// One Return-to-Agent attempt within `deadline`, which started when the
/// owner's request reached the companion: work queued past the budget never
/// moves focus.
pub fn route<N: RouteNative>(
    native: &N,
    request: &RouteRequest,
    target: &SessionTarget,
    deadline: RouteDeadline,
) -> RouteResult {
    use InputReadiness::Unknown as NotReady;
    use SessionVerification as V;
    use SurfaceResult as S;

    let started_ms = native.now_ms();
    let mut run = Run {
        native,
        started_ms,
        deadline,
        last_ms: started_ms,
        evidence: RouteEvidence::default(),
        focus_performed: false,
    };

    // Work queued past its budget, such as a notification Return that waited
    // behind another, does nothing native at all.
    if deadline.expired() {
        let (surface, verification, readiness, reason) = Run::<N>::timed_out();
        return run.finish(
            request,
            (surface, verification, readiness),
            reason,
            None,
            vec![],
        );
    }

    let (native_session_id, bindings) = match target {
        SessionTarget::NotFound => {
            return run.finish(
                request,
                (S::Unavailable, V::Unbound, NotReady),
                "SESSION_NOT_FOUND",
                None,
                vec![],
            );
        }
        SessionTarget::Fixture => {
            return run.finish(
                request,
                (S::InspectorOnly, V::Unbound, NotReady),
                "NO_NATIVE_SURFACE",
                None,
                vec![],
            );
        }
        SessionTarget::Unbound {
            native_session_id,
            reason,
        } => {
            run.evidence.native_session_id = Some(native_session_id.clone());
            return run.finish(
                request,
                (S::InspectorOnly, V::Unbound, NotReady),
                reason,
                None,
                vec![],
            );
        }
        SessionTarget::Bound {
            native_session_id,
            bindings,
        } => (native_session_id, bindings),
    };
    run.evidence.native_session_id = Some(native_session_id.clone());

    let target = match (&request.chosen_binding_id, bindings.as_slice()) {
        (Some(chosen), _) => match bindings.iter().find(|b| &b.binding_id == chosen) {
            Some(binding) => binding,
            None => {
                return run.finish(
                    request,
                    (S::Unavailable, V::Conflict, NotReady),
                    "BINDING_NOT_FOUND",
                    None,
                    choices(bindings),
                );
            }
        },
        (None, [only]) => only,
        (None, []) => {
            return run.finish(
                request,
                (S::InspectorOnly, V::Unbound, NotReady),
                "NO_LIVE_MAPPING",
                None,
                vec![],
            );
        }
        (None, _) => {
            // SPEC §4.2/§13.2: several live attachments require an explicit
            // choice; currentness is never established by picking the newest.
            return run.finish(
                request,
                (S::Ambiguous, V::Unbound, NotReady),
                "MULTIPLE_ATTACHMENTS",
                None,
                choices(bindings),
            );
        }
    };
    run.evidence.binding_id = Some(target.binding_id.clone());
    run.evidence.binding_revision_loaded = Some(target.revision.to_string());
    run.evidence.process_key = Some(target.process_key.clone());
    run.evidence.bound_executable = Some(target.executable.clone());
    run.evidence.bound_device = Some(target.device);

    if let Some(expected) = &request.expected_binding_revision
        && *expected != target.revision.to_string()
    {
        return run.finish(
            request,
            (S::Unavailable, V::Conflict, NotReady),
            "BINDING_STALE",
            Some(target),
            vec![],
        );
    }

    let pid = target.process_key.pid as i32;

    // 1. Incumbent sample.
    let pre = native.sample(pid);
    run.evidence.pre_lookup_sample = Some(sample_evidence(pid, &pre, native.now_ms()));
    run.phase("pre-lookup sample");
    if let Some((reason, verification)) = process_mismatch(target, &pre) {
        return run.finish(
            request,
            (S::Unavailable, verification, NotReady),
            reason,
            Some(target),
            vec![],
        );
    }

    // 2. Fresh provider lookup ∥ fresh enumeration of the bound surface
    //    application, both inside the process bracket.
    let is_ghostty = target.surface_kind == SURFACE_GHOSTTY;
    run.evidence.surface_kind = Some(target.surface_kind.clone());
    let authorized = if is_ghostty {
        native.ghostty_authorized(&deadline)
    } else {
        native.automation_authorized(&deadline)
    };
    let (lookup, terminal, ghostty_scan) = std::thread::scope(|scope| {
        let lookup = scope.spawn(|| native.inventory(&deadline));
        let (terminal, ghostty_scan) = match (&authorized, is_ghostty) {
            (Ok(true), false) => {
                let before = native.terminal_generation();
                let started = native.now_ms();
                let tabs = native.enumerate(&deadline);
                let ended = native.now_ms();
                let after = native.terminal_generation();
                (Some((before, started, tabs, ended, after)), None)
            }
            (Ok(true), true) => (None, Some(scan_ghostty(native, target, &deadline))),
            _ => (None, None),
        };
        let lookup = lookup
            .join()
            .unwrap_or(Err(InventoryError::Spawn("lookup thread panicked".into())));
        (lookup, terminal, ghostty_scan)
    });
    run.phase(if is_ghostty {
        "provider lookup + ghostty enumeration"
    } else {
        "provider lookup + terminal enumeration"
    });

    // 3. Post-lookup sample.
    let post = native.sample(pid);
    run.evidence.post_lookup_sample = Some(sample_evidence(pid, &post, native.now_ms()));
    run.phase("post-lookup sample");

    let lookup = match lookup {
        Ok(snapshot) => {
            run.evidence.lookup = Some(snapshot.evidence_for(pid, native_session_id));
            snapshot
        }
        Err(error) => {
            run.evidence.lookup = Some(InventoryEvidence {
                request_started_ms: 0,
                request_ended_ms: 0,
                binary: String::new(),
                row_count: 0,
                pid_rows: vec![],
                session_rows: vec![],
                error: Some(error.to_string()),
            });
            if let Some((reason, verification)) = process_mismatch(target, &post) {
                return run.finish(
                    request,
                    (S::Unavailable, verification, NotReady),
                    reason,
                    Some(target),
                    vec![],
                );
            }
            return run.finish(
                request,
                (S::InspectorOnly, V::NativeBoundLastKnown, NotReady),
                error.code(),
                Some(target),
                vec![],
            );
        }
    };
    if let Some((reason, verification)) = process_mismatch(target, &post) {
        return run.finish(
            request,
            (S::Unavailable, verification, NotReady),
            reason,
            Some(target),
            vec![],
        );
    }
    if let Some((reason, verification)) = provider_mismatch(&lookup, pid, native_session_id) {
        let surface = if verification == V::Conflict {
            S::Unavailable
        } else {
            S::InspectorOnly
        };
        return run.finish(
            request,
            (surface, verification, NotReady),
            reason,
            Some(target),
            vec![],
        );
    }
    match &authorized {
        Ok(true) => {}
        Ok(false) => {
            return run.finish(
                request,
                (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                "AUTOMATION_DENIED",
                Some(target),
                vec![],
            );
        }
        Err(_) => {
            return run.finish(
                request,
                (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                "AUTOMATION_UNKNOWN",
                Some(target),
                vec![],
            );
        }
    }

    let joined = if let Some(scan) = ghostty_scan {
        match join_ghostty(&mut run, native, target, scan) {
            Ok((app, ghostty_target, tty)) => Joined::Ghostty {
                app,
                target: ghostty_target,
                tty,
            },
            Err((outcome, reason)) => {
                return run.finish(request, outcome, &reason, Some(target), vec![]);
            }
        }
    } else {
        // Terminal surface join.
        let Some((gen_before, enum_started, tabs, enum_ended, gen_after)) = terminal else {
            return run.finish(
                request,
                (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                "AUTOMATION_UNKNOWN",
                Some(target),
                vec![],
            );
        };
        let mut terminal_evidence = TerminalEvidence {
            generation_before: gen_before.clone().ok().flatten(),
            generation_after: gen_after.clone().ok().flatten(),
            started_ms: enum_started,
            ended_ms: enum_ended,
            sender_pid: None,
            window_count: 0,
            tab_count: 0,
            matches: vec![],
            error: None,
        };
        let tabs = match tabs {
            Ok(tabs) => tabs,
            Err(error) => {
                terminal_evidence.error = Some(error);
                run.evidence.terminal = Some(terminal_evidence);
                return run.finish(
                    request,
                    (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                    "TERMINAL_ENUMERATION_FAILED",
                    Some(target),
                    vec![],
                );
            }
        };
        terminal_evidence.sender_pid = Some(tabs.sender_pid);
        terminal_evidence.window_count = tabs.windows;
        terminal_evidence.tab_count = tabs.tabs.len() as u32;
        let generation_ok = match (&gen_before, &gen_after) {
            (Ok(Some(before)), Ok(Some(after))) => {
                before == after && before.canonical() == target.terminal_generation
            }
            _ => false,
        };
        let matches: Vec<TerminalTabEvidence> = tabs
            .tabs
            .iter()
            .filter_map(|tab| {
                let rdev = native.device_of(&tab.tty).ok();
                (rdev == Some(target.device)).then(|| TerminalTabEvidence {
                    window_id: tab.window_id,
                    window_index: tab.window_index,
                    tab_index: tab.tab_index,
                    selected: tab.selected,
                    tty: tab.tty.clone(),
                    rdev,
                })
            })
            .collect();
        terminal_evidence.matches = matches.clone();
        run.evidence.terminal = Some(terminal_evidence);
        run.phase("surface join");
        if !generation_ok {
            return run.finish(
                request,
                (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                "TERMINAL_GENERATION_CHANGED",
                Some(target),
                vec![],
            );
        }
        let terminal_pid = match &gen_before {
            Ok(Some(generation)) => Some(generation.pid),
            _ => None,
        };
        let tty = match matches.as_slice() {
            [] => {
                return run.finish(
                    request,
                    (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                    "NO_MATCHING_TAB",
                    Some(target),
                    vec![],
                );
            }
            [only] => only.tty.clone(),
            _ => {
                return run.finish(
                    request,
                    (S::Ambiguous, V::CurrentNativeRevalidated, NotReady),
                    "MULTIPLE_MATCHING_TABS",
                    Some(target),
                    vec![],
                );
            }
        };
        Joined::Terminal { tty, terminal_pid }
    };
    let surface_tty = match &joined {
        Joined::Terminal { tty, .. } | Joined::Ghostty { tty, .. } => tty.clone(),
    };

    // 4. Binding still current and request still within budget.
    let revision_now = native.binding_revision(&target.binding_id, &deadline);
    run.evidence.binding_revision_before_focus = revision_now.map(|r| r.to_string());
    if revision_now != Some(target.revision) {
        return run.finish(
            request,
            (S::Unavailable, V::Conflict, NotReady),
            "BINDING_STALE",
            Some(target),
            vec![],
        );
    }
    native.reached(RoutePoint::BeforeFocus, &surface_tty, &deadline);
    if deadline.expired() {
        return run.finish_timed_out(request, target);
    }

    // 5. Focus the one proven surface and read back.
    let (readback_ok, frontmost_ok) = match &joined {
        Joined::Ghostty {
            app,
            target: ghostty_target,
            tty,
        } => match focus_ghostty(&mut run, native, target, app, ghostty_target, tty, &deadline) {
            Ok(checks) => checks,
            Err((outcome, reason)) => {
                return run.finish(request, outcome, &reason, Some(target), vec![]);
            }
        },
        Joined::Terminal { tty: _, terminal_pid } => {
            let focus_started = native.now_ms();
            let focus = native.focus(&surface_tty, &deadline);
            run.phase("focus + readback");
            let mut focus_evidence = FocusEvidence {
                outcome: "FAILED".into(),
                sender_pid: None,
                started_ms: focus_started,
                elapsed_ms: 0,
                target_window_id: None,
                target_tab_index: None,
                front_window_id: None,
                readback_tty: None,
                readback_rdev: None,
                target_window_frontmost: None,
                target_tab_selected: None,
                frontmost_application: None,
                error: None,
            };
            let readback = match focus {
                Err(error) => {
                    let denied = error.contains("-1743");
                    focus_evidence.error = Some(error);
                    run.evidence.focus = Some(focus_evidence);
                    // A timed-out or failed script may have acted: uncertain, not success.
                    run.focus_performed = !denied;
                    let reason = if denied {
                        "AUTOMATION_DENIED"
                    } else {
                        "READBACK_FAILED"
                    };
                    return run.finish(
                        request,
                        (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                        reason,
                        Some(target),
                        vec![],
                    );
                }
                Ok((outcome, sender, elapsed)) => {
                    focus_evidence.sender_pid = Some(sender);
                    focus_evidence.elapsed_ms = elapsed;
                    match outcome {
                        FocusOutcome::Gone => {
                            focus_evidence.outcome = "GONE".into();
                            run.evidence.focus = Some(focus_evidence);
                            return run.finish(
                                request,
                                (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                                "TARGET_GONE",
                                Some(target),
                                vec![],
                            );
                        }
                        FocusOutcome::Ambiguous { .. } => {
                            focus_evidence.outcome = "AMBIGUOUS".into();
                            run.evidence.focus = Some(focus_evidence);
                            return run.finish(
                                request,
                                (S::Ambiguous, V::CurrentNativeRevalidated, NotReady),
                                "MULTIPLE_MATCHING_TABS",
                                Some(target),
                                vec![],
                            );
                        }
                        FocusOutcome::Changed => {
                            focus_evidence.outcome = "CHANGED".into();
                            run.evidence.focus = Some(focus_evidence);
                            return run.finish(
                                request,
                                (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
                                "SURFACE_CHANGED",
                                Some(target),
                                vec![],
                            );
                        }
                        FocusOutcome::Focused(readback) => {
                            run.focus_performed = true;
                            focus_evidence.outcome = "FOCUSED".into();
                            readback
                        }
                    }
                }
            };
            native.reached(RoutePoint::BeforeReadback, &surface_tty, &deadline);
            let frontmost = native.frontmost(TERMINAL_BUNDLE_ID, &deadline);
            let readback_rdev = native.device_of(&readback.front_selected_tty).ok();
            focus_evidence.target_window_id = Some(readback.window_id);
            focus_evidence.target_tab_index = Some(readback.tab_index);
            focus_evidence.front_window_id = Some(readback.front_window_id);
            focus_evidence.readback_tty = Some(readback.front_selected_tty.clone());
            focus_evidence.readback_rdev = readback_rdev;
            focus_evidence.target_window_frontmost = Some(readback.target_window_frontmost);
            focus_evidence.target_tab_selected = Some(readback.target_tab_selected);
            focus_evidence.frontmost_application = frontmost.clone();
            run.evidence.focus = Some(focus_evidence);
            run.phase("frontmost + readback stat");
            // Every readback signal must agree: the selected tab's device, the
            // tab's selection, the target as AppleScript's front window, and
            // Terminal's own view that the target is its frontmost window.
            let readback_ok = readback_rdev == Some(target.device)
                && readback.target_tab_selected
                && readback.front_window_id == readback.window_id
                && readback.target_window_frontmost;
            // The frontmost application must be Terminal's proven incarnation,
            // not merely something carrying Terminal's bundle identifier.
            let frontmost_ok = frontmost.as_ref().is_some_and(|app| {
                app.bundle_identifier.as_deref() == Some(TERMINAL_BUNDLE_ID)
                    && Some(app.pid.max(0) as u32) == *terminal_pid
            });
            (readback_ok, frontmost_ok)
        }
    };
    // Focus may have happened; it is recorded above. A spent budget starts
    // no further lookup and cannot become an exact result.
    if deadline.expired() {
        return run.finish_timed_out(request, target);
    }

    // 6. Post-focus revalidation of process, provider and binding.
    let after_focus = native.sample(pid);
    run.evidence.post_focus_sample = Some(sample_evidence(pid, &after_focus, native.now_ms()));
    let post_lookup = native.inventory(&deadline);
    run.evidence.binding_revision_after_focus = native
        .binding_revision(&target.binding_id, &deadline)
        .map(|r| r.to_string());
    run.phase("post-focus revalidation");
    native.reached(RoutePoint::BeforeDecision, &surface_tty, &deadline);

    if !readback_ok {
        return run.finish(
            request,
            (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
            "READBACK_FAILED",
            Some(target),
            vec![],
        );
    }
    if !frontmost_ok {
        return run.finish(
            request,
            (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
            "ACTIVATION_REFUSED",
            Some(target),
            vec![],
        );
    }

    let mut verification = V::CurrentNativeRevalidated;
    let mut reason = "OK";
    if let Some((code, _)) = process_mismatch(target, &after_focus) {
        verification = V::Conflict;
        reason = code;
    }
    match &post_lookup {
        Ok(snapshot) => {
            run.evidence.post_focus_lookup = Some(snapshot.evidence_for(pid, native_session_id));
            if verification == V::CurrentNativeRevalidated
                && let Some((code, tier)) = provider_mismatch(snapshot, pid, native_session_id)
            {
                verification = if tier == V::Conflict {
                    V::Conflict
                } else {
                    V::NativeBoundLastKnown
                };
                reason = code;
            }
        }
        Err(error) => {
            run.evidence.post_focus_lookup = Some(InventoryEvidence {
                request_started_ms: 0,
                request_ended_ms: 0,
                binary: String::new(),
                row_count: 0,
                pid_rows: vec![],
                session_rows: vec![],
                error: Some(error.to_string()),
            });
            if verification == V::CurrentNativeRevalidated {
                verification = V::NativeBoundLastKnown;
                reason = "POST_FOCUS_LOOKUP_FAILED";
            }
        }
    }
    if verification == V::CurrentNativeRevalidated
        && run.evidence.binding_revision_after_focus != Some(target.revision.to_string())
    {
        verification = V::Conflict;
        reason = "BINDING_CHANGED_DURING_ROUTE";
    }
    // Readiness is reported only for a currently verified session.
    let ready = match &after_focus {
        Ok(incarnation) if verification == V::CurrentNativeRevalidated => readiness(incarnation),
        _ => InputReadiness::Unknown,
    };
    run.finish(
        request,
        (S::ExactNativeSurface, verification, ready),
        reason,
        Some(target),
        vec![],
    )
}

/// The one surface a route focuses.
enum Joined {
    Terminal {
        tty: String,
        terminal_pid: Option<u32>,
    },
    Ghostty {
        app: GhosttyApplication,
        target: GhosttyTarget,
        tty: String,
    },
}

/// A typed refusal from a route step; the caller records it.
type Refusal = ((SurfaceResult, SessionVerification, InputReadiness), String);

fn refusal(surface: SurfaceResult, verification: SessionVerification, reason: &str) -> Refusal {
    ((surface, verification, InputReadiness::Unknown), reason.to_owned())
}

/// The bound Ghostty process, its inventory and the process again.
struct GhosttyScan {
    before: Result<Option<GhosttyApplication>, String>,
    started_ms: i64,
    inventory: Result<GhosttyInventory, String>,
    ended_ms: i64,
    after: Result<Option<GhosttyApplication>, String>,
}

fn scan_ghostty<N: RouteNative>(
    native: &N,
    target: &BoundTarget,
    deadline: &RouteDeadline,
) -> GhosttyScan {
    let before = native.ghostty_app(&target.terminal_generation);
    let started_ms = native.now_ms();
    // Only a process whose dictionary is known is ever scripted.
    let inventory = match &before {
        Ok(Some(app)) => match &app.limitation {
            None => native.ghostty_enumerate(app, deadline),
            Some(limitation) => Err(limitation.clone()),
        },
        Ok(None) => Err("GHOSTTY_GENERATION_CHANGED".into()),
        Err(error) => Err(error.clone()),
    };
    let ended_ms = native.now_ms();
    let after = native.ghostty_app(&target.terminal_generation);
    GhosttyScan {
        before,
        started_ms,
        inventory,
        ended_ms,
        after,
    }
}

/// Exactly one terminal of the bound, unchanged Ghostty process has the
/// provider's device, and it is the bound native terminal.
fn join_ghostty<N: RouteNative>(
    run: &mut Run<'_, N>,
    native: &N,
    target: &BoundTarget,
    scan: GhosttyScan,
) -> Result<(GhosttyApplication, GhosttyTarget, String), Refusal> {
    use SessionVerification as V;
    use SurfaceResult as S;
    let mut evidence = GhosttyEvidence {
        application_before: scan.before.as_ref().ok().and_then(Option::as_ref).cloned(),
        application_after: scan.after.as_ref().ok().and_then(Option::as_ref).cloned(),
        started_ms: scan.started_ms,
        ended_ms: scan.ended_ms,
        sender_pid: None,
        window_count: 0,
        tab_count: 0,
        terminal_count: 0,
        matches: vec![],
        error: None,
    };
    let unavailable = |run: &mut Run<'_, N>, mut evidence: GhosttyEvidence, reason: &str| {
        evidence.error = Some(reason.to_owned());
        run.evidence.ghostty = Some(evidence);
        refusal(S::Unavailable, V::CurrentNativeRevalidated, reason)
    };
    let app = match &scan.before {
        Ok(Some(app)) => app.clone(),
        Ok(None) => return Err(unavailable(run, evidence, "GHOSTTY_GENERATION_CHANGED")),
        Err(_) => return Err(unavailable(run, evidence, "GHOSTTY_STATE_UNKNOWN")),
    };
    if let Some(limitation) = app.limitation.clone() {
        return Err(unavailable(run, evidence, &limitation));
    }
    let inventory = match scan.inventory {
        Ok(inventory) => inventory,
        Err(error) => {
            evidence.error = Some(error);
            run.evidence.ghostty = Some(evidence);
            return Err(refusal(
                S::Unavailable,
                V::CurrentNativeRevalidated,
                "GHOSTTY_ENUMERATION_FAILED",
            ));
        }
    };
    evidence.sender_pid = Some(inventory.sender_pid);
    evidence.window_count = inventory.windows.len() as u32;
    evidence.tab_count = inventory.tabs.len() as u32;
    evidence.terminal_count = inventory.terminals.len() as u32;
    evidence.matches = inventory
        .terminals
        .iter()
        .filter_map(|terminal| {
            let tty = terminal.tty.as_deref()?;
            let rdev = native.device_of(tty).ok();
            (rdev == Some(target.device)).then(|| GhosttyTerminalEvidence {
                window_id: terminal.window_id.clone(),
                tab_id: terminal.tab_id.clone(),
                terminal_id: terminal.id.clone(),
                tty: terminal.tty.clone(),
                rdev,
            })
        })
        .collect();
    let matches = evidence.matches.clone();
    run.evidence.ghostty = Some(evidence);
    run.phase("surface join");
    let unchanged = matches!(&scan.after, Ok(Some(after)) if same_image(after, &app))
        && app.generation.canonical() == target.terminal_generation;
    if !unchanged {
        return Err(refusal(
            S::Unavailable,
            V::CurrentNativeRevalidated,
            "GHOSTTY_GENERATION_CHANGED",
        ));
    }
    let only = match matches.as_slice() {
        [] => {
            return Err(refusal(
                S::Unavailable,
                V::CurrentNativeRevalidated,
                "NO_MATCHING_TERMINAL",
            ));
        }
        [only] => only,
        _ => {
            return Err(refusal(
                S::Ambiguous,
                V::CurrentNativeRevalidated,
                "MULTIPLE_MATCHING_TERMINALS",
            ));
        }
    };
    // The device now belongs to another native terminal: the bound one is gone.
    if only.terminal_id != target.locator {
        return Err(refusal(S::Unavailable, V::Conflict, "GHOSTTY_TERMINAL_CHANGED"));
    }
    Ok((
        app,
        GhosttyTarget {
            window_id: only.window_id.clone(),
            tab_id: only.tab_id.clone(),
            terminal_id: only.terminal_id.clone(),
        },
        only.tty.clone().unwrap_or_default(),
    ))
}

/// Activates the proven process, focuses the terminal and reads every level
/// back. Returns (readback agrees, the proven process is frontmost).
fn focus_ghostty<N: RouteNative>(
    run: &mut Run<'_, N>,
    native: &N,
    target: &BoundTarget,
    app: &GhosttyApplication,
    ghostty_target: &GhosttyTarget,
    tty: &str,
    deadline: &RouteDeadline,
) -> Result<(bool, bool), Refusal> {
    use SessionVerification as V;
    use SurfaceResult as S;
    let started = native.now_ms();
    let focus = native.ghostty_focus(app, ghostty_target, deadline);
    run.phase("focus + native readback");
    let mut evidence = GhosttyFocusEvidence {
        outcome: "FAILED".into(),
        sender_pid: None,
        started_ms: started,
        elapsed_ms: 0,
        target: GhosttyTerminalEvidence {
            window_id: ghostty_target.window_id.clone(),
            tab_id: ghostty_target.tab_id.clone(),
            terminal_id: ghostty_target.terminal_id.clone(),
            tty: Some(tty.to_owned()),
            rdev: Some(target.device),
        },
        front_window_id: None,
        selected_tab_id: None,
        focused_terminal_id: None,
        target_tab_selected: None,
        app_frontmost: None,
        script_frontmost_pid: None,
        readback_rdev: None,
        frontmost_application: None,
        application_after: None,
        error: None,
    };
    let readback = match focus {
        Err(error) => {
            let denied = error.contains("-1743");
            evidence.error = Some(error);
            run.evidence.ghostty_focus = Some(evidence);
            // A timed-out or failed script may have acted: uncertain, not success.
            run.focus_performed = !denied;
            let reason = if denied {
                "AUTOMATION_DENIED"
            } else {
                "READBACK_FAILED"
            };
            return Err(refusal(S::Unavailable, V::CurrentNativeRevalidated, reason));
        }
        Ok((outcome, sender, elapsed)) => {
            evidence.sender_pid = Some(sender);
            evidence.elapsed_ms = elapsed;
            match outcome {
                ghostty::FocusOutcome::Focused(readback) => {
                    run.focus_performed = true;
                    evidence.outcome = "FOCUSED".into();
                    readback
                }
                other => {
                    let (outcome, surface, reason) = match other {
                        ghostty::FocusOutcome::Gone => ("GONE", S::Unavailable, "TARGET_GONE"),
                        ghostty::FocusOutcome::Ambiguous { .. } => {
                            ("AMBIGUOUS", S::Ambiguous, "MULTIPLE_MATCHING_TERMINALS")
                        }
                        // CHANGED can follow an issued focus whose readback disagreed.
                        _ => {
                            run.focus_performed = true;
                            ("CHANGED", S::Unavailable, "SURFACE_CHANGED")
                        }
                    };
                    evidence.outcome = outcome.into();
                    run.evidence.ghostty_focus = Some(evidence);
                    return Err(refusal(surface, V::CurrentNativeRevalidated, reason));
                }
            }
        }
    };
    native.reached(RoutePoint::BeforeReadback, tty, deadline);
    let frontmost = native.frontmost(GHOSTTY_BUNDLE_ID, deadline);
    let readback_rdev = native.device_of(tty).ok();
    let after = native.ghostty_app(&target.terminal_generation);
    evidence.front_window_id = Some(readback.front_window_id.clone());
    evidence.selected_tab_id = Some(readback.selected_tab_id.clone());
    evidence.focused_terminal_id = Some(readback.focused_terminal_id.clone());
    evidence.target_tab_selected = Some(readback.target_tab_selected);
    evidence.app_frontmost = readback.app_frontmost;
    evidence.script_frontmost_pid = Some(readback.frontmost_app_pid);
    evidence.readback_rdev = readback_rdev;
    evidence.frontmost_application = frontmost.clone();
    evidence.application_after = after.as_ref().ok().and_then(Option::as_ref).cloned();
    run.evidence.ghostty_focus = Some(evidence);
    run.phase("frontmost + readback stat");
    // `Focused` already required the exact window, tab and terminal at every
    // level and the script's frontmost PID; the process and its image must
    // also be unchanged and the TTY must still be the provider's device.
    let readback_ok = matches!(&after, Ok(Some(now)) if same_image(now, app))
        && readback_rdev == Some(target.device)
        && readback.terminal_id == ghostty_target.terminal_id
        && readback.focused_terminal_id == ghostty_target.terminal_id;
    let pid = app.generation.pid as i32;
    let frontmost_ok = frontmost.as_ref().is_some_and(|front| {
        front.bundle_identifier.as_deref() == Some(GHOSTTY_BUNDLE_ID) && front.pid == pid
    });
    Ok((readback_ok, frontmost_ok))
}

#[cfg(test)]
mod tests;
