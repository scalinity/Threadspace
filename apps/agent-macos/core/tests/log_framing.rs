//! Companion log framing under SIGKILL (C-13), with no app bundle. The test
//! binary re-executes itself as writer processes that run the companion's own
//! `src/log.rs`, compiled in below because the crate keeps the module private.
//!
//! Each round, in its own directory: a writer logs in a tight loop and is
//! SIGKILLed at a varied delay; in some rounds the parent then appends a torn
//! record (a record prefix without its newline), standing in for a writer
//! killed partway through a write; a second writer appends a fixed number of
//! records and exits. The round's file is then parsed one record per line.

#[path = "../src/log.rs"]
#[allow(dead_code)]
mod log;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const CHILD_DIR_ENV: &str = "THREADSPACE_LOG_FRAMING_CHILD_DIR";
const CHILD_TAG_ENV: &str = "THREADSPACE_LOG_FRAMING_CHILD_TAG";
const CHILD_RECORDS_ENV: &str = "THREADSPACE_LOG_FRAMING_CHILD_RECORDS";
const EVENT: &str = "framing.probe";
const ROUNDS: u32 = 240;
const AFTER_RECORDS: u64 = 25;
/// A writer that outlives its kill (it never should) stops by itself.
const WRITER_CAP: Duration = Duration::from_secs(3);

/// Runs only as a re-executed child of the test below: initialises the log in
/// the given directory and writes probe records, without limit unless told.
#[test]
fn writer_child() {
    let Ok(dir) = std::env::var(CHILD_DIR_ENV) else {
        return;
    };
    let tag = std::env::var(CHILD_TAG_ENV).expect("tag");
    let limit: Option<u64> = std::env::var(CHILD_RECORDS_ENV)
        .ok()
        .map(|records| records.parse().expect("record count"));
    log::init(Path::new(&dir));
    println!("READY");
    let started = Instant::now();
    let mut seq = 0u64;
    while limit.is_none_or(|limit| seq < limit) && started.elapsed() < WRITER_CAP {
        // Varied lengths, so kills land at different offsets within records.
        let pad = "x".repeat((seq % 97) as usize * 7);
        log::info(EVENT, json!({ "tag": tag, "seq": seq, "pad": pad }));
        seq += 1;
    }
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Tally {
    sigkills: u32,
    kills_at_spawn: u32,
    lines: u64,
    killed_writer_records: u64,
    post_kill_records: u64,
    planted: u32,
    /// Complete lines holding one record start but not one valid record.
    torn_lines: u32,
    joined_lines: u32,
}

fn writer(exe: &Path, dir: &Path, tag: &str, records: Option<u64>) -> Command {
    let mut command = Command::new(exe);
    command
        .args(["--exact", "writer_child", "--nocapture"])
        .env(CHILD_DIR_ENV, dir)
        .env(CHILD_TAG_ENV, tag)
        .stderr(Stdio::null());
    match records {
        Some(records) => command.env(CHILD_RECORDS_ENV, records.to_string()),
        None => command.env_remove(CHILD_RECORDS_ENV),
    };
    command
}

/// Returns once the writer has opened its log (or its stdout closed).
fn wait_ready(child: &mut Child) {
    let stdout = child.stdout.take().expect("writer stdout");
    for line in BufReader::new(stdout).lines() {
        match line {
            Ok(line) if line == "READY" => return,
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

fn append(path: &Path, bytes: &[u8]) {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .expect("append to log");
}

/// Appends a prefix of a probe record with no newline, unless the file
/// already ends mid-record.
fn plant_torn(path: &Path, round: u32) -> bool {
    let tail_complete = std::fs::read(path)
        .map(|bytes| bytes.last().is_none_or(|last| *last == b'\n'))
        .unwrap_or(true);
    if !tail_complete {
        return false;
    }
    let record =
        json!({ "event": EVENT, "level": "info", "pid": 0, "seq": 0, "tag": "planted", "ts": 0 })
            .to_string();
    let cut = 1 + (round as usize * 13) % (record.len() - 1);
    append(path, &record.as_bytes()[..cut]);
    true
}

fn check_round(
    round: u32,
    path: &Path,
    planted: bool,
    tally: &mut Tally,
    violations: &mut Vec<String>,
) {
    let mut fail = |message: String| violations.push(format!("round {round}: {message}"));
    let bytes = std::fs::read(path).expect("read log");
    let Some((&b'\n', body)) = bytes.split_last() else {
        fail("the file does not end with a newline".into());
        return;
    };
    let lines: Vec<&[u8]> = body.split(|byte| *byte == b'\n').collect();
    let (mut killed_seq, mut after_seq, mut torn) = (0u64, 0u64, 0u32);
    for (index, line) in lines.iter().enumerate() {
        tally.lines += 1;
        // Probe records are flat, so `{` appears only where a record starts.
        let starts = line.iter().filter(|byte| **byte == b'{').count();
        if starts != 1 || line.first() != Some(&b'{') {
            tally.joined_lines += u32::from(starts > 1);
            fail(format!("line {} holds {starts} record starts", index + 1));
            continue;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(record) if record.is_object() && record["event"] == EVENT => {
                let seq = record["seq"].as_u64();
                match record["tag"].as_str() {
                    Some("killed") if seq == Some(killed_seq) && after_seq == 0 => killed_seq += 1,
                    Some("after") if seq == Some(after_seq) => after_seq += 1,
                    _ => fail(format!("line {} is out of sequence: {record}", index + 1)),
                }
            }
            _ => {
                // A torn record: it must end on its own line, directly before
                // the next writer's first record.
                torn += 1;
                let next_is_first = lines
                    .get(index + 1)
                    .and_then(|next| serde_json::from_slice::<Value>(next).ok())
                    .is_some_and(|next| next["tag"] == "after" && next["seq"] == 0);
                if !next_is_first {
                    fail(format!(
                        "torn line {} is not followed by the next writer's first record",
                        index + 1
                    ));
                }
            }
        }
    }
    if after_seq != AFTER_RECORDS {
        fail(format!(
            "{after_seq} of {AFTER_RECORDS} post-kill records parsed"
        ));
    }
    let planted = u32::from(planted);
    if torn < planted {
        fail("the planted torn record is missing".into());
    }
    tally.killed_writer_records += killed_seq;
    tally.post_kill_records += after_seq;
    tally.planted += planted;
    tally.torn_lines += torn;
}

#[test]
fn records_never_share_a_line_across_a_killed_writer() {
    let exe = std::env::current_exe().expect("test binary");
    let root = TempRoot(
        std::env::temp_dir().join(format!("threadspace-log-framing-{}", uuid::Uuid::new_v4())),
    );
    let mut tally = Tally::default();
    let mut violations = Vec::new();

    for round in 0..ROUNDS {
        let dir = root.0.join(format!("round-{round:03}"));
        std::fs::create_dir_all(&dir).expect("round dir");
        let path = dir.join("agent.log");

        let mut child = writer(&exe, &dir, "killed", None)
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn writer");
        if round % 8 == 0 {
            // Killed during start-up: the log may be absent, empty or opened.
            tally.kills_at_spawn += 1;
        } else {
            wait_ready(&mut child);
            let delay_us = if round % 16 == 1 {
                10_000
            } else {
                u64::from(round * 211 % 2_500)
            };
            std::thread::sleep(Duration::from_micros(delay_us));
        }
        child.kill().expect("SIGKILL writer");
        let status = child.wait().expect("reap writer");
        if status.signal() == Some(libc::SIGKILL) {
            tally.sigkills += 1;
        } else {
            violations.push(format!(
                "round {round}: the writer was not killed ({status:?})"
            ));
        }

        let planted = round % 5 == 2 && plant_torn(&path, round);

        let status = writer(&exe, &dir, "after", Some(AFTER_RECORDS))
            .stdout(Stdio::null())
            .status()
            .expect("run post-kill writer");
        if !status.success() {
            violations.push(format!(
                "round {round}: the post-kill writer failed ({status:?})"
            ));
        }

        check_round(round, &path, planted, &mut tally, &mut violations);
        let _ = std::fs::remove_dir_all(&dir);
    }

    println!(
        "log framing: {ROUNDS} rounds; {} writers confirmed SIGKILLed ({} at spawn, {} after READY); \
         {} lines; {} killed-writer records and {} post-kill records parsed; \
         {} torn records planted; {} torn lines found on their own line; {} joined lines; {} violations",
        tally.sigkills,
        tally.kills_at_spawn,
        ROUNDS - tally.kills_at_spawn,
        tally.lines,
        tally.killed_writer_records,
        tally.post_kill_records,
        tally.planted,
        tally.torn_lines,
        tally.joined_lines,
        violations.len(),
    );
    assert!(
        violations.is_empty(),
        "{} violations, first: {:?}",
        violations.len(),
        violations.iter().take(5).collect::<Vec<_>>()
    );
    assert!(tally.sigkills >= 200, "only {} kills", tally.sigkills);
    assert_eq!(tally.post_kill_records, u64::from(ROUNDS) * AFTER_RECORDS);
    assert!(tally.planted > 0);
}
