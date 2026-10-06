//! Return-to-Agent route model (SPEC §13.1–§13.3) for direct interactive
//! Claude in Terminal.app.
//!
//! `route` revalidates a stored binding against fresh native evidence and
//! focuses only a target proven on every axis:
//!
//! 1. the provider process still has the bound PID, kernel birth, executable
//!    and controlling device (sampled before and after the provider lookup);
//! 2. a fresh provider inventory maps that PID to the requested full session
//!    ID as a direct interactive client;
//! 3. a fresh Terminal enumeration, bracketed by an unchanged Terminal
//!    incarnation, has exactly one tab whose TTY `st_rdev` equals `e_tdev`;
//! 4. the binding revision is unchanged and the request is still within its
//!    two-second budget immediately before focus.
//!
//! It then selects that tab, reads back the selected TTY and the frontmost
//! application, and repeats the process and provider checks. A failed check
//! never falls back to cwd, recency, frontmost or "only candidate" choices:
//! the default fallback is NONE and the result says why.

use std::time::Duration;

use threadspace_contracts::route::{
    AppGeneration, BindingChoice, FocusEvidence, FrontmostApplication, InputReadiness,
    InventoryEvidence, PhaseTiming, ProcessEvidence, ProcessKey, RouteEvidence, RouteRequest,
    RouteResult, SampleEvidence, SessionVerification, SurfaceResult, TerminalEvidence,
    TerminalTabEvidence,
};
use threadspace_provider_claude::inventory::{InventoryError, InventorySnapshot};
use threadspace_surfaces_macos::process::{Incarnation, ProcessError};
use threadspace_surfaces_macos::terminal::{FocusOutcome, TerminalTabs};
use threadspace_surfaces_macos::tty::TtyError;

pub const TERMINAL_BUNDLE_ID: &str = "com.apple.Terminal";
/// SPEC §13.2: hard per-attempt budget.
pub const ROUTE_BUDGET: Duration = Duration::from_secs(2);

/// A stored, valid binding of one live activation to a Terminal surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundTarget {
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
    /// Terminal.app incarnation when bound.
    pub terminal_generation: String,
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
/// the AppKit bridge; tests implement it with scripted races.
pub trait RouteNative: Sync {
    fn now_ms(&self) -> i64;
    fn sample(&self, pid: i32) -> Result<Incarnation, ProcessError>;
    fn inventory(&self) -> Result<InventorySnapshot, InventoryError>;
    /// Terminal.app's incarnation, `None` when it is not running.
    fn terminal_generation(&self) -> Result<Option<AppGeneration>, String>;
    /// Apple-event authorization for Terminal, checked without prompting.
    fn automation_authorized(&self) -> Result<bool, String>;
    fn enumerate(&self) -> Result<TerminalTabs, String>;
    fn device_of(&self, tty: &str) -> Result<u32, TtyError>;
    /// Runs the focus script: (outcome, sender PID, elapsed ms).
    fn focus(&self, tty: &str) -> Result<(FocusOutcome, u32, u32), String>;
    fn frontmost(&self) -> Option<FrontmostApplication>;
    /// The binding's current revision if it is still valid.
    fn binding_revision(&self, binding_id: &str) -> Option<i64>;
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
    deadline_ms: i64,
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

    fn finish(
        self,
        request: &RouteRequest,
        (surface, verification, readiness): (SurfaceResult, SessionVerification, InputReadiness),
        reason: &str,
        target: Option<&BoundTarget>,
        choices: Vec<BindingChoice>,
    ) -> RouteResult {
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

/// One Return-to-Agent attempt. `received_ms` is when the owner's request
/// reached the companion; work queued past the budget never moves focus.
pub fn route<N: RouteNative>(
    native: &N,
    request: &RouteRequest,
    target: &SessionTarget,
    received_ms: i64,
) -> RouteResult {
    use InputReadiness::Unknown as NotReady;
    use SessionVerification as V;
    use SurfaceResult as S;

    let started_ms = native.now_ms();
    let mut run = Run {
        native,
        started_ms,
        deadline_ms: received_ms + ROUTE_BUDGET.as_millis() as i64,
        last_ms: started_ms,
        evidence: RouteEvidence::default(),
        focus_performed: false,
    };

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

    // 2. Fresh provider lookup ∥ fresh Terminal enumeration, both inside the
    //    process bracket.
    let authorized = native.automation_authorized();
    let (lookup, terminal) = std::thread::scope(|scope| {
        let lookup = scope.spawn(|| native.inventory());
        let terminal = match &authorized {
            Ok(true) => {
                let before = native.terminal_generation();
                let started = native.now_ms();
                let tabs = native.enumerate();
                let ended = native.now_ms();
                let after = native.terminal_generation();
                Some((before, started, tabs, ended, after))
            }
            _ => None,
        };
        let lookup = lookup
            .join()
            .unwrap_or(Err(InventoryError::Spawn("lookup thread panicked".into())));
        (lookup, terminal)
    });
    run.phase("provider lookup + terminal enumeration");

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
    let surface_tty = match matches.as_slice() {
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

    // 4. Binding still current and request still within budget.
    let revision_now = native.binding_revision(&target.binding_id);
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
    if native.now_ms() > run.deadline_ms {
        return run.finish(
            request,
            (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
            "TIMEOUT",
            Some(target),
            vec![],
        );
    }

    // 5. Focus the one proven tab and read back.
    let focus_started = native.now_ms();
    let focus = native.focus(&surface_tty);
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
    let frontmost = native.frontmost();
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

    // 6. Post-focus revalidation of process, provider and binding.
    let after_focus = native.sample(pid);
    run.evidence.post_focus_sample = Some(sample_evidence(pid, &after_focus, native.now_ms()));
    let post_lookup = native.inventory();
    run.evidence.binding_revision_after_focus = native
        .binding_revision(&target.binding_id)
        .map(|r| r.to_string());
    run.phase("post-focus revalidation");

    let readback_ok = readback_rdev == Some(target.device)
        && readback.target_tab_selected
        && readback.front_window_id == readback.window_id;
    if !readback_ok {
        return run.finish(
            request,
            (S::Unavailable, V::CurrentNativeRevalidated, NotReady),
            "READBACK_FAILED",
            Some(target),
            vec![],
        );
    }
    let terminal_frontmost = frontmost
        .as_ref()
        .and_then(|app| app.bundle_identifier.as_deref())
        == Some(TERMINAL_BUNDLE_ID);
    if !terminal_frontmost {
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

#[cfg(test)]
mod tests;
