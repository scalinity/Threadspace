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

/// The running WebKit WebContent processes serving `pid`: the service
/// instances in its own launchd domain (`launchctl print pid/<pid>`), so
/// another app's web views are never counted. `None` if the domain cannot
/// be read.
pub fn web_content_of(pid: i32) -> Option<Vec<i32>> {
    let out = run(
        "/bin/launchctl",
        &["print", &format!("pid/{pid}")],
        Duration::from_secs(5),
    );
    if !out.ok {
        return None;
    }
    Some(web_content_services(&out.stdout))
}

/// Instance PIDs of `com.apple.WebKit.WebContent*` in the domain's
/// `services = { <pid> <status> <label> ... }` block.
fn web_content_services(domain: &str) -> Vec<i32> {
    domain
        .lines()
        .skip_while(|line| line.trim() != "services = {")
        .skip(1)
        .take_while(|line| line.trim() != "}")
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<i32>().ok()?;
            let label = fields.nth(1)?;
            (pid > 0 && label.starts_with("com.apple.WebKit.WebContent")).then_some(pid)
        })
        .collect()
}

/// The process's physical footprint in bytes (`ri_phys_footprint`, the
/// figure Activity Monitor shows as Memory).
pub fn phys_footprint(pid: i32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: the buffer is a `rusage_info_v4`, the flavor requested.
    let rc = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            info.as_mut_ptr().cast::<libc::rusage_info_t>(),
        )
    };
    // SAFETY: a zero return filled the whole structure.
    (rc == 0).then(|| unsafe { info.assume_init() }.ri_phys_footprint)
}

pub fn signal(pid: i32, signal: i32) -> bool {
    // SAFETY: sending a signal to a PID the caller verified by incarnation.
    unsafe { libc::kill(pid, signal) == 0 }
}

#[cfg(test)]
mod tests {
    use super::web_content_services;

    #[test]
    fn web_content_instances_come_from_the_services_block_only() {
        let domain = "pid/1 = {\n\tservices = {\n\t\t       0      - \tcom.apple.WebKit.WebContent\n\t\t   83255      - \tcom.apple.WebKit.GPU.88F1\n\t\t   83256      - \tcom.apple.WebKit.WebContent.B086\n\t\t   83300      - \tcom.apple.WebKit.WebContent.C1D2\n\t}\n\n\tservice stubs = {\n\t\t   9      - \tcom.apple.WebKit.WebContent.X\n\t}\n}\n";
        assert_eq!(web_content_services(domain), vec![83256, 83300]);
    }
}
