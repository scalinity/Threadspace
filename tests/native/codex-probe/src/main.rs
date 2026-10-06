//! M0C Codex platform probe (read-only; MILESTONES M0C "Probe Codex modes and
//! the existing daemon's actual protocol/version without starting/resuming
//! provider work"). Never shipped.
//!
//!   threadspace-codex-probe <command> --out <dir> [--codex <path>]
//!       [--scratch <dir>] [--hook-schemas <dir>]
//!
//! Commands, each writing `<out>/<command>.json` (`all` writes `probe.json`):
//!   runtime        installed CLI and other runtimes, versions, hashes, signatures
//!   schema         help text and generated app-server schema vs SPEC §12.2–§12.6
//!   hooks          hook configuration surface and event set
//!   hook-ancestry  detached-hook ancestry (records why it is not run)
//!   daemon         existing shared daemon: version probe and passive handshake
//!   modes          CODEX_SHARED_DAEMON / CODEX_EMBEDDED / CODEX_DESKTOP_LOCAL table
//!   all            every item above, in order, into one probe.json
//!
//! The probe never starts, resumes or submits Codex work, never logs in, never
//! starts a daemon, and never writes the owner's ~/.codex: every CLI run uses a
//! disposable CODEX_HOME under `--scratch` that is removed afterwards. Evidence
//! strings have `$HOME` replaced with `~`.

mod daemon;
mod runtime;
mod schema;
mod support;
mod ws;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};
use threadspace_surfaces_macos::process;

use support::{Ctx, now_ms, stamp};

const USAGE: &str = "usage: threadspace-codex-probe <runtime|schema|hooks|hook-ancestry|daemon|modes|all> --out <dir> [--codex <path>] [--scratch <dir>] [--hook-schemas <dir>]";

/// Item 4. Not run: no released hook fires without a live thread.
fn hook_ancestry() -> Value {
    let now = now_ms();
    json!({
        "item": "hook-ancestry",
        "status": "NOT_RUN",
        "startedAt": stamp(now),
        "finishedAt": stamp(now),
        "blocker": "Every released Codex hook event is dispatched from a live session (thread), and every event except SessionEnd only inside a running turn. No hook fires at CLI or TUI startup alone, so capturing a hook process requires creating a thread (and, for all events but SessionEnd, a turn that proceeds to model sampling) plus an authenticated account for the TUI to start a session. All of that is outside this probe's limits: no task start, thread or rollout creation, prompt submission, model call or login.",
        "sourceEvidence": [
            {
                "source": "openai/codex d27764b (rust-v0.160.1) codex-rs/core/src/session/turn.rs L163 run_turn, L322 run_pending_session_start_hooks",
                "fact": "SessionStart runs from run_turn after the turn's input, MCP requirements and step context are captured, immediately before the first sampling request.",
            },
            {
                "source": "openai/codex d27764b codex-rs/core/src/hook_runtime.rs L128 run_pending_session_start_hooks, L471-499 run_session_end_hooks",
                "fact": "SessionStart takes a pending source from the Session; SessionEnd needs a live Session, builds a default turn context and flushes the rollout before running.",
            },
            {
                "source": "released hook input fixtures (raw/hook-input-schema-0.160.0)",
                "fact": "Every event other than SessionStart and SessionEnd carries a required turn_id.",
            },
            {
                "source": "installed app-server schema HookScope",
                "fact": "Hook runs are scoped to thread or turn.",
            },
            {
                "source": "raw/release-diff-0.160.0-0.160.1.json",
                "fact": "rust-v0.160.0 and rust-v0.160.1 differ only in codex-rs/rmcp-client/src/stdio_server_launcher.rs and the version string, so the cited files are identical in the installed build.",
            },
        ],
        "notAttempted": [
            "codex TUI (headless pty or otherwise): an empty disposable CODEX_HOME has no credentials, and starting a session would create a thread",
            "codex exec, resume, fork, review",
            "app-server thread/start or turn/start",
            "installing any hook into the owner's ~/.codex or a project .codex",
        ],
        "toUnblock": "An owner-authorized qualification run that may create one disposable Codex thread: a SessionStart command hook in a disposable CODEX_HOME hooks.json, an authenticated account, one prompt in a pty the harness owns, and a Codex executable-identity selection rule for the M0B capture (tests/native/hook-probe selects the provider by the Claude versions directory). Run it once with --no-daemon (CODEX_EMBEDDED: nearest provider ancestor is the TUI) and once attached to the shared daemon (CODEX_SHARED_DAEMON: nearest provider ancestor is the daemon, no source TTY).",
    })
}

fn get<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(value, |node, key| &node[*key])
}

/// Item 6.
fn modes(runtime: &Value, hooks: &Value, ancestry: &Value, daemon: &Value) -> Value {
    let processes = runtime["processes"].as_array().cloned().unwrap_or_default();
    let by_label = |prefix: &str| -> Vec<&Value> {
        processes
            .iter()
            .filter(|p| p["runtime"].as_str().is_some_and(|r| r.starts_with(prefix)))
            .collect()
    };
    let standalone_tui = by_label("standalone/")
        .iter()
        .filter(|p| {
            p["argv"]["subcommand"]
                .as_str()
                .is_some_and(|s| s.starts_with("(none"))
        })
        .count();
    let desktop = by_label("desktop:");
    let desktop_app_servers: Vec<&&Value> = desktop
        .iter()
        .filter(|p| {
            p["argv"]["subcommand"] == "app-server" && p["argv"]["appServerSubcommand"].is_null()
        })
        .collect();
    let desktop_stdio = desktop_app_servers
        .iter()
        .filter(|p| p["argv"]["listenEffective"] == "stdio://")
        .count();
    let desktop_bound: u64 = desktop_app_servers
        .iter()
        .map(|p| p["unixSockets"]["pathBound"].as_u64().unwrap_or(0))
        .sum();
    let bundle = runtime["otherRuntimes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["kind"] == "CODEX_DESKTOP_BUNDLED" && r["runtime"]["executable"].is_object());
    let answered = daemon["status"] == "DAEMON_ANSWERED";
    let pid_records = get(daemon, &["recordedState", "pidRecords"])
        .as_array()
        .cloned()
        .unwrap_or_default();
    let feature = |name: &str| get(hooks, &["featureFlags", "rows", name]).clone();
    let cli_sha = get(runtime, &["cli", "executable", "sha256"]).clone();
    let desktop_sha = bundle.map(|b| get(b, &["runtime", "executable", "sha256"]).clone());

    json!({
        "item": "modes",
        "status": "DERIVED",
        "derivedAt": stamp(now_ms()),
        "modes": [
            {
                "mode": "CODEX_SHARED_DAEMON",
                "availableNow": answered,
                "status": if answered { "AVAILABLE" } else { "NOT_RUNNING" },
                "evidence": {
                    "daemonVersionExit": get(daemon, &["cliDaemonVersion", "command", "exitStatus"]),
                    "daemonVersionStderr": get(daemon, &["cliDaemonVersion", "stderr"]),
                    "directConnect": {
                        "connected": get(daemon, &["directConnect", "connected"]),
                        "errorKind": get(daemon, &["directConnect", "errorKind"]),
                    },
                    "rendezvous": get(daemon, &["recordedState", "rendezvous"]),
                    "pidRecords": pid_records.iter().filter(|r| r["present"] == true).map(|r| json!({
                        "path": r["path"],
                        "pidLiveNow": r["pidLiveNow"],
                        "recordedOnCurrentBoot": r["recordedOnCurrentBoot"],
                        "processStartTime": r["processStartTime"],
                    })).collect::<Vec<_>>(),
                    "selectedDaemonPackageVersion": get(daemon, &["recordedState", "selectedDaemonPackage", "currentManifest", "version"]),
                    "installedCliVersion": get(runtime, &["versionDrift", "installedCliVersion"]),
                    "daemonAutoStartFeature": feature("daemon_auto_start"),
                    "runningAppServerVersion": daemon["runningAppServerVersion"],
                },
                "limitations": [
                    "No daemon answered, so the live wire (AF_UNIX WebSocket upgrade, initialize/initialized, thread/loaded/list) was not exercised against Codex; the hand-written client is verified only against the in-process RFC 6455 fake in the crate's tests.",
                    "The selected managed daemon package differs from the installed CLI; per the released daemon README, lifecycle commands (and TUI auto-start) use the selected package regardless of the invoking CLI, so a newly started daemon would run that package's version, which must be qualified separately from 0.160.1.",
                    "daemon_auto_start is on by default: an ordinary `codex` TUI launch starts or attaches to this daemon with no embedded fallback (rust-v0.160.1 codex-rs/tui/src/startup_orchestration.rs L494 auto_start_daemon, L530 codex_app_server_daemon::start_with_features, L547 LocalDaemon allow_embedded_fallback=false); embedded mode follows only from an exclusion such as --no-daemon, --oss, --profile, non-feature -c overrides, --strict-config or --dangerously-bypass-hook-trust (daemon_startup.rs exclusion()).",
                ],
            },
            {
                "mode": "CODEX_EMBEDDED",
                "availableNow": get(hooks, &["cliFlags", "noDaemon"]) == &json!(true) && cli_sha.is_string(),
                "status": if standalone_tui > 0 { "INSTALLED_LIVE_TUI_PRESENT" } else { "INSTALLED_NO_LIVE_SESSION" },
                "evidence": {
                    "installedCliVersion": get(runtime, &["versionDrift", "installedCliVersion"]),
                    "cliSha256": cli_sha,
                    "noDaemonFlag": get(hooks, &["cliFlags", "noDaemon"]),
                    "hooksFeature": feature("hooks"),
                    "liveStandaloneTuiProcesses": standalone_tui,
                },
                "limitations": [
                    format!("Detached-hook ancestry: {}.", ancestry["status"].as_str().unwrap_or("UNKNOWN")),
                    "Embedded CLI exposes no app-server endpoint for history or status (SPEC §12.6); coverage is hooks plus optional legacy notify.".to_string(),
                ],
            },
            {
                "mode": "CODEX_DESKTOP_LOCAL",
                "availableNow": bundle.is_some(),
                "status": match (bundle.is_some(), desktop_app_servers.len(), desktop_bound) {
                    (false, _, _) => "NOT_INSTALLED",
                    (true, 0, _) => "INSTALLED_NOT_RUNNING",
                    (true, _, 0) => "RUNNING_STDIO_ONLY_NO_PASSIVE_ENDPOINT",
                    (true, _, _) => "RUNNING_WITH_BOUND_SOCKET",
                },
                "evidence": {
                    "bundle": bundle.map(|b| b["bundle"].clone()),
                    "bundleInfo": bundle.map(|b| b["info"].clone()),
                    "bundledRuntimeVersion": bundle.map(|b| get(b, &["runtime", "versionCommand", "version"]).clone()),
                    "bundledRuntimeSha256": desktop_sha,
                    "sameExecutableAsStandaloneCli": desktop_sha.as_ref().map(|d| *d == cli_sha),
                    "desktopAppServerProcesses": desktop_app_servers.len(),
                    "desktopAppServersOnStdio": desktop_stdio,
                    "desktopAppServerPathBoundUnixSockets": desktop_bound,
                    "otherDesktopRuntimeProcesses": desktop.len() - desktop_app_servers.len(),
                },
                "limitations": [
                    "The desktop runtime's app-servers use stdio pipes owned by the desktop app (and its helpers); there is no socket a passive observer can attach to.",
                    "App activation and a verified current-conversation route were not probed (SPEC §12.1 qualifies them separately).",
                ],
            },
        ],
    })
}

fn read_item(out: &Path, name: &str) -> Value {
    std::fs::read_to_string(out.join(format!("{name}.json")))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn write_json(ctx: &Ctx, name: &str, mut value: Value) -> Result<(), String> {
    ctx.redact_value(&mut value);
    let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(ctx.out.join(name), format!("{text}\n")).map_err(|e| format!("{:?}", e.kind()))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|index| args.get(index + 1))
            .cloned()
    };
    let (Some(command), Some(out)) = (args.get(1).cloned(), option("--out")) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        eprintln!("HOME is not set");
        return ExitCode::from(64);
    }
    if std::fs::create_dir_all(&out).is_err() {
        eprintln!("cannot create {out}");
        return ExitCode::from(73);
    }
    let Ok(out) = std::fs::canonicalize(&out) else {
        eprintln!("cannot resolve {out}");
        return ExitCode::from(73);
    };
    let absolute =
        |path: String| std::fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(path));
    let codex = option("--codex")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&home).join(".local/bin/codex"));
    let scratch =
        option("--scratch").map_or_else(|| PathBuf::from("/private/tmp/claude-501"), PathBuf::from);
    let hook_schemas = option("--hook-schemas").map(absolute);
    let mut ctx = Ctx::new(out, home, codex, scratch, hook_schemas);

    let started = now_ms();
    let (name, mut value) = match command.as_str() {
        "runtime" => ("runtime.json", runtime::probe(&mut ctx)),
        "schema" => ("schema.json", schema::probe(&mut ctx)),
        "hooks" => ("hooks.json", schema::hooks(&mut ctx)),
        "hook-ancestry" => ("hook-ancestry.json", hook_ancestry()),
        "daemon" => ("daemon.json", daemon::probe(&mut ctx)),
        "modes" => (
            "modes.json",
            modes(
                &read_item(&ctx.out, "runtime"),
                &read_item(&ctx.out, "hooks"),
                &read_item(&ctx.out, "hook-ancestry"),
                &read_item(&ctx.out, "daemon"),
            ),
        ),
        "all" => {
            let runtime = runtime::probe(&mut ctx);
            let schema = schema::probe(&mut ctx);
            let hooks = schema::hooks(&mut ctx);
            let ancestry = hook_ancestry();
            let daemon = daemon::probe(&mut ctx);
            let modes = modes(&runtime, &hooks, &ancestry, &daemon);
            (
                "probe.json",
                json!({
                    "items": {
                        "runtime": runtime,
                        "schema": schema,
                        "hooks": hooks,
                        "hookAncestry": ancestry,
                        "daemon": daemon,
                        "modes": modes,
                    },
                }),
            )
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(64);
        }
    };
    let disposable = ctx.cleanup();
    if let Value::Object(map) = &mut value {
        map.insert(
            "probe".into(),
            json!({
                "name": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION"),
                "argv": args,
                "command": command,
                "startedAt": stamp(started),
                "finishedAt": stamp(now_ms()),
                "host": {
                    "macosProductVersion": process::os_product_version().ok().map(|(a, b, c)| format!("{a}.{b}.{c}")),
                    "macosBuild": process::os_build_version().ok(),
                },
                "redaction": "$HOME prefix replaced with ~",
                "disposable": disposable,
            }),
        );
    }
    match write_json(&ctx, name, value) {
        Ok(()) => {
            println!("{}", ctx.out.join(name).display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("write failed: {error}");
            ExitCode::from(74)
        }
    }
}
