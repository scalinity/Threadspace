//! Items 2 and 3. What the installed build emits about itself without network
//! or model work — help text, the generated app-server JSON Schema (stable and
//! `--experimental`) and the feature list from an empty CODEX_HOME — compared
//! with exactly the methods, fields and values SPEC §12.2–§12.6 depend on. The
//! hook stdin schema has no emitter in the CLI, so it is compared from the
//! released fixtures passed with `--hook-schemas`, and the shipped executable
//! is checked for the distinctive field identifiers.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::support::{Ctx, now_ms, sha256_file, stamp};

enum Kind {
    /// Method names in a request/notification union file.
    Methods(&'static str),
    /// Property names of the file's top level (`None`) or a definition.
    Props(&'static str, Option<&'static str>),
    /// Enum or tagged-union values of a definition.
    Variants(&'static str, &'static str),
}

struct Check {
    id: &'static str,
    spec: &'static str,
    kind: Kind,
    expected: &'static [&'static str],
}

const APP_SERVER_CHECKS: &[Check] = &[
    Check {
        id: "initialize.method",
        spec: "§12.4 step 1",
        kind: Kind::Methods("ClientRequest.json"),
        expected: &["initialize"],
    },
    Check {
        id: "initialized.notification",
        spec: "§12.4 step 1",
        kind: Kind::Methods("ClientNotification.json"),
        expected: &["initialized"],
    },
    Check {
        id: "initialize.params",
        spec: "§12.4 step 1",
        kind: Kind::Props("v1/InitializeParams.json", None),
        expected: &["clientInfo", "capabilities"],
    },
    Check {
        id: "initialize.clientInfo",
        spec: "§12.4 step 1 (distinct observer client identity)",
        kind: Kind::Props("v1/InitializeParams.json", Some("ClientInfo")),
        expected: &["name", "title", "version"],
    },
    Check {
        id: "initialize.response.userAgent",
        spec: "§12.4 (record the running daemon version)",
        kind: Kind::Props("v1/InitializeResponse.json", None),
        expected: &["userAgent"],
    },
    Check {
        id: "thread/loaded/list.method",
        spec: "§12.4 step 3",
        kind: Kind::Methods("ClientRequest.json"),
        expected: &["thread/loaded/list"],
    },
    Check {
        id: "thread/loaded/list.params",
        spec: "§12.4 step 3 (explicit bounded limit and cursor)",
        kind: Kind::Props("v2/ThreadLoadedListParams.json", None),
        expected: &["limit", "cursor"],
    },
    Check {
        id: "thread/loaded/list.response",
        spec: "§12.4 step 3",
        kind: Kind::Props("v2/ThreadLoadedListResponse.json", None),
        expected: &["data", "nextCursor"],
    },
    Check {
        id: "thread/read.method",
        spec: "§12.4 step 3",
        kind: Kind::Methods("ClientRequest.json"),
        expected: &["thread/read"],
    },
    Check {
        id: "thread/read.params",
        spec: "§12.4 step 3",
        kind: Kind::Props("v2/ThreadReadParams.json", None),
        expected: &["threadId", "includeTurns"],
    },
    Check {
        id: "thread/read.params.historyMode",
        spec: "§12.4 step 3 (\"including the supported historyMode field\")",
        kind: Kind::Props("v2/ThreadReadParams.json", None),
        expected: &["historyMode"],
    },
    Check {
        id: "thread.fields",
        spec: "§12.3, §12.6",
        kind: Kind::Props("v2/ThreadReadResponse.json", Some("Thread")),
        expected: &[
            "id",
            "sessionId",
            "parentThreadId",
            "forkedFromId",
            "ephemeral",
            "historyMode",
            "status",
            "turns",
        ],
    },
    Check {
        id: "thread.canAcceptDirectInput",
        spec: "§12.3 (\"when present\")",
        kind: Kind::Props("v2/ThreadReadResponse.json", Some("Thread")),
        expected: &["canAcceptDirectInput"],
    },
    Check {
        id: "thread.historyMode.values",
        spec: "§12.6",
        kind: Kind::Variants("v2/ThreadReadResponse.json", "ThreadHistoryMode"),
        expected: &["legacy", "paginated"],
    },
    Check {
        id: "threadStatus.values",
        spec: "§12.5",
        kind: Kind::Variants("v2/ThreadReadResponse.json", "ThreadStatus"),
        expected: &["notLoaded", "idle", "active", "systemError"],
    },
    Check {
        id: "threadStatus.activeFlags",
        spec: "§12.5",
        kind: Kind::Variants("v2/ThreadReadResponse.json", "ThreadActiveFlag"),
        expected: &["waitingOnApproval", "waitingOnUserInput"],
    },
    Check {
        id: "turn.fields",
        spec: "§12.5, §12.6",
        kind: Kind::Props("v2/ThreadReadResponse.json", Some("Turn")),
        expected: &["id", "status", "startedAt", "completedAt"],
    },
    Check {
        id: "turn.status.values",
        spec: "§12.6",
        kind: Kind::Variants("v2/ThreadReadResponse.json", "TurnStatus"),
        expected: &["completed", "interrupted", "failed", "inProgress"],
    },
    Check {
        id: "thread/turns/list.method",
        spec: "§12.6",
        kind: Kind::Methods("ClientRequest.json"),
        expected: &["thread/turns/list"],
    },
    Check {
        id: "thread/turns/list.params",
        spec: "§12.6",
        kind: Kind::Props("v2/ThreadTurnsListParams.json", None),
        expected: &["threadId", "limit", "cursor", "sortDirection", "itemsView"],
    },
    Check {
        id: "thread/turns/list.itemsView",
        spec: "§12.6 (itemsView \"notLoaded\")",
        kind: Kind::Variants("v2/ThreadTurnsListParams.json", "TurnItemsView"),
        expected: &["notLoaded"],
    },
    Check {
        id: "thread/status/changed",
        spec: "§12.4 step 4",
        kind: Kind::Methods("ServerNotification.json"),
        expected: &["thread/status/changed"],
    },
    Check {
        id: "thread/status/changed.params",
        spec: "§12.4 step 4",
        kind: Kind::Props("v2/ThreadStatusChangedNotification.json", None),
        expected: &["threadId", "status"],
    },
    Check {
        id: "status.notifications",
        spec: "§12.5",
        kind: Kind::Methods("ServerNotification.json"),
        expected: &[
            "turn/started",
            "turn/completed",
            "error",
            "thread/closed",
            "thread/archived",
            "thread/unarchived",
            "thread/deleted",
            "item/started",
            "item/completed",
            "serverRequest/resolved",
        ],
    },
    Check {
        id: "error.willRetry",
        spec: "§12.5",
        kind: Kind::Props("v2/ErrorNotification.json", None),
        expected: &["willRetry"],
    },
];

/// Every method/notification name SPEC §12 cites (observer use or prohibition).
const SPEC_CITED: &[&str] = &[
    "initialize",
    "initialized",
    "thread/loaded/list",
    "thread/read",
    "thread/turns/list",
    "thread/status/changed",
    "turn/started",
    "turn/completed",
    "error",
    "thread/closed",
    "thread/archived",
    "thread/unarchived",
    "thread/deleted",
    "item/started",
    "item/completed",
    "serverRequest/resolved",
    "thread/resume",
];

/// Files copied into the evidence as the exact shapes the checks read.
const COPIED: &[&str] = &[
    "ClientNotification.json",
    "v1/InitializeParams.json",
    "v1/InitializeResponse.json",
    "v2/ThreadLoadedListParams.json",
    "v2/ThreadLoadedListResponse.json",
    "v2/ThreadReadParams.json",
    "v2/ThreadReadResponse.json",
    "v2/ThreadTurnsListParams.json",
    "v2/ThreadStatusChangedNotification.json",
    "v2/ErrorNotification.json",
    "v2/HooksListParams.json",
    "v2/HooksListResponse.json",
    "v2/HookStartedNotification.json",
    "v2/HookCompletedNotification.json",
];

const HOOK_COMMON: [&str; 4] = ["session_id", "transcript_path", "cwd", "hook_event_name"];
/// SPEC §12.2: "no universal event UUID, ordered sequence or source-terminal client ID".
const HOOK_ABSENT: [&str; 9] = [
    "id",
    "event_id",
    "event_uuid",
    "uuid",
    "sequence",
    "seq",
    "client_id",
    "terminal_id",
    "tty",
];
/// (event, fixture stem, SPEC §12.2 "important additional fields", fields SPEC says are absent)
const HOOK_ROWS: [(&str, &str, &[&str], &[&str]); 12] = [
    (
        "SessionStart",
        "session-start",
        &["model", "permission_mode", "source"],
        &[],
    ),
    ("SessionEnd", "session-end", &["reason"], &[]),
    (
        "UserPromptSubmit",
        "user-prompt-submit",
        &["turn_id", "prompt", "agent_id", "agent_type"],
        &[],
    ),
    (
        "PreToolUse",
        "pre-tool-use",
        &["turn_id", "tool_use_id", "tool_name", "tool_input"],
        &[],
    ),
    (
        "PermissionRequest",
        "permission-request",
        &["turn_id", "tool_name", "tool_input"],
        &["tool_use_id"],
    ),
    (
        "PostToolUse",
        "post-tool-use",
        &["turn_id", "tool_use_id", "tool_response"],
        &[],
    ),
    ("PreCompact", "pre-compact", &["turn_id", "trigger"], &[]),
    ("PostCompact", "post-compact", &["turn_id", "trigger"], &[]),
    (
        "SubagentStart",
        "subagent-start",
        &["turn_id", "agent_id", "agent_type"],
        &[],
    ),
    (
        "SubagentStop",
        "subagent-stop",
        &[
            "turn_id",
            "agent_id",
            "agent_type",
            "agent_transcript_path",
            "stop_hook_active",
        ],
        &[],
    ),
    (
        "Stop",
        "stop",
        &["turn_id", "stop_hook_active", "last_assistant_message"],
        &[],
    ),
    (
        "Interrupt",
        "interrupt",
        &["turn_id", "model", "permission_mode"],
        &[],
    ),
];
const DISTINCTIVE_HOOK_IDENTIFIERS: [&str; 7] = [
    "transcript_path",
    "hook_event_name",
    "tool_use_id",
    "permission_mode",
    "stop_hook_active",
    "last_assistant_message",
    "agent_transcript_path",
];

fn load(dir: &Path, relative: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(relative)).ok()?).ok()
}

fn definition<'a>(schema: &'a Value, def: Option<&str>) -> Option<&'a Value> {
    match def {
        None => Some(schema),
        Some(name) => schema.get("definitions")?.get(name),
    }
}

fn props(schema: &Value, def: Option<&str>) -> Option<BTreeSet<String>> {
    Some(
        definition(schema, def)?
            .get("properties")?
            .as_object()?
            .keys()
            .cloned()
            .collect(),
    )
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(text)) => vec![text.clone()],
        _ => Vec::new(),
    }
}

fn variants(schema: &Value, def: &str) -> Option<BTreeSet<String>> {
    let node = definition(schema, Some(def))?;
    let mut out: BTreeSet<String> = strings(node.get("enum")).into_iter().collect();
    for alternative in node
        .get("oneOf")
        .or_else(|| node.get("anyOf"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.extend(strings(alternative.get("enum")));
        out.extend(strings(alternative.get("const")));
        let tag = alternative.get("properties").and_then(|p| p.get("type"));
        out.extend(strings(tag.and_then(|t| t.get("enum"))));
        out.extend(strings(tag.and_then(|t| t.get("const"))));
    }
    Some(out)
}

pub fn methods(schema: &Value) -> BTreeSet<String> {
    schema
        .get("oneOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|alt| alt.get("properties")?.get("method"))
        .flat_map(|method| {
            let mut names = strings(method.get("enum"));
            names.extend(strings(method.get("const")));
            names
        })
        .collect()
}

fn found_set(dir: &Path, kind: &Kind) -> Option<BTreeSet<String>> {
    match kind {
        Kind::Methods(file) => Some(methods(&load(dir, file)?)),
        Kind::Props(file, def) => props(&load(dir, file)?, *def),
        Kind::Variants(file, def) => variants(&load(dir, file)?, def),
    }
}

fn file_of(kind: &Kind) -> String {
    match kind {
        Kind::Methods(file) => file.to_string(),
        Kind::Props(file, def) => format!(
            "{file}{}",
            def.map(|d| format!("#/definitions/{d}"))
                .unwrap_or_default()
        ),
        Kind::Variants(file, def) => format!("{file}#/definitions/{def}"),
    }
}

fn run_checks(stable: &Path, experimental: Option<&Path>) -> (Vec<Value>, Map<String, Value>) {
    let mut results = Vec::new();
    let mut counts = Map::new();
    for check in APP_SERVER_CHECKS {
        let found = found_set(stable, &check.kind);
        let missing: Vec<&str> = check
            .expected
            .iter()
            .filter(|name| !found.as_ref().is_some_and(|set| set.contains(**name)))
            .copied()
            .collect();
        let experimental_found = experimental.and_then(|dir| found_set(dir, &check.kind));
        let only_experimental: Vec<&str> = missing
            .iter()
            .filter(|name| {
                experimental_found
                    .as_ref()
                    .is_some_and(|set| set.contains(**name))
            })
            .copied()
            .collect();
        let status = if found.is_none() {
            "SCHEMA_FILE_MISSING"
        } else if missing.is_empty() {
            "MATCH"
        } else if only_experimental.len() == missing.len() {
            "EXPERIMENTAL_ONLY"
        } else {
            "MISSING"
        };
        let seen = counts.get(status).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(status.into(), json!(seen + 1));
        results.push(json!({
            "id": check.id,
            "spec": check.spec,
            "schema": file_of(&check.kind),
            "expected": check.expected,
            "missing": missing,
            "presentOnlyWithExperimental": only_experimental,
            "status": status,
        }));
    }
    (results, counts)
}

fn prefixed(set: &BTreeSet<String>, prefixes: &[&str]) -> Vec<String> {
    set.iter()
        .filter(|name| {
            prefixes.iter().any(|p| name.starts_with(p)) && !SPEC_CITED.contains(&name.as_str())
        })
        .cloned()
        .collect()
}

fn method_sets(dir: &Path) -> Value {
    let mut out = Map::new();
    for file in [
        "ClientRequest",
        "ClientNotification",
        "ServerRequest",
        "ServerNotification",
    ] {
        let set = load(dir, &format!("{file}.json"))
            .map(|v| methods(&v))
            .unwrap_or_default();
        out.insert(file.into(), json!(set));
    }
    Value::Object(out)
}

fn manifest(dir: &Path) -> (Vec<Value>, u64) {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok((sha, size)) = sha256_file(&path) {
                let relative = path
                    .strip_prefix(dir)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                files.push((relative, sha, size));
            }
        }
    }
    files.sort();
    let total = files.iter().map(|(_, _, size)| size).sum();
    (
        files
            .into_iter()
            .map(|(path, sha256, bytes)| json!({ "path": path, "sha256": sha256, "bytes": bytes }))
            .collect(),
        total,
    )
}

fn executable_contains(ctx: &Ctx, exe: &str, needle: &str) -> Value {
    let ran = ctx.run_plain(
        "/usr/bin/grep",
        &["-a", "-F", "-q", "-e", needle, exe],
        Duration::from_secs(60),
        1024,
    );
    let status = ran.record.get("exitStatus").and_then(Value::as_i64);
    json!({ "identifier": needle, "present": status == Some(0), "grepExit": status })
}

fn hook_fixture_comparison(ctx: &Ctx, dir: &Path, exe: Option<&str>) -> Value {
    let mut rows = Vec::new();
    let mut all_match = true;
    for (event, stem, expected, absent) in HOOK_ROWS {
        let file = format!("{stem}.command.input.schema.json");
        let Some(schema) = load(dir, &file) else {
            all_match = false;
            rows.push(json!({ "event": event, "file": file, "status": "FIXTURE_MISSING" }));
            continue;
        };
        let fields = props(&schema, None).unwrap_or_default();
        let required: BTreeSet<String> = strings(schema.get("required")).into_iter().collect();
        let wanted: Vec<&str> = HOOK_COMMON.iter().chain(expected.iter()).copied().collect();
        let missing: Vec<&str> = wanted
            .iter()
            .filter(|f| !fields.contains(**f))
            .copied()
            .collect();
        let contradicting: Vec<&str> = absent
            .iter()
            .chain(HOOK_ABSENT.iter())
            .filter(|f| fields.contains(**f))
            .copied()
            .collect();
        let extra: Vec<&String> = fields
            .iter()
            .filter(|f| !wanted.contains(&f.as_str()))
            .collect();
        let property = |name: &str| schema.get("properties").and_then(|p| p.get(name));
        let transcript_nullable = property("transcript_path")
            .and_then(|p| p.get("$ref"))
            .and_then(Value::as_str)
            .and_then(|r| r.rsplit('/').next())
            .and_then(|d| schema.get("definitions")?.get(d))
            .is_some_and(|d| strings(d.get("type")).iter().any(|t| t == "null"));
        let event_const = property("hook_event_name")
            .and_then(|p| p.get("const"))
            .cloned();
        let mut values = Map::new();
        for name in ["source", "reason", "trigger"] {
            if let Some(p) = property(name) {
                let mut listed = strings(p.get("enum"));
                listed.extend(strings(p.get("const")));
                values.insert(name.into(), json!(listed));
            }
        }
        let ok = missing.is_empty()
            && contradicting.is_empty()
            && transcript_nullable
            && event_const == Some(json!(event));
        all_match &= ok;
        rows.push(json!({
            "event": event,
            "file": file,
            "hookEventNameConst": event_const,
            "specFields": wanted,
            "missing": missing,
            "specAbsentButPresent": contradicting,
            "extraFields": extra,
            "required": required,
            "transcriptPathNullable": transcript_nullable,
            "values": values,
            "status": if ok { "MATCH" } else { "DIFFERS" },
        }));
    }
    let session_start_sources: Vec<String> = rows
        .iter()
        .find(|r| r["event"] == "SessionStart")
        .and_then(|r| r["values"]["source"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let spec_sources = ["startup", "resume", "clear", "compact"];
    let fixture_hashes: Vec<Value> = HOOK_ROWS
        .iter()
        .map(|(_, stem, _, _)| {
            let file = format!("{stem}.command.input.schema.json");
            json!({ "file": file, "sha256": sha256_file(&dir.join(&file)).ok().map(|(s, _)| s) })
        })
        .collect();
    json!({
        "fixtures": dir.display().to_string(),
        "fixtureSha256": fixture_hashes,
        "events": rows,
        "allRowsMatch": all_match,
        "sessionStartSource": {
            "spec": spec_sources,
            "released": session_start_sources,
            "extra": session_start_sources.iter().filter(|s| !spec_sources.contains(&s.as_str())).collect::<Vec<_>>(),
        },
        "executableIdentifiers": exe.map(|exe| {
            DISTINCTIVE_HOOK_IDENTIFIERS.iter().map(|id| executable_contains(ctx, exe, id)).collect::<Vec<_>>()
        }),
    })
}

fn help_capture(ctx: &mut Ctx, name: &str, args: &[&str]) -> Value {
    let codex = ctx.codex.clone();
    let ran = ctx.run_codex(&codex, args, Duration::from_secs(20), 256 * 1024);
    let raw = ctx.save_raw(&format!("help/{name}.txt"), &ran.stdout);
    json!({ "command": ran.record, "raw": raw, "stderr": ran.stderr.trim() })
}

fn bundle_summary(ctx: &mut Ctx, experimental: bool) -> (Option<PathBuf>, Value) {
    let label = if experimental {
        "experimental"
    } else {
        "stable"
    };
    let generated = ctx.schema_bundle(experimental);
    let record = if experimental {
        &ctx.experimental_schema
    } else {
        &ctx.stable_schema
    };
    let command = record
        .as_ref()
        .map(|(_, r)| r.clone())
        .unwrap_or(Value::Null);
    let Some((dir, _)) = generated else {
        return (None, json!({ "command": command, "generated": false }));
    };
    let (files, total) = manifest(&dir);
    let count = files.len();
    let manifest_path = ctx.save_raw_json(
        &format!("app-server-schema/{label}-manifest.json"),
        &Value::Array(files),
    );
    let copies: &[&str] = if experimental {
        &["v2/ThreadReadResponse.json"]
    } else {
        COPIED
    };
    let mut copied = Vec::new();
    for relative in copies {
        if let Ok(text) = std::fs::read_to_string(dir.join(relative)) {
            copied.push(json!(
                ctx.save_raw(&format!("app-server-schema/{label}/{relative}"), &text)
            ));
        }
    }
    (
        Some(dir),
        json!({
            "command": command,
            "generated": true,
            "fileCount": count,
            "totalBytes": total,
            "manifest": manifest_path,
            "copiedFiles": copied,
        }),
    )
}

/// Item 2.
pub fn probe(ctx: &mut Ctx) -> Value {
    let started = now_ms();
    let help = json!({
        "codex": help_capture(ctx, "codex", &["--help"]),
        "appServer": help_capture(ctx, "app-server", &["app-server", "--help"]),
        "appServerDaemon": help_capture(ctx, "app-server-daemon", &["app-server", "daemon", "--help"]),
        "appServerDaemonVersion": help_capture(ctx, "app-server-daemon-version", &["app-server", "daemon", "version", "--help"]),
        "generateJsonSchema": help_capture(ctx, "app-server-generate-json-schema", &["app-server", "generate-json-schema", "--help"]),
    });
    let (stable_dir, stable) = bundle_summary(ctx, false);
    let (experimental_dir, experimental) = bundle_summary(ctx, true);

    let mut comparison = Map::new();
    if let Some(stable_dir) = &stable_dir {
        let (checks, counts) = run_checks(stable_dir, experimental_dir.as_deref());
        let stable_methods = method_sets(stable_dir);
        let experimental_methods = experimental_dir
            .as_deref()
            .map(method_sets)
            .unwrap_or(Value::Null);
        let methods_path = ctx.save_raw_json(
            "app-server-schema/methods.json",
            &json!({ "stable": stable_methods, "experimental": experimental_methods }),
        );
        let set = |value: &Value, key: &str| -> BTreeSet<String> {
            strings(value.get(key)).into_iter().collect()
        };
        let stable_requests = set(&stable_methods, "ClientRequest");
        let stable_notifications = set(&stable_methods, "ServerNotification");
        let experimental_requests = set(&experimental_methods, "ClientRequest");
        let stable_thread = load(stable_dir, "v2/ThreadReadResponse.json")
            .and_then(|s| props(&s, Some("Thread")))
            .unwrap_or_default();
        let experimental_thread = experimental_dir
            .as_deref()
            .and_then(|d| load(d, "v2/ThreadReadResponse.json"))
            .and_then(|s| props(&s, Some("Thread")))
            .unwrap_or_default();
        let initialize = load(stable_dir, "v1/InitializeParams.json");
        comparison.insert("checks".into(), Value::Array(checks));
        comparison.insert("statusCounts".into(), Value::Object(counts));
        comparison.insert("methodLists".into(), json!(methods_path));
        comparison.insert(
            "counts".into(),
            json!({
                "stableClientRequests": stable_requests.len(),
                "stableServerNotifications": stable_notifications.len(),
                "experimentalClientRequests": experimental_requests.len(),
            }),
        );
        comparison.insert(
            "extra".into(),
            json!({
                "threadHookClientRequestsNotCitedBySpec": prefixed(&stable_requests, &["thread/", "hooks/"]),
                "threadTurnItemHookNotificationsNotCitedBySpec": prefixed(&stable_notifications, &["thread/", "turn/", "item/", "hook/"]),
                "experimentalOnlyClientRequests": experimental_requests.difference(&stable_requests).collect::<Vec<_>>(),
                "experimentalOnlyThreadFields": experimental_thread.difference(&stable_thread).collect::<Vec<_>>(),
                "initializeCapabilities": initialize.as_ref().and_then(|s| props(s, Some("InitializeCapabilities"))),
                "initializeResponseFields": load(stable_dir, "v1/InitializeResponse.json").and_then(|s| props(&s, None)),
            }),
        );
    }

    let exe = std::fs::canonicalize(&ctx.codex)
        .ok()
        .map(|p| p.display().to_string());
    let hook_input = match ctx.hook_schemas.clone() {
        Some(dir) => hook_fixture_comparison(ctx, &dir, exe.as_deref()),
        None => json!({ "status": "NOT_RUN", "reason": "no --hook-schemas directory given" }),
    };

    json!({
        "item": "schema",
        "status": "OBSERVED",
        "startedAt": stamp(started),
        "finishedAt": stamp(now_ms()),
        "help": help,
        "appServerSchema": { "stable": stable, "experimental": experimental, "comparison": comparison },
        "hookInputSchema": {
            "emittedByInstalledBuild": false,
            "why": "the CLI has no hook-schema generator; generate-json-schema covers only the app-server protocol",
            "comparison": hook_input,
        },
    })
}

fn feature_rows(text: &str, names: &[&str]) -> Value {
    let mut out = Map::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let (Some(name), Some(enabled)) = (tokens.first(), tokens.last()) else {
            continue;
        };
        if tokens.len() >= 3 && names.contains(name) {
            out.insert(
                name.to_string(),
                json!({ "stage": tokens[1..tokens.len() - 1].join(" "), "enabled": *enabled == "true" }),
            );
        }
    }
    Value::Object(out)
}

/// Item 3.
pub fn hooks(ctx: &mut Ctx) -> Value {
    let started = now_ms();
    let codex = ctx.codex.clone();
    let features = ctx.run_codex(
        &codex,
        &["features", "list"],
        Duration::from_secs(30),
        256 * 1024,
    );
    let features_raw = ctx.save_raw("features-list-empty-codex-home.txt", &features.stdout);
    let help = ctx.run_codex(&codex, &["--help"], Duration::from_secs(20), 256 * 1024);
    let stable = ctx.schema_bundle(false).map(|(dir, _)| dir);

    let enums = stable.as_deref().map(|dir| {
        let list = load(dir, "v2/HooksListResponse.json");
        let started = load(dir, "v2/HookStartedNotification.json");
        let from = |schema: &Option<Value>, def: &str| schema.as_ref().and_then(|s| variants(s, def));
        json!({
            "HookEventName": from(&list, "HookEventName"),
            "HookSource": from(&list, "HookSource"),
            "HookTrustStatus": from(&list, "HookTrustStatus"),
            "HookHandlerType": from(&started, "HookHandlerType"),
            "HookExecutionMode": from(&started, "HookExecutionMode"),
            "HookScope": from(&started, "HookScope"),
            "HookRunStatus": from(&started, "HookRunStatus"),
            "hooksListParams": load(dir, "v2/HooksListParams.json").and_then(|s| props(&s, None)),
            "hookMetadataFields": list.as_ref().and_then(|s| props(s, Some("HookMetadata"))),
            "hooksListMethod": load(dir, "ClientRequest.json").map(|s| methods(&s).contains("hooks/list")),
            "hookNotifications": load(dir, "ServerNotification.json").map(|s| {
                methods(&s).into_iter().filter(|m| m.starts_with("hook/")).collect::<Vec<_>>()
            }),
        })
    });
    let spec_events: Vec<String> = HOOK_ROWS
        .iter()
        .map(|(event, ..)| {
            let mut chars = event.chars();
            chars
                .next()
                .map(|first| first.to_ascii_lowercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    let schema_events: BTreeSet<String> = enums
        .as_ref()
        .map(|e| strings(e.get("HookEventName")).into_iter().collect())
        .unwrap_or_default();
    let exe = std::fs::canonicalize(&ctx.codex)
        .ok()
        .map(|p| p.display().to_string());

    json!({
        "item": "hooks",
        "status": "OBSERVED",
        "startedAt": stamp(started),
        "finishedAt": stamp(now_ms()),
        "featureFlags": {
            "source": "codex features list in an empty disposable CODEX_HOME (build defaults, not the owner's effective configuration)",
            "command": features.record,
            "raw": features_raw,
            "rows": feature_rows(&features.stdout, &["hooks", "plugin_hooks", "daemon_auto_start", "plugins"]),
        },
        "cliFlags": {
            "dangerouslyBypassHookTrust": help.stdout.contains("--dangerously-bypass-hook-trust"),
            "noDaemon": help.stdout.contains("--no-daemon"),
            "command": help.record,
        },
        "appServerSchema": enums,
        "eventSetVsSpec": {
            "spec": spec_events,
            "installedHookEventName": schema_events,
            "missing": spec_events.iter().filter(|e| !schema_events.contains(*e)).collect::<Vec<_>>(),
            "extra": schema_events.iter().filter(|e| !spec_events.contains(e)).collect::<Vec<_>>(),
        },
        "configurationLocations": {
            "source": "openai/codex rust-v0.160.0 (a956835d) codex-rs/hooks/src/engine/discovery.rs: load_hooks_json L339-344, load_toml_hooks_from_layer L382-387, layer source mapping L825-836, unsupported handlers L592/L638/L648",
            "perConfigLayer": [
                "<layer hooks folder>/hooks.json (JSON {\"hooks\": {...}})",
                "[hooks] table in the same layer's config.toml",
            ],
            "layers": "system, user (CODEX_HOME), project (.codex/), MDM, enterprise-managed, legacy managed config, session flags; plus managed requirements and plugin hook sources",
            "bothFormsInOneLayer": "loaded together with a warning",
            "handlerTypesExecuted": ["command", "mcpTool (not for SessionEnd)"],
            "handlerTypesSkipped": ["prompt (\"not supported yet\")", "agent (\"not supported yet\")"],
            "trust": "non-managed hooks run only when enabled and trusted for their current hash, unless --dangerously-bypass-hook-trust",
            "executableCarriesHooksJsonName": exe.as_deref().map(|exe| executable_contains(ctx, exe, "hooks.json")),
        },
        "ownerConfiguration": "not read and not modified; nothing was installed",
    })
}
