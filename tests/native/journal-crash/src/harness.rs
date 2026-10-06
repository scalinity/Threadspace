//! Parent-side plumbing: the run context, worker processes, case outcomes,
//! engine checks on every reopen, and journal census helpers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Map, Value, json};
use threadspace_journal::{
    CRASH_AT_ENV, CRASH_POINT_ENV, CrashPoint, Journal, REQUIRED_SQLITE_SOURCE_ID,
    REQUIRED_SQLITE_VERSION, WriterLock,
};

use crate::fixture::{DB_NAME, SOURCE, now_ms};

/// A worker that prints nothing for this long is treated as hung and killed.
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Ctx {
    pub exe: PathBuf,
    pub root: PathBuf,
    results: File,
    pub engine_checks: u64,
    pub engine_violations: u64,
    pub fullfsync_seen: BTreeSet<bool>,
    pub engine_sample: Option<Value>,
    pub tallies: BTreeMap<String, Tally>,
    pub violations: Vec<Value>,
}

#[derive(Default)]
pub struct Tally {
    pub runs: u64,
    pub passed: u64,
    pub acked: u64,
    pub lost: u64,
    pub duplicate_rows: u64,
    pub duplicate_admissions: u64,
    pub unacked_present: u64,
}

impl Ctx {
    pub fn new(exe: PathBuf, root: PathBuf, results: File) -> Self {
        Self {
            exe,
            root,
            results,
            engine_checks: 0,
            engine_violations: 0,
            fullfsync_seen: BTreeSet::new(),
            engine_sample: None,
            tallies: BTreeMap::new(),
            violations: Vec::new(),
        }
    }

    /// A fresh directory for one case run, inside this run's root.
    pub fn case_dir(&self, case: &str, run: u16, leaf: &str) -> Result<PathBuf, String> {
        let dir = self.root.join(format!("{case}-{run:02}")).join(leaf);
        std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        Ok(dir)
    }

    /// Records one finished case run: a JSONL line, tallies and violations.
    pub fn finish(&mut self, mut outcome: Outcome, result: Result<(), String>) {
        if let Err(detail) = result {
            outcome.violation(format!("case aborted: {detail}"));
        }
        let pass = outcome.violations.is_empty();
        let tally = self.tallies.entry(outcome.case.clone()).or_default();
        tally.runs += 1;
        tally.passed += u64::from(pass);
        tally.acked += outcome.acked;
        tally.lost += outcome.lost;
        tally.duplicate_rows += outcome.duplicate_rows;
        tally.duplicate_admissions += outcome.duplicate_admissions;
        tally.unacked_present += outcome.unacked_present;
        for detail in &outcome.violations {
            self.violations.push(json!({
                "case": outcome.case, "run": outcome.run, "detail": detail,
            }));
        }
        let mut line = Map::new();
        line.insert("case".into(), json!(outcome.case));
        line.insert("run".into(), json!(outcome.run));
        line.insert("pass".into(), json!(pass));
        line.insert("ackedRecords".into(), json!(outcome.acked));
        line.insert("lostAckedRecords".into(), json!(outcome.lost));
        line.insert("duplicateRows".into(), json!(outcome.duplicate_rows));
        line.insert(
            "duplicateAdmissions".into(),
            json!(outcome.duplicate_admissions),
        );
        line.insert("unackedPresent".into(), json!(outcome.unacked_present));
        line.append(&mut outcome.details);
        line.insert("violations".into(), json!(outcome.violations));
        let _ = writeln!(self.results, "{}", Value::Object(line));
        let _ = self.results.flush();
        eprintln!(
            "[{}] {} run {:02}{}",
            if pass { "pass" } else { "FAIL" },
            outcome.case,
            outcome.run,
            if pass {
                String::new()
            } else {
                format!(": {}", outcome.violations.join("; "))
            }
        );
    }

    /// Asserts the linked engine and connection policy after a (re)open.
    pub fn check_engine(&mut self, journal: &Journal, outcome: &mut Outcome) {
        self.engine_checks += 1;
        let diagnostics = match journal.sqlite_diagnostics() {
            Ok(diagnostics) => diagnostics,
            Err(error) => {
                self.engine_violations += 1;
                outcome.violation(format!("diagnostics unavailable: {error}"));
                return;
            }
        };
        self.fullfsync_seen.insert(diagnostics.fullfsync);
        let ok = diagnostics.version == REQUIRED_SQLITE_VERSION
            && diagnostics.source_id == REQUIRED_SQLITE_SOURCE_ID
            && diagnostics.journal_mode == "wal"
            && diagnostics.synchronous == 2
            && diagnostics.foreign_keys;
        if !ok {
            self.engine_violations += 1;
            outcome.violation(format!(
                "engine/policy after reopen: {} {} journal_mode={} synchronous={} foreign_keys={}",
                diagnostics.version,
                diagnostics.source_id,
                diagnostics.journal_mode,
                diagnostics.synchronous,
                diagnostics.foreign_keys
            ));
        }
        if self.engine_sample.is_none() {
            self.engine_sample = Some(json!({
                "sqliteVersion": diagnostics.version,
                "sourceId": diagnostics.source_id,
                "journalMode": diagnostics.journal_mode,
                "synchronous": diagnostics.synchronous,
                "foreignKeys": diagnostics.foreign_keys,
                "fullfsync": diagnostics.fullfsync,
                "compileOptions": diagnostics.compile_options,
            }));
        }
    }

    /// Acquires the store's writer lock and opens the journal, checking the
    /// engine. A dead holder's lock must be acquirable.
    pub fn reopen(
        &mut self,
        dir: &Path,
        outcome: &mut Outcome,
    ) -> Result<(WriterLock, Journal), String> {
        let lock = WriterLock::acquire(dir)
            .map_err(|error| format!("writer lock not recoverable: {error}"))?;
        let journal = self.open(dir, outcome)?;
        Ok((lock, journal))
    }

    /// Opens the journal in `dir` (the caller holds its lock) and checks the engine.
    pub fn open(&mut self, dir: &Path, outcome: &mut Outcome) -> Result<Journal, String> {
        let journal = Journal::open(&dir.join(DB_NAME), "journal-crash-parent", now_ms())
            .map_err(|error| format!("reopen failed: {error}"))?;
        self.check_engine(&journal, outcome);
        Ok(journal)
    }

    pub fn spawn(
        &self,
        args: &[String],
        crash: Option<(CrashPoint, u64)>,
    ) -> Result<Worker, String> {
        let mut command = Command::new(&self.exe);
        command
            .arg("worker")
            .args(args)
            .env_remove(CRASH_POINT_ENV)
            .env_remove(CRASH_AT_ENV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some((point, at)) = crash {
            command
                .env(CRASH_POINT_ENV, point.name())
                .env(CRASH_AT_ENV, at.to_string());
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn worker: {error}"))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "worker stdout missing".to_owned())?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Worker {
            child,
            rx,
            _stdin: stdin,
        })
    }
}

pub struct Outcome {
    pub case: String,
    pub run: u16,
    pub details: Map<String, Value>,
    pub violations: Vec<String>,
    pub acked: u64,
    pub lost: u64,
    pub duplicate_rows: u64,
    pub duplicate_admissions: u64,
    pub unacked_present: u64,
}

impl Outcome {
    pub fn new(case: &str, run: u16) -> Self {
        Self {
            case: case.to_owned(),
            run,
            details: Map::new(),
            violations: Vec::new(),
            acked: 0,
            lost: 0,
            duplicate_rows: 0,
            duplicate_admissions: 0,
            unacked_present: 0,
        }
    }

    pub fn violation(&mut self, detail: impl Into<String>) {
        self.violations.push(detail.into());
    }

    pub fn detail(&mut self, key: &str, value: Value) {
        self.details.insert(key.to_owned(), value);
    }
}

pub enum Next {
    Event(Value),
    Eof,
    Timeout,
}

pub struct Worker {
    child: Child,
    rx: Receiver<String>,
    _stdin: Option<ChildStdin>,
}

impl Worker {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn next(&mut self, timeout: Duration) -> Next {
        match self.rx.recv_timeout(timeout) {
            Ok(line) => Next::Event(
                serde_json::from_str(&line)
                    .unwrap_or_else(|_| json!({ "event": "unparsed", "line": line })),
            ),
            Err(RecvTimeoutError::Disconnected) => Next::Eof,
            Err(RecvTimeoutError::Timeout) => Next::Timeout,
        }
    }

    /// Everything printed so far, without waiting.
    pub fn drain(&mut self) -> Vec<Value> {
        let mut events = Vec::new();
        while let Ok(line) = self.rx.try_recv() {
            events.push(
                serde_json::from_str(&line)
                    .unwrap_or_else(|_| json!({ "event": "unparsed", "line": line })),
            );
        }
        events
    }

    /// Reads events until the worker prints `event` (returned) or fails.
    pub fn expect(&mut self, event: &str, acks: &mut Vec<Ack>) -> Result<Value, String> {
        loop {
            match self.next(WORKER_TIMEOUT) {
                Next::Event(value) => {
                    if value["event"] == event {
                        return Ok(value);
                    }
                    absorb(&value, acks)?;
                }
                Next::Eof => return Err(format!("worker exited before {event}")),
                Next::Timeout => {
                    self.kill();
                    return Err(format!(
                        "worker silent for {WORKER_TIMEOUT:?} before {event}"
                    ));
                }
            }
        }
    }

    /// SIGKILL (std's `Child::kill` on Unix).
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    pub fn wait(&mut self) -> Result<ExitStatus, String> {
        self.child
            .wait()
            .map_err(|error| format!("wait worker: {error}"))
    }

    /// Reads the rest of the worker's output after it died or was killed.
    pub fn drain_to_eof(&mut self, acks: &mut Vec<Ack>) -> Result<(), String> {
        loop {
            match self.next(WORKER_TIMEOUT) {
                Next::Event(value) => absorb(&value, acks)?,
                Next::Eof => return Ok(()),
                Next::Timeout => {
                    self.kill();
                    return Err("worker output did not end".to_owned());
                }
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// One ACK line: a receipt the worker printed after the journal returned it.
#[derive(Debug, Clone)]
pub struct Ack {
    pub index: u64,
    pub id: String,
    pub cursor: i64,
    pub status: String,
}

/// Collects an ACK; `ready`/`done` are ignored; anything else is an error.
pub fn absorb(value: &Value, acks: &mut Vec<Ack>) -> Result<(), String> {
    match value["event"].as_str() {
        Some("ack") => {
            let receipt = &value["receipt"];
            let ack = Ack {
                index: value["index"].as_u64().ok_or("ack without index")?,
                id: receipt["observationId"]
                    .as_str()
                    .ok_or("ack without id")?
                    .to_owned(),
                cursor: receipt["cursor"].as_i64().ok_or("ack without cursor")?,
                status: receipt["status"]
                    .as_str()
                    .ok_or("ack without status")?
                    .to_owned(),
            };
            acks.push(ack);
            Ok(())
        }
        Some("ready" | "done") => Ok(()),
        _ => Err(format!("worker reported {value}")),
    }
}

/// `(observation_id, cursor)` rows from the runner's source, oldest first.
pub fn census(journal: &Journal) -> Result<Vec<(String, i64)>, String> {
    journal
        .admitted_observations(SOURCE)
        .map_err(|error| format!("census: {error}"))
}

pub fn census_map(rows: &[(String, i64)]) -> HashMap<String, i64> {
    rows.iter().cloned().collect()
}

/// Counts ACKed records missing or moved in `present`, adding violations.
pub fn check_acks(acks: &[Ack], present: &HashMap<String, i64>, outcome: &mut Outcome) -> u64 {
    let mut lost = 0;
    for ack in acks {
        match present.get(&ack.id) {
            Some(cursor) if *cursor == ack.cursor => {}
            Some(cursor) => {
                lost += 1;
                outcome.violation(format!(
                    "ACKed record {} moved from cursor {} to {cursor}",
                    ack.index, ack.cursor
                ));
            }
            None => {
                lost += 1;
                outcome.violation(format!(
                    "ACKed record {} (cursor {}) lost",
                    ack.index, ack.cursor
                ));
            }
        }
    }
    lost
}

pub fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// Scans a WAL file: the contiguous prefix of frames whose salts match the
/// WAL header (the current generation SQLite would read; checksums are not
/// recomputed), its commit frames, and the frames after the last commit,
/// which belong to a transaction that never committed.
pub fn wal_scan(path: &Path) -> Value {
    let Ok(bytes) = std::fs::read(path) else {
        return Value::Null;
    };
    if bytes.len() < 32 {
        return json!({ "bytes": bytes.len(), "frames": 0, "commitFrames": 0, "uncommittedFrames": 0 });
    }
    let word =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let page_size = usize::try_from(word(8)).unwrap_or(0);
    let salts = (word(16), word(20));
    let frame = 24 + page_size;
    let (mut frames, mut commits, mut uncommitted) = (0_u64, 0_u64, 0_u64);
    let mut offset = 32;
    while offset + frame <= bytes.len() && (word(offset + 8), word(offset + 12)) == salts {
        frames += 1;
        if word(offset + 4) == 0 {
            uncommitted += 1;
        } else {
            commits += 1;
            uncommitted = 0;
        }
        offset += frame;
    }
    json!({
        "bytes": bytes.len(),
        "pageSize": page_size,
        "frames": frames,
        "commitFrames": commits,
        "uncommittedFrames": uncommitted,
    })
}

pub fn file_hash(path: &Path) -> Option<String> {
    threadspace_journal::file_sha256(path).ok()
}

pub fn status_text(status: &ExitStatus) -> Value {
    use std::os::unix::process::ExitStatusExt;
    json!({ "code": status.code(), "signal": status.signal() })
}

pub fn killed(status: &ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.signal() == Some(9)
}
