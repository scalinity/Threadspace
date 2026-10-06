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
