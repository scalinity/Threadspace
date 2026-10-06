//! Threadspace.app: the Tauri 3 desktop shell (SPEC §2.1, §18). It owns the
//! office window, the renderer bridge and the outer ServiceManagement
//! bootstrap. It is never the journal writer; quitting it does not stop the
//! independently supervised companion.

mod bootstrap;
pub mod bridge;
mod commands;
mod diagnostics;
mod incarnation;
mod launch;
#[cfg(feature = "qualification")]
mod qualification;
mod window;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{Manager, RunEvent};
use threadspace_contracts::limits::HEARTBEAT_INTERVAL_MS;
use threadspace_surfaces_macos::process;

use crate::bridge::Bridge;
use crate::launch::LaunchOptions;

pub const EXIT_UNSUPPORTED_OS: i32 = 78;

/// Process entry. Returns an exit status for the native bootstrap path; the
/// GUI path runs the Tauri event loop until the application quits.
pub fn main_entry() -> i32 {
    let launch = LaunchOptions::parse(std::env::args().skip(1));
    // The deployment floor is enforced at runtime as well as in the bundle.
    if !process::meets_minimum_macos() {
        eprintln!("Threadspace requires macOS 26 or later.");
        return EXIT_UNSUPPORTED_OS;
    }
    let context = tauri::generate_context!();
    let app_identifier = context.config().identifier.clone();
    if let Some(command) = launch.service {
        return bootstrap::run_cli(command, &app_identifier);
    }

    let bridge = Arc::new(Bridge::new(app_identifier));
    let setup_bridge = Arc::clone(&bridge);
    let setup_launch = launch.clone();
    let app = tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        .manage(Arc::clone(&bridge))
        .invoke_handler(tauri::generate_handler![
            commands::ui_connect,
            commands::ui_ack,
            commands::ui_disconnect,
            commands::ui_query,
            commands::ui_action
        ])
        .setup(move |app| {
            window::create_office(app.handle(), &setup_bridge, &setup_launch)?;
            #[cfg(feature = "qualification")]
            if setup_launch.qualify_acl_probe {
                qualification::open_acl_probe(app.handle(), &setup_launch)?;
            }
            let heartbeat = Arc::downgrade(&setup_bridge);
            thread::Builder::new().name("bridge-heartbeat".into()).spawn(move || {
                while let Some(bridge) = heartbeat.upgrade() {
                    bridge.heartbeat_tick();
                    drop(bridge);
                    thread::sleep(Duration::from_millis(u64::from(HEARTBEAT_INTERVAL_MS)));
                }
            })?;
            Ok(())
        })
        .build(context)
        .expect("Threadspace runtime failed");

    let reopen_bridge = Arc::clone(&bridge);
    app.run(move |handle, event| {
        if let RunEvent::Reopen { has_visible_windows: false, .. } = event {
            match handle.get_webview_window(window::OFFICE_LABEL) {
                Some(office) => {
                    let _ = office.unminimize();
                    let _ = office.show();
                    let _ = office.set_focus();
                }
                None => {
                    if let Err(error) = window::create_office(handle, &reopen_bridge, &launch) {
                        eprintln!("threadspace: could not reopen office: {error}");
                    }
                }
            }
        }
    });
    0
}
