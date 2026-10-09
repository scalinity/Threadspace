//! Native mod-batch measurement (SPEC §8.2–§8.3; MILESTONES M2): the real
//! `threadspace-hook mod-batch` is spawned per batch, as the observer mod's
//! `$.process.run` does, against the real companion writer and event socket
//! on a disposable store (`threadspace_agent::fixture`), with the budgets
//! the mod passes (230 ms for a drain, 80 ms for the end-of-session drain).
//! Each answer must be a typed receipt with exactly one result per record.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use threadspace_contracts::canonical::capture::{ModBatchReceipt, RecordStatus};

use crate::evidence::{Area, percentiles};

const WARMUP: usize = 10;
const P95_TARGET_US: u64 = 25_000;
const RECORDS_PER_BATCH: u32 = 6;

fn record(epoch: &str, batch: usize, index: u32) -> Value {
    let n = batch as u32 * RECORDS_PER_BATCH + index + 1;
    let (event, phase) = [("prompt.submit", "entry"), ("turn.start", "result"), ("tool.call", "entry"), ("tool.call", "result"), ("turn.step", "result"), ("turn.complete", "result")][index as usize];
    json!({
        "schemaVersion": 1, "observationId": format!("{}-{n:012x}", &epoch[..23]), "adapterId": "threadspace-observer", "adapterVersion": "0.1.0",
        "sourceEpoch": epoch, "sequenceMeaning": "OBSERVER_CAPTURE",
        "callbackEntrySequence": n.to_string(), "callbackResultSequence": if phase == "entry" { Value::Null } else { Value::from((n + 1).to_string()) },
        "phase": phase, "nativeEvent": event,
        "dispatchOrigin": { "plugin": "engine", "tier": "core" }, "engineDispatch": true,
        "sessionId": format!("bench-session-{}", batch % 4), "sessionIdSource": "classic.SessionStart", "sessionGeneration": 1,
        "nativeTurnId": format!("bench-turn-{batch}"), "nativeOccurrenceId": if event == "tool.call" { Value::from(format!("toolu-{batch}")) } else { Value::Null },
        "payload": { "tool": "Bash", "reason": "answer", "resultKind": "result", "origin": { "kind": "composer" },
                     "core": { "links": 1, "endPlugin": "engine", "endTier": "core", "endOutcome": "returned", "coreSettled": true } },
    })
}

fn batch(epoch: &str, index: usize) -> Vec<u8> {
    let records: Vec<Value> = (0..RECORDS_PER_BATCH).map(|i| record(epoch, index, i)).collect();
    serde_json::to_vec(&json!({ "receiptVersion": 1, "kind": "mod-batch", "sourceEpoch": epoch, "droppedRecords": 0, "records": records }))
        .unwrap_or_default()
}

struct Answer {
    micros: u64,
    exit_zero: bool,
    receipt: Option<ModBatchReceipt>,
}

fn spawn(hook: &Path, store: &Path, home: &Path, budget_ms: u64, stdin: &[u8]) -> Result<Answer, String> {
    let started = Instant::now();
    let mut child = Command::new(hook)
        .args(["mod-batch", "--store-dir"])
        .arg(store)
        .arg("--home")
        .arg(home)
        .args(["--budget-ms", &budget_ms.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin);
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(Answer {
        micros: started.elapsed().as_micros() as u64,
        exit_zero: output.status.code() == Some(0),
        receipt: serde_json::from_slice(&output.stdout).ok(),
    })
}

fn all(answer: &Answer, status: RecordStatus) -> bool {
    answer.receipt.as_ref().is_some_and(|r| {
        r.results.len() == RECORDS_PER_BATCH as usize && r.results.iter().all(|x| x.status == status)
    })
}

fn scratch(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("threadspace-m2-mod-batch-{label}-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn measure(root: &Path, hook: &Path, runs: usize) -> Result<Value, String> {
    let area = Area::new(root, "mod-batch")?;
    let store = scratch("store");
    let home = scratch("home");
    let fixture = threadspace_agent::fixture::start(&store)?;
    let epoch = "6d1c7f0e-2222-4aaa-8bbb-000000000001";
    for index in 0..WARMUP {
        spawn(hook, &store, &home, 230, &batch(epoch, index))?;
    }
    let mut answers = Vec::new();
    for index in 0..runs {
        answers.push(spawn(hook, &store, &home, 230, &batch(epoch, WARMUP + index))?);
    }
    let committed = answers.iter().filter(|a| a.exit_zero && all(a, RecordStatus::Committed)).count();
    let mut micros: Vec<u64> = answers.iter().map(|a| a.micros).collect();
    let wall = percentiles(&mut micros);
    let journal: i64 = Connection::open_with_flags(store.join("journal.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM observations WHERE source_id = 'claude.observer'", [], |r| r.get(0)))
        .unwrap_or(-1);
    // A retried batch is answered ALREADY_COMMITTED and stored once.
    let retried = spawn(hook, &store, &home, 230, &batch(epoch, WARMUP))?;
    let journal_after_retry: i64 = Connection::open_with_flags(store.join("journal.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM observations WHERE source_id = 'claude.observer'", [], |r| r.get(0)))
        .unwrap_or(-1);
    // The end-of-session drain's budget.
    let mut end = Vec::new();
    for index in 0..25 {
        end.push(spawn(hook, &store, &home, 80, &batch(epoch, WARMUP + runs + 1 + index))?);
    }
    let mut end_micros: Vec<u64> = end.iter().map(|a| a.micros).collect();
    let end_wall = percentiles(&mut end_micros);
    drop(fixture);
    // No companion: every record is spooled within the budget.
    let absent = scratch("absent");
    let mut spooled = Vec::new();
    for index in 0..25 {
        spooled.push(spawn(hook, &absent, &home, 230, &batch(epoch, index))?);
    }
    let mut spool_micros: Vec<u64> = spooled.iter().map(|a| a.micros).collect();
    let spool_wall = percentiles(&mut spool_micros);
    let expected = ((WARMUP + runs) as u32 * RECORDS_PER_BATCH) as i64;
    let p95 = wall["p95Us"].as_u64().unwrap_or(u64::MAX);
    let summary = json!({
        "pass": committed == runs && journal == expected && p95 <= P95_TARGET_US
            && all(&retried, RecordStatus::AlreadyCommitted) && journal_after_retry == journal
            && end.iter().all(|a| all(a, RecordStatus::Committed)) && end_wall["maxUs"].as_u64().unwrap_or(u64::MAX) <= 100_000
            && spooled.iter().all(|a| all(a, RecordStatus::LocalSpooled)) && spool_wall["maxUs"].as_u64().unwrap_or(u64::MAX) <= 250_000,
        "hook": hook.display().to_string().replace(&std::env::var("HOME").unwrap_or_default(), "~"),
        "recordsPerBatch": RECORDS_PER_BATCH,
        "drain": { "runs": runs, "allCommitted": committed, "wall": wall, "p95TargetUs": P95_TARGET_US, "journalRecords": journal, "expectedRecords": expected },
        "retry": { "allAlreadyCommitted": all(&retried, RecordStatus::AlreadyCommitted), "journalRecordsAfterRetry": journal_after_retry },
        "endOfSessionDrain": { "runs": end.len(), "budgetMs": 80, "allCommitted": end.iter().all(|a| all(a, RecordStatus::Committed)), "wall": end_wall },
        "companionAbsent": { "runs": spooled.len(), "allSpooled": spooled.iter().all(|a| all(a, RecordStatus::LocalSpooled)), "wall": spool_wall },
    });
    area.json("summary.json", &summary)?;
    let _ = std::fs::remove_dir_all(&store);
    let _ = std::fs::remove_dir_all(&absent);
    let _ = std::fs::remove_dir_all(&home);
    Ok(summary)
}
