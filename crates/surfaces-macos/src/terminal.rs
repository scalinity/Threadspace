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

/// One live Terminal tab as enumerated. `window_id`, `window_index` and
/// `tab_index` are diagnostics/hints only; the TTY path is a locator, never
/// durable identity (SPEC §4.12, §13.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTab {
    pub window_id: i64,
    pub window_index: i64,
    pub window_miniaturized: bool,
    pub tab_index: i64,
    pub selected: bool,
    pub tty: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabs {
    pub windows: u32,
    pub tabs: Vec<TerminalTab>,
    /// PID of the `osascript` process that sent the Apple events.
    pub sender_pid: u32,
    pub elapsed_ms: u32,
}

impl TerminalTabs {
    pub fn summary(&self) -> TerminalInventorySummary {
        TerminalInventorySummary {
            window_count: self.windows,
            tab_count: self.tabs.len() as u32,
            tty_paths: self.tabs.iter().map(|tab| tab.tty.clone()).collect(),
            elapsed_ms: self.elapsed_ms,
        }
    }
}

fn parse_bool(text: &str, field: &str) -> Result<bool, TerminalError> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(TerminalError::Parse(format!("{field} is not a boolean"))),
    }
}

fn parse_int(text: &str, field: &str) -> Result<i64, TerminalError> {
    text.parse()
        .map_err(|_| TerminalError::Parse(format!("{field} is not an integer")))
}

fn parse_tty(text: &str) -> Result<String, TerminalError> {
    if !text.starts_with("/dev/") {
        return Err(TerminalError::Parse("tty is not a device path".into()));
    }
    Ok(text.to_owned())
}

/// Parses the inventory script's `W`/`T` records, rejecting any enumeration
/// whose per-window counts disagree with the tabs actually returned.
pub fn parse_tabs(output: &str) -> Result<(u32, Vec<TerminalTab>), TerminalError> {
    let mut windows = 0u32;
    let mut tabs = Vec::new();
    // (window id, index, miniaturized, expected tabs, seen tabs)
    let mut current: Option<(i64, i64, bool, usize, usize)> = None;
    let close = |current: Option<(i64, i64, bool, usize, usize)>| -> Result<(), TerminalError> {
        match current {
            Some((_, _, _, expected, seen)) if expected != seen => Err(TerminalError::Parse(
                "window tab count changed during enumeration".into(),
            )),
            _ => Ok(()),
        }
    };
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["W", id, index, mini, tty_count, selected_count] => {
                close(current.take())?;
                let tty_count = parse_int(tty_count, "tty count")?;
                if tty_count != parse_int(selected_count, "selected count")? || tty_count < 0 {
                    return Err(TerminalError::Parse(
                        "tab lists changed during enumeration".into(),
                    ));
                }
                windows += 1;
                current = Some((
                    parse_int(id, "window id")?,
                    parse_int(index, "window index")?,
                    parse_bool(mini, "miniaturized")?,
                    tty_count as usize,
                    0,
                ));
            }
            ["T", id, tab_index, selected, tty] => {
                let Some((window_id, window_index, mini, expected, seen)) = current.as_mut() else {
                    return Err(TerminalError::Parse("tab before its window".into()));
                };
                if parse_int(id, "window id")? != *window_id {
                    return Err(TerminalError::Parse("tab outside its window".into()));
                }
                *seen += 1;
                if *seen > *expected {
                    return Err(TerminalError::Parse("more tabs than counted".into()));
                }
                tabs.push(TerminalTab {
                    window_id: *window_id,
                    window_index: *window_index,
                    window_miniaturized: *mini,
                    tab_index: parse_int(tab_index, "tab index")?,
                    selected: parse_bool(selected, "selected")?,
                    tty: parse_tty(tty)?,
                });
            }
            _ => return Err(TerminalError::Parse("unrecognized record".into())),
        }
    }
    close(current)?;
    Ok((windows, tabs))
}

fn osascript(
    script: &Path,
    args: &[&str],
    max_bytes: usize,
) -> Result<(String, u32, u32), TerminalError> {
    let mut command = BoundedCommand::new(OSASCRIPT, Duration::from_secs(2), max_bytes).arg(script);
    for arg in args {
        command = command.arg(*arg);
    }
    let output = run_bounded(&command)?;
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
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.pid,
        output.elapsed.as_millis() as u32,
    ))
}

/// Runs the bundled read-only inventory script. The caller must only invoke
/// this while Terminal is already running and automation is authorized:
/// addressing a stopped application would launch it, and an unauthorized
/// sender would prompt.
pub fn enumerate(script: &Path) -> Result<TerminalTabs, TerminalError> {
    let (output, sender_pid, elapsed_ms) = osascript(script, &[], MAX_INVENTORY_BYTES)?;
    let (windows, tabs) = parse_tabs(&output)?;
    Ok(TerminalTabs {
        windows,
        tabs,
        sender_pid,
        elapsed_ms,
    })
}

/// What the focus script did and read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusOutcome {
    Gone,
    Ambiguous { count: u32 },
    Changed,
    Focused(FocusReadback),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusReadback {
    pub window_id: i64,
    pub tab_index: i64,
    pub front_window_id: i64,
    pub front_selected_tty: String,
    pub target_window_frontmost: bool,
    pub target_tab_selected: bool,
}

pub fn parse_focus(output: &str) -> Result<FocusOutcome, TerminalError> {
    let line = output.trim();
    let fields: Vec<&str> = line.split('\t').collect();
    match fields.as_slice() {
        ["GONE"] => Ok(FocusOutcome::Gone),
        ["CHANGED"] => Ok(FocusOutcome::Changed),
        ["AMBIGUOUS", count] => Ok(FocusOutcome::Ambiguous {
            count: parse_int(count, "match count")?.clamp(0, i64::from(u32::MAX)) as u32,
        }),
        [
            "FOCUSED",
            window,
            tab,
            front,
            front_tty,
            frontmost,
            selected,
        ] => Ok(FocusOutcome::Focused(FocusReadback {
            window_id: parse_int(window, "window id")?,
            tab_index: parse_int(tab, "tab index")?,
            front_window_id: parse_int(front, "front window id")?,
            front_selected_tty: parse_tty(front_tty)?,
            target_window_frontmost: parse_bool(frontmost, "frontmost")?,
            target_tab_selected: parse_bool(selected, "selected")?,
        })),
        _ => Err(TerminalError::Parse("unrecognized focus result".into())),
    }
}

/// Runs the bundled focus script for one TTY locator, passed as data.
pub fn focus(script: &Path, tty: &str) -> Result<(FocusOutcome, u32, u32), TerminalError> {
    let tty = parse_tty(tty)?;
    let (output, sender_pid, elapsed_ms) = osascript(script, &[&tty], 4096)?;
    Ok((parse_focus(&output)?, sender_pid, elapsed_ms))
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
    fn parses_window_and_tab_records() {
        let (windows, tabs) = parse_tabs(
            "W\t101\t1\tfalse\t2\t2\nT\t101\t1\tfalse\t/dev/ttys001\nT\t101\t2\ttrue\t/dev/ttys002\n\
             W\t202\t2\ttrue\t1\t1\nT\t202\t1\ttrue\t/dev/ttys004\n",
        )
        .expect("parse");
        assert_eq!(windows, 2);
        assert_eq!(tabs.len(), 3);
        assert!(tabs[1].selected && tabs[2].window_miniaturized);
        assert_eq!(tabs[2].tty, "/dev/ttys004");
    }

    #[test]
    fn rejects_incomplete_or_racing_enumerations() {
        // A window promised two tabs but delivered one.
        assert!(parse_tabs("W\t1\t1\tfalse\t2\t2\nT\t1\t1\ttrue\t/dev/ttys001\n").is_err());
        // tty and selected lists of different lengths.
        assert!(parse_tabs("W\t1\t1\tfalse\t2\t1\n").is_err());
        // A tab without its window, a non-device tty, an unknown record.
        assert!(parse_tabs("T\t1\t1\ttrue\t/dev/ttys001\n").is_err());
        assert!(parse_tabs("W\t1\t1\tfalse\t1\t1\nT\t1\t1\ttrue\tttys001\n").is_err());
        assert!(parse_tabs("X\n").is_err());
    }

    #[test]
    fn parses_focus_outcomes() {
        assert_eq!(parse_focus("GONE\n").expect("gone"), FocusOutcome::Gone);
        assert_eq!(
            parse_focus("AMBIGUOUS\t2").expect("ambiguous"),
            FocusOutcome::Ambiguous { count: 2 }
        );
        let FocusOutcome::Focused(readback) =
            parse_focus("FOCUSED\t7\t2\t7\t/dev/ttys009\ttrue\ttrue").expect("focused")
        else {
            panic!("expected focused");
        };
        assert_eq!(readback.front_selected_tty, "/dev/ttys009");
        assert!(readback.target_window_frontmost && readback.target_tab_selected);
        assert!(parse_focus("FOCUSED\t7").is_err());
    }

    #[test]
    fn reads_terminal_version() {
        assert!(application_version(Path::new(TERMINAL_APP_PATH)).is_some());
    }
}
