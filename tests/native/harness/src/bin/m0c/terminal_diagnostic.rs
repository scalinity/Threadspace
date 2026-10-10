//! Short, read-only Terminal dictionary probes; no launch, focus or readiness retry.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_harness::{procs, run};

const TERMINAL: &str = "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal";

pub fn probe() -> Result<Value, String> {
    let started = Instant::now();
    let overall = Duration::from_secs(55);
    let caller = std::env::current_exe().map_err(|error| error.to_string())?;
    let initial = procs::with_executable(std::path::Path::new(TERMINAL));
    let mut rows = Vec::new();
    let mut passes = Vec::new();
    let boot = run::run(
        "/usr/sbin/sysctl",
        &["-n", "kern.bootsessionuuid"],
        Duration::from_secs(3),
    );
    let signature = run::run(
        "/usr/bin/codesign",
        &["-dvv", &caller.display().to_string()],
        Duration::from_secs(3),
    );
    let queries = [
        ("application", "return {id, version, running}"),
        ("windowCount", "return count of windows"),
        ("windowIds", "return id of every window"),
        (
            "frontWindow",
            "if count of windows is 0 then return \"NO_WINDOWS\"\nreturn id of front window",
        ),
        (
            "selectedTab",
            "if count of windows is 0 then return \"NO_WINDOWS\"\nreturn selected tab of front window",
        ),
        (
            "selectedTty",
            "if count of windows is 0 then return \"NO_WINDOWS\"\nreturn tty of selected tab of front window",
        ),
    ];
    // Kernel discovery first prevents a dictionary query from implicitly launching Terminal.
    if let [terminal] = initial.as_slice() {
        for pass in 1..=2 {
            let before = procs::Incarnation::of(terminal.pid);
            let mut successful = true;
            for (name, body) in queries {
                let Some(remaining) = overall.checked_sub(started.elapsed()) else {
                    successful = false;
                    break;
                };
                let script = format!(
                    "with timeout of 3 seconds\ntell application id \"com.apple.Terminal\"\n{body}\nend tell\nend timeout"
                );
                let out = run::osascript(&script, &[], remaining.min(Duration::from_secs(4)));
                let classification = if out.timed_out {
                    "SUBPROCESS_TIMEOUT"
                } else if out.ok {
                    "ANSWERED"
                } else if out.stderr.contains("(-1743)") {
                    "AUTOMATION_DENIED"
                } else if out.stderr.contains("(-600)") {
                    "TARGET_NOT_RUNNING"
                } else if out.stderr.contains("(-1712)") {
                    "APPLEEVENT_TIMEOUT"
                } else {
                    "APPLEEVENT_OR_SUBPROCESS_ERROR"
                };
                successful &= out.ok;
                rows.push(
                    json!({"pass":pass,"query":name,"classification":classification,
                    "stdout":out.stdout,"stderr":out.stderr,"exitStatus":out.status,
                    "timedOut":out.timed_out,"elapsedMs":out.elapsed_ms,"subprocessPid":out.pid,
                    "workerDeadlineMs":remaining.min(Duration::from_secs(4)).as_millis()}),
                );
            }
            let after = procs::Incarnation::of(terminal.pid);
            let stable = before.as_ref() == Some(terminal) && after.as_ref() == Some(terminal);
            passes.push(json!({"pass":pass,"before":before,"after":after,"stable":stable,"answered":successful}));
            if !stable || started.elapsed() >= overall {
                break;
            }
        }
    }
    let restored = passes.len() == 2
        && passes
            .iter()
            .all(|pass| pass["stable"] == true && pass["answered"] == true);
    Ok(
        json!({"verdict":if restored {"TERMINAL SCRIPTABILITY RESTORED"} else {"TERMINAL SCRIPTABILITY ENVIRONMENT BLOCKED"},
        "elapsedMs":started.elapsed().as_millis(),"overallDeadlineMs":overall.as_millis(),
        "callerExecutable":caller,"callerIncarnation":procs::Incarnation::of(std::process::id() as i32),
        "callerSignature":{"exitStatus":signature.status,"stdout":signature.stdout,"stderr":signature.stderr},
        "bootIdentity":boot.stdout.trim(),"terminalExecutable":TERMINAL,"initialTerminalIncarnations":initial,
        "passes":passes,"queries":rows,"mutatesTerminal":false,"productRouteDeadlineChanged":false}),
    )
}
