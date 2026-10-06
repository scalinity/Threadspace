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
