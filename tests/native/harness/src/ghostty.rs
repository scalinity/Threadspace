//! Disposable Ghostty processes and panes for session qualification.
//!
//! Apple events addressed to a PID are delivered to the application
//! registered at that PID's bundle path. A second process launched from an
//! installation that is already running would therefore be answered by the
//! running one (an owner's Ghostty). Each fixture runs its own copy of the
//! installed bundle at a new private path, acquires the one process running
//! that exact executable, and proves the addressed process is new and empty
//! before creating anything. Every surface it touches was created by it, and
//! cleanup requires the terminal ID set to be exactly its own.
//!
//! Values travel to JXA as argv, never spliced into script source.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use threadspace_surfaces_macos::{ghostty, process};

use crate::procs::{self, Incarnation};
use crate::run::run;

/// Command-line settings of a disposable process: no owner configuration,
/// no restored windows, no updates, and no confirmation prompts.
const LAUNCH_FLAGS: [&str; 7] = [
    "--config-default-files=false",
    "--window-save-state=never",
    "--initial-window=false",
    "--auto-update=off",
    "--quit-after-last-window-closed=false",
    "--confirm-close-surface=false",
    "--shell-integration=none",
];

/// A harness-created terminal. Window and tab IDs are current as of the
/// last [`Fixture::locate`]; the terminal ID and TTY never change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub window_id: String,
    pub tab_id: String,
    /// The tab's position in its window when last located.
    pub tab_index: i64,
    pub terminal_id: String,
    pub tty: String,
    pub dir: PathBuf,
}

pub struct Fixture {
    /// Private directory holding the copied bundle.
    pub root: PathBuf,
    pub bundle: PathBuf,
    pub executable: PathBuf,
    pub app: Incarnation,
    /// `dev:ino` of the mapped executable image.
    pub image: String,
    /// SHA-256 of the copy's dictionary, qualified before the first event.
    pub dictionary_sha256: String,
    script: PathBuf,
    owned: Mutex<Vec<String>>,
    closed: Mutex<bool>,
}

fn parse(text: &str) -> Result<Value, String> {
    serde_json::from_str(text.trim()).map_err(|error| format!("fixture output: {error}: {text}"))
}

fn pane_of(value: &Value, dir: &Path) -> Result<Pane, String> {
    let field = |name: &str| {
        value[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("fixture pane without {name}: {value}"))
    };
    Ok(Pane {
        window_id: field("windowId")?,
        tab_id: field("tabId")?,
        tab_index: value["tabIndex"].as_i64().unwrap_or(-1),
        terminal_id: field("terminalId")?,
        tty: field("tty")?,
        dir: dir.to_path_buf(),
    })
}

impl Fixture {
    /// Copies `source` (an installed Ghostty.app) under the new private
    /// directory `root`, launches the copy and acquires its process.
    pub fn launch(source: &Path, root: &Path, script: &Path) -> Result<Self, String> {
        std::fs::create_dir(root).map_err(|error| format!("fixture root must be new: {error}"))?;
        let bundle = root.join("Ghostty.app");
        let copied = run(
            "/usr/bin/ditto",
            &[&source.display().to_string(), &bundle.display().to_string()],
            Duration::from_secs(120),
        );
        if !copied.ok {
            return Err(format!("ditto: {}", copied.stderr.trim()));
        }
        let verify = run(
            "/usr/bin/codesign",
            &["--verify", "--deep", "--strict", &bundle.display().to_string()],
            Duration::from_secs(60),
        );
        if !verify.ok {
            return Err(format!("copied bundle fails codesign: {}", verify.stderr.trim()));
        }
        let executable = bundle
            .join("Contents/MacOS/ghostty")
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !procs::with_executable(&executable).is_empty() {
            return Err("a process already runs the new copy".into());
        }
        let mut argv = vec!["-n", "-g", "-a"];
        let bundle_text = bundle.display().to_string();
        argv.push(&bundle_text);
        argv.push("--args");
        argv.extend_from_slice(&LAUNCH_FLAGS);
        let opened = run("/usr/bin/open", &argv, Duration::from_secs(30));
        if !opened.ok {
            return Err(format!("open: {}", opened.stderr.trim()));
        }
        let started = Instant::now();
        let app = loop {
            match procs::with_executable(&executable).as_slice() {
                [one] => break one.clone(),
                [] if started.elapsed() < Duration::from_secs(15) => crate::pause_ms(100),
                [] => return Err("the copy never started".into()),
                many => return Err(format!("{} processes run the copy", many.len())),
            }
        };
        let path = executable.display().to_string();
        let image = process::mapped_image_file_id(app.pid)
            .map_err(|error| error.to_string())?
            .ok_or("the copy's image is not mapped")?;
        if process::path_file_id(&path).as_deref() != Some(image.as_str()) {
            return Err("the acquired process does not run the copy".into());
        }
        // The copy's own dictionary, read with its executable unchanged, must
        // support the terms the fixture scripts (including terminal TTYs).
        let dictionary = ghostty::probe_dictionary(&bundle).map_err(|error| error.to_string())?;
        if process::path_file_id(&path).as_deref() != Some(image.as_str()) {
            return Err("the copy changed while its dictionary was read".into());
        }
        if !dictionary.supports_focus() || !dictionary.has_tty_property {
            let _ = procs::signal(app.pid, libc::SIGTERM);
            return Err("the copy's dictionary lacks TTY or focus support".into());
        }
        std::fs::create_dir(root.join("zdotdir")).map_err(|error| error.to_string())?;
        let fixture = Self {
            root: root.to_path_buf(),
            bundle,
            executable,
            app,
            image,
            dictionary_sha256: dictionary.sha256,
            script: script.to_path_buf(),
            owned: Mutex::new(Vec::new()),
            closed: Mutex::new(false),
        };
        // Addressing control: only the new process can answer empty. A
        // non-empty answer means events reached another process: stop
        // without sending any creating event, and end the copy by signal.
        let count = fixture.call("count", &[])?;
        if count["windows"] != json!(0) || count["terminals"] != json!(0) {
            fixture.terminate();
            return Err(format!("addressing control failed: {count}"));
        }
        Ok(fixture)
    }

    /// One JXA call to exactly this process, which must still be the sole
    /// process running the copy.
    fn call(&self, op: &str, args: &[&str]) -> Result<Value, String> {
        if !self.app.alive() {
            return Err("fixture process is gone".into());
        }
        if procs::with_executable(&self.executable).len() != 1 {
            return Err("the copy is no longer run by exactly one process".into());
        }
        if process::mapped_image_file_id(self.app.pid).ok().flatten().as_deref() != Some(self.image.as_str()) {
            return Err("the fixture process no longer maps the copy's image".into());
        }
        let pid = self.app.pid.to_string();
        let script = self.script.display().to_string();
        let mut argv = vec!["-l", "JavaScript", script.as_str(), pid.as_str(), op];
        argv.extend_from_slice(args);
        let out = run("/usr/bin/osascript", &argv, Duration::from_secs(20));
        if !out.ok {
            return Err(format!("fixture {op}: {}", out.stderr.trim()));
        }
        parse(&out.stdout)
    }

    fn adopt(&self, value: &Value, dir: &Path) -> Result<Pane, String> {
        let pane = pane_of(value, dir)?;
        self.owned
            .lock()
            .map_err(|_| "owned list poisoned")?
            .push(pane.terminal_id.clone());
        Ok(pane)
    }

    /// A creating call that failed, or whose answer named no complete pane,
    /// may still have created a terminal. The process was proven empty and is
    /// used only by this fixture, so every terminal it holds is the fixture's:
    /// record any not yet owned.
    fn adopt_strays(&self) {
        let Ok(ids) = self.call("ids", &[]) else {
            return;
        };
        if let (Ok(mut owned), Some(ids)) = (self.owned.lock(), ids["ids"].as_array()) {
            for id in ids.iter().filter_map(Value::as_str) {
                if !owned.iter().any(|known| known == id) {
                    owned.push(id.to_owned());
                }
            }
        }
    }

    fn create(&self, op: &str, args: &[&str], dir: &Path) -> Result<Pane, String> {
        let created = self.call(op, args).and_then(|value| self.adopt(&value, dir));
        if created.is_err() {
            self.adopt_strays();
        }
        created
    }

    /// Fixture shells read no owner shell configuration: `ZDOTDIR` names an
    /// empty fixture directory, so the owner's startup files (and the shell
    /// history they configure) are neither read nor written.
    fn environment(&self) -> String {
        json!([format!("ZDOTDIR={}", self.root.join("zdotdir").display())]).to_string()
    }

    pub fn window(&self, dir: &Path) -> Result<Pane, String> {
        let env = self.environment();
        self.create("window", &[&dir.display().to_string(), &env], dir)
    }

    pub fn tab(&self, beside: &Pane, dir: &Path) -> Result<Pane, String> {
        let env = self.environment();
        self.create("tab", &[&beside.window_id, &dir.display().to_string(), &env], dir)
    }

    pub fn split(&self, of: &Pane, direction: &str, dir: &Path) -> Result<Pane, String> {
        let env = self.environment();
        self.create("split", &[&of.terminal_id, direction, &dir.display().to_string(), &env], dir)
    }

    /// The pane's current window and tab.
    pub fn locate(&self, pane: &Pane) -> Result<Pane, String> {
        let value = self.call("locate", &[&pane.terminal_id])?;
        pane_of(&value, &pane.dir)
    }

    pub fn input(&self, pane: &Pane, text: &str) -> Result<(), String> {
        self.call("input", &[&pane.terminal_id, text]).map(|_| ())
    }

    /// A key by Ghostty's AppleScript key name (`enter`, `arrowDown`, `c`).
    pub fn key(&self, pane: &Pane, key: &str, modifiers: Option<&str>) -> Result<(), String> {
        let mut args = vec![pane.terminal_id.as_str(), key];
        if let Some(modifiers) = modifiers {
            args.push(modifiers);
        }
        self.call("key", &args).map(|_| ())
    }

    /// Pastes `text` and presses Return, as a person submits a line.
    pub fn submit(&self, pane: &Pane, text: &str) -> Result<(), String> {
        self.input(pane, text)?;
        crate::pause_ms(150);
        self.key(pane, "enter", None)
    }

    /// A Ghostty keybinding action (`toggle_fullscreen`, `move_tab:1`).
    pub fn action(&self, pane: &Pane, action: &str) -> Result<Value, String> {
        self.call("action", &[&pane.terminal_id, action])
    }

    /// Runs `tool args` (a native helper emitting JSON) in `control`, an owned
    /// pane. Processes the agent CLI starts are not credited to the terminal
    /// that hosts it, so they lack its Accessibility grant; a command started
    /// by the copy's own shell is credited to the copy, which shares the
    /// installed Ghostty's signed identity and grant.
    pub fn native(&self, control: &Pane, tool: &Path, args: &[&str]) -> Result<Value, String> {
        let stem = self.root.join(format!("native-{}", uuid::Uuid::new_v4()));
        let output = stem.with_extension("json");
        let done = stem.with_extension("done");
        let quote = crate::terminal::shell_quote;
        let mut command = quote(&tool.display().to_string());
        for arg in args {
            command.push(' ');
            command.push_str(&quote(arg));
        }
        command.push_str(&format!(
            " > {} 2>&1; : > {}",
            quote(&output.display().to_string()),
            quote(&done.display().to_string())
        ));
        self.submit(control, &command)?;
        let started = Instant::now();
        while !done.exists() {
            if started.elapsed() > Duration::from_secs(20) {
                return Err("native helper did not finish in the control pane".into());
            }
            crate::pause_ms(100);
        }
        let text = std::fs::read_to_string(&output).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(&done);
        serde_json::from_str(text.trim()).map_err(|_| format!("native helper: {}", text.trim()))
    }

    /// Gives the pane's tab a unique title, so Accessibility can find its window.
    pub fn title(&self, pane: &Pane, marker: &str) -> Result<(), String> {
        self.call("title", &[&pane.terminal_id, marker]).map(|_| ())
    }

    pub fn select_tab(&self, pane: &Pane) -> Result<(), String> {
        self.call("select-tab", &[&pane.tab_id]).map(|_| ())
    }

    /// Activates the process and focuses `pane` (the fixture's own focus,
    /// used to put another pane in front before a Return).
    pub fn focus(&self, pane: &Pane) -> Result<Value, String> {
        self.call("focus", &[&pane.terminal_id])
    }

    /// Front window, its selected tab and that tab's focused terminal.
    pub fn readback(&self) -> Result<Value, String> {
        self.call("readback", &[])
    }

    pub fn inventory(&self) -> Result<Value, String> {
        self.call("inventory", &[])
    }

    /// The pane's TTY is held by a login process this fixture started.
    pub fn owns_tty(&self, pane: &Pane) -> bool {
        let Some(name) = pane.tty.strip_prefix("/dev/") else {
            return false;
        };
        let listing = run("/bin/ps", &["-t", name, "-o", "pid=,ppid=,comm="], Duration::from_secs(5));
        listing.stdout.lines().any(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            matches!(fields.as_slice(), [_, ppid, "/usr/bin/login"] if *ppid == self.app.pid.to_string())
        })
    }

    pub fn close_pane(&self, pane: &Pane) -> Result<Value, String> {
        let owned = self.owned_json()?;
        let result = self.call("close-terminal", &[&pane.terminal_id, &owned])?;
        self.owned
            .lock()
            .map_err(|_| "owned list poisoned")?
            .retain(|id| *id != pane.terminal_id);
        Ok(result)
    }

    fn owned_json(&self) -> Result<String, String> {
        let owned = self.owned.lock().map_err(|_| "owned list poisoned")?.clone();
        serde_json::to_string(&owned).map_err(|error| error.to_string())
    }

    /// Ends the copy's process by signal, after proving it is still this
    /// fixture's childless process; used only when events may be misdirected.
    fn terminate(&self) {
        if self.app.alive() && procs::with_executable(&self.executable).len() == 1 {
            let _ = procs::signal(self.app.pid, libc::SIGTERM);
            let _ = procs::wait_exit(&self.app, Duration::from_secs(10));
        }
    }

    /// Closes every owned window and quits the copy, then removes the copied
    /// bundle. The terminal set must be exactly the owned set.
    pub fn close(&self) -> Value {
        let mut closed = match self.closed.lock() {
            Ok(guard) => guard,
            Err(_) => return json!({ "ok": false, "error": "close state poisoned" }),
        };
        if *closed {
            return json!({ "ok": true, "already": true });
        }
        let outcome = self.owned_json().and_then(|owned| self.call("close", &[&owned]));
        let exited = procs::wait_exit(&self.app, Duration::from_secs(10));
        let removed = exited.is_some() && std::fs::remove_dir_all(&self.bundle).is_ok();
        *closed = exited.is_some();
        json!({
            "ok": outcome.is_ok() && exited.is_some() && removed,
            "result": outcome.as_ref().ok(),
            "error": outcome.as_ref().err(),
            "exitedMs": exited,
            "copyRemoved": removed,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// PID of the native frontmost application (NSWorkspace in a separate
/// process), independent of the companion and of Ghostty's own scripting.
pub fn frontmost_pid() -> Option<i32> {
    let out = run(
        "/usr/bin/osascript",
        &[
            "-l",
            "JavaScript",
            "-e",
            "ObjC.import('AppKit'); $.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier",
        ],
        Duration::from_secs(5),
    );
    out.ok.then(|| out.stdout.trim().parse().ok()).flatten()
}
