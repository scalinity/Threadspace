//! Qualification-only entry point for the unchanged production Ghostty adapter.
//! The outer fixture owns creation/cleanup; this process checks the exact app
//! image and incarnation and never selects a recipient by bundle ID.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_surfaces_macos::{ghostty, process};

fn run() -> Result<Value, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 && args.len() != 7 {
        return Err("usage: threadspace-ghostty-probe APP RESOURCES PID inventory|focus [WINDOW TAB TERMINAL]".into());
    }
    let app = PathBuf::from(&args[0]);
    let resources = PathBuf::from(&args[1]);
    let pid = args[2].parse::<i32>().map_err(|e| e.to_string())?;
    let expected_image = app
        .join("Contents/MacOS/ghostty")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let before = process::sample_incarnation(pid).map_err(|e| e.to_string())?;
    if PathBuf::from(&before.executable.path)
        .canonicalize()
        .map_err(|e| e.to_string())?
        != expected_image
    {
        return Err("application image mismatch".into());
    }
    // The dictionary on disk describes only the executable installed now;
    // its identity must be unchanged across the probe.
    let image_path = expected_image.display().to_string();
    let installed = process::path_file_id(&image_path).ok_or("installed executable unreadable")?;
    let dictionary = ghostty::probe_dictionary(&app).map_err(|e| e.to_string())?;
    // Immediately before any event: Apple events addressed to a PID reach the
    // application registered at its bundle path, so the process must be the
    // only one running that path; it must map the installed image the
    // dictionary describes; and it must be the same incarnation.
    let sharing = process::pids_with_executable(&image_path).map_err(|e| e.to_string())?;
    if sharing != [pid] {
        return Err(format!("{} processes run {image_path}; events cannot address one exactly", sharing.len()));
    }
    let mapped = process::mapped_image_file_id(pid).map_err(|e| e.to_string())?;
    if mapped.as_deref() != Some(installed.as_str())
        || process::path_file_id(&image_path).as_deref() != Some(installed.as_str())
    {
        return Err("running image is not the installed executable the dictionary describes".into());
    }
    let now = process::sample_incarnation(pid).map_err(|e| e.to_string())?;
    if !now.same_process_and_image(&before) {
        return Err("application incarnation changed".into());
    }
    let budget = Duration::from_secs(2);
    let result = match args[3].as_str() {
        "inventory" if args.len() == 4 => {
            let inventory = ghostty::enumerate(
                &resources.join("ghostty-inventory.applescript"),
                &dictionary,
                pid,
                budget,
            )
            .map_err(|e| e.to_string())?;
            json!({
                "kind": "inventory", "dictionarySha256": dictionary.sha256,
                "hasTTY": dictionary.has_tty_property, "hasPID": dictionary.has_pid_property,
                "windows": inventory.windows.iter().map(|w| json!({"id":w.id,"selectedTabId":w.selected_tab_id})).collect::<Vec<_>>(),
                "tabs": inventory.tabs.iter().map(|t| json!({"id":t.id,"windowId":t.window_id,"selected":t.selected,"focusedTerminalId":t.focused_terminal_id})).collect::<Vec<_>>(),
                "terminals": inventory.terminals.iter().map(|s| json!({"id":s.id,"windowId":s.window_id,"tabId":s.tab_id,"tty":s.tty,"pid":s.pid})).collect::<Vec<_>>(),
                "senderPid": inventory.sender_pid, "elapsedMs": inventory.elapsed_ms,
            })
        }
        "focus" if args.len() == 7 => {
            let target = ghostty::GhosttyTarget {
                window_id: args[4].clone(),
                tab_id: args[5].clone(),
                terminal_id: args[6].clone(),
            };
            let (outcome, sender_pid, elapsed_ms) = ghostty::focus(
                &resources.join("ghostty-focus.applescript"),
                &dictionary,
                pid,
                &target,
                budget,
            )
            .map_err(|e| e.to_string())?;
            match outcome {
                ghostty::FocusOutcome::Focused(r) => json!({
                    "kind":"focus", "outcome":"Focused", "windowId":r.window_id,
                    "tabId":r.tab_id, "terminalId":r.terminal_id,
                    "frontWindowId":r.front_window_id, "selectedTabId":r.selected_tab_id,
                    "focusedTerminalId":r.focused_terminal_id, "tabSelected":r.target_tab_selected,
                    "frontmost":r.app_frontmost, "frontmostPid":r.frontmost_app_pid,
                    "senderPid":sender_pid, "elapsedMs":elapsed_ms,
                }),
                ghostty::FocusOutcome::Gone => json!({"kind":"focus","outcome":"Gone"}),
                ghostty::FocusOutcome::Changed => json!({"kind":"focus","outcome":"Changed"}),
                ghostty::FocusOutcome::Ambiguous { count } => {
                    json!({"kind":"focus","outcome":"Ambiguous","count":count})
                }
            }
        }
        _ => return Err("invalid operation or arguments".into()),
    };
    let after = process::sample_incarnation(pid).map_err(|e| e.to_string())?;
    if !before.same_process_and_image(&after) {
        return Err("application changed during operation".into());
    }
    Ok(
        json!({"ok":true,"appPid":pid,"appStartSeconds":before.sample.start_seconds.to_string(),
        "appStartMicroseconds":before.sample.start_microseconds,"result":result}),
    )
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(result) => {
            println!("{result}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", json!({"ok":false,"error":error}));
            std::process::ExitCode::FAILURE
        }
    }
}
