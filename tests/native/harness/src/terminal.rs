//! Disposable Terminal.app windows for routing qualification. The harness only
//! ever manipulates windows it created itself, and proves ownership before
//! each destructive step: the window ID it recorded, a tab whose TTY it
//! recorded, and a process on that TTY that is either its marker process or a
//! provider running in its own disposable directory. Unrelated Terminal
//! windows (including the one hosting the harness) are never touched.
//!
//! Values travel to AppleScript as argv, never spliced into script source,
//! except the harness-generated marker, which is a UUID.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::procs::{self, Incarnation};
use crate::run::{osascript, run};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub marker: String,
    pub window_id: i64,
    pub tty: String,
    /// Disposable working directory the tab's command runs in.
    pub dir: PathBuf,
}

const OPEN: &str = r#"on run argv
set sep to character id 9
tell application "Terminal"
  set t to do script (item 1 of argv)
  delay 0.4
  set custom title of t to (item 2 of argv)
  set ttyPath to tty of t
  repeat with w in windows
    repeat with x in tabs of w
      if tty of x is ttyPath then return ((id of w) as text) & sep & ttyPath
    end repeat
  end repeat
end tell
return ""
end run"#;

const WINDOW_TTYS: &str = r#"on run argv
set out to ""
tell application "Terminal"
  repeat with x in tabs of window id ((item 1 of argv) as integer)
    set out to out & (tty of x) & linefeed
  end repeat
end tell
return out
end run"#;

const TYPE_INTO: &str = r#"on run argv
tell application "Terminal"
  repeat with x in tabs of window id ((item 1 of argv) as integer)
    if tty of x is (item 2 of argv) then
      do script (item 3 of argv) in x
      return "ok"
    end if
  end repeat
end tell
return "missing"
end run"#;

const CLOSE: &str = r#"on run argv
tell application "Terminal" to close (window id ((item 1 of argv) as integer))
return "closed"
end run"#;

const SET_BOUNDS: &str = r#"on run argv
tell application "Terminal" to set bounds of window id ((item 1 of argv) as integer) to {(item 2 of argv) as integer, (item 3 of argv) as integer, (item 4 of argv) as integer, (item 5 of argv) as integer}
return "ok"
end run"#;

const SELECT_TAB: &str = r#"on run argv
tell application "Terminal"
  set w to window id ((item 1 of argv) as integer)
  repeat with x in tabs of w
    if tty of x is (item 2 of argv) then
      set selected of x to true
      return "ok"
    end if
  end repeat
end tell
return "missing"
end run"#;

const SELECTED_TTY: &str =
    r#"tell application "Terminal" to return tty of selected tab of front window"#;

fn timeout() -> Duration {
    Duration::from_secs(15)
}

/// The single live Terminal.app process (kernel executable path).
pub fn terminal_process() -> Vec<Incarnation> {
    procs::with_executable(std::path::Path::new(
        "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal",
    ))
}

impl Tab {
    /// Opens a new window running `command` (in `dir`) under a marker title.
    pub fn open(dir: PathBuf, command: &str) -> Result<Self, String> {
        let marker = uuid::Uuid::new_v4().to_string();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let title = format!("TSQ-{marker}");
        let out = osascript(OPEN, &[command, &title], timeout());
        let text = out.stdout.trim().to_owned();
        let (window, tty) = text
            .split_once('\t')
            .ok_or_else(|| format!("open failed: {} {}", text, out.stderr.trim()))?;
        Ok(Self {
            marker,
            window_id: window
                .parse()
                .map_err(|_| format!("bad window id {window}"))?,
            tty: tty.to_owned(),
            dir,
        })
    }

    /// A tab that just discards its input: harmless under stray keystrokes.
    pub fn open_inert(dir: PathBuf) -> Result<Self, String> {
        let marker_dir = dir.clone();
        let command = format!(
            "cd {} && exec /bin/sh -c 'while read -r _; do :; done' tsq-inert",
            shell_quote(&marker_dir.display().to_string())
        );
        Self::open(dir, &command)
    }

    pub fn title(&self) -> String {
        format!("TSQ-{}", self.marker)
    }

    /// Processes whose controlling terminal is this tab's TTY.
    pub fn processes(&self) -> Vec<(i32, String)> {
        let device = self.tty.trim_start_matches("/dev/");
        let out = run(
            "/bin/ps",
            &["-t", device, "-o", "pid=,command="],
            Duration::from_secs(5),
        );
        out.stdout
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let (pid, command) = line.split_once(' ')?;
                Some((pid.parse().ok()?, command.trim().to_owned()))
            })
            .collect()
    }

    /// Ownership: the recorded window still holds the recorded TTY, and a
    /// process on it runs in this tab's disposable directory.
    pub fn owned(&self) -> Value {
        let ttys = osascript(WINDOW_TTYS, &[&self.window_id.to_string()], timeout());
        let window_has_tty = ttys.stdout.lines().any(|line| line.trim() == self.tty);
        let dir = self.dir.canonicalize().unwrap_or_else(|_| self.dir.clone());
        let in_dir: Vec<i32> = self
            .processes()
            .into_iter()
            .filter(|(pid, _)| {
                procs::cwd(*pid).is_some_and(|cwd| {
                    PathBuf::from(cwd).canonicalize().ok().as_ref() == Some(&dir)
                })
            })
            .map(|(pid, _)| pid)
            .collect();
        json!({ "windowHasTty": window_has_tty, "processesInDir": in_dir, "owned": window_has_tty && !in_dir.is_empty() })
    }

    /// Types `text` + Return into this tab (setup of the harness's own tab).
    pub fn type_line(&self, text: &str) -> bool {
        osascript(
            TYPE_INTO,
            &[&self.window_id.to_string(), &self.tty, text],
            timeout(),
        )
        .stdout
        .trim()
            == "ok"
    }

    pub fn select(&self) -> bool {
        osascript(
            SELECT_TAB,
            &[&self.window_id.to_string(), &self.tty],
            timeout(),
        )
        .stdout
        .trim()
            == "ok"
    }

    pub fn set_bounds(&self, left: i32, top: i32, right: i32, bottom: i32) -> bool {
        osascript(
            SET_BOUNDS,
            &[
                &self.window_id.to_string(),
                &left.to_string(),
                &top.to_string(),
                &right.to_string(),
                &bottom.to_string(),
            ],
            timeout(),
        )
        .ok
    }

    /// Ends this tab's processes (only those in its directory or its marker
    /// process), then closes its window. Refuses if ownership is unproven.
    pub fn close(&self) -> Value {
        let ownership = self.owned();
        if ownership["windowHasTty"] != json!(true) {
            return json!({ "closed": false, "reason": "window no longer holds the recorded TTY", "ownership": ownership });
        }
        let dir = self.dir.canonicalize().unwrap_or_else(|_| self.dir.clone());
        let mut ended = Vec::new();
        for (pid, command) in self.processes() {
            let in_dir = procs::cwd(pid)
                .is_some_and(|cwd| PathBuf::from(cwd).canonicalize().ok().as_ref() == Some(&dir));
            if in_dir || command.contains("tsq-inert") {
                procs::signal(pid, libc::SIGHUP);
                ended.push(pid);
            }
        }
        crate::pause_ms(800);
        // Terminal may already have closed the window when its shell ended.
        let close = osascript(CLOSE, &[&self.window_id.to_string()], timeout());
        let remains = osascript(WINDOW_TTYS, &[&self.window_id.to_string()], timeout()).ok;
        json!({ "closed": !remains, "closedByHarness": close.ok, "endedPids": ended, "ownership": ownership })
    }
}

pub fn selected_tty() -> Option<String> {
    let out = osascript(SELECTED_TTY, &[], timeout());
    out.ok.then(|| out.stdout.trim().to_owned())
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}
