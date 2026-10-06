//! The owner-idle gate: before a step that steals focus or moves windows, wait
//! until the keyboard and pointer have been idle for a while, so automation
//! never lands keystrokes or clicks from active use in a test window.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::native::Native;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleWait {
    pub required_seconds: f64,
    pub waited_ms: u64,
    pub idle_at_start: f64,
    pub satisfied: bool,
}

pub fn wait_for_idle(native: &Native, required_seconds: f64, max_wait: Duration) -> IdleWait {
    let started = Instant::now();
    let idle_at_start = native.idle_seconds();
    loop {
        let idle = native.idle_seconds();
        if idle >= required_seconds {
            return IdleWait {
                required_seconds,
                waited_ms: started.elapsed().as_millis() as u64,
                idle_at_start,
                satisfied: true,
            };
        }
        if started.elapsed() >= max_wait {
            return IdleWait {
                required_seconds,
                waited_ms: started.elapsed().as_millis() as u64,
                idle_at_start,
                satisfied: false,
            };
        }
        crate::pause_ms(500);
    }
}

/// A cooperative, machine-wide lock for GUI automation segments
/// (`/private/tmp/mac-gui-automation.lock`). Any harness on this Mac that
/// sends input, activates or moves windows, or launches a focus-taking app
/// holds it for one short segment, so two harnesses never race for focus or
/// type into each other's windows. Dropping the guard releases it.
pub struct GuiLock {
    file: std::fs::File,
    pub waited_ms: u64,
}

pub const GUI_LOCK_PATH: &str = "/private/tmp/mac-gui-automation.lock";

impl GuiLock {
    pub fn acquire(label: &str) -> std::io::Result<Self> {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        let started = Instant::now();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(GUI_LOCK_PATH)?;
        // SAFETY: the descriptor belongs to `file`, held for the guard's life.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let _ = file.set_len(0);
        let _ = writeln!(
            file,
            "{}",
            serde_json::json!({ "pid": std::process::id(), "label": label, "sinceMs": crate::now_ms() })
        );
        Ok(Self {
            file,
            waited_ms: started.elapsed().as_millis() as u64,
        })
    }
}

impl Drop for GuiLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        let _ = self.file.set_len(0);
        // SAFETY: releasing the lock this guard took on its own descriptor.
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}
