//! The desktop UI process: packaged launch through LaunchServices, `tauri dev`
//! launch in its own process group, hydration observed in the companion log,
//! quit/kill by verified incarnation, and the qualification reports the
//! renderer records natively.

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::companion::{Companion, LogCursor};
use crate::identity::Identity;
use crate::procs::{self, Incarnation};
use crate::run::run;

pub struct App<'a> {
    pub id: &'a Identity,
}

#[derive(Debug, Clone)]
pub struct Launched {
    pub started_ms: i64,
    pub ui: Incarnation,
    pub process_appeared_ms: u64,
}

impl<'a> App<'a> {
    pub fn new(id: &'a Identity) -> Self {
        Self { id }
    }

    pub fn processes(&self) -> Vec<Incarnation> {
        procs::with_executable(&self.id.executable)
    }

    /// `open -a <bundle> [--args ...]`, then waits for a new UI incarnation.
    pub fn launch_packaged(&self, args: &[&str]) -> Result<Launched, String> {
        let before = self.processes();
        let started_ms = crate::now_ms();
        let started = Instant::now();
        let bundle = self.id.bundle.display().to_string();
        let mut argv = vec!["-a", bundle.as_str()];
        if !args.is_empty() {
            argv.push("--args");
            argv.extend_from_slice(args);
        }
        let out = run("/usr/bin/open", &argv, Duration::from_secs(20));
        if !out.ok {
            return Err(format!("open failed: {}", out.stderr.trim()));
        }
        while started.elapsed() < Duration::from_secs(20) {
            if let Some(ui) = self.processes().into_iter().find(|p| !before.contains(p)) {
                return Ok(Launched {
                    started_ms,
                    ui,
                    process_appeared_ms: started.elapsed().as_millis() as u64,
                });
            }
            crate::pause_ms(50);
        }
        Err("no new UI process appeared".into())
    }

    /// Waits in the companion log for a view that attached and hydrated after
    /// `cursor` was taken; returns (attached, hydrated) lines.
    pub fn wait_hydrated(
        &self,
        cursor: &mut LogCursor,
        timeout: Duration,
    ) -> Option<(Value, Value)> {
        let mut seen = Vec::new();
        let attached = cursor.wait_for("VIEW_ATTACHED", |_| true, timeout, &mut seen)?;
        let subscription = attached["subscriptionId"].as_str()?.to_owned();
        let hydrated = cursor.wait_for(
            "VIEW_HYDRATED",
            |line| line["subscriptionId"].as_str() == Some(subscription.as_str()),
            timeout,
            &mut seen,
        )?;
        Some((attached, hydrated))
    }

    /// Ends a UI incarnation with SIGTERM (or SIGKILL), verifying the exit.
    pub fn stop(&self, ui: &Incarnation, kill: bool) -> Option<u64> {
        if ui.alive() {
            procs::signal(ui.pid, if kill { libc::SIGKILL } else { libc::SIGTERM });
        }
        procs::wait_exit(ui, Duration::from_secs(10))
    }

    /// Stops every UI process of this identity.
    pub fn stop_all(&self) {
        for ui in self.processes() {
            self.stop(&ui, false);
        }
    }

    /// Qualification reports of `kind` written at or after `since_ms`.
    pub fn reports(&self, kind: &str, since_ms: i64) -> Vec<(PathBuf, Value)> {
        let Ok(entries) = fs::read_dir(self.id.reports_dir()) else {
            return Vec::new();
        };
        let mut found: Vec<(PathBuf, Value)> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with(&format!("{kind}-")) && name.ends_with(".json")
                    })
            })
            .filter_map(|path| {
                let value: Value = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
                (value["recordedAtMs"].as_i64()? >= since_ms).then_some((path, value))
            })
            .collect();
        found.sort_by_key(|(_, value)| value["recordedAtMs"].as_i64().unwrap_or(0));
        found
    }

    pub fn wait_report(
        &self,
        kind: &str,
        since_ms: i64,
        matches: impl Fn(&Value) -> bool,
        timeout: Duration,
    ) -> Option<(PathBuf, Value)> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if let Some(hit) = self
                .reports(kind, since_ms)
                .into_iter()
                .find(|(_, v)| matches(v))
            {
                return Some(hit);
            }
            crate::pause_ms(200);
        }
        None
    }

    /// Sends a qualification command to the hydrated view and waits for its
    /// `qualification-command` report.
    pub fn view_command(
        &self,
        command: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let since = crate::now_ms() - 5;
        let (intent_id, views) = Companion::new(self.id).view_command(command, args)?;
        if views == 0 {
            return Err("no hydrated view to receive the command".into());
        }
        self.wait_report(
            "qualification-command",
            since,
            |report| report["report"]["intentId"].as_str() == Some(intent_id.as_str()),
            timeout,
        )
        .map(|(_, report)| report["report"].clone())
        .ok_or_else(|| format!("no report for {command} within {timeout:?}"))
    }

    pub fn desktop_log(&self) -> LogCursor {
        LogCursor::at_end(self.id.desktop_log())
    }
}

/// A `tauri dev` session in its own process group (Vite, cargo and the app),
/// stopped as a whole so no dev server outlives it.
pub struct DevSession {
    child: Child,
    pub started_ms: i64,
}

impl DevSession {
    pub fn start(repo: &Path, log: &Path, extra_args: &[&str]) -> Result<Self, String> {
        let log_file = fs::File::create(log).map_err(|e| e.to_string())?;
        let err_file = log_file.try_clone().map_err(|e| e.to_string())?;
        let mut command = Command::new("/opt/homebrew/bin/npx");
        command
            .current_dir(repo.join("apps/desktop"))
            .args([
                "--no-install",
                "tauri",
                "dev",
                "--config",
                "src-tauri/tauri.dev.conf.json",
                "--features",
                "qualification",
            ])
            .stdin(Stdio::null())
            .stdout(log_file)
            .stderr(err_file)
            .process_group(0);
        if !extra_args.is_empty() {
            command.arg("--").arg("--");
            command.args(extra_args);
        }
        let child = command.spawn().map_err(|e| e.to_string())?;
        Ok(Self {
            child,
            started_ms: crate::now_ms(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// SIGTERM to the whole group, then SIGKILL after a grace period.
    pub fn stop(mut self) -> Value {
        let group = -(self.child.id() as i32);
        procs::signal(group, libc::SIGTERM);
        let started = Instant::now();
        let mut exited = false;
        while started.elapsed() < Duration::from_secs(10) {
            if let Ok(Some(_)) = self.child.try_wait() {
                exited = true;
                break;
            }
            crate::pause_ms(100);
        }
        if !exited {
            procs::signal(group, libc::SIGKILL);
            let _ = self.child.wait();
        }
        // Nothing of the group may keep the dev port.
        let lsof = run(
            "/usr/sbin/lsof",
            &["-nP", "-iTCP:1420", "-sTCP:LISTEN", "-t"],
            Duration::from_secs(5),
        );
        json!({
            "graceful": exited,
            "stopMs": started.elapsed().as_millis() as u64,
            "port1420Listeners": lsof.stdout.split_whitespace().count(),
        })
    }
}
