//! Return-to-Agent in the companion (SPEC §13): the native implementation of
//! the route model's evidence and effects, run on a native-op worker. Routes
//! are serialized so competing focus requests never interleave; every result
//! is journaled with its evidence before it is returned.

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::time::{Duration, Instant};

use serde_json::json;
use threadspace_contracts::control::{ControlError, ControlErrorCode};
use threadspace_contracts::route::{
    AppGeneration, FrontmostApplication, ProcessKey, RouteRequest, RouteResult,
};
use threadspace_journal::RouteTargetRow;
use threadspace_provider_claude::inventory::{
    ClaudeInstall, Inventory, InventoryError, InventorySnapshot,
};
use threadspace_surfaces::{
    BoundTarget, RouteDeadline, RouteNative, SessionTarget, TERMINAL_BUNDLE_ID, route,
};
use threadspace_surfaces_macos::process::{self, Incarnation, ProcessError};
use threadspace_surfaces_macos::terminal::{self, FocusOutcome, TerminalTabs};
use threadspace_surfaces_macos::tty::{self, TtyError};
use uuid::Uuid;

use crate::bridge;
use crate::discovery::{self, DiscoveryContext, Trigger};
use crate::log;
use crate::writer::WriterCommand;

/// Frontmost-application readback: NSWorkspace learns of an activation
/// asynchronously, so the reading is retried briefly, within the route's
/// remaining budget, before it is reported.
const FRONTMOST_SETTLE: Duration = Duration::from_millis(250);
/// Recording a route result happens after its attempt, outside its budget.
const RECORD_TIMEOUT: Duration = Duration::from_secs(5);

static ROUTES: Mutex<()> = Mutex::new(());

struct Native<'a> {
    context: &'a DiscoveryContext,
    install: ClaudeInstall,
}

fn writer_call<T>(
    writer: &SyncSender<WriterCommand>,
    timeout: Duration,
    build: impl FnOnce(Sender<T>) -> WriterCommand,
) -> Option<T> {
    let (reply, answer) = mpsc::channel();
    writer.send(build(reply)).ok()?;
    answer.recv_timeout(timeout).ok()
}

impl RouteNative for Native<'_> {
    fn now_ms(&self) -> i64 {
        log::now_ms()
    }

    fn sample(&self, pid: i32) -> Result<Incarnation, ProcessError> {
        process::sample_incarnation(pid)
    }

    fn inventory(&self, deadline: &RouteDeadline) -> Result<InventorySnapshot, InventoryError> {
        self.context
            .cli(&self.install, deadline.remaining())
            .fetch()
    }

    fn terminal_generation(&self) -> Result<Option<AppGeneration>, String> {
        discovery::terminal_generation()
    }

    fn automation_authorized(&self, deadline: &RouteDeadline) -> Result<bool, String> {
        discovery::automation_authorized(deadline.remaining())
    }

    fn enumerate(&self, deadline: &RouteDeadline) -> Result<TerminalTabs, String> {
        terminal::enumerate(
            &self
                .context
                .resources_dir
                .join("terminal-inventory.applescript"),
            deadline.remaining(),
        )
        .map_err(|error| error.to_string())
    }

    fn device_of(&self, path: &str) -> Result<u32, TtyError> {
        tty::character_device(path)
    }

    fn focus(
        &self,
        tty: &str,
        deadline: &RouteDeadline,
    ) -> Result<(FocusOutcome, u32, u32), String> {
        terminal::focus(
            &self
                .context
                .resources_dir
                .join("terminal-focus.applescript"),
            tty,
            deadline.remaining(),
        )
        .map_err(|error| error.to_string())
    }

    fn frontmost(&self, deadline: &RouteDeadline) -> Option<FrontmostApplication> {
        let settled = Instant::now() + FRONTMOST_SETTLE.min(deadline.remaining());
        loop {
            let front = bridge::frontmost_application(deadline.remaining());
            let is_terminal = front
                .as_ref()
                .and_then(|app| app.bundle_identifier.as_deref())
                == Some(TERMINAL_BUNDLE_ID);
            if is_terminal || Instant::now() >= settled {
                return front;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn binding_revision(&self, binding_id: &str, deadline: &RouteDeadline) -> Option<i64> {
        writer_call(&self.context.writer, deadline.remaining(), |reply| {
            WriterCommand::BindingRevision {
                binding_id: binding_id.to_owned(),
                reply,
            }
        })
        .flatten()
    }

    #[cfg(feature = "qualification")]
    fn reached(
        &self,
        point: threadspace_surfaces::RoutePoint,
        tty: &str,
        deadline: &RouteDeadline,
    ) {
        barrier::pass(point, tty, deadline);
    }
}

fn session_target(row: RouteTargetRow) -> SessionTarget {
    match row {
        RouteTargetRow::NotFound => SessionTarget::NotFound,
        RouteTargetRow::Fixture => SessionTarget::Fixture,
        RouteTargetRow::Unbound {
            native_session_id,
            reason,
        } => SessionTarget::Unbound {
            native_session_id,
            reason,
        },
        RouteTargetRow::Bound {
            native_session_id,
            bindings,
        } => SessionTarget::Bound {
            native_session_id: native_session_id.clone(),
            bindings: bindings
                .into_iter()
                .map(|binding| BoundTarget {
                    binding_id: binding.binding_id,
                    revision: binding.revision,
                    execution_id: binding.execution_id,
                    native_session_id: native_session_id.clone(),
                    process_key: ProcessKey {
                        endpoint_id: binding.endpoint_id,
                        boot_id: binding.boot_id,
                        pid: binding.pid,
                        start_seconds: binding.start_seconds.to_string(),
                        start_microseconds: binding.start_microseconds,
                    },
                    executable: binding.executable,
                    device: binding.device,
                    tty_hint: binding.tty,
                    terminal_generation: binding.terminal_generation,
                })
                .collect(),
        },
    }
}

fn canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

fn bad_request(detail: &str) -> ControlError {
    ControlError::new(ControlErrorCode::BadRequest, detail)
}

/// Runs one Return-to-Agent request. `received` is when the request reached
/// the companion, on the monotonic clock: the attempt's one deadline starts
/// there, so work queued past its budget never moves focus.
pub fn return_to_session(
    request: RouteRequest,
    context: &DiscoveryContext,
    discovery: &SyncSender<Trigger>,
    received: Instant,
) -> Result<RouteResult, ControlError> {
    let deadline = RouteDeadline::for_request(received);
    if !canonical_uuid(&request.request_id) || !canonical_uuid(&request.session_id) {
        return Err(bad_request("request and session IDs must be UUIDs"));
    }
    if request
        .chosen_binding_id
        .as_deref()
        .is_some_and(|id| !canonical_uuid(id))
    {
        return Err(bad_request("chosen binding ID must be a UUID"));
    }
    if request
        .expected_binding_revision
        .as_deref()
        .is_some_and(|revision| revision.parse::<i64>().is_err())
    {
        return Err(bad_request("expected binding revision must be a cursor"));
    }
    let _serialized = ROUTES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(Ok(row)) = writer_call(&context.writer, deadline.remaining(), |reply| {
        WriterCommand::RouteTarget {
            session_id: request.session_id.clone(),
            reply,
        }
    }) else {
        return Err(ControlError::new(
            ControlErrorCode::Unavailable,
            "writer unavailable",
        ));
    };
    let Some(install) = ClaudeInstall::resolve(&context.launcher()) else {
        return Err(ControlError::new(
            ControlErrorCode::Unavailable,
            "Claude CLI not installed",
        ));
    };
    let native = Native { context, install };
    let result = route(&native, &request, &session_target(row), deadline);
    match writer_call(&context.writer, RECORD_TIMEOUT, |reply| {
        WriterCommand::RecordRoute {
            result: Box::new(result.clone()),
            reply,
        }
    }) {
        Some(Ok(_)) => {}
        _ => log::warn(
            "ROUTE_UNRECORDED",
            json!({ "requestId": result.request_id }),
        ),
    }
    log::info(
        "ROUTE_RESULT",
        json!({
            "requestId": result.request_id,
            "sessionId": result.session_id,
            "surfaceResult": result.surface_result,
            "sessionVerification": result.session_verification,
            "inputReadiness": result.input_readiness,
            "reasonCode": result.reason_code,
            "focusPerformed": result.focus_performed,
            "latencyMs": result.latency_ms,
            "focusSenderPid": result.evidence.focus.as_ref().and_then(|focus| focus.sender_pid),
        }),
    );
    if result.reason_code != "OK" {
        // A route conflict is a reconciliation trigger (SPEC §4.14).
        let _ = discovery.try_send(Trigger::Refresh {
            force_surface: false,
            reply: None,
        });
    }
    Ok(result)
}

/// Qualification only (SPEC §21.4 race and deadline witnesses): one armed
/// route holds at a named point until the harness releases it, so a target
/// close or the attempt's deadline can be made to overlap a route in flight.
/// The route's own logic and its deadline are unchanged; only its timing is
/// held. Release builds contain none of this.
#[cfg(feature = "qualification")]
pub mod barrier {
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::json;
    use threadspace_surfaces::{RouteDeadline, RoutePoint};

    use crate::log;

    /// A held route always resumes: a harness that never releases cannot
    /// wedge Return-to-Agent.
    const MAX_HOLD: Duration = Duration::from_secs(20);

    struct State {
        armed: Option<RoutePoint>,
        holding: Option<RoutePoint>,
        released: bool,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        armed: None,
        holding: None,
        released: false,
    });
    static WAKE: Condvar = Condvar::new();

    pub fn arm(point: RoutePoint) {
        if let Ok(mut state) = STATE.lock() {
            state.armed = Some(point);
            log::info("ROUTE_BARRIER_ARMED", json!({ "point": point.code() }));
        }
    }

    /// Releases a held route; false when none is held.
    pub fn release() -> bool {
        let Ok(mut state) = STATE.lock() else {
            return false;
        };
        if state.holding.is_none() {
            return false;
        }
        state.released = true;
        WAKE.notify_all();
        true
    }

    /// Holds the calling route when `point` is armed (one shot), reporting
    /// what remains of its budget when it arrives and when it resumes.
    pub fn pass(point: RoutePoint, tty: &str, deadline: &RouteDeadline) {
        let Ok(mut state) = STATE.lock() else {
            return;
        };
        if state.armed != Some(point) {
            return;
        }
        state.armed = None;
        state.holding = Some(point);
        state.released = false;
        let reached = Instant::now();
        log::info(
            "ROUTE_BARRIER_REACHED",
            json!({
                "point": point.code(),
                "tty": tty,
                "reachedAtMs": log::now_ms(),
                "remainingMs": deadline.remaining().as_millis() as u64,
            }),
        );
        while !state.released && reached.elapsed() < MAX_HOLD {
            let remaining = MAX_HOLD.saturating_sub(reached.elapsed());
            state = match WAKE.wait_timeout(state, remaining) {
                Ok((guard, _)) => guard,
                Err(_) => return,
            };
        }
        let by = if state.released { "COMMAND" } else { "TIMEOUT" };
        state.holding = None;
        state.released = false;
        log::info(
            "ROUTE_BARRIER_RELEASED",
            json!({
                "point": point.code(),
                "releasedBy": by,
                "heldMs": reached.elapsed().as_millis() as u64,
                "releasedAtMs": log::now_ms(),
                "remainingMs": deadline.remaining().as_millis() as u64,
            }),
        );
    }
}
