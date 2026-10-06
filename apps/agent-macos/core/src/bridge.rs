//! The thin Apple bridge (SPEC §2.1): Rust asks the Swift AppKit layer for
//! UserNotifications, Apple-event permission, running-application and
//! accessibility facts through one C callback carrying JSON; Swift answers
//! (and reports notification responses) through `ts_core_deliver`.

use std::collections::HashMap;
use std::ffi::{CString, c_char};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use threadspace_contracts::diagnostics::{AccessibilityPreferences, NotificationSettings};
use threadspace_contracts::route::FrontmostApplication;

pub type BridgeCallback = extern "C" fn(*const c_char);

/// Requests from the core to the Apple layer.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum BridgeRequest {
    NotificationSettings {
        correlation_id: u64,
    },
    RequestNotificationAuthorization {
        correlation_id: u64,
    },
    PostNotification {
        correlation_id: u64,
        request_id: String,
        title: String,
        body: String,
        attention_id: String,
        session_id: String,
    },
    AutomationPermission {
        correlation_id: u64,
        bundle_identifier: String,
        ask_user: bool,
    },
    RunningApplication {
        correlation_id: u64,
        bundle_identifier: String,
    },
    AccessibilityPreferences {
        correlation_id: u64,
    },
    /// The native frontmost application (NSWorkspace), for route readback.
    FrontmostApplication {
        correlation_id: u64,
    },
    /// Open (or activate) the containing Threadspace application.
    OpenContainingApp,
}

/// Events from the Apple layer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum BridgeEvent {
    NotificationSettings {
        correlation_id: u64,
        settings: NotificationSettings,
    },
    NotificationAuthorization {
        correlation_id: u64,
        granted: bool,
        settings: NotificationSettings,
    },
    NotificationPosted {
        correlation_id: u64,
        request_id: String,
        error: Option<String>,
    },
    AutomationPermission {
        correlation_id: u64,
        status: i32,
    },
    RunningApplication {
        correlation_id: u64,
        running: bool,
        #[serde(default)]
        instances: u32,
        /// Present only when exactly one instance runs.
        #[serde(default)]
        pid: Option<i32>,
    },
    AccessibilityPreferences {
        correlation_id: u64,
        preferences: AccessibilityPreferences,
    },
    FrontmostApplication {
        correlation_id: u64,
        bundle_identifier: Option<String>,
        pid: i32,
    },
    /// The user interacted with a delivered notification. Only internal IDs.
    NotificationResponse {
        schema: u32,
        request_id: String,
        action_identifier: String,
        attention_id: String,
        session_id: String,
    },
}

impl BridgeEvent {
    fn correlation_id(&self) -> Option<u64> {
        match self {
            Self::NotificationSettings { correlation_id, .. }
            | Self::NotificationAuthorization { correlation_id, .. }
            | Self::NotificationPosted { correlation_id, .. }
            | Self::AutomationPermission { correlation_id, .. }
            | Self::RunningApplication { correlation_id, .. }
            | Self::AccessibilityPreferences { correlation_id, .. }
            | Self::FrontmostApplication { correlation_id, .. } => Some(*correlation_id),
            Self::NotificationResponse { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum BridgeError {
    NotInstalled,
    Timeout,
    Unexpected,
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => f.write_str("native bridge not installed"),
            Self::Timeout => f.write_str("native bridge did not answer in time"),
            Self::Unexpected => f.write_str("native bridge answered with an unexpected event"),
        }
    }
}

static CALLBACK: OnceLock<BridgeCallback> = OnceLock::new();
static NEXT_CORRELATION: AtomicU64 = AtomicU64::new(1);
static PENDING: OnceLock<Mutex<HashMap<u64, Sender<BridgeEvent>>>> = OnceLock::new();

fn pending() -> &'static Mutex<HashMap<u64, Sender<BridgeEvent>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn install(callback: BridgeCallback) {
    let _ = CALLBACK.set(callback);
}

fn send(request: &BridgeRequest) -> Result<(), BridgeError> {
    let callback = CALLBACK.get().ok_or(BridgeError::NotInstalled)?;
    let json = serde_json::to_string(request).map_err(|_| BridgeError::Unexpected)?;
    let text = CString::new(json).map_err(|_| BridgeError::Unexpected)?;
    callback(text.as_ptr());
    Ok(())
}

/// Fire-and-forget request.
pub fn notify(request: &BridgeRequest) {
    let _ = send(request);
}

/// Sends a correlated request and waits for its answer.
pub fn call(
    build: impl FnOnce(u64) -> BridgeRequest,
    timeout: Duration,
) -> Result<BridgeEvent, BridgeError> {
    let correlation_id = NEXT_CORRELATION.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver): (Sender<BridgeEvent>, Receiver<BridgeEvent>) = mpsc::channel();
    if let Ok(mut map) = pending().lock() {
        map.insert(correlation_id, sender);
    }
    let result = send(&build(correlation_id)).and_then(|()| {
        receiver
            .recv_timeout(timeout)
            .map_err(|_| BridgeError::Timeout)
    });
    if let Ok(mut map) = pending().lock() {
        map.remove(&correlation_id);
    }
    result
}

/// Routes a correlated answer to its waiting caller. Returns uncorrelated
/// events (notification responses) for the core to handle.
pub fn route(event: BridgeEvent) -> Option<BridgeEvent> {
    let Some(correlation_id) = event.correlation_id() else {
        return Some(event);
    };
    let sender = pending()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&correlation_id));
    if let Some(sender) = sender {
        let _ = sender.send(event);
    }
    None
}

pub fn notification_settings(timeout: Duration) -> Option<NotificationSettings> {
    match call(
        |correlation_id| BridgeRequest::NotificationSettings { correlation_id },
        timeout,
    ) {
        Ok(BridgeEvent::NotificationSettings { settings, .. }) => Some(settings),
        _ => None,
    }
}

pub fn accessibility_preferences(timeout: Duration) -> Option<AccessibilityPreferences> {
    match call(
        |correlation_id| BridgeRequest::AccessibilityPreferences { correlation_id },
        timeout,
    ) {
        Ok(BridgeEvent::AccessibilityPreferences { preferences, .. }) => Some(preferences),
        _ => None,
    }
}

pub fn application_running(
    bundle_identifier: &str,
    timeout: Duration,
) -> Result<bool, BridgeError> {
    match call(
        |correlation_id| BridgeRequest::RunningApplication {
            correlation_id,
            bundle_identifier: bundle_identifier.to_owned(),
        },
        timeout,
    )? {
        BridgeEvent::RunningApplication { running, .. } => Ok(running),
        _ => Err(BridgeError::Unexpected),
    }
}

/// The PID of the single running instance of an application, `None` when it
/// is not running or more than one instance runs.
pub fn running_application_pid(
    bundle_identifier: &str,
    timeout: Duration,
) -> Result<Option<i32>, BridgeError> {
    match call(
        |correlation_id| BridgeRequest::RunningApplication {
            correlation_id,
            bundle_identifier: bundle_identifier.to_owned(),
        },
        timeout,
    )? {
        BridgeEvent::RunningApplication { pid, .. } => Ok(pid),
        _ => Err(BridgeError::Unexpected),
    }
}

pub fn frontmost_application(timeout: Duration) -> Option<FrontmostApplication> {
    match call(
        |correlation_id| BridgeRequest::FrontmostApplication { correlation_id },
        timeout,
    ) {
        Ok(BridgeEvent::FrontmostApplication {
            bundle_identifier,
            pid,
            ..
        }) => Some(FrontmostApplication {
            bundle_identifier,
            pid,
        }),
        _ => None,
    }
}

pub fn automation_permission(
    bundle_identifier: &str,
    ask_user: bool,
    timeout: Duration,
) -> Result<i32, BridgeError> {
    match call(
        |correlation_id| BridgeRequest::AutomationPermission {
            correlation_id,
            bundle_identifier: bundle_identifier.to_owned(),
            ask_user,
        },
        timeout,
    )? {
        BridgeEvent::AutomationPermission { status, .. } => Ok(status),
        _ => Err(BridgeError::Unexpected),
    }
}
