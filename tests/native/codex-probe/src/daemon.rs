//! Item 5: the owner's existing shared app-server daemon, read-only.
//!
//! 1. Recorded state: the rendezvous symlink and its derivation, the pid
//!    records and their liveness/boot, and the selected daemon package.
//! 2. `codex app-server daemon version` — the released read-only probe — run
//!    in the disposable CODEX_HOME whose control-socket path is a symlink to
//!    the owner's rendezvous path, because every CLI start writes
//!    `CODEX_HOME/tmp/arg0`. It connects and initializes only if a daemon
//!    answers; it never starts one.
//! 3. A direct AF_UNIX connect to the rendezvous path. Only if it connects
//!    does the passive observer run (`ws::observe`): upgrade, `initialize`,
//!    `initialized`, one `thread/loaded/list` with limit 5 (count only), close.
//!
//! Nothing here starts, stops, restarts or updates a daemon, and the shared
//! rendezvous directory is checked before and after to show none appeared.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use threadspace_surfaces_macos::process;

use crate::runtime::package_selection;
use crate::support::{Ctx, hex, now_ms, stamp};
use crate::ws;

const RENDEZVOUS: &str = ".codex/app-server-control/app-server-control.sock";
const PID_RECORDS: [&str; 4] = [
    "daemon.pid",
    "daemon-updater.pid",
    "app-server.pid",
    "app-server-updater.pid",
];
const LOADED_LIST_LIMIT: u32 = 5;
const RESPONSE_BUDGET: Duration = Duration::from_secs(2);

/// `/tmp` canonicalized + `codex-daemon-<euid>` (released codex-uds rule).
fn shared_directory(uid: u32) -> PathBuf {
    std::fs::canonicalize("/tmp")
        .unwrap_or_else(|_| PathBuf::from("/private/tmp"))
        .join(format!("codex-daemon-{uid}"))
}

/// The physical socket path the released server derives from a rendezvous
/// path: SHA-256 of `canonicalize(parent)/file_name` under the shared directory.
fn derived_target(rendezvous: &Path, shared: &Path) -> Option<PathBuf> {
    let parent = std::fs::canonicalize(rendezvous.parent()?).ok()?;
    let joined = parent.join(rendezvous.file_name()?);
    Some(shared.join(hex(&Sha256::digest(joined.as_os_str().as_encoded_bytes()))))
}

fn rendezvous_state(path: &Path, shared: &Path) -> Value {
    let metadata = std::fs::symlink_metadata(path);
    let target = std::fs::read_link(path).ok();
    let derived = derived_target(path, shared);
    json!({
        "path": path.display().to_string(),
        "exists": metadata.is_ok(),
        "isSymlink": metadata.as_ref().is_ok_and(|m| m.file_type().is_symlink()),
        "target": target.as_ref().map(|t| t.display().to_string()),
        "targetExists": target.as_ref().map(|t| t.exists()),
        "derivedTarget": derived.as_ref().map(|t| t.display().to_string()),
        "targetMatchesDerivation": target.is_some() && target == derived,
    })
}

fn pid_record(path: &Path, current_boot: Option<&str>) -> Value {
    let Ok(text) = std::fs::read_to_string(path) else {
        return json!({ "path": path.display().to_string(), "present": false });
    };
    let Ok(record) = serde_json::from_str::<Value>(&text) else {
        return json!({ "path": path.display().to_string(), "present": true, "parsed": false });
    };
    let identity = record.get("processIdentity");
    let pid = record.get("pid").and_then(Value::as_i64).unwrap_or(0) as i32;
    let start_seconds = identity
        .and_then(|i| i.get("startSeconds"))
        .and_then(Value::as_u64);
    let start_micros = identity
        .and_then(|i| i.get("startMicroseconds"))
        .and_then(Value::as_u64);
    let boot = identity
        .and_then(|i| i.get("bootId"))
        .and_then(Value::as_str);
    let live = if pid > 0 {
        Some(process::sample(pid))
    } else {
        None
    };
    let digest = record
        .get("executableIdentity")
        .and_then(|e| e.get("digest"))
        .and_then(Value::as_array)
        .map(|bytes| {
            hex(&bytes
                .iter()
                .filter_map(|b| b.as_u64().map(|v| v as u8))
                .collect::<Vec<_>>())
        });
    json!({
        "path": path.display().to_string(),
        "present": true,
        "pid": pid,
        "processStartTime": record.get("processStartTime"),
        "startSeconds": start_seconds,
        "recordedOnCurrentBoot": match (boot, current_boot) {
            (Some(recorded), Some(current)) => Some(recorded == current),
            _ => None,
        },
        "pidLiveNow": live.as_ref().map(|r| r.is_ok()),
        "livePidIsRecordedIncarnation": live.as_ref().and_then(|r| r.as_ref().ok()).map(|s| {
            Some(s.start_seconds) == start_seconds && Some(u64::from(s.start_microseconds)) == start_micros
        }),
        "pidLookupError": live.as_ref().and_then(|r| r.as_ref().err()).map(|e| e.code()),
        "executableIdentityDigest": digest,
    })
}

/// `codex app-server daemon version` with the control-socket path aliased to
/// the owner's rendezvous path inside the disposable CODEX_HOME.
fn cli_version_probe(ctx: &mut Ctx, rendezvous: &Path) -> Value {
    let alias = match ctx.disposable() {
        Ok(d) => d.home.join("app-server-control/app-server-control.sock"),
        Err(error) => return json!({ "error": error }),
    };
    if let Some(parent) = alias.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let link = std::os::unix::fs::symlink(rendezvous, &alias);
    let codex = ctx.codex.clone();
    let ran = ctx.run_codex(
        &codex,
        &["app-server", "daemon", "version"],
        Duration::from_secs(20),
        64 * 1024,
    );
    let parsed = serde_json::from_str::<Value>(ran.stdout.trim()).ok();
    json!({
        "aliasPath": alias.display().to_string(),
        "aliasTarget": rendezvous.display().to_string(),
        "aliasCreated": link.is_ok(),
        "command": ran.record,
        "succeeded": ran.ok,
        "stdoutJson": parsed,
        "stdout": if parsed.is_none() { json!(ran.stdout.trim()) } else { Value::Null },
        "stderr": ran.stderr.trim(),
        "whyNotOwnerCodexHome": "every CLI start creates CODEX_HOME/tmp/arg0/codex-arg0* (and may leave it behind) and runs a janitor over stale arg0 dirs (rust-v0.160.0 codex-rs/arg0/src/lib.rs prepare_path_entry_for_codex_aliases/janitor_cleanup); the owner's ~/.codex is never written",
    })
}

fn direct_observer(rendezvous: &Path) -> Value {
    let started = now_ms();
    match UnixStream::connect(rendezvous) {
        Err(error) => json!({
            "attemptedAt": stamp(started),
            "connected": false,
            "errorKind": format!("{:?}", error.kind()),
            "errno": error.raw_os_error(),
            "observerRun": false,
        }),
        Ok(stream) => {
            let timeouts = stream
                .set_read_timeout(Some(RESPONSE_BUDGET))
                .and_then(|()| stream.set_write_timeout(Some(RESPONSE_BUDGET)));
            if timeouts.is_err() {
                return json!({ "connected": true, "observerRun": false, "error": "SET_TIMEOUT_FAILED" });
            }
            json!({
                "attemptedAt": stamp(started),
                "connected": true,
                "observerRun": true,
                "client": { "name": ws::CLIENT_NAME, "title": ws::CLIENT_TITLE, "capabilities": Value::Null },
                "observer": ws::observe(stream, RESPONSE_BUDGET, Some(LOADED_LIST_LIMIT)),
            })
        }
    }
}

pub fn probe(ctx: &mut Ctx) -> Value {
    let started = now_ms();
    let uid = process::sample(std::process::id() as i32)
        .map(|s| s.uid)
        .unwrap_or(501);
    let shared = shared_directory(uid);
    let shared_before = shared.exists();
    let current_boot = process::boot_session_id().ok();
    let rendezvous = ctx.owner_path(RENDEZVOUS);
    let state_dir = ctx.owner_path(".codex/app-server-daemon");

    let mut recorded = Map::new();
    recorded.insert("rendezvous".into(), rendezvous_state(&rendezvous, &shared));
    recorded.insert(
        "sharedDirectory".into(),
        json!(shared.display().to_string()),
    );
    recorded.insert(
        "pidRecords".into(),
        Value::Array(
            PID_RECORDS
                .iter()
                .map(|n| pid_record(&state_dir.join(n), current_boot.as_deref()))
                .collect(),
        ),
    );
    recorded.insert(
        "settingsFilePresent".into(),
        json!(state_dir.join("settings.json").is_file()),
    );
    recorded.insert(
        "selectedDaemonPackage".into(),
        package_selection(ctx, "app-server-daemon"),
    );

    let cli = cli_version_probe(ctx, &rendezvous);
    let direct = direct_observer(&rendezvous);
    let shared_after = shared.exists();

    let answered = direct["observer"]["completed"] == json!(true);
    let cli_version = cli["stdoutJson"].clone();
    json!({
        "item": "daemon",
        "status": if answered { "DAEMON_ANSWERED" } else { "NO_DAEMON_ANSWERING" },
        "startedAt": stamp(started),
        "finishedAt": stamp(now_ms()),
        "recordedState": recorded,
        "cliDaemonVersion": cli,
        "directConnect": direct,
        "runningAppServerVersion": if answered {
            direct["observer"]["initialize"]["response"]["appServerVersion"].clone()
        } else {
            cli_version.get("appServerVersion").cloned().unwrap_or(Value::Null)
        },
        "sharedDirectoryExistedBefore": shared_before,
        "sharedDirectoryExistsAfter": shared_after,
        "sharedDirectoryAppearedDuringProbe": !shared_before && shared_after,
        "disposableHomeAfter": ctx.disposable_home_tree(),
    })
}
