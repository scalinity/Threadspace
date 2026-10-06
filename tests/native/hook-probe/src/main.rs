//! M0B hook ancestry probe (SPEC §4.5, MILESTONES M0B step 5). Installed as a
//! Claude command hook only in the M0B test directory's project settings, it:
//!
//! 1. samples its own ProcessKey before doing anything else;
//! 2. reads at most 64 KiB of hook stdin and keeps only allowlisted fields
//!    (`session_id`, `hook_event_name`, `source`, `reason`) — no prompt text,
//!    tool payloads or transcript paths;
//! 3. walks at most 24 ancestors with per-edge validation and records each
//!    link's ProcessKey, executable and controlling device;
//! 4. selects the nearest proven ancestor whose executable is in the Claude
//!    installer's versions directory, rechecked at selection;
//! 5. appends one JSON line to the evidence file and exits 0 silently.
//!
//! It never reads `/dev/tty` or treats stdin as a terminal: hooks can be
//! detached from the provider's terminal. It sends nothing to Threadspace, so
//! discovery can never depend on it.
//!
//!   threadspace-hook-probe --out <evidence.jsonl> --versions-dir <dir>

use std::io::{Read, Write};
use std::path::Path;

use serde_json::{Value, json};
use threadspace_surfaces_macos::ancestry::{
    AncestryStop, KernelSampler, MAX_ANCESTORS, select_provider, walk,
};
use threadspace_surfaces_macos::process::{self, Incarnation};

const MAX_STDIN: u64 = 64 * 1024;
const ALLOWED: [&str; 4] = ["session_id", "hook_event_name", "source", "reason"];

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn link(incarnation: &Incarnation) -> Value {
    let sample = &incarnation.sample;
    json!({
        "pid": sample.pid,
        "ppid": sample.ppid,
        "startSeconds": sample.start_seconds.to_string(),
        "startMicroseconds": sample.start_microseconds,
        "comm": sample.comm,
        "executable": incarnation.executable.canonical(),
        "controllingDevice": sample.controlling_device,
        "pgid": sample.pgid,
        "tpgid": sample.tpgid,
    })
}

fn stop(stop: &AncestryStop) -> Value {
    match stop {
        AncestryStop::ReachedRoot => json!({ "kind": "REACHED_ROOT" }),
        AncestryStop::Unreadable { pid, code } => {
            json!({ "kind": "UNREADABLE", "pid": pid, "code": code })
        }
        AncestryStop::Reparented { pid } => json!({ "kind": "REPARENTED", "pid": pid }),
        AncestryStop::ParentReused { pid } => json!({ "kind": "PARENT_REUSED", "pid": pid }),
        AncestryStop::Cycle { pid } => json!({ "kind": "CYCLE", "pid": pid }),
        AncestryStop::DepthLimit => json!({ "kind": "DEPTH_LIMIT" }),
    }
}

fn main() {
    let captured_ms = now_ms();
    let own_pid = std::process::id() as i32;
    // 1. Own ProcessKey first.
    let own = process::sample_incarnation(own_pid);

    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|index| args.get(index + 1))
            .cloned()
    };
    let (Some(out), Some(versions_dir)) = (arg("--out"), arg("--versions-dir")) else {
        return;
    };

    // 2. Bounded, allowlisted hook metadata.
    let mut raw = Vec::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_end(&mut raw);
    let hook: Value = serde_json::from_slice::<Value>(&raw)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|object| {
            Value::Object(
                object
                    .into_iter()
                    .filter(|(key, _)| ALLOWED.contains(&key.as_str()))
                    .collect(),
            )
        })
        .unwrap_or(Value::Null);

    // 3–4. Validated ancestry and provider selection.
    let ancestry = walk(&KernelSampler, own_pid, MAX_ANCESTORS);
    let versions = Path::new(&versions_dir);
    let provider = select_provider(&KernelSampler, &ancestry, |link| {
        Path::new(&link.executable.path).parent() == Some(versions)
    });

    let record = json!({
        "capturedMs": captured_ms,
        "hook": hook,
        "stdinBytes": raw.len(),
        "self": own.as_ref().map(link).unwrap_or_else(|error| json!({ "error": error.code() })),
        "chain": ancestry.chain.iter().map(link).collect::<Vec<_>>(),
        "stop": stop(&ancestry.stop),
        "provider": provider.map(link),
        "providerDepth": provider.and_then(|selected| {
            ancestry.chain.iter().position(|l| l.sample.pid == selected.sample.pid)
        }),
        "selfHasControllingTerminal": own.as_ref().ok().map(|o| o.sample.controlling_device.is_some()),
        "finishedMs": now_ms(),
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&out)
    {
        let _ = writeln!(file, "{record}");
    }
}
