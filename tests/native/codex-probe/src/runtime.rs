//! Item 1: the installed Codex CLI and every other Codex runtime on this
//! machine — path, symlink resolution, version, executable SHA-256 and
//! code-signature summary — plus the live processes running those
//! executables, classified from an allowlist of argv tokens only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};
use threadspace_surfaces_macos::process;

use crate::support::{Ctx, now_ms, sha256_file, stamp};

pub const SPEC_RESEARCHED_VERSION: &str = "0.160.1";
pub const SPEC_RELEASE_COMMIT: &str = "d27764b82f7118f674371e6d6e76271d9d606edb";
const DESKTOP_BUNDLES: [&str; 2] = ["/Applications/ChatGPT.app", "/Applications/Codex.app"];
const BUNDLED_RUNTIME: &str = "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex";
const BUNDLED_MANIFEST: &str = "Contents/Resources/codex-cli/codex-package.json";
const CODESIGN_KEYS: [&str; 17] = [
    "Executable",
    "Identifier",
    "Format",
    "CodeDirectory v",
    "Hash type",
    "CandidateCDHashFull sha256",
    "CDHash",
    "Signature size",
    "Authority",
    "Timestamp",
    "Info.plist",
    "Info.plist entries",
    "TeamIdentifier",
    "Runtime Version",
    "Sealed Resources",
    "Sealed Resources version",
    "Internal requirements count",
];
const SUBCOMMANDS: [&str; 22] = [
    "app-server",
    "exec-server",
    "exec",
    "e",
    "review",
    "resume",
    "fork",
    "mcp",
    "mcp-server",
    "agents",
    "remote-control",
    "login",
    "logout",
    "sandbox",
    "debug",
    "cloud",
    "apply",
    "queue",
    "plugin",
    "features",
    "doctor",
    "update",
];
const VALUE_OPTIONS: [&str; 6] = ["-c", "--config", "--enable", "--disable", "--remote", "-p"];

fn symlink_chain(start: &Path) -> Vec<Value> {
    let mut hops = Vec::new();
    let mut current = start.to_path_buf();
    for _ in 0..16 {
        let Ok(target) = std::fs::read_link(&current) else {
            break;
        };
        let resolved = if target.is_absolute() {
            target.clone()
        } else {
            current.parent().unwrap_or(Path::new("/")).join(&target)
        };
        hops.push(json!({ "link": current.display().to_string(), "target": target.display().to_string() }));
        current = resolved;
    }
    hops
}

fn codesign_summary(ctx: &Ctx, path: &Path) -> Value {
    let path_text = path.display().to_string();
    let display = ctx.run_plain(
        "/usr/bin/codesign",
        &["-d", "--verbose=4", &path_text],
        Duration::from_secs(30),
        64 * 1024,
    );
    let mut fields = Map::new();
    let mut authorities = Vec::new();
    for line in display.stderr.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !CODESIGN_KEYS.contains(&key) {
            continue;
        }
        if key == "Authority" {
            authorities.push(json!(value));
        } else {
            fields.insert(key.to_string(), json!(value));
        }
    }
    fields.insert("Authority".into(), Value::Array(authorities));
    let verify = ctx.run_plain(
        "/usr/bin/codesign",
        &["--verify", "--strict", &path_text],
        Duration::from_secs(120),
        16 * 1024,
    );
    json!({
        "display": { "command": display.record, "fields": fields },
        "verify": {
            "command": verify.record,
            "valid": verify.ok,
            "stderr": verify.stderr.trim(),
        },
    })
}

pub fn executable_facts(ctx: &Ctx, path: &Path) -> Value {
    let digest = sha256_file(path);
    json!({
        "path": path.display().to_string(),
        "sha256": digest.as_ref().ok().map(|(sha, _)| sha.clone()),
        "sizeBytes": digest.as_ref().ok().map(|(_, size)| *size),
        "readError": digest.err(),
        "codeSignature": codesign_summary(ctx, path),
    })
}

pub fn manifest(path: &Path) -> Value {
    let parsed = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    match parsed {
        Some(value) => json!({
            "path": path.display().to_string(),
            "version": value.get("version"),
            "target": value.get("target"),
            "variant": value.get("variant"),
            "layoutVersion": value.get("layoutVersion"),
        }),
        None => json!({ "path": path.display().to_string(), "present": false }),
    }
}

/// Runs `<exe> --version` in the disposable environment.
fn version_of(ctx: &mut Ctx, exe: &Path) -> Value {
    let ran = ctx.run_codex(exe, &["--version"], Duration::from_secs(20), 4096);
    json!({
        "command": ran.record,
        "stdout": ran.stdout.trim(),
        "stderr": ran.stderr.trim(),
        "version": ran.stdout.split_whitespace().last(),
    })
}

/// A package root's selection (`current` symlink), auto-update pin and
/// installed release directory names.
pub fn package_selection(ctx: &Ctx, name: &str) -> Value {
    let root = ctx.owner_path(&format!(".codex/packages/{name}"));
    let current = root.join("current");
    let target = std::fs::read_link(&current).ok();
    let mut releases: Vec<String> = std::fs::read_dir(root.join("releases"))
        .map(|read| {
            read.flatten()
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    releases.sort();
    json!({
        "root": root.display().to_string(),
        "present": root.is_dir(),
        "current": target.as_ref().map(|t| t.display().to_string()),
        "currentManifest": manifest(&current.join("codex-package.json")),
        "autoUpdateVersion": std::fs::read_to_string(root.join("auto-update-version")).ok().map(|t| t.trim().to_string()),
        "installedReleases": releases,
    })
}

fn classify_argv(ctx: &Ctx, args: &str) -> Value {
    let tokens: Vec<&str> = args.split_whitespace().skip(1).collect();
    let mut subcommand = None;
    let mut app_server_subcommand = None;
    let mut listen = None;
    let mut config_overrides = 0;
    let mut previous = "";
    for (index, token) in tokens.iter().enumerate() {
        let is_value = VALUE_OPTIONS.contains(&previous);
        if matches!(*token, "-c" | "--config") {
            config_overrides += 1;
        }
        if !is_value && subcommand.is_none() && SUBCOMMANDS.contains(token) {
            subcommand = Some(*token);
            if *token == "app-server" {
                app_server_subcommand = tokens
                    .get(index + 1)
                    .filter(|next| {
                        matches!(
                            **next,
                            "daemon" | "proxy" | "generate-ts" | "generate-json-schema"
                        )
                    })
                    .copied();
            }
        }
        if previous == "--listen" {
            listen = Some(ctx.redact(token));
        }
        previous = token;
    }
    let listen_effective = match (subcommand, &listen, tokens.contains(&"--stdio")) {
        (Some("app-server"), None, _) | (Some("app-server"), _, true)
            if app_server_subcommand.is_none() =>
        {
            Some("stdio://".to_string())
        }
        _ => listen.clone(),
    };
    json!({
        "subcommand": subcommand.unwrap_or("(none: interactive TUI)"),
        "appServerSubcommand": app_server_subcommand,
        "listen": listen,
        "listenEffective": listen_effective,
        "noDaemon": tokens.contains(&"--no-daemon"),
        "configOverrideCount": config_overrides,
        "remoteGiven": tokens.contains(&"--remote"),
        "argvRecorded": "allowlisted tokens only; option values other than --listen are withheld",
    })
}

/// Live processes whose kernel executable path is one of `executables`.
pub fn runtime_processes(ctx: &Ctx, executables: &[(String, PathBuf)]) -> Vec<Value> {
    let mut found = Vec::new();
    for (label, path) in executables {
        let path_text = path.display().to_string();
        let pids = match process::pids_with_executable(&path_text) {
            Ok(pids) => pids,
            Err(error) => {
                found.push(json!({ "runtime": label, "enumerationError": error.code() }));
                continue;
            }
        };
        for pid in pids {
            let incarnation = process::sample_incarnation(pid);
            let pid_text = pid.to_string();
            let ps = ctx.run_plain(
                "/bin/ps",
                &["-ww", "-o", "args=", "-p", &pid_text],
                Duration::from_secs(10),
                64 * 1024,
            );
            let lsof = ctx.run_plain(
                "/usr/sbin/lsof",
                &["-a", "-p", &pid_text, "-U", "-F", "n"],
                Duration::from_secs(20),
                256 * 1024,
            );
            let names: Vec<&str> = lsof.stdout.lines().filter(|l| l.starts_with('n')).collect();
            found.push(json!({
                "runtime": label,
                "executable": path_text,
                "pid": pid,
                "process": match &incarnation {
                    Ok(i) => json!({
                        "ppid": i.sample.ppid,
                        "startSeconds": i.sample.start_seconds.to_string(),
                        "startMicroseconds": i.sample.start_microseconds,
                        "hasControllingTerminal": i.sample.controlling_device.is_some(),
                    }),
                    Err(error) => json!({ "error": error.code() }),
                },
                "argv": classify_argv(ctx, ps.stdout.trim()),
                "unixSockets": {
                    "total": names.len(),
                    "pathBound": names.iter().filter(|n| n.starts_with("n/")).count(),
                    "command": lsof.record,
                },
            }));
        }
    }
    found
}

fn desktop_bundle(ctx: &mut Ctx, bundle: &Path) -> Value {
    let plist = bundle.join("Contents/Info.plist").display().to_string();
    let mut info = Map::new();
    for key in [
        "CFBundleIdentifier",
        "CFBundleShortVersionString",
        "CFBundleVersion",
        "CFBundleName",
    ] {
        let ran = ctx.run_plain(
            "/usr/bin/plutil",
            &["-extract", key, "raw", "-o", "-", &plist],
            Duration::from_secs(10),
            4096,
        );
        info.insert(
            key.into(),
            if ran.ok {
                json!(ran.stdout.trim())
            } else {
                Value::Null
            },
        );
    }
    let runtime = bundle.join(BUNDLED_RUNTIME);
    let runtime_value = if runtime.is_file() {
        json!({
            "manifest": manifest(&bundle.join(BUNDLED_MANIFEST)),
            "executable": executable_facts(ctx, &runtime),
            "versionCommand": version_of(ctx, &runtime),
        })
    } else {
        json!({ "present": false, "expectedPath": runtime.display().to_string() })
    };
    json!({
        "kind": "CODEX_DESKTOP_BUNDLED",
        "bundle": bundle.display().to_string(),
        "info": info,
        "runtime": runtime_value,
    })
}

pub fn probe(ctx: &mut Ctx) -> Value {
    let started = now_ms();
    let entry = ctx.codex.clone();
    let canonical = std::fs::canonicalize(&entry).ok();
    let mut cli = Map::new();
    cli.insert("entry".into(), json!(entry.display().to_string()));
    cli.insert("symlinkChain".into(), Value::Array(symlink_chain(&entry)));
    cli.insert(
        "canonicalPath".into(),
        json!(canonical.as_ref().map(|p| p.display().to_string())),
    );
    let mut installed_version = None;
    if let Some(exe) = &canonical {
        cli.insert("executable".into(), executable_facts(ctx, exe));
        let release_dir = exe.parent().and_then(Path::parent);
        if let Some(dir) = release_dir {
            cli.insert(
                "packageManifest".into(),
                manifest(&dir.join("codex-package.json")),
            );
        }
        let version = version_of(ctx, exe);
        installed_version = version["version"].as_str().map(str::to_string);
        cli.insert("versionCommand".into(), version);
    }

    let drift = json!({
        "specResearchedVersion": SPEC_RESEARCHED_VERSION,
        "specReleaseCommit": SPEC_RELEASE_COMMIT,
        "installedCliVersion": installed_version,
        "matchesSpec": installed_version.as_deref() == Some(SPEC_RESEARCHED_VERSION),
        "releaseComparison": "raw/release-diff-0.160.0-0.160.1.json (GitHub tag/compare readback; not produced by this binary)",
    });

    let standalone = package_selection(ctx, "standalone");
    let daemon_package = package_selection(ctx, "app-server-daemon");
    let mut others = Vec::new();
    for bundle in DESKTOP_BUNDLES {
        let path = Path::new(bundle);
        if path.is_dir() {
            others.push(desktop_bundle(ctx, path));
        } else {
            others.push(
                json!({ "kind": "CODEX_DESKTOP_BUNDLED", "bundle": bundle, "present": false }),
            );
        }
    }
    let daemon_exe = ctx.owner_path(".codex/packages/app-server-daemon/current/bin/codex");
    if let Ok(exe) = std::fs::canonicalize(&daemon_exe) {
        others.push(json!({
            "kind": "CODEX_MANAGED_DAEMON_PACKAGE",
            "selection": daemon_package.clone(),
            "executable": executable_facts(ctx, &exe),
            "versionCommand": version_of(ctx, &exe),
        }));
    }

    // Every installed Codex executable, for live-process attribution.
    let mut executables: Vec<(String, PathBuf)> = Vec::new();
    for (label, package) in [
        ("standalone", &standalone),
        ("app-server-daemon", &daemon_package),
    ] {
        for release in package["installedReleases"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(release) = release.as_str() else {
                continue;
            };
            let exe = ctx.owner_path(&format!(
                ".codex/packages/{label}/releases/{release}/bin/codex"
            ));
            if exe.is_file() {
                executables.push((format!("{label}/{release}"), exe));
            }
        }
    }
    for bundle in DESKTOP_BUNDLES {
        let exe = Path::new(bundle).join(BUNDLED_RUNTIME);
        if exe.is_file() {
            executables.push((format!("desktop:{bundle}"), exe));
        }
    }
    let processes = runtime_processes(ctx, &executables);

    json!({
        "item": "runtime",
        "status": "OBSERVED",
        "startedAt": stamp(started),
        "finishedAt": stamp(now_ms()),
        "cli": cli,
        "versionDrift": drift,
        "packages": { "standalone": standalone, "appServerDaemon": daemon_package },
        "otherRuntimes": others,
        "processes": processes,
        "disposableHomeAfter": ctx.disposable_home_tree(),
    })
}
