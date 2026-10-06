//! Bounded argv subprocess execution (SPEC §13.3, §19.3): an absolute program
//! path, argument vector without a shell, bounded stdin/stdout/stderr, a hard
//! timeout and a recorded exit status. Values are always data arguments, and
//! the environment is cleared except for an explicit allowlist.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BoundedCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Explicit environment; everything else is cleared.
    pub env: Vec<(OsString, OsString)>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl BoundedCommand {
    pub fn new(program: impl Into<PathBuf>, timeout: Duration, max_output_bytes: usize) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            stdin: None,
            timeout,
            max_output_bytes,
        }
    }

    pub fn arg(mut self, value: impl Into<OsString>) -> Self {
        self.args.push(value.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    /// PID of the spawned child (evidence of which process did the work).
    pub pid: u32,
    /// Exit code; `None` when killed by a signal (including timeout).
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub elapsed: Duration,
}

impl BoundedOutput {
    pub fn succeeded(&self) -> bool {
        self.status == Some(0) && !self.timed_out
    }
}

#[derive(Debug)]
pub enum ExecError {
    /// Only absolute executable paths are accepted; nothing is resolved via PATH.
    RelativeProgram(PathBuf),
    Spawn(std::io::Error),
    Wait(std::io::Error),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RelativeProgram(path) => write!(f, "program {} is not absolute", path.display()),
            Self::Spawn(error) => write!(f, "spawn failed: {error}"),
            Self::Wait(error) => write!(f, "wait failed: {error}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// Reads up to `cap` bytes, then drains (and discards) the rest so the child
/// never blocks on a full pipe.
fn capped_reader<R: Read + Send + 'static>(
    mut source: R,
    cap: usize,
) -> thread::JoinHandle<(Vec<u8>, bool)> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        let mut chunk = [0u8; 8192];
        loop {
            match source.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let room = cap.saturating_sub(kept.len());
                    if room >= read {
                        kept.extend_from_slice(&chunk[..read]);
                    } else {
                        kept.extend_from_slice(&chunk[..room]);
                        truncated = true;
                    }
                }
            }
        }
        (kept, truncated)
    })
}

pub fn run_bounded(command: &BoundedCommand) -> Result<BoundedOutput, ExecError> {
    if !command.program.is_absolute() {
        return Err(ExecError::RelativeProgram(command.program.clone()));
    }
    let started = Instant::now();
    let mut child = Command::new(&command.program)
        .args(&command.args)
        .env_clear()
        .env("LANG", "en_US.UTF-8")
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .stdin(if command.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ExecError::Spawn)?;
    let pid = child.id();

    if let (Some(input), Some(mut pipe)) = (command.stdin.clone(), child.stdin.take()) {
        thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
    let stdout = child
        .stdout
        .take()
        .map(|pipe| capped_reader(pipe, command.max_output_bytes));
    let stderr = child
        .stderr
        .take()
        .map(|pipe| capped_reader(pipe, command.max_output_bytes));

    let deadline = started + command.timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait().map_err(ExecError::Wait)? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().map_err(ExecError::Wait)?;
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    };
    let join = |handle: Option<thread::JoinHandle<(Vec<u8>, bool)>>| {
        handle
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    };
    let (stdout, stdout_truncated) = join(stdout);
    let (stderr, stderr_truncated) = join(stderr);
    Ok(BoundedOutput {
        pid,
        status: status.code(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out,
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_status() {
        let output = run_bounded(
            &BoundedCommand::new("/bin/echo", Duration::from_secs(2), 1024).arg("hello; rm -rf /"),
        )
        .expect("run");
        assert!(output.succeeded());
        assert!(output.pid > 0);
        // The argument is data: shell metacharacters reach the program verbatim.
        assert_eq!(output.stdout, b"hello; rm -rf /\n");
    }

    #[test]
    fn environment_is_cleared_except_the_allowlist() {
        let output = run_bounded(
            &BoundedCommand::new("/usr/bin/env", Duration::from_secs(2), 4096)
                .env("HOME", "/nowhere"),
        )
        .expect("run");
        let text = String::from_utf8_lossy(&output.stdout);
        let mut keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.split('=').next())
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["HOME", "LANG"], "{text}");
    }

    #[test]
    fn truncates_output_at_the_cap() {
        let output = run_bounded(
            &BoundedCommand::new("/usr/bin/yes", Duration::from_millis(300), 64).arg("abc"),
        )
        .expect("run");
        assert_eq!(output.stdout.len(), 64);
        assert!(output.stdout_truncated);
        assert!(output.timed_out, "yes never exits on its own");
        assert_eq!(output.status, None, "killed");
    }

    #[test]
    fn enforces_timeout() {
        let output = run_bounded(
            &BoundedCommand::new("/bin/sleep", Duration::from_millis(100), 64).arg("5"),
        )
        .expect("run");
        assert!(output.timed_out);
        assert!(output.elapsed < Duration::from_secs(2));
    }

    #[test]
    fn passes_stdin_and_rejects_relative_programs() {
        let mut command = BoundedCommand::new("/bin/cat", Duration::from_secs(2), 64);
        command.stdin = Some(b"{\"ok\":true}".to_vec());
        assert_eq!(run_bounded(&command).expect("run").stdout, b"{\"ok\":true}");
        assert!(matches!(
            run_bounded(&BoundedCommand::new("echo", Duration::from_secs(1), 8)),
            Err(ExecError::RelativeProgram(_))
        ));
    }
}
