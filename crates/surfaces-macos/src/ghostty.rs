//! Ghostty's native scripting surface. IDs belong to the installed dictionary,
//! not to `GHOSTTY_SURFACE_ID`, and identify no provider/session by themselves.
//! The caller retains the application generation and checks automation consent
//! without prompting before invoking scripts against an already-running app.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::exec::{BoundedCommand, BoundedOutput, ExecError, run_bounded};

pub const GHOSTTY_BUNDLE_ID: &str = "com.mitchellh.ghostty";
pub const GHOSTTY_APP_PATH: &str = "/Applications/Ghostty.app";
pub const GHOSTTY_EXECUTABLE: &str = "/Applications/Ghostty.app/Contents/MacOS/ghostty";
const MAX_DICTIONARY_BYTES: usize = 512 * 1024;

/// The application bundle of a Ghostty executable path
/// (`…/<name>.app/Contents/MacOS/ghostty`), wherever it is installed.
/// Separate installations share the bundle identifier; each is its own
/// application path, which is what makes PID-addressed events exact.
pub fn bundle_of(executable: &Path) -> Option<std::path::PathBuf> {
    if executable.file_name()? != "ghostty" {
        return None;
    }
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}
const MAX_INVENTORY_BYTES: usize = 2 * 1024 * 1024;
const MAX_WINDOWS: usize = 128;
const MAX_TABS: usize = 1024;
const MAX_TERMINALS: usize = 4096;

#[derive(Debug)]
pub enum GhosttyError {
    Exec(ExecError),
    Failed {
        program: &'static str,
        status: Option<i32>,
        timed_out: bool,
        stderr: String,
    },
    UnsupportedDictionary,
    Parse(String),
}

impl std::fmt::Display for GhosttyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exec(error) => write!(f, "{error}"),
            Self::Failed {
                program,
                status,
                timed_out,
                stderr,
            } => write!(
                f,
                "{program} failed (status {status:?}, timed out {timed_out}): {stderr}"
            ),
            Self::UnsupportedDictionary => {
                write!(f, "Ghostty dictionary lacks required native terms")
            }
            Self::Parse(detail) => write!(f, "unexpected Ghostty output: {detail}"),
        }
    }
}

impl std::error::Error for GhosttyError {}

impl From<ExecError> for GhosttyError {
    fn from(error: ExecError) -> Self {
        Self::Exec(error)
    }
}

fn invalid(detail: &str) -> GhosttyError {
    GhosttyError::Parse(detail.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyDictionary {
    pub sha256: String,
    pub byte_length: u32,
    pub has_windows: bool,
    pub has_window_ids: bool,
    pub has_tab_ids: bool,
    pub has_terminal_ids: bool,
    pub has_window_tabs: bool,
    pub has_tab_terminals: bool,
    pub has_application_terminals: bool,
    pub has_selected_tab_property: bool,
    pub has_selected_property: bool,
    pub has_focused_terminal_property: bool,
    pub has_front_window_property: bool,
    pub has_frontmost_property: bool,
    pub has_focus_command: bool,
    pub has_tty_property: bool,
    pub has_pid_property: bool,
    // JXA getter names are selected from the typed dictionary, then passed as
    // data. Every query uses the same PID-addressed application instance.
    tty_property: Option<String>,
    pid_property: Option<String>,
}

impl GhosttyDictionary {
    pub fn supports_inventory(&self) -> bool {
        self.has_windows
            && self.has_window_ids
            && self.has_tab_ids
            && self.has_terminal_ids
            && self.has_window_tabs
            && self.has_tab_terminals
            && self.has_application_terminals
            && self.has_selected_tab_property
            && self.has_selected_property
            && self.has_focused_terminal_property
    }

    pub fn supports_focus(&self) -> bool {
        self.supports_inventory() && self.has_front_window_property && self.has_focus_command
    }
}

fn checked(program: &'static str, command: &BoundedCommand) -> Result<BoundedOutput, GhosttyError> {
    if command.timeout.is_zero() {
        return Err(invalid("native process budget exhausted"));
    }
    let output = run_bounded(command)?;
    if !output.succeeded() || output.stdout_truncated || output.stderr_truncated {
        return Err(GhosttyError::Failed {
            program,
            status: output.status,
            timed_out: output.timed_out,
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(240)
                .collect(),
        });
    }
    Ok(output)
}

fn boolean(value: &str) -> Result<bool, GhosttyError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid("invalid boolean")),
    }
}

fn optional_property(value: &str, present: bool) -> Result<Option<String>, GhosttyError> {
    if !present {
        return Ok(None);
    }
    match value {
        "tty" | "pid" => Ok(Some(value.into())),
        "process id" => Ok(Some("processId".into())),
        _ => Err(invalid("invalid optional property")),
    }
}

// Structural, class-scoped and typed XPath checks avoid mistaking a comment,
// description, writable property, or another class's PID for a capability.
fn dictionary_from_xml(xml: &[u8], timeout: Duration) -> Result<GhosttyDictionary, GhosttyError> {
    if xml.len() > MAX_DICTIONARY_BYTES {
        return Err(invalid("dictionary exceeds limit"));
    }
    let app = "/dictionary/suite/class[@name='application']";
    let window = "/dictionary/suite/class[@name='window']";
    let tab = "/dictionary/suite/class[@name='tab']";
    let terminal = "/dictionary/suite/class[@name='terminal']";
    let prop = |class: &str, name: &str, kind: &str| {
        format!("{class}/property[@name='{name}' and @type='{kind}' and not(@access='w')]")
    };
    let tty = prop(terminal, "tty", "text");
    let pid = format!(
        "{terminal}/property[(@name='pid' or @name='process id') and @type='integer' and not(@access='w')]"
    );
    let paths = [
        format!("{app}/element[@type='window' and not(@access='w')]"),
        prop(window, "id", "text"), prop(tab, "id", "text"), prop(terminal, "id", "text"),
        format!("{window}/element[@type='tab' and not(@access='w')]"),
        format!("{tab}/element[@type='terminal' and not(@access='w')]"),
        format!("{app}/element[@type='terminal' and not(@access='w')]"),
        prop(window, "selected tab", "tab"), prop(tab, "selected", "boolean"),
        prop(tab, "focused terminal", "terminal"), prop(app, "front window", "window"),
        prop(app, "frontmost", "boolean"),
        "/dictionary/suite/command[@name='focus' and direct-parameter[@type='specifier'] and /dictionary/suite/class[@name='terminal']/responds-to[@command='focus']]".into(),
        tty.clone(), pid.clone(),
    ];
    let mut expressions: Vec<String> = paths
        .iter()
        .map(|path| format!("count({path}) = 1"))
        .collect();
    expressions.push(format!("string({tty}/@name)"));
    expressions.push(format!("string({pid}/@name)"));
    let xpath = format!("concat({})", expressions.join(", '|', "));
    let mut command = BoundedCommand::new("/usr/bin/xmllint", timeout, 4096)
        .arg("--nonet")
        .arg("--xpath")
        .arg(xpath)
        .arg("-");
    command.stdin = Some(xml.to_vec());
    let output = checked("xmllint", &command)?;
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| invalid("dictionary probe is not UTF-8"))?;
    let fields: Vec<_> = text.trim_end_matches('\n').split('|').collect();
    if fields.len() != 17 {
        return Err(invalid("invalid dictionary probe"));
    }
    Ok(GhosttyDictionary {
        sha256: Sha256::digest(xml)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        byte_length: xml.len() as u32,
        has_windows: boolean(fields[0])?,
        has_window_ids: boolean(fields[1])?,
        has_tab_ids: boolean(fields[2])?,
        has_terminal_ids: boolean(fields[3])?,
        has_window_tabs: boolean(fields[4])?,
        has_tab_terminals: boolean(fields[5])?,
        has_application_terminals: boolean(fields[6])?,
        has_selected_tab_property: boolean(fields[7])?,
        has_selected_property: boolean(fields[8])?,
        has_focused_terminal_property: boolean(fields[9])?,
        has_front_window_property: boolean(fields[10])?,
        has_frontmost_property: boolean(fields[11])?,
        has_focus_command: boolean(fields[12])?,
        has_tty_property: boolean(fields[13])?,
        has_pid_property: boolean(fields[14])?,
        tty_property: optional_property(fields[15], boolean(fields[13])?)?,
        pid_property: optional_property(fields[16], boolean(fields[14])?)?,
    })
}

/// Reads and hashes the installed dictionary without contacting or launching
/// Ghostty. Both native subprocesses share a five-second total budget.
pub fn probe_dictionary(app: &Path) -> Result<GhosttyDictionary, GhosttyError> {
    let started = Instant::now();
    let budget = Duration::from_secs(5);
    let output = checked(
        "sdef",
        &BoundedCommand::new("/usr/bin/sdef", budget, MAX_DICTIONARY_BYTES).arg(app),
    )?;
    dictionary_from_xml(&output.stdout, budget.saturating_sub(started.elapsed()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyWindow {
    pub id: String,
    pub selected_tab_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyTab {
    pub window_id: String,
    pub id: String,
    pub selected: bool,
    pub focused_terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyTerminal {
    pub window_id: String,
    pub tab_id: String,
    pub id: String,
    /// When available, this TTY is the passive process-to-surface join input.
    pub tty: Option<String>,
    /// Diagnostic metadata only: Ghostty may report a root-owned login wrapper,
    /// not the foreground provider process. It establishes no provider ownership.
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyInventory {
    pub windows: Vec<GhosttyWindow>,
    pub tabs: Vec<GhosttyTab>,
    pub terminals: Vec<GhosttyTerminal>,
    pub sender_pid: u32,
    pub elapsed_ms: u32,
}

fn native_id(value: &str) -> Result<String, GhosttyError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim().is_empty()
        || value.chars().any(char::is_control)
    {
        return Err(invalid("invalid native ID"));
    }
    Ok(value.into())
}

fn count(value: &str, max: usize) -> Result<usize, GhosttyError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("invalid count"));
    }
    let parsed = value
        .parse::<usize>()
        .map_err(|_| invalid("invalid count"))?;
    if parsed > max {
        return Err(invalid("count exceeds limit"));
    }
    Ok(parsed)
}

fn unique(value: &str, seen: &mut HashSet<String>) -> Result<String, GhosttyError> {
    let value = native_id(value)?;
    if !seen.insert(value.clone()) {
        return Err(invalid("duplicate native ID"));
    }
    Ok(value)
}

fn tty(value: &str, supported: bool) -> Result<Option<String>, GhosttyError> {
    if value == "-" {
        return Ok(None);
    }
    if !supported
        || !value.starts_with("/dev/")
        || value.len() > 256
        || value[5..].is_empty()
        || value[5..].contains('/')
        || value.chars().any(char::is_control)
    {
        return Err(invalid("invalid or unadvertised tty"));
    }
    Ok(Some(value.into()))
}

fn pid(value: &str, supported: bool) -> Result<Option<u32>, GhosttyError> {
    if value == "-" {
        return Ok(None);
    }
    if !supported {
        return Err(invalid("unadvertised pid"));
    }
    let pid = count(value, i32::MAX as usize)?;
    if pid == 0 {
        return Err(invalid("invalid pid"));
    }
    Ok(Some(pid as u32))
}

/// Parses a complete, bounded hierarchy. The script also compares IDs before
/// and after enumeration, including equal-count replacements and split moves.
pub fn parse_inventory(
    output: &str,
    dictionary: &GhosttyDictionary,
) -> Result<GhosttyInventory, GhosttyError> {
    if !dictionary.supports_inventory() {
        return Err(GhosttyError::UnsupportedDictionary);
    }
    if output.len() > MAX_INVENTORY_BYTES {
        return Err(invalid("inventory exceeds limit"));
    }
    let mut lines = output
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>());
    let Some(header) = lines.next() else {
        return Err(invalid("missing inventory header"));
    };
    let ["I", "1", window_count, terminal_count] = header.as_slice() else {
        return Err(invalid("invalid inventory header"));
    };
    let expected_windows = count(window_count, MAX_WINDOWS)?;
    let expected_terminals = count(terminal_count, MAX_TERMINALS)?;
    let mut inventory = GhosttyInventory {
        windows: vec![],
        tabs: vec![],
        terminals: vec![],
        sender_pid: 0,
        elapsed_ms: 0,
    };
    let (mut windows, mut tabs, mut terminals) = (HashSet::new(), HashSet::new(), HashSet::new());
    for _ in 0..expected_windows {
        let row = lines.next().ok_or_else(|| invalid("missing window"))?;
        let ["W", window_id, selected_id, tab_count] = row.as_slice() else {
            return Err(invalid("invalid window record"));
        };
        let window_id = unique(window_id, &mut windows)?;
        let selected_tab_id = native_id(selected_id)?;
        let expected_tabs = count(tab_count, MAX_TABS)?;
        if expected_tabs == 0 || inventory.tabs.len() + expected_tabs > MAX_TABS {
            return Err(invalid("invalid window tab count"));
        }
        let mut selected_tabs = 0;
        for _ in 0..expected_tabs {
            let row = lines.next().ok_or_else(|| invalid("missing tab"))?;
            let ["T", parent, tab_id, selected, focused_id, terminal_count] = row.as_slice() else {
                return Err(invalid("invalid tab record"));
            };
            if *parent != window_id {
                return Err(invalid("tab outside its window"));
            }
            let tab_id = unique(tab_id, &mut tabs)?;
            let selected = boolean(selected)?;
            if selected != (tab_id == selected_tab_id) {
                return Err(invalid("selected tab disagrees with window"));
            }
            selected_tabs += usize::from(selected);
            let focused_terminal_id = native_id(focused_id)?;
            let expected = count(terminal_count, MAX_TERMINALS)?;
            if expected == 0 || inventory.terminals.len() + expected > expected_terminals {
                return Err(invalid("invalid tab terminal count"));
            }
            let mut focused_seen = false;
            for _ in 0..expected {
                let row = lines.next().ok_or_else(|| invalid("missing terminal"))?;
                let [
                    "S",
                    parent_window,
                    parent_tab,
                    terminal_id,
                    terminal_tty,
                    terminal_pid,
                ] = row.as_slice()
                else {
                    return Err(invalid("invalid terminal record"));
                };
                if *parent_window != window_id || *parent_tab != tab_id {
                    return Err(invalid("terminal outside its tab"));
                }
                let id = unique(terminal_id, &mut terminals)?;
                focused_seen |= id == focused_terminal_id;
                inventory.terminals.push(GhosttyTerminal {
                    window_id: window_id.clone(),
                    tab_id: tab_id.clone(),
                    id,
                    tty: tty(terminal_tty, dictionary.has_tty_property)?,
                    pid: pid(terminal_pid, dictionary.has_pid_property)?,
                });
            }
            if !focused_seen {
                return Err(invalid("focused terminal absent from tab"));
            }
            inventory.tabs.push(GhosttyTab {
                window_id: window_id.clone(),
                id: tab_id,
                selected,
                focused_terminal_id,
            });
        }
        if selected_tabs != 1 {
            return Err(invalid("window must have one selected tab"));
        }
        inventory.windows.push(GhosttyWindow {
            id: window_id,
            selected_tab_id,
        });
    }
    let footer = lines
        .next()
        .ok_or_else(|| invalid("missing inventory footer"))?;
    let ["E", windows_after, terminals_after] = footer.as_slice() else {
        return Err(invalid("invalid inventory footer"));
    };
    if count(windows_after, MAX_WINDOWS)? != expected_windows
        || count(terminals_after, MAX_TERMINALS)? != expected_terminals
        || inventory.terminals.len() != expected_terminals
        || lines.next().is_some()
    {
        return Err(invalid("inventory changed or has extra records"));
    }
    Ok(inventory)
}

fn osascript(
    script: &Path,
    args: &[String],
    limit: usize,
    timeout: Duration,
) -> Result<(String, u32, u32), GhosttyError> {
    let mut command = BoundedCommand::new("/usr/bin/osascript", timeout, limit)
        .arg("-l")
        .arg("JavaScript")
        .arg(script);
    for arg in args {
        command = command.arg(arg);
    }
    let output = checked("osascript", &command)?;
    let text =
        String::from_utf8(output.stdout).map_err(|_| invalid("script output is not UTF-8"))?;
    Ok((
        text,
        output.pid,
        output.elapsed.as_millis().min(u128::from(u32::MAX)) as u32,
    ))
}

fn limits() -> [String; 3] {
    [
        MAX_WINDOWS.to_string(),
        MAX_TABS.to_string(),
        MAX_TERMINALS.to_string(),
    ]
}

fn app_pid_argument(app_pid: i32) -> Result<String, GhosttyError> {
    if app_pid <= 0 {
        return Err(invalid("invalid application PID"));
    }
    Ok(app_pid.to_string())
}

/// Read-only inventory of the exact supplied PID; unavailable optional
/// properties are never queried. The caller validates that PID's generation
/// and that this dictionary belongs to its executable, before and after use.
pub fn enumerate(
    script: &Path,
    dictionary: &GhosttyDictionary,
    app_pid: i32,
    timeout: Duration,
) -> Result<GhosttyInventory, GhosttyError> {
    let app_pid_arg = app_pid_argument(app_pid)?;
    if !dictionary.supports_inventory() {
        return Err(GhosttyError::UnsupportedDictionary);
    }
    let mut args = vec![
        app_pid_arg,
        dictionary
            .tty_property
            .clone()
            .unwrap_or_else(|| "-".into()),
        dictionary
            .pid_property
            .clone()
            .unwrap_or_else(|| "-".into()),
    ];
    args.extend(limits());
    let (output, sender_pid, elapsed_ms) = osascript(script, &args, MAX_INVENTORY_BYTES, timeout)?;
    let mut inventory = parse_inventory(&output, dictionary)?;
    inventory.sender_pid = sender_pid;
    inventory.elapsed_ms = elapsed_ms;
    Ok(inventory)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyTarget {
    pub window_id: String,
    pub tab_id: String,
    pub terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusOutcome {
    Gone,
    Ambiguous { count: u32 },
    Changed,
    Focused(FocusReadback),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusReadback {
    pub window_id: String,
    pub tab_id: String,
    pub terminal_id: String,
    pub front_window_id: String,
    pub selected_tab_id: String,
    pub focused_terminal_id: String,
    pub target_tab_selected: bool,
    /// `None` means the dictionary cannot establish application activation.
    pub app_frontmost: Option<bool>,
    /// Independent NSWorkspace readback; must equal the supplied application PID.
    pub frontmost_app_pid: i32,
}

pub fn parse_focus(
    output: &str,
    dictionary: &GhosttyDictionary,
    app_pid: i32,
    target: &GhosttyTarget,
) -> Result<FocusOutcome, GhosttyError> {
    app_pid_argument(app_pid)?;
    if !dictionary.supports_focus() {
        return Err(GhosttyError::UnsupportedDictionary);
    }
    let fields: Vec<_> = output
        .strip_suffix('\n')
        .unwrap_or(output)
        .split('\t')
        .collect();
    let result = match fields.as_slice() {
        ["GONE"] => FocusOutcome::Gone,
        ["CHANGED"] => FocusOutcome::Changed,
        ["AMBIGUOUS", value] => {
            let count = count(value, MAX_TERMINALS)?;
            if count < 2 {
                return Err(invalid("invalid ambiguous count"));
            }
            FocusOutcome::Ambiguous {
                count: count as u32,
            }
        }
        [
            "FOCUSED",
            window,
            tab,
            terminal,
            front,
            selected_tab,
            focused_terminal,
            selected,
            frontmost,
            frontmost_pid,
        ] => {
            let app_frontmost = if dictionary.has_frontmost_property {
                Some(boolean(frontmost)?)
            } else if *frontmost == "-" {
                None
            } else {
                return Err(invalid("unadvertised frontmost readback"));
            };
            let readback = FocusReadback {
                window_id: native_id(window)?,
                tab_id: native_id(tab)?,
                terminal_id: native_id(terminal)?,
                front_window_id: native_id(front)?,
                selected_tab_id: native_id(selected_tab)?,
                focused_terminal_id: native_id(focused_terminal)?,
                target_tab_selected: boolean(selected)?,
                app_frontmost,
                frontmost_app_pid: count(frontmost_pid, i32::MAX as usize)? as i32,
            };
            if readback.window_id != target.window_id
                || readback.front_window_id != target.window_id
                || readback.tab_id != target.tab_id
                || readback.selected_tab_id != target.tab_id
                || readback.terminal_id != target.terminal_id
                || readback.focused_terminal_id != target.terminal_id
                || !readback.target_tab_selected
                || readback.app_frontmost == Some(false)
                || readback.frontmost_app_pid != app_pid
            {
                FocusOutcome::Changed
            } else {
                FocusOutcome::Focused(readback)
            }
        }
        _ => return Err(invalid("invalid focus result")),
    };
    Ok(result)
}

/// Focuses only the exact supplied PID/native ID triple and validates all available
/// readback. An error or timeout may follow an already-performed focus. Native
/// focus is not evidence that a provider/session owns this terminal.
pub fn focus(
    script: &Path,
    dictionary: &GhosttyDictionary,
    app_pid: i32,
    target: &GhosttyTarget,
    budget: Duration,
) -> Result<(FocusOutcome, u32, u32), GhosttyError> {
    let app_pid_arg = app_pid_argument(app_pid)?;
    if !dictionary.supports_focus() {
        return Err(GhosttyError::UnsupportedDictionary);
    }
    let mut args = vec![
        app_pid_arg,
        native_id(&target.window_id)?,
        native_id(&target.tab_id)?,
        native_id(&target.terminal_id)?,
        budget.as_millis().min(5000).to_string(),
        dictionary.has_frontmost_property.to_string(),
    ];
    args.extend(limits());
    let (output, sender_pid, elapsed_ms) = osascript(script, &args, 4096, budget)?;
    Ok((
        parse_focus(&output, dictionary, app_pid, target)?,
        sender_pid,
        elapsed_ms,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A small synthetic dictionary: no upstream implementation is embedded.
    const DICTIONARY: &str = r#"<dictionary><suite>
      <class name="application">
        <element type="window"/><element type="terminal"/>
        <property name="frontmost" type="boolean"/>
        <property name="front window" type="window"/>
      </class>
      <class name="window"><property name="id" type="text"/>
        <element type="tab"/><property name="selected tab" type="tab"/>
      </class>
      <class name="tab"><property name="id" type="text"/>
        <element type="terminal"/><property name="selected" type="boolean"/>
        <property name="focused terminal" type="terminal"/>
      </class>
      <class name="terminal"><property name="id" type="text"/>
        <responds-to command="focus"/>
      </class>
      <command name="focus"><direct-parameter type="specifier"/></command>
    </suite></dictionary>"#;

    fn dictionary() -> GhosttyDictionary {
        dictionary_from_xml(DICTIONARY.as_bytes(), Duration::from_secs(2)).expect("dictionary")
    }

    const INVENTORY: &str = "I\t1\t1\t3\nW\twindow-A\ttab-B\t2\n\
        T\twindow-A\ttab-A\tfalse\tterminal-A\t1\n\
        S\twindow-A\ttab-A\tterminal-A\t-\t-\n\
        T\twindow-A\ttab-B\ttrue\tterminal-C\t2\n\
        S\twindow-A\ttab-B\tterminal-B\t-\t-\n\
        S\twindow-A\ttab-B\tterminal-C\t-\t-\nE\t1\t3\n";

    fn target() -> GhosttyTarget {
        GhosttyTarget {
            window_id: "window-A".into(),
            tab_id: "tab-B".into(),
            terminal_id: "terminal-C".into(),
        }
    }

    const FOCUSED: &str =
        "FOCUSED\twindow-A\ttab-B\tterminal-C\twindow-A\ttab-B\tterminal-C\ttrue\ttrue\t1234\n";

    #[test]
    fn dictionary_is_fingerprinted_and_requires_scoped_typed_terms() {
        let dictionary = dictionary();
        assert!(dictionary.supports_inventory() && dictionary.supports_focus());
        assert!(!dictionary.has_tty_property && !dictionary.has_pid_property);
        assert_eq!(dictionary.sha256.len(), 64);
        assert_eq!(dictionary.byte_length as usize, DICTIONARY.len());
        let changed = DICTIONARY.replace(
            "name=\"front window\" type=\"window\"",
            "name=\"front window\" type=\"text\"",
        );
        let changed =
            dictionary_from_xml(changed.as_bytes(), Duration::from_secs(2)).expect("typed");
        assert_ne!(changed.sha256, dictionary.sha256);
        assert!(changed.supports_inventory());
        assert!(!changed.supports_focus());
        assert!(dictionary_from_xml(b"<dictionary>", Duration::from_secs(2)).is_err());
        let wrong = "<dictionary><suite><class name='window'><property name='tty' code='ptty' type='text'/><property name='pid' code='ppid' type='integer'/></class><!-- <class name='terminal'><property name='tty' type='text'/></class> --></suite></dictionary>";
        let wrong =
            dictionary_from_xml(wrong.as_bytes(), Duration::from_secs(2)).expect("unrelated terms");
        assert!(!wrong.has_tty_property && !wrong.has_pid_property && !wrong.supports_inventory());
    }

    #[test]
    fn optional_properties_require_unique_readable_typed_dictionary_entries() {
        let extra = "<property name='tty' code='ptty' type='text' access='r'/><property name='process id' code='ppid' type='integer' access='r'/>";
        let xml = DICTIONARY.replace("<responds-to command=\"focus\"/>", extra);
        let dictionary =
            dictionary_from_xml(xml.as_bytes(), Duration::from_secs(2)).expect("optional");
        assert!(dictionary.has_tty_property && dictionary.has_pid_property);
        assert_eq!(dictionary.tty_property.as_deref(), Some("tty"));
        assert_eq!(dictionary.pid_property.as_deref(), Some("processId"));
        let rows = INVENTORY.replace("terminal-A\t-\t-", "terminal-A\t/dev/ttys123\t4321");
        let inventory = parse_inventory(&rows, &dictionary).expect("optional values");
        assert_eq!(inventory.terminals[0].tty.as_deref(), Some("/dev/ttys123"));
        assert_eq!(inventory.terminals[0].pid, Some(4321));
        assert!(parse_inventory(&rows.replace("4321", "0"), &dictionary).is_err());
        assert!(parse_inventory(&rows.replace("4321", "2147483648"), &dictionary).is_err());
        assert!(parse_inventory(&rows.replace("/dev/ttys123", "ttys123"), &dictionary).is_err());
        let writable = xml.replace("access='r'", "access='w'");
        let writable =
            dictionary_from_xml(writable.as_bytes(), Duration::from_secs(2)).expect("writable");
        assert!(!writable.has_tty_property && !writable.has_pid_property);
        let duplicate = xml.replace(extra, &format!("{extra}{extra}"));
        let duplicate = dictionary_from_xml(duplicate.as_bytes(), Duration::from_secs(2))
            .expect("duplicate properties");
        assert!(!duplicate.has_tty_property && !duplicate.has_pid_property);
    }

    #[test]
    fn inventory_preserves_string_ids_and_split_hierarchy_without_optional_properties() {
        let dictionary = dictionary();
        let inventory = parse_inventory(INVENTORY, &dictionary).expect("inventory");
        assert_eq!(inventory.windows.len(), 1);
        assert_eq!(inventory.tabs.len(), 2);
        assert_eq!(inventory.terminals.len(), 3);
        assert_eq!(inventory.terminals[2].tab_id, "tab-B");
        assert_eq!(inventory.terminals[2].id, "terminal-C");
        assert!(
            inventory
                .terminals
                .iter()
                .all(|row| row.tty.is_none() && row.pid.is_none())
        );
        let empty = parse_inventory("I\t1\t0\t0\nE\t0\t0\n", &dictionary).expect("empty");
        assert!(empty.windows.is_empty() && empty.terminals.is_empty());
        assert!(
            parse_inventory(
                &INVENTORY.replace("terminal-A\t-\t-", "terminal-A\t/dev/ttys123\t-"),
                &dictionary
            )
            .is_err()
        );
        assert!(
            parse_inventory(
                &INVENTORY.replace("terminal-A\t-\t-", "terminal-A\t-\t4321"),
                &dictionary
            )
            .is_err()
        );
    }

    #[test]
    fn inventory_rejects_counts_duplicate_ids_and_hierarchy_races() {
        let dictionary = dictionary();
        for broken in [
            INVENTORY.replace("I\t1\t1\t3", "I\t1\t2\t3"),
            INVENTORY.replace("W\twindow-A\ttab-B\t2", "W\twindow-A\ttab-B\t1"),
            INVENTORY.replace("terminal-C\t2", "terminal-C\t3"),
            INVENTORY.replace("terminal-B\t-\t-", "terminal-A\t-\t-"),
            INVENTORY.replace("tab-A", "tab-B"),
            INVENTORY.replace("T\twindow-A", "T\twindow-X"),
            INVENTORY.replace("S\twindow-A\ttab-B", "S\twindow-A\ttab-X"),
            INVENTORY.replace("tab-B\ttrue\tterminal-C", "tab-B\tfalse\tterminal-C"),
            INVENTORY.replace("tab-B\ttrue\tterminal-C", "tab-B\ttrue\tterminal-X"),
            INVENTORY.replace("E\t1\t3", "E\t1\t2"),
            INVENTORY.replace("E\t1\t3\n", ""),
            format!("{INVENTORY}unexpected\n"),
        ] {
            assert!(
                parse_inventory(&broken, &dictionary).is_err(),
                "accepted {broken:?}"
            );
        }
        let duplicate_windows = "I\t1\t2\t2\nW\tw\tt1\t1\nT\tw\tt1\ttrue\ts1\t1\nS\tw\tt1\ts1\t-\t-\nW\tw\tt2\t1\nT\tw\tt2\ttrue\ts2\t1\nS\tw\tt2\ts2\t-\t-\nE\t2\t2\n";
        assert!(parse_inventory(duplicate_windows, &dictionary).is_err());
    }

    #[test]
    fn inventory_rejects_malformed_and_unbounded_records() {
        let dictionary = dictionary();
        for broken in [
            "",
            "CHANGED\n",
            "I\t1\t129\t0\nE\t129\t0\n",
            "I\t1\t0\t4097\nE\t0\t4097\n",
            "I\t1\t-1\t0\n",
            "I\t1\t0\t0\n\nE\t0\t0\n",
        ] {
            assert!(parse_inventory(broken, &dictionary).is_err());
        }
        assert!(
            parse_inventory(
                &INVENTORY.replace("terminal-C", &"x".repeat(257)),
                &dictionary
            )
            .is_err()
        );
        assert!(
            parse_inventory(
                &INVENTORY.replace("terminal-C", "terminal\u{1b}C"),
                &dictionary
            )
            .is_err()
        );
        assert!(parse_inventory(&INVENTORY.replace("false", "1"), &dictionary).is_err());
    }

    #[test]
    fn focus_requires_exact_native_readback_and_activation_when_supported() {
        let dictionary = dictionary();
        assert!(matches!(
            parse_focus(FOCUSED, &dictionary, 1234, &target()).expect("focused"),
            FocusOutcome::Focused(_)
        ));
        for index in 1..=8 {
            let mut fields: Vec<_> = FOCUSED.trim_end().split('\t').collect();
            fields[index] = if index >= 7 { "false" } else { "other-id" };
            assert_eq!(
                parse_focus(&fields.join("\t"), &dictionary, 1234, &target()).expect("changed"),
                FocusOutcome::Changed
            );
        }
        let other = GhosttyTarget {
            terminal_id: "terminal-c".into(),
            ..target()
        };
        assert_eq!(
            parse_focus(FOCUSED, &dictionary, 1234, &other).expect("case matters"),
            FocusOutcome::Changed
        );
        assert_eq!(
            parse_focus("GONE\n", &dictionary, 1234, &target()).expect("gone"),
            FocusOutcome::Gone
        );
        assert_eq!(
            parse_focus("CHANGED", &dictionary, 1234, &target()).expect("changed"),
            FocusOutcome::Changed
        );
        assert_eq!(
            parse_focus("AMBIGUOUS\t2", &dictionary, 1234, &target()).expect("ambiguous"),
            FocusOutcome::Ambiguous { count: 2 }
        );
        for broken in [
            "AMBIGUOUS\t1",
            "AMBIGUOUS\t-2",
            "FOCUSED\tw",
            "GONE\nCHANGED",
            "GONE\t",
            "",
        ] {
            assert!(parse_focus(broken, &dictionary, 1234, &target()).is_err());
        }
    }

    #[test]
    fn absent_dictionary_frontmost_still_requires_independent_pid_readback() {
        let mut dictionary = dictionary();
        dictionary.has_frontmost_property = false;
        let output = FOCUSED.replace("\ttrue\t1234\n", "\t-\t1234\n");
        let FocusOutcome::Focused(readback) =
            parse_focus(&output, &dictionary, 1234, &target()).expect("focused")
        else {
            panic!("focus");
        };
        assert_eq!(readback.app_frontmost, None);
        assert_eq!(readback.frontmost_app_pid, 1234);
        assert_eq!(
            parse_focus(
                &output.replace("\t1234\n", "\t5678\n"),
                &dictionary,
                1234,
                &target()
            )
            .expect("other same-bundle process"),
            FocusOutcome::Changed
        );
        assert!(parse_focus(FOCUSED, &dictionary, 1234, &target()).is_err());
        assert!(parse_focus("FOCUSED\ttrue", &dictionary, 1234, &target()).is_err());
        dictionary.has_focus_command = false;
        assert!(matches!(
            parse_focus(&output, &dictionary, 1234, &target()),
            Err(GhosttyError::UnsupportedDictionary)
        ));
    }

    #[test]
    fn activation_and_pane_readback_require_the_exact_frontmost_application_pid() {
        let dictionary = dictionary();
        for pid in ["0", "5678", "2147483647"] {
            let output = FOCUSED.replace("\t1234\n", &format!("\t{pid}\n"));
            assert_eq!(
                parse_focus(&output, &dictionary, 1234, &target()).expect("wrong frontmost PID"),
                FocusOutcome::Changed
            );
        }
        for pid in ["-1", "1.0", "2147483648", "com.mitchellh.ghostty"] {
            let output = FOCUSED.replace("\t1234\n", &format!("\t{pid}\n"));
            assert!(parse_focus(&output, &dictionary, 1234, &target()).is_err());
        }
        assert!(parse_focus(FOCUSED, &dictionary, 0, &target()).is_err());
    }

    #[test]
    fn installed_dictionary_and_bundled_scripts_compile_without_optional_terms() {
        let dictionary =
            probe_dictionary(Path::new(GHOSTTY_APP_PATH)).expect("installed Ghostty sdef");
        assert!(dictionary.supports_inventory() && dictionary.supports_focus());
        let resources =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/agent-macos/Resources");
        for name in ["ghostty-inventory", "ghostty-focus"] {
            let compiled = std::env::temp_dir()
                .join(format!("threadspace-{name}-{}.scpt", std::process::id()));
            let result = checked(
                "osacompile",
                &BoundedCommand::new("/usr/bin/osacompile", Duration::from_secs(5), 4096)
                    .arg("-l")
                    .arg("JavaScript")
                    .arg("-o")
                    .arg(&compiled)
                    .arg(resources.join(format!("{name}.applescript"))),
            );
            let _ = std::fs::remove_file(&compiled);
            result.expect("compile against installed dictionary");
        }
    }

    #[test]
    fn native_inventory_helpers_compare_exact_ids_and_skip_absent_properties() {
        let script =
            include_str!("../../../apps/agent-macos/Resources/ghostty-inventory.applescript");
        let (helpers, _) = script
            .split_once("function run(argv)")
            .expect("inventory entry point");
        // Execute the bundled helpers without entering the live inventory or
        // sending any Apple events. An absent property must return immediately.
        let source = format!(
            "{helpers}function run(argv) {{ return JSON.stringify([uniqueIds(['same','same']), uniqueIds(['same','Same']), optionalValue(null, '-', 0, 0, 0)]); }}"
        );
        let mut command = BoundedCommand::new("/usr/bin/osascript", Duration::from_secs(5), 4096)
            .arg("-l")
            .arg("JavaScript")
            .arg("-");
        command.stdin = Some(source.into_bytes());
        let output = checked("osascript", &command).expect("native helpers");
        assert_eq!(
            std::str::from_utf8(&output.stdout).expect("text").trim(),
            "[false,true,\"-\"]"
        );
    }

    #[test]
    fn bundles_are_found_at_any_installation_path() {
        for (path, bundle) in [
            ("/Applications/Ghostty.app/Contents/MacOS/ghostty", Some("/Applications/Ghostty.app")),
            ("/Users/u/Applications/Ghostty Tip.app/Contents/MacOS/ghostty", Some("/Users/u/Applications/Ghostty Tip.app")),
            ("/Applications/Ghostty.app/Contents/MacOS/ghostty-helper", None),
            ("/Applications/Ghostty.app/Contents/Resources/ghostty", None),
            ("/usr/local/bin/ghostty", None),
            ("/Applications/Ghostty/Contents/MacOS/ghostty", None),
        ] {
            assert_eq!(
                bundle_of(Path::new(path)).as_deref(),
                bundle.map(Path::new),
                "{path}"
            );
        }
    }

    #[test]
    fn invalid_application_pids_are_rejected_before_starting_a_script() {
        let dictionary = dictionary();
        for pid in [0, -1, i32::MIN] {
            let script = Path::new("/does/not/exist");
            assert!(matches!(
                enumerate(script, &dictionary, pid, Duration::from_secs(1)),
                Err(GhosttyError::Parse(_))
            ));
            assert!(matches!(
                focus(script, &dictionary, pid, &target(), Duration::from_secs(1)),
                Err(GhosttyError::Parse(_))
            ));
        }
    }

    #[test]
    fn bundled_scripts_keep_inventory_optional_queries_and_focus_on_the_numeric_pid() {
        // Lexically replace Application with two inert same-bundle stand-ins.
        // The bundled entry points run unchanged; no real app receives events.
        // Like JXA, a collection answers a property of every element at once
        // (`windows.tabs.id()` is one list of tab IDs per window).
        let fixture = r#"
          function every(read, target = function () { return read(); }) {
            const map = (value, f) => Array.isArray(value) ? value.map(item => map(item, f)) : f(value);
            return new Proxy(target, {
              get: (own, name) => name in own ? own[name] : every(() => map(read(), element => element[name]())),
            });
          }
          function collection(objects) {
            const get = () => objects;
            get.byId = id => objects.find(object => object.id() === id);
            return every(get, get);
          }
          const focused = [];
          const activated = [];
          // Uneven on purpose: window 1 holds a one-terminal tab and a
          // two-terminal tab, window 2 one tab, so a transposed or dropped
          // level changes the rows.
          function makeApp(pid) {
            let active = false;
            const surface = n => ({id: () => 's' + pid + n, tty: () => '/dev/ttys10' + n, pid: () => 100 + n});
            const [s1, s2, s3, s4] = [1, 2, 3, 4].map(surface);
            const tab = (n, terminals, focus) => {
              const t = {id: () => 't' + pid + n, focus, terminals: collection(terminals)};
              t.focusedTerminal = () => t.focus;
              return t;
            };
            const tabs = [tab(1, [s1], s1), tab(2, [s2, s3], s3), tab(3, [s4], s4)];
            const window = (n, own, selected) => {
              const w = {id: () => 'w' + pid + n, current: selected, tabs: collection(own)};
              w.selectedTab = () => w.current;
              return w;
            };
            const windows = [window(1, tabs.slice(0, 2), tabs[1]), window(2, tabs.slice(2), tabs[2])];
            tabs.forEach(t => { t.selected = () => windows.some(w => w.current === t); });
            let front = windows[0];
            return {running: () => true, windows: collection(windows), terminals: collection([s1, s2, s3, s4]),
                    activate: () => { active = true; activated.push(pid); },
                    focus: target => {
                      if (!active) throw Error('Focus preceded activation');
                      windows.forEach(w => w.tabs().forEach(t => {
                        if (t.terminals().includes(target)) { front = w; w.current = t; t.focus = target; }
                      }));
                      focused.push(target.id());
                    },
                    frontWindow: () => front, frontmost: () => active};
          }
          const apps = {1234: makeApp(1234), 5678: makeApp(5678)};
          const requested = [];
          function address(pid) {
            if (typeof pid !== 'number' || !apps[pid]) throw Error('Recipient must be an exact numeric PID');
            requested.push(pid);
            return apps[pid];
          }
        "#;
        for (script, args, expected) in [
            (
                include_str!("../../../apps/agent-macos/Resources/ghostty-inventory.applescript"),
                "['5678','tty','pid','128','1024','4096']",
                "I\t1\t2\t4\n\
                 W\tw56781\tt56782\t2\n\
                 T\tw56781\tt56781\tfalse\ts56781\t1\n\
                 S\tw56781\tt56781\ts56781\t/dev/ttys101\t101\n\
                 T\tw56781\tt56782\ttrue\ts56783\t2\n\
                 S\tw56781\tt56782\ts56782\t/dev/ttys102\t102\n\
                 S\tw56781\tt56782\ts56783\t/dev/ttys103\t103\n\
                 W\tw56782\tt56783\t1\n\
                 T\tw56782\tt56783\ttrue\ts56784\t1\n\
                 S\tw56782\tt56783\ts56784\t/dev/ttys104\t104\n\
                 E\t2\t4\nPID=5678;ACTIVATE=;FOCUS=",
            ),
            (
                include_str!("../../../apps/agent-macos/Resources/ghostty-focus.applescript"),
                "['5678','w56781','t56782','s56782','100','true','128','1024','4096']",
                "FOCUSED\tw56781\tt56782\ts56782\tw56781\tt56782\ts56782\ttrue\ttrue\t5678\nPID=5678;ACTIVATE=5678;FOCUS=s56782",
            ),
            (
                include_str!("../../../apps/agent-macos/Resources/ghostty-focus.applescript"),
                "['5678','w12341','t12341','s12341','100','true','128','1024','4096']",
                "GONE\nPID=5678;ACTIVATE=;FOCUS=",
            ),
        ] {
            let source = format!(
                "function run(argv) {{ {fixture}\nconst output = (function(Application, $) {{ {script}\nreturn run({args}); }})(address, {{NSWorkspace: {{sharedWorkspace: {{frontmostApplication: {{processIdentifier: 5678}}}}}}, NSThread: {{sleepForTimeInterval: function() {{}}}}}}); return output + '\\nPID=' + requested.join(',') + ';ACTIVATE=' + activated.join(',') + ';FOCUS=' + focused.join(','); }}"
            );
            let mut command =
                BoundedCommand::new("/usr/bin/osascript", Duration::from_secs(5), 4096)
                    .arg("-l")
                    .arg("JavaScript")
                    .arg("-");
            command.stdin = Some(source.into_bytes());
            let output = checked("osascript", &command).expect("inert PID fixture");
            assert_eq!(
                std::str::from_utf8(&output.stdout)
                    .expect("text")
                    .trim_end(),
                expected
            );
        }
    }

    #[test]
    fn bundled_scripts_detect_a_replaced_parent_and_bound_each_level() {
        // An inert application counts whole-level reads and can replace the
        // only window (keeping its tab and terminal IDs) after any read. Every
        // replacement before the last read must fail the inventory and make
        // focus answer CHANGED without activating. Each limit must stop the
        // reads at its own level.
        let program = r#"
          let reads = 0, switchAt = Infinity, activations = 0, state;
          function every(read, extra = {}) {
            const target = Object.assign(function () {
              const value = read();
              if (++reads === switchAt) state = after;
              return value;
            }, extra);
            const map = (value, f) => Array.isArray(value) ? value.map(item => map(item, f)) : f(value);
            return new Proxy(target, {
              get: (own, name) => name in own ? own[name] : every(() => map(read(), element => element[name]())),
            });
          }
          const listed = items => Object.assign(() => items, { byId: id => items.find(item => item.id() === id) });
          function hierarchy(windowId) {
            const s1 = { id: () => 's1', tty: () => '/dev/ttys101', pid: () => 101 };
            const t1 = { id: () => 't1', selected: () => true, focusedTerminal: () => s1, terminals: listed([s1]) };
            return { windows: [{ id: () => windowId, selectedTab: () => t1, tabs: listed([t1]) }], terminals: [s1] };
          }
          const before = hierarchy('w1'), after = hierarchy('w-new');
          const app = {
            running: () => true,
            windows: every(() => state.windows, { byId: id => state.windows.find(w => w.id() === id) }),
            terminals: every(() => state.terminals),
            activate: () => { activations++; }, focus: () => {},
            frontWindow: () => state.windows[0], frontmost: () => true,
          };
          const stub = { NSWorkspace: { sharedWorkspace: { frontmostApplication: { processIdentifier: 42 } } },
                         NSThread: { sleepForTimeInterval: () => {} } };
          const inventory = (function (Application, $) { __INVENTORY__
            return run; })(() => app, stub);
          const focus = (function (Application, $) { __FOCUS__
            return run; })(() => app, stub);
          function attempt(script, args, at) {
            reads = 0; switchAt = at; state = before;
            try { return { out: script(args), reads }; } catch (error) { return { out: 'ERROR ' + error.message, reads }; }
          }
          function run(argv) {
            const inventoryArgs = limits => ['42', 'tty', '-', ...limits];
            const focusArgs = limits => ['42', 'w1', 't1', 's1', '100', 'true', ...limits];
            const open = ['8', '8', '8'];
            const failures = [];
            const inventoryReads = attempt(inventory, inventoryArgs(open), Infinity).reads;
            for (let at = 1; at < inventoryReads; at++) {
              const result = attempt(inventory, inventoryArgs(open), at);
              if (!result.out.startsWith('ERROR')) failures.push('inventory after read ' + at + ': ' + result.out.split('\n')[1]);
            }
            const focusReads = attempt(focus, focusArgs(open), Infinity).reads;
            for (let at = 1; at < focusReads; at++) {
              activations = 0;
              const result = attempt(focus, focusArgs(open), at);
              if (result.out !== 'CHANGED' || activations) failures.push('focus after read ' + at + ': ' + result.out);
            }
            const bounded = [['0', '8', '8'], ['8', '0', '8'], ['8', '8', '0']].map(limits => [
              attempt(inventory, inventoryArgs(limits), Infinity), attempt(focus, focusArgs(limits), Infinity),
            ].map(result => (result.out.includes('exceeds limit') ? 'limit' : result.out) + '@' + result.reads).join(' '));
            return [inventoryReads >= 8 && focusReads >= 8 ? 'reads ok' : 'reads ' + inventoryReads + ' ' + focusReads,
                    'failures ' + (failures.join('; ') || 'none'), 'bounded ' + bounded.join(', ')].join('\n');
          }
        "#
        .replace(
            "__INVENTORY__",
            include_str!("../../../apps/agent-macos/Resources/ghostty-inventory.applescript"),
        )
        .replace(
            "__FOCUS__",
            include_str!("../../../apps/agent-macos/Resources/ghostty-focus.applescript"),
        );
        let mut command = BoundedCommand::new("/usr/bin/osascript", Duration::from_secs(5), 4096)
            .arg("-l")
            .arg("JavaScript")
            .arg("-");
        command.stdin = Some(program.into_bytes());
        let output = checked("osascript", &command).expect("inert replacement fixture");
        assert_eq!(
            std::str::from_utf8(&output.stdout).expect("text").trim_end(),
            "reads ok\nfailures none\nbounded limit@1 limit@1, limit@2 limit@2, limit@3 limit@3"
        );
    }
}
