//! Reusable native qualification harness (M0C onward; never shipped).
//!
//! Each module is one controller over a real macOS surface, built so later
//! milestones can reuse it: installed app identities and paths, bounded
//! process execution, evidence run directories, the `ts-native` Swift helper
//! (Accessibility, input, pixels, idle time, displays), process discovery,
//! the companion's qualification client and log, the desktop's reports and
//! log, the ServiceManagement bootstrap, disposable Terminal windows, and an
//! owner-idle gate that keeps focus-stealing steps away from active use.

pub mod app;
pub mod companion;
pub mod evidence;
pub mod identity;
pub mod idle;
pub mod native;
pub mod procs;
pub mod run;
pub mod service;
pub mod terminal;

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Sleeps without pretending to measure anything.
pub fn pause_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}
