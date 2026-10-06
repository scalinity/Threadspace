//! Process discovery by kernel executable path (never by name), with PID and
//! kernel birth as the incarnation (SPEC §4.2).

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use threadspace_surfaces_macos::process;

use crate::run::run;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Incarnation {
    pub pid: i32,
    pub ppid: i32,
    pub start_seconds: u64,
    pub start_microseconds: u32,
}

impl Incarnation {
    pub fn of(pid: i32) -> Option<Self> {
        let sample = process::sample(pid).ok()?;
        Some(Self {
            pid,
            ppid: sample.ppid,
            start_seconds: sample.start_seconds,
            start_microseconds: sample.start_microseconds,
        })
    }

    /// Whether this exact incarnation (PID and birth) still exists.
    pub fn alive(&self) -> bool {
        Self::of(self.pid).is_some_and(|now| {
            now.start_seconds == self.start_seconds
                && now.start_microseconds == self.start_microseconds
        })
    }
}

/// Every live process whose kernel executable path is `executable`.
pub fn with_executable(executable: &Path) -> Vec<Incarnation> {
    let listing = run("/bin/ps", &["-axo", "pid="], Duration::from_secs(5));
    let wanted = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_path_buf());
    listing
        .stdout
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .filter(|pid| {
            process::executable_path(*pid)
                .ok()
                .is_some_and(|path| path.canonicalize().unwrap_or(path) == wanted)
        })
        .filter_map(Incarnation::of)
        .collect()
}

/// Waits until `incarnation` is gone; returns the wait in milliseconds.
pub fn wait_exit(incarnation: &Incarnation, timeout: Duration) -> Option<u64> {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        if !incarnation.alive() {
            return Some(started.elapsed().as_millis() as u64);
        }
        crate::pause_ms(50);
    }
    None
}

/// The process's current working directory (`lsof`), for ownership proofs.
pub fn cwd(pid: i32) -> Option<String> {
    let out = run(
        "/usr/sbin/lsof",
        &["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"],
        Duration::from_secs(5),
    );
    out.stdout
        .lines()
        .find_map(|line| line.strip_prefix('n').map(str::to_owned))
}

pub fn signal(pid: i32, signal: i32) -> bool {
    // SAFETY: sending a signal to a PID the caller verified by incarnation.
    unsafe { libc::kill(pid, signal) == 0 }
}
