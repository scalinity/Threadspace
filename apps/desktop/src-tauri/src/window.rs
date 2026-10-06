//! The single `office` window (SPEC §18.8): native traffic lights over an
//! overlay titlebar, an opaque content surface, explicit web drag regions,
//! navigation limited to the application's own assets and no new windows.

use std::sync::Arc;

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::window::Color;
use tauri::{AppHandle, Manager, Runtime, TitleBarStyle, Url, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

use crate::bridge::Bridge;
use crate::incarnation::OfficeIncarnation;
use crate::launch::LaunchOptions;

pub const OFFICE_LABEL: &str = "office";
const DEV_ORIGIN: &str = "http://localhost:1420";

/// Bundled assets in production; only the exact configured dev origin in
/// development. Everything else is cancelled.
pub fn navigation_allowed(url: &Url) -> bool {
    if tauri::is_dev() {
        return url.origin().ascii_serialization() == DEV_ORIGIN;
    }
    url.scheme() == "tauri" && url.host_str() == Some("localhost")
}

pub fn create_office<R: Runtime>(app: &AppHandle<R>, bridge: &Arc<Bridge>, launch: &LaunchOptions) -> tauri::Result<()> {
    let incarnation = Uuid::new_v4();
    let page_bridge = Arc::clone(bridge);
    let window = WebviewWindowBuilder::new(app, OFFICE_LABEL, WebviewUrl::App("index.html".into()))
        .title("Threadspace")
        .inner_size(1280.0, 820.0)
        .min_inner_size(960.0, 640.0)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .background_color(Color(245, 244, 241, 255))
        .initialization_script(launch.initialization_script(None))
        .on_navigation(navigation_allowed)
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_page_load(move |_window, payload| {
            // A new document in this view ends every stream it held, even if
            // the renderer never disconnected (SPEC §18.5).
            if payload.event() == PageLoadEvent::Started {
                page_bridge.retire_incarnation(incarnation);
            }
        })
        .build()?;
    let rid = window.resources_table().add(OfficeIncarnation { id: incarnation });
    bridge.views.activate(incarnation, rid);
    Ok(())
}
