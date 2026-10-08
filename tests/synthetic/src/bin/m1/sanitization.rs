//! Sanitization evidence (SPEC §8.5, §19.4): Claude hook inputs carrying
//! planted secrets in every body and path field are captured by the real
//! `threadspace-hook` into the fixture companion (journal path) and, with
//! admission closed, into the spool (spool path). The stored rows are
//! snapshotted, and every file under the store (database, WAL, spool, logs)
//! is scanned for the planted values.

use std::path::Path;
use std::process::{Command, Stdio};

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::evidence::Area;

const SECRETS: &[&str] = &[
    "PLANTED-PROMPT-7d1",
    "PLANTED-TOOL-INPUT-e42",
    "PLANTED-TOOL-OUTPUT-9b0",
    "PLANTED-ASSISTANT-c55",
    "PLANTED-MESSAGE-a18",
    "PLANTED-PATH-f63",
    "PLANTED-ERROR-0aa",
];

const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Stop",
    "SubagentStart",
    "SubagentStop",
    "Notification",
    "StopFailure",
    "SessionEnd",
    "Elicitation",
    "ElicitationResult",
];

fn input(event: &str, index: usize) -> Vec<u8> {
    let value = json!({
        "hook_event_name": event,
        "session_id": "sanitize-session-1",
        "agent_id": if event.starts_with("Subagent") { json!("agent-1") } else { Value::Null },
        "prompt_id": format!("p-{index}"),
        "tool_use_id": format!("toolu_{index}"),
        "tool_name": "Bash",
        "prompt": SECRETS[0],
        "tool_input": { "command": SECRETS[1] },
        "tool_response": { "stdout": SECRETS[2] },
        "last_assistant_message": SECRETS[3],
        "message": SECRETS[4],
        "title": SECRETS[4],
        "transcript_path": format!("/Users/someone/{}/t.jsonl", SECRETS[5]),
        "cwd": format!("/Users/someone/{}", SECRETS[5]),
        "error": { "message": SECRETS[6] },
        "content": { "answer": SECRETS[0] },
        "url": format!("https://example.invalid/{}", SECRETS[5]),
        "source": "startup",
        "reason": "other",
        "stop_hook_active": false,
    });
    serde_json::to_vec(&value).unwrap_or_default()
}

fn capture(hook: &Path, store: &Path, home: &Path, stdin: &[u8]) -> Result<bool, String> {
    let mut child = Command::new(hook)
        .args(["hook", "--store-dir", &store.display().to_string(), "--home", &home.display().to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut input) = child.stdin.take() {
        use std::io::Write;
        let _ = input.write_all(stdin);
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(output.status.code() == Some(0) && output.stdout.is_empty() && output.stderr.is_empty())
}

/// Every file under `dir`, recursively, scanned for the planted values.
fn scan(dir: &Path, found: &mut Vec<String>, files: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, found, files);
        } else if let Ok(bytes) = std::fs::read(&path) {
            *files += 1;
            let text = String::from_utf8_lossy(&bytes);
            for secret in SECRETS {
                if text.contains(secret) {
                    found.push(format!("{} in {}", secret, path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                }
            }
        }
    }
}

pub fn run(root: &Path, hook: &Path) -> Result<Value, String> {
    let area = Area::new(root, "sanitization")?;
    let store = std::env::temp_dir().join(format!("threadspace-m1-sanitize-{}", uuid::Uuid::new_v4()));
    let home = std::env::temp_dir().join(format!("threadspace-m1-sanitize-home-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let fixture = threadspace_agent::fixture::start(&store)?;
    let mut silent = true;
    for (index, event) in EVENTS.iter().enumerate() {
        silent &= capture(hook, &store, &home, &input(event, index))?;
    }
    // The spool path: admission closed, so every capture is spooled.
    fixture.set_admission(false);
    for (index, event) in EVENTS.iter().enumerate() {
        silent &= capture(hook, &store, &home, &input(event, 100 + index))?;
    }
    let spooled = threadspace_relay::spool::Spool::at(&store).stats().ready_records;
    let mut spool_found = Vec::new();
    let mut spool_files = 0;
    scan(&store.join("capture-spool"), &mut spool_found, &mut spool_files);
    fixture.set_admission(true);
    let drained = fixture.drain_spool();

    let (observations, facts) = {
        let conn = Connection::open_with_flags(store.join("journal.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        let mut statement = conn
            .prepare("SELECT native_event, payload_json FROM observations WHERE source_id = 'claude.hook' ORDER BY ingest_seq LIMIT 14")
            .map_err(|e| e.to_string())?;
        let observations: Vec<Value> = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|(event, payload)| {
                let mut envelope: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
                // Machine-specific evidence (process samples, boot ID) is not
                // part of the sanitization claim; shown by count only.
                let samples = envelope["evidence"].as_array().map_or(0, Vec::len);
                envelope["evidence"] = json!(format!("{samples} process samples"));
                envelope["capturedAt"] = json!("<capture clock>");
            // The profile names a disposable home directory on this machine.
            envelope["sessionKey"]["profileRef"] = json!("claude-cli:<home>/.claude");
                json!({ "nativeEvent": event, "storedEnvelope": envelope })
            })
            .collect();
        let mut statement = conn
            .prepare("SELECT kind, fact_json FROM facts WHERE observation_id IN (SELECT observation_id FROM observations WHERE source_id = 'claude.hook') ORDER BY ingest_seq, fact_index LIMIT 40")
            .map_err(|e| e.to_string())?;
        let facts: Vec<Value> = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|(kind, fact)| {
                let fact: Value = serde_json::from_str(&fact).unwrap_or(Value::Null);
                let mut native = fact["native"].clone();
                if native["session"].is_object() {
                    native["session"]["profileRef"] = json!("claude-cli:<home>/.claude");
                }
                json!({ "kind": kind, "payload": fact["payload"], "native": native })
            })
            .collect();
        (observations, facts)
    };
    let mut store_found = Vec::new();
    let mut store_files = 0;
    scan(&store, &mut store_found, &mut store_files);
    area.json("snapshots.json", &json!({ "storedObservations": observations, "facts": facts }))?;
    let pass = silent && spool_found.is_empty() && store_found.is_empty() && spooled == EVENTS.len() && drained == EVENTS.len();
    let summary = json!({
        "area": "sanitization",
        "pass": pass,
        "plantedFields": ["prompt", "tool_input", "tool_response", "last_assistant_message", "message", "title", "transcript_path", "cwd", "error", "content", "url"],
        "hookEvents": EVENTS,
        "capturesPerPath": EVENTS.len(),
        "exit0AndSilent": silent,
        "spoolPath": { "spooled": spooled, "filesScanned": spool_files, "plantedValuesFound": spool_found, "drainedAfterReopen": drained },
        "storeScan": { "filesScanned": store_files, "plantedValuesFound": store_found, "includes": "journal.sqlite3, -wal, -shm, capture-spool/**, logs/**" },
        "snapshots": "evidence/M1/sanitization/snapshots.json",
    });
    area.json("summary.json", &summary)?;
    let _ = std::fs::remove_dir_all(&home);
    Ok(summary)
}
