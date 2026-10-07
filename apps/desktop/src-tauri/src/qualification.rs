//! Qualification-only support, compiled only with the `qualification`
//! feature: persisting renderer-produced reports as native evidence, and the
//! unauthorized ACL probe view. Release builds contain none of this.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use threadspace_contracts::ui::{UiError, UiErrorCode};

use crate::launch::LaunchOptions;
use crate::window::navigation_allowed;

const REPORT_MAX_BYTES: usize = 64 * 1024;
pub const PROBE_LABEL: &str = "acl-probe";
const PROBE_TITLE_PREFIX: &str = "ACL-PROBE:";
pub const ORIGIN_PROBE_LABEL: &str = "origin-probe";
const ORIGIN_TITLE_PREFIX: &str = "ORIGIN-PROBE:";
/// Keeps two reports of one kind in the same millisecond from colliding.
static REPORT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

/// Writes `~/Library/Logs/<identifier>/qualification/<kind>-<ms>.json`.
pub fn record<R: Runtime>(
    app: &AppHandle<R>,
    kind: &str,
    report: &Value,
) -> Result<String, UiError> {
    let valid_kind = !kind.is_empty()
        && kind.len() <= 48
        && kind
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid_kind {
        return Err(UiError::invalid("reportKind must be 1-48 of [a-z0-9-]"));
    }
    let recorded_at_ms = now_ms();
    let body = serde_json::to_vec_pretty(&json!({
        "kind": kind,
        "recordedAtMs": recorded_at_ms,
        "appIdentifier": app.config().identifier,
        "executable": std::env::current_exe().ok().map(|path| threadspace_relay::paths::redact_home(&path.display().to_string())),
        "report": report,
    }))
    .map_err(|error| UiError::invalid(error.to_string()))?;
    if body.len() > REPORT_MAX_BYTES {
        return Err(UiError::new(
            UiErrorCode::ReplyTooLarge,
            "report exceeds 64 KiB",
        ));
    }
    let directory = app
        .path()
        .app_log_dir()
        .map_err(|error| UiError::new(UiErrorCode::Internal, error.to_string()))?
        .join("qualification");
    fs::create_dir_all(&directory)
        .map_err(|error| UiError::new(UiErrorCode::Internal, error.to_string()))?;
    let sequence = REPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = format!("{kind}-{recorded_at_ms}-{sequence:06}.json");
    fs::write(directory.join(&file_name), body)
        .map_err(|error| UiError::new(UiErrorCode::Internal, error.to_string()))?;
    Ok(file_name)
}

/// Opens a hidden second view that no capability names. Its renderer tries
/// the app commands and a core command, then reports the refusals through
/// the document title, a channel that needs no IPC permission.
pub fn open_acl_probe<R: Runtime>(app: &AppHandle<R>, launch: &LaunchOptions) -> tauri::Result<()> {
    let handle = app.clone();
    WebviewWindowBuilder::new(app, PROBE_LABEL, WebviewUrl::App("index.html".into()))
        .title("Threadspace ACL probe")
        .inner_size(480.0, 320.0)
        .visible(false)
        .initialization_script(launch.initialization_script(Some("acl")))
        .on_navigation(navigation_allowed)
        .on_document_title_changed(move |window, title| {
            if let Some(payload) = title.strip_prefix(PROBE_TITLE_PREFIX) {
                let value = serde_json::from_str(payload)
                    .unwrap_or_else(|_| Value::String(payload.to_owned()));
                let _ = record(&handle, "acl-probe", &value);
                let _ = window.destroy();
            }
        })
        .build()?;
    Ok(())
}

/// Opens a hidden view on a loopback HTTP page the harness serves. That page
/// tries the bridge commands from a non-local origin and reports the outcome
/// through its title. The view's own navigation is limited to that origin.
pub fn open_origin_probe<R: Runtime>(app: &AppHandle<R>, url: &str) -> tauri::Result<()> {
    let handle = app.clone();
    let parsed: tauri::Url = url
        .parse()
        .map_err(|_| tauri::Error::InvalidWebviewUrl("origin probe URL"))?;
    let origin = parsed.origin().ascii_serialization();
    WebviewWindowBuilder::new(app, ORIGIN_PROBE_LABEL, WebviewUrl::External(parsed))
        .title("Threadspace origin probe")
        .inner_size(480.0, 320.0)
        .visible(false)
        .on_navigation(move |target| target.origin().ascii_serialization() == origin)
        .on_document_title_changed(move |window, title| {
            if let Some(payload) = title.strip_prefix(ORIGIN_TITLE_PREFIX) {
                let value = serde_json::from_str(payload)
                    .unwrap_or_else(|_| Value::String(payload.to_owned()));
                let _ = record(&handle, "origin-probe", &value);
                let _ = window.destroy();
            }
        })
        .build()?;
    Ok(())
}

/// G15 overlap witness (SPEC §18.5, §21.4): holds the office view's next
/// same-scheme response whose path contains an armed needle. The hook runs in
/// Tauri's own `tauri://` handler, on the protocol's async task, after the
/// asset is resolved and before the response is sent, so the WebView's
/// request is genuinely outstanding while it is held. One shot; a held
/// response always resumes after `MAX_RESOURCE_HOLD`.
mod resource_hold {
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};
    use uuid::Uuid;

    pub const MAX_RESOURCE_HOLD: Duration = Duration::from_secs(60);
    const HISTORY: usize = 8;

    pub struct Held {
        pub hold_id: String,
        pub path: String,
        pub incarnation: Uuid,
        pub held_at_ms: u128,
    }

    pub struct State {
        pub armed: Option<String>,
        pub held: Option<Held>,
        pub released: bool,
        pub history: Vec<Value>,
    }

    pub static STATE: Mutex<State> = Mutex::new(State {
        armed: None,
        held: None,
        released: false,
        history: Vec::new(),
    });
    pub static WAKE: Condvar = Condvar::new();

    pub fn status(state: &State) -> Value {
        json!({
            "armed": state.armed,
            "held": state.held.as_ref().map(|held| json!({
                "holdId": held.hold_id,
                "path": held.path,
                "incarnation": held.incarnation,
                "heldAtMs": held.held_at_ms,
            })),
            "history": state.history,
        })
    }

    pub fn remember(state: &mut State, record: Value) {
        if state.history.len() >= HISTORY {
            state.history.remove(0);
        }
        state.history.push(record);
    }

    pub fn elapsed_ms(since: Instant) -> u64 {
        since.elapsed().as_millis() as u64
    }
}

/// Arms, releases or reports the resource hold.
pub fn resource_hold(
    op: threadspace_contracts::ui::ResourceHoldOp,
    path_contains: Option<String>,
) -> Result<Value, UiError> {
    use threadspace_contracts::ui::ResourceHoldOp;
    let mut state = resource_hold::STATE
        .lock()
        .map_err(|_| UiError::new(UiErrorCode::Internal, "resource hold poisoned"))?;
    match op {
        ResourceHoldOp::Arm => {
            let needle = path_contains.unwrap_or_default();
            let valid = (1..=64).contains(&needle.len())
                && needle.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')
                });
            if !valid {
                return Err(UiError::invalid(
                    "pathContains must be 1-64 of [A-Za-z0-9-_./]",
                ));
            }
            if state.held.is_some() {
                return Err(UiError::invalid("a response is already held"));
            }
            state.armed = Some(needle);
        }
        ResourceHoldOp::Release => {
            if state.held.is_none() {
                return Err(UiError::new(UiErrorCode::Conflict, "no response is held"));
            }
            state.released = true;
            resource_hold::WAKE.notify_all();
        }
        ResourceHoldOp::Status => {}
    }
    Ok(resource_hold::status(&state))
}

/// The `on_web_resource_request` hook of one office incarnation.
pub fn hold_resource<R: Runtime>(
    app: &AppHandle<R>,
    bridge: &std::sync::Arc<crate::bridge::Bridge>,
    incarnation: uuid::Uuid,
    path: &str,
) {
    let Ok(mut state) = resource_hold::STATE.lock() else {
        return;
    };
    match &state.armed {
        Some(needle) if path.contains(needle.as_str()) => {}
        _ => return,
    }
    state.armed = None;
    state.released = false;
    let hold_id = uuid::Uuid::new_v4().to_string();
    let held_at_ms = now_ms();
    state.held = Some(resource_hold::Held {
        hold_id: hold_id.clone(),
        path: path.to_owned(),
        incarnation,
        held_at_ms,
    });
    crate::prefs::log(
        app,
        "QUALIFY_RESOURCE_HELD",
        json!({ "holdId": hold_id, "path": path, "incarnation": incarnation, "heldAtMs": held_at_ms, "incarnationActive": bridge.views.is_active(incarnation) }),
    );
    let started = std::time::Instant::now();
    while !state.released && started.elapsed() < resource_hold::MAX_RESOURCE_HOLD {
        let remaining = resource_hold::MAX_RESOURCE_HOLD.saturating_sub(started.elapsed());
        state = match resource_hold::WAKE.wait_timeout(state, remaining) {
            Ok((guard, _)) => guard,
            Err(_) => return,
        };
    }
    let record = json!({
        "holdId": hold_id,
        "path": path,
        "incarnation": incarnation,
        "heldAtMs": held_at_ms,
        "releasedAtMs": now_ms(),
        "heldMs": resource_hold::elapsed_ms(started),
        "releasedBy": if state.released { "COMMAND" } else { "TIMEOUT" },
        // The late response goes back to Tauri's responder after this hook;
        // a retired view's task is no longer valid to receive it.
        "incarnationActiveAtRelease": bridge.views.is_active(incarnation),
    });
    state.held = None;
    state.released = false;
    resource_hold::remember(&mut state, record.clone());
    drop(state);
    crate::prefs::log(app, "QUALIFY_RESOURCE_RELEASED", record);
}
