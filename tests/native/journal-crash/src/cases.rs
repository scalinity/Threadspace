//! The qualification cases. Each runs against a fresh store directory under
//! the run root and records what it observed in its `Outcome`; any
//! violation fails the run.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use threadspace_journal::{
    AdmissionReceipt, BackupError, BackupInfo, CrashPoint, LockError, WriterLock,
    backup_store_into, restore_backup, verify_backup,
};

use crate::fixture::{DB_NAME, Fixture, now_ms, side};
use crate::harness::{
    Ack, Ctx, Next, Outcome, WORKER_TIMEOUT, Worker, absorb, census, census_map, check_acks,
    file_hash, file_len, killed, status_text, wal_scan,
};
use crate::worker::EXIT_LOCK_HELD;

const COMMITTED: &str = "COMMITTED";
const ALREADY_COMMITTED: &str = "ALREADY_COMMITTED";

/// Where the worker dies relative to one admission.
#[derive(Debug, Clone, Copy)]
pub enum Crash {
    /// A journal crash point inside `admit_observation`.
    Point(CrashPoint),
    /// The parent SIGKILLs the worker right after reading ACK `k`.
    AfterReceipt,
}

impl Crash {
    pub const ALL: [Self; 4] = [
        Self::Point(CrashPoint::BeforeTransaction),
        Self::Point(CrashPoint::InTransaction),
        Self::Point(CrashPoint::AfterCommitBeforeReceipt),
        Self::AfterReceipt,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Point(point) => point.name(),
            Self::AfterReceipt => "after-receipt",
        }
    }
}

fn admit_args(dir: &Path, case: &str, run: u16, from: u64, count: u64, hold: bool) -> Vec<String> {
    let mut args = vec![
        "admit".to_owned(),
        "--store".to_owned(),
        dir.display().to_string(),
        "--case".to_owned(),
        case.to_owned(),
        "--run".to_owned(),
        run.to_string(),
        "--from".to_owned(),
        from.to_string(),
        "--count".to_owned(),
        count.to_string(),
    ];
    if hold {
        args.push("--hold".to_owned());
    }
    args
}

fn status_of(receipt: &AdmissionReceipt) -> String {
    serde_json::to_value(&receipt.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn store_hashes(db: &Path) -> Vec<Option<String>> {
    ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| file_hash(&side(db, suffix)))
        .collect()
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(drop)
        .map_err(|error| format!("copy {} -> {}: {error}", from.display(), to.display()))
}

fn count(items: usize) -> u64 {
    u64::try_from(items).unwrap_or(u64::MAX)
}

/// One crash-boundary run: a worker admits `n` records and dies at `k`;
/// the parent reopens, checks every ACKed record, then re-delivers every
/// attempted record twice.
pub fn crash(ctx: &mut Ctx, out: &mut Outcome, kind: Crash, n: u64, k: u64) -> Result<(), String> {
    let case = out.case.clone();
    let run = out.run;
    let dir = ctx.case_dir(&case, run, "store")?;
    let db = dir.join(DB_NAME);
    let after_receipt = matches!(kind, Crash::AfterReceipt);
    let armed = match kind {
        Crash::Point(point) => Some((point, k)),
        Crash::AfterReceipt => None,
    };
    out.detail("records", json!(n));
    out.detail("crashAt", json!(k));

    let mut worker = ctx.spawn(&admit_args(&dir, &case, run, 1, n, after_receipt), armed)?;
    let mut acks = Vec::new();
    let mut killed_after = None;
    let mut finished = false;
    loop {
        match worker.next(WORKER_TIMEOUT) {
            Next::Event(value) => {
                finished |= value["event"] == "done";
                absorb(&value, &mut acks)?;
                if after_receipt
                    && killed_after.is_none()
                    && acks.last().is_some_and(|ack| ack.index == k)
                {
                    worker.kill();
                    killed_after = Some(k);
                }
            }
            Next::Eof => break,
            Next::Timeout => {
                worker.kill();
                return Err("worker silent".to_owned());
            }
        }
    }
    let status = worker.wait()?;
    out.detail("worker", status_text(&status));
    if !killed(&status) {
        out.violation(format!("worker ended {status}, not by SIGKILL"));
    }
    if armed.is_some() && finished {
        out.violation("worker finished although a crash point was armed");
    }
    out.acked = count(acks.len());
    if armed.is_some() && out.acked != k - 1 {
        out.violation(format!(
            "{} ACKs before the crash, expected {}",
            out.acked,
            k - 1
        ));
    }
    if acks.iter().any(|ack| ack.status != COMMITTED) {
        out.violation("a first delivery was not COMMITTED");
    }
    out.detail("killedAfterAck", json!(killed_after));
    out.detail("lastAckedIndex", json!(acks.last().map(|ack| ack.index)));
    out.detail("walAtCrash", wal_scan(&side(&db, "-wal")));

    let attempted = if armed.is_some() { k } else { n };
    let (lock, mut journal) = ctx.reopen(&dir, out)?;
    let rows = census(&journal)?;
    let present = census_map(&rows);
    out.lost = check_acks(&acks, &present, out);

    let crashing = Fixture::new(&case, run, k);
    match kind {
        Crash::Point(CrashPoint::AfterCommitBeforeReceipt) => {
            if !present.contains_key(&crashing.id) {
                out.violation(format!(
                    "record {k} was committed before the crash but is missing"
                ));
            }
        }
        Crash::Point(_) => {
            if present.contains_key(&crashing.id) {
                out.violation(format!(
                    "record {k} is visible although the crash preceded COMMIT"
                ));
            }
        }
        Crash::AfterReceipt => {}
    }
    let fixtures: Vec<Fixture> = (1..=n)
        .map(|index| Fixture::new(&case, run, index))
        .collect();
    let known: HashMap<&str, u64> = fixtures
        .iter()
        .zip(1..)
        .map(|(fixture, index)| (fixture.id.as_str(), index))
        .collect();
    for (id, _) in &rows {
        match known.get(id.as_str()) {
            Some(index) if *index <= attempted => {}
            Some(index) => out.violation(format!("record {index} present but never attempted")),
            None => out.violation(format!("unknown row {id}")),
        }
    }
    let acked: HashMap<&str, i64> = acks
        .iter()
        .map(|ack| (ack.id.as_str(), ack.cursor))
        .collect();
    out.unacked_present = count(
        rows.iter()
            .filter(|(id, _)| !acked.contains_key(id.as_str()))
            .count(),
    );

    // Duplicate delivery after restart: every attempted record, twice.
    let attempted_fixtures = &fixtures[..usize::try_from(attempted).unwrap_or(fixtures.len())];
    let (mut already, mut committed, mut second_pass_ok) = (0_u64, 0_u64, true);
    for pass in 0..2 {
        for fixture in attempted_fixtures {
            let receipt = journal
                .admit_observation(&fixture.admission(), now_ms())
                .map_err(|error| format!("retry {}: {error}", fixture.id))?;
            let status = status_of(&receipt);
            if pass == 1 {
                second_pass_ok &= status == ALREADY_COMMITTED;
                continue;
            }
            if status == ALREADY_COMMITTED {
                already += 1;
            } else {
                committed += 1;
            }
            if let Some(cursor) = acked.get(fixture.id.as_str())
                && (status != ALREADY_COMMITTED || receipt.cursor != *cursor)
            {
                out.duplicate_admissions += 1;
                out.violation(format!(
                    "retry of ACKed {} returned {status} at {} (ACKed at {cursor})",
                    fixture.id, receipt.cursor
                ));
            }
        }
    }
    if !second_pass_ok {
        out.violation("a second retry was not ALREADY_COMMITTED");
    }
    let after = census(&journal)?;
    let distinct: HashSet<&str> = after.iter().map(|(id, _)| id.as_str()).collect();
    out.duplicate_rows = count(after.len() - distinct.len());
    if count(after.len()) != attempted {
        out.violation(format!(
            "{} rows after retries, expected one per attempted record ({attempted})",
            after.len()
        ));
    }
    out.detail(
        "retry",
        json!({
            "attempted": attempted,
            "firstPassAlreadyCommitted": already,
            "firstPassCommitted": committed,
            "secondPassAllAlreadyCommitted": second_pass_ok,
            "rowsAfterRetries": after.len(),
        }),
    );
    drop(journal);
    drop(lock);
    Ok(())
}

/// Copies only the main database file into `scratch` and returns its census.
fn main_only_census(
    ctx: &mut Ctx,
    out: &mut Outcome,
    db: &Path,
    scratch: &Path,
) -> Result<Vec<(String, i64)>, String> {
    copy(db, &scratch.join(DB_NAME))?;
    let (_lock, journal) = ctx.reopen(scratch, out)?;
    census(&journal)
}

/// Committed frames left in the WAL by a killed writer are replayed on
/// reopen; a TRUNCATE checkpoint then moves them into the main file alone.
pub fn wal_replay(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    const RECORDS: u64 = 40;
    let case = out.case.clone();
    let dir = ctx.case_dir(&case, run, "store")?;
    let db = dir.join(DB_NAME);
    let wal = side(&db, "-wal");
    {
        // Baseline: schema and fixture checkpointed into the main file.
        let (_lock, mut journal) = ctx.reopen(&dir, out)?;
        let checkpoint = journal
            .checkpoint_truncate()
            .map_err(|error| format!("baseline checkpoint: {error}"))?;
        if checkpoint.busy {
            out.violation("baseline checkpoint busy");
        }
    }
    let mut worker = ctx.spawn(&admit_args(&dir, &case, run, 1, RECORDS, true), None)?;
    let mut acks = Vec::new();
    worker.expect("done", &mut acks)?;
    worker.kill();
    let status = worker.wait()?;
    worker.drain_to_eof(&mut acks)?;
    out.detail("worker", status_text(&status));
    out.acked = count(acks.len());
    if out.acked != RECORDS {
        out.violation(format!("{} ACKs, expected {RECORDS}", out.acked));
    }
    let wal_bytes = file_len(&wal).unwrap_or(0);
    out.detail("walBytesBeforeReplay", json!(wal_bytes));
    if wal_bytes == 0 {
        out.violation("no committed frames left in the WAL; replay not exercised");
    }

    let before_dir = ctx.case_dir(&case, run, "main-only-before-replay")?;
    let before = main_only_census(ctx, out, &db, &before_dir)?;
    out.detail("mainOnlyRowsBeforeReplay", json!(before.len()));
    if !before.is_empty() {
        out.violation("main file already held the records; replay not exercised");
    }

    let (lock, mut journal) = ctx.reopen(&dir, out)?;
    let replayed = census(&journal)?;
    out.lost = check_acks(&acks, &census_map(&replayed), out);
    let checkpoint = journal
        .checkpoint_truncate()
        .map_err(|error| format!("checkpoint: {error}"))?;
    let wal_after = file_len(&wal).unwrap_or(0);
    out.detail("checkpoint", json!(checkpoint));
    out.detail("walBytesAfterTruncate", json!(wal_after));
    if checkpoint.busy || wal_after != 0 {
        out.violation("TRUNCATE checkpoint did not empty the WAL");
    }
    let after_dir = ctx.case_dir(&case, run, "main-only-after-checkpoint")?;
    let after = main_only_census(ctx, out, &db, &after_dir)?;
    out.detail("mainOnlyRowsAfterCheckpoint", json!(after.len()));
    if after != replayed {
        out.violation(format!(
            "main file alone holds {} rows after checkpoint, replayed store {}",
            after.len(),
            replayed.len()
        ));
    }
    drop(journal);
    drop(lock);
    Ok(())
}

/// The in-transaction crash point with a record large enough that SQLite
/// spills the open transaction's pages to the WAL before COMMIT: the WAL
/// holds uncommitted frames when the writer dies, and they stay invisible.
pub fn in_transaction_spill(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    const PAD: usize = 3 * 1024 * 1024;
    const RECORDS: u64 = 3;
    let case = out.case.clone();
    let dir = ctx.case_dir(&case, run, "store")?;
    let db = dir.join(DB_NAME);
    let mut args = admit_args(&dir, &case, run, 1, RECORDS, false);
    args.extend(["--pad".to_owned(), PAD.to_string()]);
    let mut worker = ctx.spawn(&args, Some((CrashPoint::InTransaction, RECORDS)))?;
    let mut acks = Vec::new();
    worker.drain_to_eof(&mut acks)?;
    let status = worker.wait()?;
    out.detail("worker", status_text(&status));
    if !killed(&status) {
        out.violation(format!("worker ended {status}, not by SIGKILL"));
    }
    out.acked = count(acks.len());
    if out.acked != RECORDS - 1 {
        out.violation(format!("{} ACKs, expected {}", out.acked, RECORDS - 1));
    }
    let scan = wal_scan(&side(&db, "-wal"));
    let uncommitted = scan["uncommittedFrames"].as_u64().unwrap_or(0);
    out.detail("payloadPadBytes", json!(PAD));
    out.detail("walAtCrash", scan);
    if uncommitted == 0 {
        out.violation("premise: no uncommitted frames reached the WAL before the crash");
    }

    let (lock, mut journal) = ctx.reopen(&dir, out)?;
    let present = census_map(&census(&journal)?);
    out.lost = check_acks(&acks, &present, out);
    let crashing = Fixture::padded(&case, run, RECORDS, PAD);
    if present.contains_key(&crashing.id) {
        out.violation("uncommitted WAL frames became visible after reopen");
    }
    let mut statuses = Vec::new();
    for index in 1..=RECORDS {
        let receipt = journal
            .admit_observation(
                &Fixture::padded(&case, run, index, PAD).admission(),
                now_ms(),
            )
            .map_err(|error| format!("retry {index}: {error}"))?;
        statuses.push(status_of(&receipt));
    }
    out.detail("retryStatuses", json!(statuses));
    if statuses != [ALREADY_COMMITTED, ALREADY_COMMITTED, COMMITTED] {
        out.violation(format!("retry statuses {statuses:?}"));
    }
    drop(journal);
    drop(lock);
    Ok(())
}

fn contend(ctx: &Ctx, dir: &Path) -> Result<(Option<i32>, Vec<Value>), String> {
    let mut worker = ctx.spawn(
        &[
            "contend".to_owned(),
            "--store".to_owned(),
            dir.display().to_string(),
        ],
        None,
    )?;
    let mut events = Vec::new();
    loop {
        match worker.next(WORKER_TIMEOUT) {
            Next::Event(value) => events.push(value),
            Next::Eof => break,
            Next::Timeout => {
                worker.kill();
                return Err("contender silent".to_owned());
            }
        }
    }
    Ok((worker.wait()?.code(), events))
}

fn refused(result: &(Option<i32>, Vec<Value>)) -> bool {
    result.0 == Some(i32::from(EXIT_LOCK_HELD))
        && result.1.iter().any(|event| event["event"] == "lock-held")
}

/// A second writer is refused while one holds the lock, in-process and as
/// another process; after the holder is SIGKILLed the lock is recoverable.
pub fn writer_lock(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    let case = out.case.clone();
    let dir = ctx.case_dir(&case, run, "store")?;
    let mut worker = ctx.spawn(&admit_args(&dir, &case, run, 1, 3, true), None)?;
    let mut acks = Vec::new();
    worker.expect("done", &mut acks)?;
    let holder = worker.pid();

    match WriterLock::acquire(&dir) {
        Err(LockError::Held { .. }) => {}
        Ok(_lock) => out.violation("a second in-process writer acquired the lock"),
        Err(error) => return Err(format!("lock probe: {error}")),
    }
    let second = contend(ctx, &dir)?;
    out.detail("secondWriterWhileWorkerHolds", json!(second.0));
    if !refused(&second) {
        out.violation(format!("second writer process not refused: {second:?}"));
    }
    let recorded = std::fs::read_to_string(dir.join("writer.lock")).unwrap_or_default();
    out.detail("lockFilePid", json!(recorded.trim()));
    if recorded.trim() != holder.to_string() {
        out.violation("writer.lock does not name the holder");
    }

    worker.kill();
    let status = worker.wait()?;
    worker.drain_to_eof(&mut acks)?;
    out.detail("holder", status_text(&status));
    if !killed(&status) {
        out.violation("holder not killed by SIGKILL");
    }
    out.acked = count(acks.len());

    let (lock, journal) = ctx.reopen(&dir, out)?;
    out.detail("recoveredAfterHolderKilled", json!(true));
    out.lost = check_acks(&acks, &census_map(&census(&journal)?), out);
    let third = contend(ctx, &dir)?;
    out.detail("secondWriterWhileParentHolds", json!(third.0));
    if !refused(&third) {
        out.violation(format!(
            "writer process not refused while the parent holds the lock: {third:?}"
        ));
    }
    drop(journal);
    drop(lock);
    Ok(())
}

/// Backups through a separate read-only connection while a worker commits
/// continuously. Each must verify and hold exactly the committed prefix
/// through its cursor, including every record ACKed before it began.
pub fn backup_concurrent(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    const BACKUPS: i64 = 8;
    let case = out.case.clone();
    let dir = ctx.case_dir(&case, run, "store")?;
    let backups = ctx.case_dir(&case, run, "backups")?;
    let db = dir.join(DB_NAME);
    let mut worker = ctx.spawn(&admit_args(&dir, &case, run, 1, 1_000_000, false), None)?;
    let mut acks = Vec::new();
    while acks.len() < 10 {
        match worker.next(WORKER_TIMEOUT) {
            Next::Event(value) => absorb(&value, &mut acks)?,
            Next::Eof => return Err("worker ended early".to_owned()),
            Next::Timeout => {
                worker.kill();
                return Err("worker silent".to_owned());
            }
        }
    }
    let mut taken = Vec::new();
    for index in 0..BACKUPS {
        drain_acks(&mut worker, &mut acks)?;
        let before = acks.len();
        let before_cursor = acks.last().map_or(0, |ack| ack.cursor);
        let started = Instant::now();
        let info = backup_store_into(&db, &backups, now_ms() + index)
            .map_err(|error| format!("backup {index}: {error}"))?;
        let elapsed = started.elapsed();
        drain_acks(&mut worker, &mut acks)?;
        taken.push((info, before, before_cursor, acks.len(), elapsed));
        std::thread::sleep(Duration::from_millis(20));
    }
    worker.kill();
    let status = worker.wait()?;
    worker.drain_to_eof(&mut acks)?;
    out.detail("worker", status_text(&status));
    out.acked = count(acks.len());

    let (lock, journal) = ctx.reopen(&dir, out)?;
    let live = census(&journal)?;
    out.lost = check_acks(&acks, &census_map(&live), out);
    drop(journal);
    drop(lock);

    let mut overlapping = 0;
    let mut records = Vec::new();
    for (index, (info, before, before_cursor, after, elapsed)) in taken.iter().enumerate() {
        match verify_backup(&info.path) {
            Ok(again) if again.sha256 == info.sha256 && again.cursor == info.cursor => {}
            Ok(_) => out.violation(format!("backup {index} re-verified differently")),
            Err(error) => out.violation(format!("backup {index} no longer verifies: {error}")),
        }
        if *before_cursor > info.cursor {
            out.violation(format!(
                "backup {index} at cursor {} misses the record ACKed at {before_cursor} before it began",
                info.cursor
            ));
        }
        let scratch = ctx.case_dir(&case, run, &format!("backup-{index}"))?;
        copy(&info.path, &scratch.join(DB_NAME))?;
        let (_lock, copy_journal) = ctx.reopen(&scratch, out)?;
        let held = census(&copy_journal)?;
        let expected: Vec<(String, i64)> = live
            .iter()
            .filter(|(_, cursor)| *cursor <= info.cursor)
            .cloned()
            .collect();
        if held != expected {
            out.violation(format!(
                "backup {index} at cursor {} holds {} rows; the committed prefix through it has {}",
                info.cursor,
                held.len(),
                expected.len()
            ));
        }
        if after > before {
            overlapping += 1;
        }
        records.push(json!({
            "cursor": info.cursor,
            "rows": held.len(),
            "ackedBefore": before,
            "ackedAfter": after,
            "elapsedMs": elapsed.as_secs_f64() * 1000.0,
            "bytes": info.bytes,
            "sha256": info.sha256,
        }));
    }
    out.detail("backups", json!(records));
    out.detail("backupsOverlappingCommits", json!(overlapping));
    if overlapping == 0 {
        out.violation("no backup overlapped a commit; concurrency not exercised");
    }
    Ok(())
}

fn drain_acks(worker: &mut Worker, acks: &mut Vec<Ack>) -> Result<(), String> {
    for value in worker.drain() {
        absorb(&value, acks)?;
    }
    Ok(())
}

struct Prepared {
    dir: PathBuf,
    backup: BackupInfo,
    parent_acks: Vec<Ack>,
    worker_acks: Vec<Ack>,
    generation: String,
}

/// The parent commits `before` records and backs up; a worker then commits
/// `after` more and holds. With `kill`, the worker is SIGKILLed so the
/// original keeps committed frames in `-wal` beside its `-shm`.
fn prepare(
    ctx: &mut Ctx,
    out: &mut Outcome,
    before: u64,
    after: u64,
    kill: bool,
) -> Result<(Prepared, Option<Worker>), String> {
    let case = out.case.clone();
    let run = out.run;
    let dir = ctx.case_dir(&case, run, "store")?;
    let backups = ctx.case_dir(&case, run, "backups")?;
    let (backup, parent_acks, generation) = {
        let (_lock, mut journal) = ctx.reopen(&dir, out)?;
        let mut acks = Vec::new();
        for index in 1..=before {
            let receipt = journal
                .admit_observation(&Fixture::new(&case, run, index).admission(), now_ms())
                .map_err(|error| format!("admit {index}: {error}"))?;
            acks.push(Ack {
                index,
                id: receipt.observation_id.clone(),
                cursor: receipt.cursor,
                status: status_of(&receipt),
            });
        }
        let info = journal
            .backup_into(&backups, now_ms())
            .map_err(|error| format!("backup: {error}"))?;
        (info, acks, journal.store_generation().to_owned())
    };
    let mut worker = ctx.spawn(&admit_args(&dir, &case, run, before + 1, after, true), None)?;
    let mut worker_acks = Vec::new();
    worker.expect("done", &mut worker_acks)?;
    let worker = if kill {
        worker.kill();
        let status = worker.wait()?;
        worker.drain_to_eof(&mut worker_acks)?;
        if !killed(&status) {
            return Err(format!("worker ended {status}, not by SIGKILL"));
        }
        None
    } else {
        Some(worker)
    };
    out.acked = count(parent_acks.len() + worker_acks.len());
    out.detail(
        "backup",
        json!({
            "cursor": backup.cursor,
            "bytes": backup.bytes,
            "sha256": backup.sha256,
            "schemaVersion": backup.schema_version,
            "sqliteVersion": backup.sqlite_version,
        }),
    );
    Ok((
        Prepared {
            dir,
            backup,
            parent_acks,
            worker_acks,
            generation,
        },
        worker,
    ))
}

/// Restoring a verified backup over a crashed store installs exactly the
/// backup's snapshot and preserves the original, WAL included, byte-identical.
pub fn restore_success(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    let case = out.case.clone();
    let (prepared, _) = prepare(ctx, out, 20, 15, true)?;
    let db = prepared.dir.join(DB_NAME);
    let originals = store_hashes(&db);
    if originals[1].is_none() || file_len(&side(&db, "-wal")) == Some(0) {
        out.violation("premise: the original has no committed WAL frames");
    }

    let lock = WriterLock::acquire(&prepared.dir).map_err(|error| format!("lock: {error}"))?;
    let restored = restore_backup(&lock, &db, &prepared.backup.path, now_ms())
        .map_err(|error| format!("restore: {error}"))?;
    if restored.installed.sha256 != prepared.backup.sha256
        || file_hash(&db).as_deref() != Some(prepared.backup.sha256.as_str())
    {
        out.violation("installed database is not the verified backup");
    }
    for (suffix, original) in ["", "-wal", "-shm"].iter().zip(&originals) {
        if let Some(original) = original {
            let kept = restored.preserved_dir.join(format!("{DB_NAME}{suffix}"));
            if file_hash(&kept).as_ref() != Some(original) {
                out.violation(format!(
                    "preserved {DB_NAME}{suffix} differs from the original"
                ));
            }
        }
    }
    for suffix in ["-wal", "-shm"] {
        if side(&db, suffix).exists() {
            out.violation(format!("stale {suffix} left beside the restored database"));
        }
    }
    let journal = ctx.open(&prepared.dir, out)?;
    let rows = census(&journal)?;
    let expected: Vec<(String, i64)> = prepared
        .parent_acks
        .iter()
        .map(|ack| (ack.id.clone(), ack.cursor))
        .collect();
    if rows != expected {
        out.violation(format!(
            "restored store holds {} rows; the backup snapshot had {}",
            rows.len(),
            expected.len()
        ));
    }
    if journal.store_generation() != prepared.generation {
        out.violation("store generation changed across restore");
    }
    if journal.cursor().ok() != Some(prepared.backup.cursor) {
        out.violation("restored cursor is not the backup cursor");
    }
    drop(journal);
    drop(lock);

    // The preserved original, with its WAL, still holds every ACKed record.
    let scratch = ctx.case_dir(&case, run, "preserved-original")?;
    for suffix in ["", "-wal"] {
        let kept = restored.preserved_dir.join(format!("{DB_NAME}{suffix}"));
        if kept.exists() {
            copy(&kept, &scratch.join(format!("{DB_NAME}{suffix}")))?;
        }
    }
    let (_lock, original) = ctx.reopen(&scratch, out)?;
    let all: Vec<Ack> = prepared
        .parent_acks
        .iter()
        .chain(&prepared.worker_acks)
        .cloned()
        .collect();
    out.lost = check_acks(&all, &census_map(&census(&original)?), out);
    out.detail(
        "preserved",
        json!({
            "dir": restored.preserved_dir.file_name().map(|name| name.to_string_lossy().into_owned()),
            "files": restored.preserved_files.iter().filter_map(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        }),
    );
    out.detail("restoredRows", json!(rows.len()));
    out.detail(
        "ackedAfterBackupOnlyInPreservedOriginal",
        json!(prepared.worker_acks.len()),
    );
    Ok(())
}

/// Bad backups are rejected and the original database, `-wal` and `-shm`
/// stay byte-identical, with nothing added beside them.
pub fn restore_rejected(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    let case = out.case.clone();
    let (prepared, _) = prepare(ctx, out, 20, 10, true)?;
    let db = prepared.dir.join(DB_NAME);
    let bad = ctx.case_dir(&case, run, "bad")?;
    let good = std::fs::read(&prepared.backup.path).map_err(|error| error.to_string())?;
    let page = match u16::from_be_bytes([good[16], good[17]]) {
        1 => 65_536,
        size => usize::from(size),
    };
    let mut page_headers = good.clone();
    for offset in (2 * page..page_headers.len()).step_by(page) {
        page_headers[offset..offset + 8].fill(0xff);
    }
    let mut last_page = good.clone();
    let last = last_page.len() - page;
    last_page[last + 1..last + 5].fill(0xff);
    let live_main = std::fs::read(&db).map_err(|error| error.to_string())?;
    let variants: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("not-sqlite", vec![0x5a; 8192]),
        ("truncated-mid-page", good[..good.len() / 2 + 100].to_vec()),
        (
            "interrupted-missing-last-page",
            good[..good.len() - page].to_vec(),
        ),
        ("corrupt-page-headers", page_headers),
        ("corrupt-last-page", last_page),
        ("live-main-file-copy", live_main),
    ];

    let lock = WriterLock::acquire(&prepared.dir).map_err(|error| format!("lock: {error}"))?;
    let listing_before = listing(&prepared.dir);
    let mut rejections = Vec::new();
    for (name, bytes) in variants {
        let path = bad.join(format!("{name}.sqlite3"));
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        let before = store_hashes(&db);
        match restore_backup(&lock, &db, &path, now_ms()) {
            Err(BackupError::Rejected { reason, .. }) => {
                rejections.push(json!({ "variant": name, "reason": reason }));
            }
            Err(error) => out.violation(format!("{name}: expected rejection, got {error}")),
            Ok(_) => return Err(format!("{name}: restore accepted a bad backup")),
        }
        if store_hashes(&db) != before {
            out.violation(format!("{name}: original changed by a rejected restore"));
        }
        if listing(&prepared.dir) != listing_before {
            out.violation(format!(
                "{name}: files added or removed beside the original"
            ));
        }
    }
    out.detail("rejections", json!(rejections));

    let journal = ctx.open(&prepared.dir, out)?;
    let all: Vec<Ack> = prepared
        .parent_acks
        .iter()
        .chain(&prepared.worker_acks)
        .cloned()
        .collect();
    out.lost = check_acks(&all, &census_map(&census(&journal)?), out);
    drop(journal);
    drop(lock);
    Ok(())
}

/// Restore cannot run while another process holds the store's writer lock,
/// and a lock for a different directory is refused.
pub fn restore_without_lock(ctx: &mut Ctx, out: &mut Outcome, run: u16) -> Result<(), String> {
    let case = out.case.clone();
    let (prepared, holder) = prepare(ctx, out, 5, 3, false)?;
    let mut holder = holder.ok_or("no live holder")?;
    let db = prepared.dir.join(DB_NAME);
    let before = store_hashes(&db);

    match WriterLock::acquire(&prepared.dir) {
        Err(LockError::Held { .. }) => out.detail("storeLockWhileHeld", json!("HELD")),
        Ok(_lock) => out.violation("the store lock was acquired while its writer is alive"),
        Err(error) => return Err(format!("lock probe: {error}")),
    }
    let elsewhere = ctx.case_dir(&case, run, "elsewhere")?;
    let other = WriterLock::acquire(&elsewhere).map_err(|error| format!("other lock: {error}"))?;
    match restore_backup(&other, &db, &prepared.backup.path, now_ms()) {
        Err(BackupError::LockNotHeld { .. }) => {
            out.detail("foreignLockRestore", json!("LOCK_NOT_HELD"))
        }
        Err(error) => out.violation(format!("expected LockNotHeld, got {error}")),
        Ok(_) => out.violation("restore proceeded under another store's lock"),
    }
    if store_hashes(&db) != before {
        out.violation("original changed by a refused restore");
    }
    drop(other);

    holder.kill();
    holder.wait()?;
    let mut acks = prepared.worker_acks.clone();
    holder.drain_to_eof(&mut acks)?;
    let (lock, journal) = ctx.reopen(&prepared.dir, out)?;
    let all: Vec<Ack> = prepared.parent_acks.iter().chain(&acks).cloned().collect();
    out.lost = check_acks(&all, &census_map(&census(&journal)?), out);
    drop(journal);
    drop(lock);
    Ok(())
}
