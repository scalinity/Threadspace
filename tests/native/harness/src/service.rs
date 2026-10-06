//! The outer application's native bootstrap (`--service ...`), run as the
//! installed executable so ServiceManagement sees the real containing app.

use std::time::Duration;

use serde_json::{Value, json};

use crate::identity::Identity;
use crate::run::run;

pub fn bootstrap(id: &Identity, command: &str) -> Value {
    let out = run(
        &id.executable.display().to_string(),
        &["--service", command],
        Duration::from_secs(180),
    );
    let mut value = out.json().unwrap_or_else(
        || json!({ "parseError": true, "stdout": out.stdout, "stderr": out.stderr }),
    );
    value["exitStatus"] = json!(out.status);
    value["elapsedMs"] = json!(out.elapsed_ms);
    value
}

pub fn status(id: &Identity) -> String {
    bootstrap(id, "status")["detail"]["status"]
        .as_str()
        .unwrap_or("UNKNOWN")
        .to_owned()
}

/// The PID launchd runs for the login-item job itself (`gui/<uid>/<label>`),
/// as opposed to a copy LaunchServices started for a notification.
pub fn login_item_pid(id: &Identity) -> Option<u32> {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let out = run(
        "/bin/launchctl",
        &["print", &format!("gui/{uid}/{}", id.agent_identifier)],
        Duration::from_secs(10),
    );
    out.stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("pid = ")?.trim().parse().ok())
}
