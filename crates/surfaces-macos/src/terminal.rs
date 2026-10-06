//! Terminal.app support (SPEC §13.3). The installed scripting dictionary is the
//! qualification contract; scripts are fixed bundled resources run through
//! `osascript` by the bounded worker, so the Apple-event sender is the caller's
//! own identity. Inventory is read-only: it never selects, focuses or types.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use threadspace_contracts::diagnostics::{TerminalDictionary, TerminalInventorySummary};

use crate::exec::{BoundedCommand, ExecError, run_bounded};

pub const TERMINAL_BUNDLE_ID: &str = "com.apple.Terminal";
pub const TERMINAL_APP_PATH: &str = "/System/Applications/Utilities/Terminal.app";
const SDEF: &str = "/usr/bin/sdef";
const OSASCRIPT: &str = "/usr/bin/osascript";
const PLUTIL: &str = "/usr/bin/plutil";
const MAX_DICTIONARY_BYTES: usize = 512 * 1024;
const MAX_INVENTORY_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub enum TerminalError {
    Exec(ExecError),
    Failed {
        program: &'static str,
        status: Option<i32>,
        timed_out: bool,
        stderr: String,
    },
    Parse(String),
}

impl std::fmt::Display for TerminalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exec(error) => write!(f, "{error}"),
            Self::Failed {
                program,
                status,
                timed_out,
                stderr,
            } => {
                write!(
                    f,
                    "{program} failed (status {status:?}, timed out {timed_out}): {stderr}"
                )
            }
            Self::Parse(detail) => write!(f, "unexpected output: {detail}"),
        }
    }
}

impl std::error::Error for TerminalError {}

impl From<ExecError> for TerminalError {
    fn from(error: ExecError) -> Self {
        Self::Exec(error)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn checked(program: &'static str, command: &BoundedCommand) -> Result<Vec<u8>, TerminalError> {
    let output = run_bounded(command)?;
    if !output.succeeded() || output.stdout_truncated {
        return Err(TerminalError::Failed {
            program,
            status: output.status,
            timed_out: output.timed_out,
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(240)
                .collect(),
        });
    }
    Ok(output.stdout)
}

/// Reads and fingerprints the installed dictionary, checking for the terms the
/// return path depends on.
pub fn probe_dictionary(app: &Path) -> Result<TerminalDictionary, TerminalError> {
    let xml = checked(
        "sdef",
        &BoundedCommand::new(SDEF, Duration::from_secs(5), MAX_DICTIONARY_BYTES).arg(app),
    )?;
    let text = String::from_utf8_lossy(&xml);
    Ok(TerminalDictionary {
        sha256: hex(&Sha256::digest(&xml)),
        byte_length: xml.len() as u32,
        has_tab_class: text.contains("<class name=\"tab\""),
        has_tty_property: text.contains("<property name=\"tty\""),
        // Declared as a `<contents>` element in the installed dictionary.
        has_selected_tab_property: text.contains("name=\"selected tab\""),
        has_frontmost_property: text.contains("<property name=\"frontmost\""),
    })
}

/// A string value from a property list, read by the bounded `plutil` worker.
pub fn plist_string(plist: &Path, key: &str) -> Option<String> {
    let output = run_bounded(
        &BoundedCommand::new(PLUTIL, Duration::from_secs(2), 256)
            .arg("-extract")
            .arg(key)
            .arg("raw")
            .arg(plist),
    )
    .ok()?;
    output
        .succeeded()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `CFBundleShortVersionString` of an installed application.
pub fn application_version(app: &Path) -> Option<String> {
    let info: PathBuf = app.join("Contents/Info.plist");
    plist_string(&info, "CFBundleShortVersionString")
}

/// Parses the fixed inventory script's output: one `<window id>\t<tty>` line per tab.
pub fn parse_inventory(
    output: &str,
    elapsed_ms: u32,
) -> Result<TerminalInventorySummary, TerminalError> {
    let mut windows = std::collections::BTreeSet::new();
    let mut tty_paths = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let (window, tty) = line
            .split_once('\t')
            .ok_or_else(|| TerminalError::Parse("missing tab separator".into()))?;
        let window: i64 = window
            .trim()
            .parse()
            .map_err(|_| TerminalError::Parse("window id".into()))?;
        let tty = tty.trim();
        if !tty.starts_with("/dev/") {
            return Err(TerminalError::Parse("tty is not a device path".into()));
        }
        windows.insert(window);
        tty_paths.push(tty.to_owned());
    }
    Ok(TerminalInventorySummary {
        window_count: windows.len() as u32,
        tab_count: tty_paths.len() as u32,
        tty_paths,
        elapsed_ms,
    })
}

/// Runs the bundled read-only inventory script. The caller must only invoke
/// this while Terminal is already running: addressing a stopped application
/// would launch it.
pub fn inventory(script: &Path) -> Result<TerminalInventorySummary, TerminalError> {
    let output = run_bounded(
        &BoundedCommand::new(OSASCRIPT, Duration::from_secs(2), MAX_INVENTORY_BYTES).arg(script),
    )?;
    if !output.succeeded() || output.stdout_truncated {
        return Err(TerminalError::Failed {
            program: "osascript",
            status: output.status,
            timed_out: output.timed_out,
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(240)
                .collect(),
        });
    }
    parse_inventory(
        &String::from_utf8_lossy(&output.stdout),
        output.elapsed.as_millis() as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_dictionary_exposes_the_required_terms() {
        let dictionary = probe_dictionary(Path::new(TERMINAL_APP_PATH)).expect("sdef");
        assert!(dictionary.has_tab_class);
        assert!(dictionary.has_tty_property);
        assert!(dictionary.has_selected_tab_property);
        assert!(dictionary.has_frontmost_property);
        assert_eq!(dictionary.sha256.len(), 64);
    }

    #[test]
    fn parses_inventory_lines() {
        let summary = parse_inventory(
            "101\t/dev/ttys001\n101\t/dev/ttys002\n202\t/dev/ttys004\n",
            12,
        )
        .expect("parse");
        assert_eq!(summary.window_count, 2);
        assert_eq!(summary.tab_count, 3);
        assert!(parse_inventory("101 /dev/ttys001", 0).is_err());
        assert!(parse_inventory("101\tttys001", 0).is_err());
    }

    #[test]
    fn reads_terminal_version() {
        assert!(application_version(Path::new(TERMINAL_APP_PATH)).is_some());
    }
}
