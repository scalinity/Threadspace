//! Native capture measurement (MILESTONES M1: normal capture p95 ≤25 ms,
//! application wall ≤250 ms; failure paths exit 0 with no provider-control
//! output). The real `threadspace-hook` executable is spawned per capture,
//! exactly as a provider runs a hook, against the real companion writer and
//! event socket on a disposable store (`threadspace_agent::fixture`); its
//! lifetime is measured from spawn to exit.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use threadspace_relay::spool::Spool;

use crate::evidence::{Area, percentiles};

const WARMUP: usize = 20;
const FAILURE_RUNS: usize = 25;
const P95_TARGET_US: u64 = 25_000;
const WALL_BUDGET_US: u64 = 250_000;
/// One saturation-fixture record; 100 of them exceed the 256 MiB byte bound.
const SATURATION_FILE_BYTES: usize = 2_700_000;

struct Run {
    micros: u64,
    exit_zero: bool,
    stdout: usize,
    stderr: usize,
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("threadspace-m1-capture-{label}-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn spawn(hook: &Path, args: &[&str], stdin: &[u8]) -> Result<Run, String> {
    let started = Instant::now();
    let mut child = Command::new(hook)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut input) = child.stdin.take() {
        // The hook may stop reading at its bound and exit first.
        let _ = input.write_all(stdin);
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(Run {
        micros: started.elapsed().as_micros() as u64,
        exit_zero: output.status.code() == Some(0),
        stdout: output.stdout.len(),
        stderr: output.stderr.len(),
    })
}

/// A Claude-shaped hook input with realistic bodies the hook must drop.
fn hook_input(index: usize) -> Vec<u8> {
    let session = format!("bench-session-{}", index % 4);
    let body = "lorem ipsum dolor sit amet ".repeat(40);
    let input = match index % 5 {
        0 => json!({ "hook_event_name": "SessionStart", "session_id": session, "source": "startup",
                     "transcript_path": "/private/bench/transcript.jsonl", "cwd": "/private/bench", "model": "m" }),
        1 => json!({ "hook_event_name": "UserPromptSubmit", "session_id": session, "prompt_id": format!("p-{index}"),
                     "prompt": body, "cwd": "/private/bench" }),
        2 => json!({ "hook_event_name": "PreToolUse", "session_id": session, "tool_use_id": format!("toolu_{index}"),
                     "tool_name": "Bash", "tool_input": { "command": body } }),
        3 => json!({ "hook_event_name": "PostToolUse", "session_id": session, "tool_use_id": format!("toolu_{}", index - 1),
                     "tool_name": "Bash", "tool_response": { "stdout": body.repeat(3) } }),
        _ => json!({ "hook_event_name": "Stop", "session_id": session, "stop_hook_active": false,
                     "last_assistant_message": body }),
    };
    serde_json::to_vec(&input).unwrap_or_default()
}

fn count(store: &Path, sql: &str) -> i64 {
    Connection::open_with_flags(store.join("journal.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| conn.query_row(sql, [], |row| row.get(0)))
        .unwrap_or(-1)
}

/// The ready records' bytes as their names claim them and as the file
/// system holds them (the sum of file metadata sizes).
fn ready_bytes(store: &Path) -> Value {
    let claimed: u64 = Spool::at(store)
        .ready(usize::MAX)
        .map(|records| records.iter().map(|r| r.bytes).sum())
        .unwrap_or(0);
    let on_disk: u64 = std::fs::read_dir(store.join("capture-spool/ready"))
        .map(|entries| entries.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0);
    json!({ "claimedByNames": claimed, "onDisk": on_disk })
}

fn silent(runs: &[Run]) -> bool {
    runs.iter().all(|r| r.exit_zero && r.stdout == 0 && r.stderr == 0)
}

fn failure_case(hook: &Path, args: &[&str], stdin: &[u8], runs: usize) -> Result<(Vec<Run>, Value), String> {
    let mut results = Vec::new();
    for _ in 0..runs {
        results.push(spawn(hook, args, stdin)?);
    }
    let mut micros: Vec<u64> = results.iter().map(|r| r.micros).collect();
    let stats = percentiles(&mut micros);
    let quiet = silent(&results);
    let ok = quiet && stats["maxUs"].as_u64().unwrap_or(u64::MAX) <= WALL_BUDGET_US;
    Ok((results, json!({ "runs": runs, "exit0AndSilent": quiet, "wall": stats, "withinWallBudget": ok })))
}

pub fn measure(root: &Path, hook: &Path, runs: usize) -> Result<Value, String> {
    let area = Area::new(root, "capture")?;
    let store = scratch("store");
    let home = scratch("home");
    let fixture = threadspace_agent::fixture::start(&store)?;
    let store_arg = store.display().to_string();
    let home_arg = home.display().to_string();
    let args = ["hook", "--store-dir", &store_arg, "--home", &home_arg];

    // Normal capture.
    for index in 0..WARMUP {
        spawn(hook, &args, &hook_input(index))?;
    }
    let mut normal = Vec::new();
    let mut input_sizes = Vec::new();
    for index in 0..runs {
        let input = hook_input(WARMUP + index);
        input_sizes.push(input.len() as u64);
        normal.push(spawn(hook, &args, &input)?);
    }
    let committed = count(&store, "SELECT COUNT(*) FROM observations WHERE source_id = 'claude.hook'");
    let mut envelope_sizes: Vec<u64> = Connection::open_with_flags(store.join("journal.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| {
            let mut statement = conn.prepare("SELECT LENGTH(payload_json) FROM observations WHERE source_id = 'claude.hook'")?;
            let sizes = statement.query_map([], |row| row.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(sizes.into_iter().map(|s| s.max(0) as u64).collect())
        })
        .unwrap_or_default();
    let spool_after_normal = Spool::at(&store).stats();
    let mut micros: Vec<u64> = normal.iter().map(|r| r.micros).collect();
    let wall = percentiles(&mut micros);
    input_sizes.sort_unstable();
    envelope_sizes.sort_unstable();
    let p95 = wall["p95Us"].as_u64().unwrap_or(u64::MAX);
    let max = wall["maxUs"].as_u64().unwrap_or(u64::MAX);
    let normal_pass = silent(&normal)
        && committed == (WARMUP + runs) as i64
        && spool_after_normal.ready_records == 0
        && p95 <= P95_TARGET_US
        && max <= WALL_BUDGET_US;

    // Fail-open paths.
    let mut failures = serde_json::Map::new();
    fixture.set_admission(false);
    let (_, closed) = failure_case(hook, &args, &hook_input(1), FAILURE_RUNS)?;
    let spooled_closed = Spool::at(&store).stats().ready_records;
    fixture.set_admission(true);
    let drained = fixture.drain_spool();
    let after_drain = count(&store, "SELECT COUNT(*) FROM observations WHERE source_id = 'claude.hook'");
    failures.insert("admissionClosed".into(), json!({
        "result": closed, "spooledWhileClosed": spooled_closed, "drainedAfterReopen": drained,
        "journalCountAfterDrain": after_drain, "spoolEmptyAfterDrain": Spool::at(&store).stats().ready_records == 0,
        "pass": closed["withinWallBudget"] == true && spooled_closed == FAILURE_RUNS && drained == FAILURE_RUNS,
    }));

    let absent = scratch("absent");
    let absent_arg = absent.display().to_string();
    let absent_args = ["hook", "--store-dir", &absent_arg, "--home", &home_arg];
    let (_, unavailable) = failure_case(hook, &absent_args, &hook_input(2), FAILURE_RUNS)?;
    let spooled_absent = Spool::at(&absent).stats().ready_records;
    failures.insert("companionUnavailable".into(), json!({
        "result": unavailable, "spooled": spooled_absent,
        "pass": unavailable["withinWallBudget"] == true && spooled_absent == FAILURE_RUNS,
    }));

    let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
    let oversized = vec![b' '; threadspace_contracts::limits::capture::RAW_INPUT_MAX_BYTES + 1];
    let unsafe_id = serde_json::to_vec(&json!({ "hook_event_name": "Stop", "session_id": "not / an id" })).unwrap_or_default();
    for (name, stdin) in [
        ("malformedJson", b"{nope".to_vec()),
        ("tooDeep", deep.into_bytes()),
        ("oversized", oversized),
        ("unsafeIdentifier", unsafe_id),
        ("emptyInput", Vec::new()),
    ] {
        let runs = if name == "oversized" { 5 } else { FAILURE_RUNS };
        let (_, result) = failure_case(hook, &args, &stdin, runs)?;
        failures.insert(name.into(), json!({ "result": result, "pass": result["withinWallBudget"] == true }));
    }
    let markers_before_drain = Spool::at(&store).stats().dropped_markers;
    fixture.drain_spool();
    std::thread::sleep(Duration::from_millis(300));
    let gaps = count(&store, "SELECT COUNT(*) FROM facts WHERE kind = 'OBSERVATION_GAP_DETECTED'");
    failures.insert("lossRecorded".into(), json!({
        "dropMarkers": markers_before_drain, "gapFactsInJournal": gaps,
        "pass": markers_before_drain > 0 && gaps >= 1,
    }));

    // Saturation: a spool whose ready records already exceed the byte bound
    // in real bytes on disk; the bound counts file sizes, not name claims.
    let saturated = scratch("saturated");
    let saturated_arg = saturated.display().to_string();
    let ready = saturated.join("capture-spool/ready");
    std::fs::create_dir_all(&ready).map_err(|e| e.to_string())?;
    let filler = vec![b' '; SATURATION_FILE_BYTES];
    for index in 0..100u32 {
        // 100 x 2.7 MB = 270 MB, above the 256 MiB bound; each name states
        // its file's real size. Synced, so no measured run waits on the
        // fixture's own flush.
        let name = format!("00000000-0000-4000-8000-{index:012}.{SATURATION_FILE_BYTES}.json");
        std::fs::File::create(ready.join(name))
            .and_then(|mut file| file.write_all(&filler).and_then(|()| file.sync_all()))
            .map_err(|e| e.to_string())?;
    }
    let saturated_args = ["hook", "--store-dir", &saturated_arg, "--home", &home_arg];
    let before = Spool::at(&saturated).stats();
    let bytes_before = ready_bytes(&saturated);
    let (_, saturation) = failure_case(hook, &saturated_args, &hook_input(4), FAILURE_RUNS)?;
    let after = Spool::at(&saturated).stats();
    let bytes_after = ready_bytes(&saturated);
    let saturation_report = json!({
        "boundRecords": threadspace_contracts::limits::capture::SPOOL_MAX_RECORDS,
        "boundBytes": threadspace_contracts::limits::capture::SPOOL_MAX_BYTES,
        "before": before, "after": after, "result": saturation,
        "readyBytes": { "before": bytes_before, "after": bytes_after },
        "pass": saturation["withinWallBudget"] == true
            && after.ready_records == before.ready_records
            && after.dropped_markers == FAILURE_RUNS
            && after.pending_files == 0
            && bytes_before["onDisk"].as_u64() > Some(threadspace_contracts::limits::capture::SPOOL_MAX_BYTES)
            && bytes_after == bytes_before,
    });
    area.json("saturation.json", &saturation_report)?;

    let failures_pass = failures.values().all(|v| v["pass"] == true) && saturation_report["pass"] == true;
    let summary = json!({
        "area": "capture",
        "pass": normal_pass && failures_pass,
        "hookExecutable": hook.file_name().map(|n| n.to_string_lossy().into_owned()),
        "hookSha256": crate::evidence::sha256_file(hook),
        "method": "Each capture spawns the release threadspace-hook binary with Claude-shaped stdin and waits for exit; wall time is spawn-to-exit on the monotonic clock. The companion is the real writer and event socket on a disposable store (fixture), SQLite WAL with synchronous=FULL.",
        "targets": { "p95Us": P95_TARGET_US, "wallBudgetUs": WALL_BUDGET_US },
        "normal": {
            "runs": runs, "warmup": WARMUP, "wall": wall,
            "exit0AndSilent": silent(&normal),
            "committedInJournal": committed,
            "inputBytes": { "min": input_sizes.first(), "max": input_sizes.last(), "median": input_sizes.get(input_sizes.len() / 2) },
            "storedEnvelopeBytes": { "min": envelope_sizes.first(), "max": envelope_sizes.last(), "median": envelope_sizes.get(envelope_sizes.len() / 2) },
            "spool": spool_after_normal,
            "pass": normal_pass,
        },
        "failurePaths": failures,
        "saturation": "evidence/M1/capture/saturation.json",
    });
    area.json("summary.json", &summary)?;
    for dir in [&home, &absent, &saturated] {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(summary)
}
