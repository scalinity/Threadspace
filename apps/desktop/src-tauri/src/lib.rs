//! Threadspace.app: the Tauri 3 desktop shell (SPEC §2.1, §18). It owns the
//! office window, the renderer bridge and the outer ServiceManagement
//! bootstrap. It is never the journal writer; quitting it does not stop the
//! independently supervised companion.

mod bootstrap;
pub mod bridge;
mod commands;
mod diagnostics;
mod incarnation;
mod instance;
mod launch;
#[cfg(test)]
mod mock_ipc_tests;
mod prefs;
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
/// The UI lock is held by an incumbent that could not be verified or reached.
pub const EXIT_INSTANCE_UNAVAILABLE: i32 = 69;

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

    let incumbent = match instance::claim(&app_identifier) {
        instance::Claim::Incumbent(incumbent) => incumbent,
        instance::Claim::Forwarded => return 0,
        instance::Claim::Failed(error) => {
            eprintln!("Threadspace is already running but could not be reached: {error}");
            return EXIT_INSTANCE_UNAVAILABLE;
        }
    };
    let bridge = Arc::new(Bridge::new(app_identifier));
    let shell = Arc::new(window::Shell::default());
    let setup_bridge = Arc::clone(&bridge);
    let setup_launch = launch.clone();
    let app = tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        .manage(Arc::clone(&bridge))
        .manage(Arc::clone(&shell))
        .invoke_handler(tauri::generate_handler![
            commands::ui_connect,
            commands::ui_ack,
            commands::ui_disconnect,
            commands::ui_query,
            commands::ui_action
        ])
        .setup(move |app| {
            let hook_app = app.handle().clone();
            let hook_bridge = Arc::downgrade(&setup_bridge);
            let hook_launch = setup_launch.clone();
            setup_bridge.set_recovery_hook(Box::new(move |incarnation, reason| {
                if let Some(bridge) = hook_bridge.upgrade() {
                    window::recover(&hook_app, &bridge, &hook_launch, incarnation, reason);
                }
            }));
            let activate_app = app.handle().clone();
            let activate_bridge = Arc::clone(&setup_bridge);
            let activate_launch = setup_launch.clone();
            instance::serve(incumbent, move || {
                let app = activate_app.clone();
                let bridge = Arc::clone(&activate_bridge);
                let launch = activate_launch.clone();
                let _ =
                    activate_app.run_on_main_thread(move || show_office(&app, &bridge, &launch));
            });
            window::create_office(app.handle(), &setup_bridge, &setup_launch)?;
            #[cfg(feature = "qualification")]
            if setup_launch.qualify_acl_probe {
                qualification::open_acl_probe(app.handle(), &setup_launch)?;
            }
            let heartbeat = Arc::downgrade(&setup_bridge);
            let heartbeat_app = app.handle().clone();
            thread::Builder::new()
                .name("bridge-heartbeat".into())
                .spawn(move || {
                    while let Some(bridge) = heartbeat.upgrade() {
                        // The bridge heartbeat runs while the office is
                        // visible (SPEC §18.5); a hidden renderer is not
                        // expected to acknowledge it.
                        let visible = heartbeat_app
                            .get_webview_window(window::OFFICE_LABEL)
                            .is_some_and(|office| {
                                office.is_visible().unwrap_or(false)
                                    && !office.is_minimized().unwrap_or(true)
                            });
                        if visible {
                            bridge.heartbeat_tick();
                        }
                        drop(bridge);
                        thread::sleep(Duration::from_millis(u64::from(HEARTBEAT_INTERVAL_MS)));
                    }
                })?;
            Ok(())
        })
        .build(context)
        .expect("Threadspace runtime failed");

    let reopen_bridge = Arc::clone(&bridge);
    app.run(move |handle, event| match event {
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => show_office(handle, &reopen_bridge, &launch),
        // Only a controlled view recovery may leave the app without its
        // office window for a moment (SPEC §18.5).
        RunEvent::ExitRequested {
            code: None, api, ..
        } if shell.recovering() => {
            api.prevent_exit();
        }
        _ => {}
    });
    0
}

/// Brings the office forward, recreating it if it was closed.
fn show_office<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    bridge: &Arc<Bridge>,
    launch: &LaunchOptions,
) {
    match handle.get_webview_window(window::OFFICE_LABEL) {
        Some(office) => {
            let _ = office.unminimize();
            let _ = office.show();
            let _ = office.set_focus();
        }
        None => {
            if let Err(error) = window::create_office(handle, bridge, launch) {
                eprintln!("threadspace: could not reopen office: {error}");
            }
        }
    }
}
