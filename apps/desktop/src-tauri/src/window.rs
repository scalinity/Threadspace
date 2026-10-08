//! The single `office` window (SPEC §18.8): native traffic lights over an
//! overlay titlebar, an opaque content surface, explicit web drag regions,
//! navigation limited to the application's own assets and no new windows.
//!
//! View recovery (SPEC §18.5): a main-document replacement after the initial
//! load, or the retirement of a subscription that may have left unconsumed
//! sent data in the framework's per-view cache, retires the native
//! incarnation and recreates the actual office view. The old view is
//! destroyed and its label removal awaited before the label is reused;
//! bounds and minimized/hidden state carry over; pending native intents stay
//! queued in the companion. Repeated recovery is rate-bounded.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use objc2::msg_send;
use objc2::runtime::AnyObject;
use serde_json::json;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::window::Color;
use tauri::{
    AppHandle, Manager, Runtime, TitleBarStyle, Url, Webview, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use threadspace_contracts::ui::{UiError, UiErrorCode, WindowState};
use uuid::Uuid;

use crate::bridge::Bridge;
use crate::incarnation::OfficeIncarnation;
use crate::launch::LaunchOptions;
use crate::prefs::{self, Bounds};

pub const OFFICE_LABEL: &str = "office";
const DEV_ORIGIN: &str = "http://localhost:1420";
const DEFAULT_WIDTH: f64 = 1280.0;
const DEFAULT_HEIGHT: f64 = 820.0;
/// At most this many recoveries per minute run immediately; more are delayed.
const RECOVERIES_PER_MINUTE: usize = 3;
const REMOVAL_WAIT: Duration = Duration::from_secs(5);

/// Shell-wide window state: the recovery guard and its history.
#[derive(Default)]
pub struct Shell {
    recovering: AtomicBool,
    history: Mutex<VecDeque<Instant>>,
}

impl Shell {
    /// True while a controlled recovery has no office window; last-window
    /// exit is suppressed only then.
    pub fn recovering(&self) -> bool {
        self.recovering.load(Ordering::Acquire)
    }
}

/// Bundled assets in production; only the exact configured dev origin in
/// development. Everything else is cancelled.
pub fn navigation_allowed(url: &Url) -> bool {
    if tauri::is_dev() {
        return url.origin().ascii_serialization() == DEV_ORIGIN;
    }
    url.scheme() == "tauri" && url.host_str() == Some("localhost")
}

/// Persisted bounds, corrected onto a current display's work area.
fn restored_bounds<R: Runtime>(app: &AppHandle<R>) -> Option<Bounds> {
    let saved = prefs::load_bounds(app)?;
    let monitors = app.available_monitors().ok()?;
    let areas: Vec<Bounds> = monitors
        .iter()
        .map(|monitor| {
            let area = monitor.work_area();
            let scale = monitor.scale_factor();
            Bounds {
                x: f64::from(area.position.x) / scale,
                y: f64::from(area.position.y) / scale,
                width: f64::from(area.size.width) / scale,
                height: f64::from(area.size.height) / scale,
            }
        })
        .collect();
    Some(prefs::constrain(saved, &areas))
}

pub fn create_office<R: Runtime>(
    app: &AppHandle<R>,
    bridge: &Arc<Bridge>,
    launch: &LaunchOptions,
) -> tauri::Result<()> {
    let incarnation = Uuid::new_v4();
    let page_bridge = Arc::clone(bridge);
    let page_app = app.clone();
    let page_launch = launch.clone();
    let loaded = Arc::new(AtomicBool::new(false));
    let page_loaded = Arc::clone(&loaded);
    let bounds = restored_bounds(app);
    let mut builder =
        WebviewWindowBuilder::new(app, OFFICE_LABEL, WebviewUrl::App("index.html".into()))
            .title("Threadspace")
            .min_inner_size(960.0, 640.0)
            .title_bar_style(TitleBarStyle::Overlay)
            .hidden_title(true)
            .background_color(Color(245, 244, 241, 255))
            .initialization_script(launch.initialization_script(None))
            .on_navigation(navigation_allowed)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_page_load(move |_window, payload| {
                // The closure captured its own native incarnation; a late
                // callback from a retired view only retires what is already
                // retired.
                match payload.event() {
                    PageLoadEvent::Started => {
                        page_bridge.retire_incarnation(incarnation);
                        if page_loaded.load(Ordering::Acquire)
                            && page_bridge.views.is_active(incarnation)
                        {
                            recover(
                                &page_app,
                                &page_bridge,
                                &page_launch,
                                incarnation,
                                "MAIN_DOCUMENT_REPLACED",
                            );
                        }
                    }
                    PageLoadEvent::Finished => page_loaded.store(true, Ordering::Release),
                }
            });
    #[cfg(feature = "qualification")]
    {
        let hold_app = app.clone();
        let hold_bridge = Arc::clone(bridge);
        builder = builder.on_web_resource_request(move |request, _response| {
            crate::qualification::hold_resource(
                &hold_app,
                &hold_bridge,
                incarnation,
                request.uri().path(),
            );
        });
    }
    builder = match bounds {
        Some(bounds) => builder
            .inner_size(bounds.width, bounds.height)
            .position(bounds.x, bounds.y),
        None => builder.inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT),
    };
    let window = builder.build()?;
    let rid = window
        .resources_table()
        .add(OfficeIncarnation { id: incarnation });
    bridge.views.activate(incarnation, rid);
    let event_bridge = Arc::clone(bridge);
    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Destroyed => {
            // Deactivate first: retiring this view's subscriptions must not
            // be mistaken for a recovery request.
            event_bridge.views.retire(incarnation);
            event_bridge.retire_incarnation(incarnation);
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            if let Ok(state) = native_state(&event_window)
                && state.visible
                && !state.minimized
                && !state.fullscreen
            {
                prefs::save_bounds(
                    event_window.app_handle(),
                    Bounds {
                        x: f64::from(state.x),
                        y: f64::from(state.y),
                        width: f64::from(state.width),
                        height: f64::from(state.height),
                    },
                );
            }
            if matches!(event, WindowEvent::Resized(_)) {
                keep_web_focus(&event_window);
            }
        }
        WindowEvent::Focused(true) => keep_web_focus(&event_window),
        _ => {}
    });
    prefs::log(
        app,
        "OFFICE_VIEW_CREATED",
        json!({ "incarnation": incarnation, "restoredBounds": bounds }),
    );
    Ok(())
}

fn native_state<R: Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<WindowState> {
    let scale = window.scale_factor()?;
    let position = window.outer_position()?.to_logical::<f64>(scale);
    let size = window.inner_size()?.to_logical::<f64>(scale);
    Ok(WindowState {
        visible: window.is_visible()?,
        minimized: window.is_minimized()?,
        fullscreen: window.is_fullscreen()?,
        focused: window.is_focused()?,
        scale_factor_milli: (scale * 1000.0).round() as u32,
        x: position.x.round() as i32,
        y: position.y.round() as i32,
        width: size.width.round().max(0.0) as u32,
        height: size.height.round().max(0.0) as u32,
    })
}

/// The calling view's window, as AppKit reports it.
pub fn state_of<R: Runtime>(webview: &Webview<R>) -> Result<WindowState, UiError> {
    let window = webview
        .app_handle()
        .get_webview_window(webview.label())
        .ok_or_else(|| UiError::new(UiErrorCode::StaleView, "view has no window"))?;
    native_state(&window).map_err(|error| UiError::new(UiErrorCode::Internal, error.to_string()))
}

/// Retires `incarnation` and recreates the office view (SPEC §18.5). Only the
/// active incarnation can be recovered; a second request while recovering,
/// or one for a retired view, is ignored.
pub fn recover<R: Runtime>(
    app: &AppHandle<R>,
    bridge: &Arc<Bridge>,
    launch: &LaunchOptions,
    incarnation: Uuid,
    reason: &'static str,
) {
    let Some(shell) = app
        .try_state::<Arc<Shell>>()
        .map(|state| Arc::clone(&state))
    else {
        return;
    };
    if !bridge.views.is_active(incarnation)
        || shell
            .recovering
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    bridge.views.retire(incarnation);
    bridge.retire_incarnation(incarnation);
    let delay = {
        let mut history = shell.history.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        while history
            .front()
            .is_some_and(|at| now.duration_since(*at) > Duration::from_secs(60))
        {
            history.pop_front();
        }
        history.push_back(now);
        let excess = history.len().saturating_sub(RECOVERIES_PER_MINUTE) as u32;
        Duration::from_secs(u64::from(excess.min(5)) * 5)
    };
    let app = app.clone();
    let bridge = Arc::clone(bridge);
    let launch = launch.clone();
    let _ = thread::Builder::new()
        .name("office-recovery".into())
        .spawn(move || {
            let started = Instant::now();
            thread::sleep(delay);
            let previous = app
                .get_webview_window(OFFICE_LABEL)
                .and_then(|window| native_state(&window).ok());
            if let Some(window) = app.get_webview_window(OFFICE_LABEL) {
                #[cfg(feature = "qualification")]
                shells::track(&window, incarnation);
                free_window_device_on_close(&window);
                let _ = window.destroy();
            }
            let removal = Instant::now();
            while app.get_webview_window(OFFICE_LABEL).is_some() && removal.elapsed() < REMOVAL_WAIT
            {
                thread::sleep(Duration::from_millis(20));
            }
            let removed = app.get_webview_window(OFFICE_LABEL).is_none();
            let created_app = app.clone();
            let (done, wait) = std::sync::mpsc::channel();
            let _ = app.run_on_main_thread(move || {
                let created = if removed {
                    create_office(&created_app, &bridge, &launch).map_err(|error| error.to_string())
                } else {
                    Err("old office view was not removed".to_owned())
                };
                if created.is_ok()
                    && let (Some(state), Some(window)) = (
                        previous.as_ref(),
                        created_app.get_webview_window(OFFICE_LABEL),
                    )
                {
                    if state.minimized {
                        let _ = window.minimize();
                    } else if !state.visible {
                        let _ = window.hide();
                    }
                }
                let _ = done.send(created);
            });
            let created = wait
                .recv_timeout(REMOVAL_WAIT)
                .unwrap_or_else(|_| Err("timeout".into()));
            shell.recovering.store(false, Ordering::Release);
            prefs::log(
                &app,
                "OFFICE_VIEW_RECOVERED",
                json!({
                    "reason": reason,
                    "retiredIncarnation": incarnation,
                    "delayMs": delay.as_millis() as u64,
                    "removed": removed,
                    "created": created.is_ok(),
                    "error": created.err(),
                    "elapsedMs": started.elapsed().as_millis() as u64,
                }),
            );
            #[cfg(feature = "qualification")]
            shells::report(&app, incarnation);
        });
}

/// Marks the retiring office window one-shot, so closing it also frees its
/// window-server window (C-04). tao 0.37 never releases one reference it
/// takes when it creates a window, so a closed office window is never
/// deallocated; without this, every recovery left that window's device
/// behind as an off-screen shell. Only a documented `NSWindow` property
/// changes, on a pointer borrowed for this main-thread call; nothing is
/// retained or released.
fn free_window_device_on_close<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let (done, wait) = std::sync::mpsc::channel();
    let sent = window.with_webview(move |platform| {
        if let Some(webview) = platform.downcast_ref::<tauri_runtime_wry::Webview>() {
            // SAFETY: the live view's window, borrowed on the main thread for
            // this call only.
            if let Some(ns_window) = unsafe { webview.ns_window().cast::<AnyObject>().as_ref() } {
                // SAFETY: NSWindow's `oneShot` setter and `orderOut:`, on the
                // main thread.
                unsafe {
                    let () = msg_send![ns_window, setOneShot: true];
                    let () = msg_send![ns_window, orderOut: std::ptr::null::<AnyObject>()];
                }
            }
        }
        let _ = done.send(());
    });
    if sent.is_ok() {
        let _ = wait.recv_timeout(REMOVAL_WAIT);
    }
}

/// Qualification-only (C-04): weak references to each retired office
/// window's native objects, read back after its recovery to show which of
/// them AppKit has released, beside AppKit's own window list. A weak
/// reference neither retains nor owns; every pointer is borrowed from the
/// live view on the main thread and only for the duration of that call.
#[cfg(feature = "qualification")]
mod shells {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use objc2::rc::{Retained, Weak};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use serde_json::{Value, json};
    use tauri::{AppHandle, Manager, Runtime, WebviewWindow};
    use uuid::Uuid;

    /// Long enough for the closed window's own deferred release.
    const SETTLE: Duration = Duration::from_secs(1);

    struct Retired {
        incarnation: Uuid,
        number: isize,
        /// `retainCount` just before destruction, while the runtime still
        /// holds the window.
        retain_count_live: usize,
        window: Weak<AnyObject>,
        delegate: Option<Weak<AnyObject>>,
        content_view: Option<Weak<AnyObject>>,
        webview: Option<Weak<AnyObject>>,
    }

    thread_local! {
        // Touched on the main thread only.
        static RETIRED: RefCell<Vec<Retired>> = const { RefCell::new(Vec::new()) };
    }

    /// Before destruction: remembers the window, its delegate, its content
    /// view and the web view, weakly.
    pub fn track<R: Runtime>(window: &WebviewWindow<R>, incarnation: Uuid) {
        let (done, wait) = mpsc::channel();
        let sent = window.with_webview(move |platform| {
            if let Some(webview) = platform.downcast_ref::<tauri_runtime_wry::Webview>() {
                remember(incarnation, webview.ns_window(), webview.inner());
            }
            let _ = done.send(());
        });
        if sent.is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        }
    }

    fn remember(incarnation: Uuid, ns_window: *const c_void, webview: *const c_void) {
        // SAFETY: both pointers come from the live view, on the main thread,
        // and are borrowed only for this call.
        let Some(window) = (unsafe { ns_window.cast::<AnyObject>().as_ref() }) else {
            return;
        };
        // SAFETY: as above.
        let webview = unsafe { webview.cast::<AnyObject>().as_ref() };
        // SAFETY: NSWindow getters, on the main thread.
        let number: isize = unsafe { msg_send![window, windowNumber] };
        // SAFETY: as above.
        let retain_count_live: usize = unsafe { msg_send![window, retainCount] };
        // SAFETY: as above.
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![window, delegate] };
        // SAFETY: as above.
        let content_view: Option<Retained<AnyObject>> = unsafe { msg_send![window, contentView] };
        RETIRED.with_borrow_mut(|retired| {
            retired.push(Retired {
                incarnation,
                number,
                retain_count_live,
                window: Weak::new(window),
                delegate: delegate.as_deref().map(Weak::new),
                content_view: content_view.as_deref().map(Weak::new),
                webview: webview.map(Weak::new),
            });
        });
    }

    /// After a recovery: logs `OFFICE_NATIVE_WINDOWS` once its retired
    /// window has had time to be released.
    pub fn report<R: Runtime>(app: &AppHandle<R>, retired_incarnation: Uuid) {
        thread::sleep(SETTLE);
        let logged = app.clone();
        let _ = app.run_on_main_thread(move || {
            let mut detail = snapshot();
            detail["afterRecoveryOf"] = json!(retired_incarnation);
            crate::prefs::log(&logged, "OFFICE_NATIVE_WINDOWS", detail);
        });
    }

    fn alive(weak: Option<&Weak<AnyObject>>) -> Value {
        weak.map_or(Value::Null, |weak| json!(weak.load().is_some()))
    }

    fn snapshot() -> Value {
        let retired: Vec<Value> = RETIRED.with_borrow(|retired| {
            retired
                .iter()
                .map(|r| {
                    let window = r.window.load();
                    // Includes this probe's own reference.
                    // Retain count (including this probe's own reference),
                    // current window number (at most 0 once the window
                    // device is freed) and `oneShot`, while it is alive.
                    let now = window.as_ref().map(|window| {
                        // SAFETY: NSObject/NSWindow getters on a live window,
                        // on the main thread.
                        unsafe {
                            let count: usize = msg_send![&**window, retainCount];
                            let number: isize = msg_send![&**window, windowNumber];
                            let one_shot: bool = msg_send![&**window, isOneShot];
                            (count, number, one_shot)
                        }
                    });
                    json!({
                        "incarnation": r.incarnation,
                        "windowNumber": r.number,
                        "windowAlive": window.is_some(),
                        "windowRetainCountLive": r.retain_count_live,
                        "windowRetainCount": now.map(|n| n.0),
                        "windowNumberNow": now.map(|n| n.1),
                        "oneShot": now.map(|n| n.2),
                        "delegateAlive": alive(r.delegate.as_ref()),
                        "contentViewAlive": alive(r.content_view.as_ref()),
                        "webviewAlive": alive(r.webview.as_ref()),
                    })
                })
                .collect()
        });
        // SAFETY: NSApplication's shared instance and window list, on the
        // main thread.
        let windows: Retained<AnyObject> = unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            msg_send![&app, windows]
        };
        // SAFETY: NSArray count.
        let count: usize = unsafe { msg_send![&windows, count] };
        let app_windows: Vec<Value> = (0..count)
            .map(|index| {
                // SAFETY: an index below the array's count; NSWindow getters.
                let (number, visible, class) = unsafe {
                    let window: Retained<AnyObject> = msg_send![&windows, objectAtIndex: index];
                    let number: isize = msg_send![&window, windowNumber];
                    let visible: bool = msg_send![&window, isVisible];
                    (number, visible, window.class().name().to_string_lossy().into_owned())
                };
                json!({ "number": number, "visible": visible, "class": class })
            })
            .collect();
        json!({ "retired": retired, "appWindows": app_windows })
    }
}

/// Fullscreen and zoom transitions leave the window itself as first
/// responder, so Tab no longer reaches the office. While the window is key,
/// keyboard focus goes back to its web view (first responder only; the
/// window is not raised or activated).
fn keep_web_focus<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    if window.is_focused().unwrap_or(false) {
        let webview: &tauri::Webview<R> = window.as_ref();
        let _ = webview.set_focus();
    }
}
