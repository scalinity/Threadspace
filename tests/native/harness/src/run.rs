//! Bounded process execution for the harness: absolute-path argv only, no
//! shell, explicit minimal environment, bounded output and a timeout.

use std::time::{Duration, Instant};

use serde_json::Value;
use threadspace_surfaces_macos::exec::{BoundedCommand, run_bounded};

#[derive(Debug, Clone)]
pub struct Output {
    pub ok: bool,
    pub status: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
    pub pid: u32,
}

impl Output {
    /// The last stdout line parsed as JSON (helpers print one object).
    pub fn json(&self) -> Option<Value> {
        self.stdout
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with('{'))
            .and_then(|line| serde_json::from_str(line).ok())
    }
}

pub fn run(program: &str, args: &[&str], timeout: Duration) -> Output {
    let mut command = BoundedCommand::new(program, timeout, 4 * 1024 * 1024);
    for arg in args {
        command = command.arg(*arg);
    }
    if let Some(home) = std::env::var_os("HOME") {
        command = command.env("HOME", home);
    }
    command = command
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LANG", "en_US.UTF-8");
    let started = Instant::now();
    match run_bounded(&command) {
        Ok(output) => Output {
            ok: output.succeeded(),
            status: output.status,
            timed_out: output.timed_out,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            pid: output.pid,
        },
        Err(error) => Output {
            ok: false,
            status: None,
            timed_out: false,
            stdout: String::new(),
            stderr: error.to_string(),
            elapsed_ms: started.elapsed().as_millis() as u64,
            pid: 0,
        },
    }
}

/// `osascript -e <script> [args...]`; values travel as argv, never spliced
/// into the script source.
pub fn osascript(script: &str, args: &[&str], timeout: Duration) -> Output {
    let mut argv = vec!["-e", script];
    argv.extend_from_slice(args);
    run("/usr/bin/osascript", &argv, timeout)
}
