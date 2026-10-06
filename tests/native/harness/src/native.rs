//! The `ts-native` Swift helper (tests/native/swift/ts-native.swift):
//! Accessibility window inspection and actions, notification banners,
//! synthetic input, pixel statistics, owner idle time and displays.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::run::{Output, run};

pub struct Native {
    binary: PathBuf,
}

impl Native {
    /// Builds the helper into `target/native-tools/` when it is missing or
    /// older than its source.
    pub fn ensure(repo: &Path) -> Result<Self, String> {
        let source = repo.join("tests/native/swift/ts-native.swift");
        let binary = repo.join("target/native-tools/ts-native");
        let stale = match (binary.metadata(), source.metadata()) {
            (Ok(built), Ok(src)) => match (built.modified(), src.modified()) {
                (Ok(built), Ok(src)) => built < src,
                _ => true,
            },
            _ => true,
        };
        if stale {
            std::fs::create_dir_all(binary.parent().unwrap_or(repo)).map_err(|e| e.to_string())?;
            let out = run(
                "/usr/bin/swiftc",
                &[
                    "-O",
                    "-target",
                    "arm64-apple-macos26.0",
                    &source.display().to_string(),
                    "-o",
                    &binary.display().to_string(),
                ],
                Duration::from_secs(300),
            );
            if !out.ok {
                return Err(format!(
                    "swiftc failed: {}",
                    out.stderr.chars().take(400).collect::<String>()
                ));
            }
        }
        Ok(Self { binary })
    }

    pub fn call(&self, args: &[&str], timeout: Duration) -> Output {
        run(&self.binary.display().to_string(), args, timeout)
    }

    /// Runs a subcommand and returns its JSON report (even on a failed check).
    pub fn json(&self, args: &[&str]) -> Value {
        let out = self.call(args, Duration::from_secs(30));
        out.json().unwrap_or_else(
            || serde_json::json!({ "error": out.stderr.trim(), "status": out.status }),
        )
    }

    /// Starts a long-running subcommand (such as `display-mode-hold`) whose
    /// first stdout line is its JSON report.
    pub fn spawn(&self, args: &[&str]) -> std::io::Result<std::process::Child> {
        std::process::Command::new(&self.binary)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
    }

    pub fn idle_seconds(&self) -> f64 {
        self.json(&["idle"])["idleSeconds"].as_f64().unwrap_or(0.0)
    }

    /// The accessibility tree of `pid`. WebKit builds a web view's tree in its
    /// web process on the first request, which therefore lists only the
    /// native chrome; this asks again until the web area appears.
    pub fn ax_tree(&self, pid: u32, depth: u32) -> Value {
        let (pid, depth) = (pid.to_string(), depth.to_string());
        let mut tree = Value::Null;
        for _ in 0..5 {
            tree = self.json(&["ax-tree", &pid, &depth]);
            if tree["nodes"]
                .as_array()
                .is_some_and(|nodes| nodes.iter().any(|n| n["role"] == "AXWebArea"))
            {
                break;
            }
            crate::pause_ms(1000);
        }
        tree
    }

    pub fn ax_window(&self, pid: u32, title: Option<&str>) -> Value {
        let pid = pid.to_string();
        let mut args = vec!["ax-window", pid.as_str()];
        if let Some(title) = title {
            args.push(title);
        }
        self.json(&args)
    }

    pub fn ax_action(&self, pid: u32, action: &str, title: Option<&str>) -> Value {
        let pid = pid.to_string();
        let mut args = vec!["ax-action", pid.as_str(), action];
        if let Some(title) = title {
            args.push(title);
        }
        self.json(&args)
    }

    /// Captures one window (by CG window id) to `path`, shadowless.
    pub fn capture_window(window_id: u64, path: &Path) -> Output {
        run(
            "/usr/sbin/screencapture",
            &[
                "-x",
                "-o",
                "-l",
                &window_id.to_string(),
                &path.display().to_string(),
            ],
            Duration::from_secs(20),
        )
    }

    /// The largest on-screen window owned by `pid`, if any.
    pub fn main_window_id(&self, pid: u32) -> Option<u64> {
        let report = self.json(&["windows", &pid.to_string()]);
        report["windows"]
            .as_array()?
            .iter()
            .filter(|w| w["layer"].as_i64() == Some(0))
            .max_by(|a, b| {
                let area = |w: &&Value| {
                    w["width"].as_f64().unwrap_or(0.0) * w["height"].as_f64().unwrap_or(0.0)
                };
                area(a).total_cmp(&area(b))
            })
            .and_then(|w| w["id"].as_u64())
    }
}
