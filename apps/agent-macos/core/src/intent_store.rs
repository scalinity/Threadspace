//! Durable ownership of accepted notification work (SPEC §7.5, §18.9).
//!
//! Two kinds of durable owner live in the store directory:
//!
//! - A **response record** (`responses/<notification request>.json`) is
//!   written when a live notification response is received, before any work
//!   on it. While it exists the response is unresolved. A writer that finds
//!   one when it starts opens the inspector for it and never replays a
//!   Return: a focus planned that late would move the owner's screen long
//!   after the click.
//! - The **pending-intent backlog** (`pending-intents.json`): the accepted,
//!   unconsumed notification and inspector intents in acceptance order, a
//!   bounded memory of intent IDs views consumed, and a revision.
//!
//! A response is durably accepted once its record or its intent in the
//! backlog is committed, and not before. Ownership moves from one owner to
//! the next only by committing the next owner first: a record is removed
//! only after its intent is in a committed backlog. Every storage step
//! returns a typed result, and a failed step leaves the previous owner
//! authoritative.
//!
//! A commit is temporary file → write → fsync → rename → directory fsync. A
//! failure before the rename leaves the previous file intact and the
//! temporary file removed. After the rename the new state survives a process
//! crash; if the directory fsync then fails, the commit is reported with
//! power-loss durability unconfirmed, the same domain the journal qualifies
//! (process crash, not power loss). An unreadable backlog is never
//! overwritten: a malformed file is moved aside, kept and reported; a file
//! that cannot be read at all disables backlog commits.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use threadspace_contracts::projection::{IntentAction, NativeIntent};
use uuid::Uuid;

use crate::log;
use crate::writer::WriterCommand;

const BACKLOG: &str = "pending-intents.json";
const RECORDS: &str = "responses";
const SCHEMA: u32 = 2;
/// Accepted, unconsumed intents the backlog holds. Beyond it a response is
/// refused before it is accepted; accepted work is never evicted.
pub const BACKLOG_LIMIT: usize = 256;
/// Unresolved response records the store holds; beyond it a live response
/// is refused before it is accepted.
pub const RECORD_LIMIT: usize = 256;
/// Consumed intent IDs remembered, so a lingering record of an intent a
/// view already consumed is not applied again.
pub const CONSUMED_MEMORY: usize = 256;

/// Whether this process's writer still takes responses. Held while one is
/// recorded and handed over, so `seal` waits for any in flight.
static ACCEPTING: Mutex<bool> = Mutex::new(true);
/// Recorded responses the writer could not be handed while its queue was
/// full; it takes them on its next retry.
static UNQUEUED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The storage step that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Step {
    Encode,
    CreateTemporary,
    Write,
    SyncFile,
    Rename,
    Remove,
    /// The bound was reached; nothing was written.
    Full,
    /// The backlog could not be read at start, so it is never overwritten.
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct StorageFailure {
    pub step: Step,
    pub detail: String,
}

impl StorageFailure {
    pub fn new(step: Step, detail: impl ToString) -> Self {
        Self {
            step,
            detail: detail.to_string(),
        }
    }

    pub fn json(&self) -> Value {
        json!({ "step": self.step, "detail": self.detail })
    }
}

/// A committed write; `power_loss_confirmed` is false when the directory
/// fsync after the rename failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Commit {
    pub power_loss_confirmed: bool,
}

fn valid_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
}

/// Storage steps a qualification build can make fail, per store directory
/// (never present in release builds).
#[cfg(any(test, feature = "qualification"))]
pub mod faults {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Target {
        Backlog,
        Record,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Op {
        CreateTemporary,
        Write,
        SyncFile,
        Rename,
        SyncDirectory,
        Remove,
    }

    static PLAN: Mutex<Vec<(PathBuf, Target, Op, u32)>> = Mutex::new(Vec::new());

    /// The next `count` attempts of `op` on `target` in `dir` fail.
    pub fn arm(dir: &Path, target: Target, op: Op, count: u32) {
        if let Ok(mut plan) = PLAN.lock() {
            plan.retain(|(d, t, o, _)| !(d == dir && *t == target && *o == op));
            if count > 0 {
                plan.push((dir.to_path_buf(), target, op, count));
            }
        }
    }

    pub(super) fn hit(dir: &Path, target: Target, op: Op) -> bool {
        let Ok(mut plan) = PLAN.lock() else {
            return false;
        };
        let Some(entry) = plan
            .iter_mut()
            .find(|(d, t, o, n)| d == dir && *t == target && *o == op && *n > 0)
        else {
            return false;
        };
        entry.3 -= 1;
        true
    }
}

#[cfg(any(test, feature = "qualification"))]
use faults::{Op, Target};

#[cfg(not(any(test, feature = "qualification")))]
#[derive(Clone, Copy)]
enum Target {
    Backlog,
    Record,
}

#[cfg(not(any(test, feature = "qualification")))]
#[derive(Clone, Copy)]
enum Op {
    CreateTemporary,
    Write,
    SyncFile,
    Rename,
    SyncDirectory,
    Remove,
}

/// An injected failure for this step, if one is armed.
fn injected(dir: &Path, target: Target, op: Op) -> Option<std::io::Error> {
    #[cfg(any(test, feature = "qualification"))]
    if faults::hit(dir, target, op) {
        return Some(std::io::Error::other(format!(
            "qualification fault: {op:?} on {target:?}"
        )));
    }
    let _ = (dir, target, op);
    None
}

/// Runs one storage step, unless a fault is armed for it: an injected
/// failure happens instead of the step, never after it.
fn checked(
    dir: &Path,
    target: Target,
    op: Op,
    step: Step,
    action: impl FnOnce() -> std::io::Result<()>,
) -> Result<(), StorageFailure> {
    if let Some(error) = injected(dir, target, op) {
        return Err(StorageFailure::new(step, error));
    }
    action().map_err(|error| StorageFailure::new(step, error))
}

/// Writes `bytes` as `dir/name` so a reader sees the old or the new file,
/// never a partial one.
fn write_atomic(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    target: Target,
) -> Result<Commit, StorageFailure> {
    let path = dir.join(name);
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let staged = (|| {
        if let Some(error) = injected(dir, target, Op::CreateTemporary) {
            return Err(StorageFailure::new(Step::CreateTemporary, error));
        }
        let mut file = File::create(&temporary)
            .map_err(|error| StorageFailure::new(Step::CreateTemporary, error))?;
        checked(dir, target, Op::Write, Step::Write, || {
            file.write_all(bytes)
        })?;
        checked(dir, target, Op::SyncFile, Step::SyncFile, || {
            file.sync_all()
        })
    })();
    if let Err(failure) = staged {
        let _ = fs::remove_file(&temporary);
        return Err(failure);
    }
    #[cfg(feature = "qualification")]
    if matches!(target, Target::Backlog) {
        crate::writer::handoff::pass(crate::writer::handoff::Point::BeforeBacklogCommit);
    }
    if let Err(failure) = checked(dir, target, Op::Rename, Step::Rename, || {
        fs::rename(&temporary, &path)
    }) {
        let _ = fs::remove_file(&temporary);
        return Err(failure);
    }
    let directory_synced = injected(dir, target, Op::SyncDirectory).is_none()
        && File::open(dir).and_then(|d| d.sync_all()).is_ok();
    Ok(Commit {
        power_loss_confirmed: directory_synced,
    })
}

/// Temporary files left by a process that died mid-commit; they never hold
/// committed state.
fn remove_temporaries(dir: &Path, name: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let prefix = format!(".{name}.");
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        if file_name
            .to_str()
            .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".tmp"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Intents that belong to the owner: notification, navigation and inspector
/// intents. Qualification commands stay with the process that received them.
pub fn durable(intent: &NativeIntent) -> bool {
    matches!(intent.action, IntentAction::OpenAttention { .. })
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BacklogFile {
    schema: u32,
    revision: u64,
    pending: Vec<NativeIntent>,
    consumed: Vec<String>,
}

/// The committed backlog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backlog {
    pub revision: u64,
    pub pending: Vec<NativeIntent>,
    pub consumed: VecDeque<String>,
}

/// How the backlog was found at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Absent,
    Loaded,
    /// Malformed: moved aside under `kept_as`, kept, and a new backlog begun.
    Quarantined {
        kept_as: String,
        error: String,
    },
    /// Unreadable: left in place, and no backlog commit is attempted.
    Unreadable {
        error: String,
    },
}

pub fn load_backlog(dir: &Path) -> (Backlog, Found) {
    remove_temporaries(dir, BACKLOG);
    let path = dir.join(BACKLOG);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Backlog::default(), Found::Absent);
        }
        Err(error) => {
            return (
                Backlog::default(),
                Found::Unreadable {
                    error: error.to_string(),
                },
            );
        }
    };
    let parsed = serde_json::from_slice::<BacklogFile>(&bytes)
        .map(|file| (file.revision, file.pending, file.consumed))
        // The first M0C store wrote a bare array of pending intents.
        .or_else(|_| {
            serde_json::from_slice::<Vec<NativeIntent>>(&bytes).map(|pending| (0, pending, vec![]))
        });
    match parsed {
        Ok((revision, pending, consumed)) => (
            Backlog {
                revision,
                pending: pending
                    .into_iter()
                    .filter(|intent| durable(intent) && valid_uuid(&intent.intent_id))
                    .collect(),
                consumed: consumed.into_iter().collect(),
            },
            Found::Loaded,
        ),
        Err(error) => {
            let kept_as = format!("pending-intents.corrupt-{}.json", log::now_ms());
            match fs::rename(&path, dir.join(&kept_as)) {
                Ok(()) => (
                    Backlog::default(),
                    Found::Quarantined {
                        kept_as,
                        error: error.to_string(),
                    },
                ),
                Err(rename) => (
                    Backlog::default(),
                    Found::Unreadable {
                        error: format!("{error}; not moved aside: {rename}"),
                    },
                ),
            }
        }
    }
}

/// Commits a whole backlog, replacing the previous one only on success.
pub fn commit_backlog(
    dir: &Path,
    revision: u64,
    pending: &[&NativeIntent],
    consumed: &VecDeque<String>,
) -> Result<Commit, StorageFailure> {
    let file = BacklogFile {
        schema: SCHEMA,
        revision,
        pending: pending.iter().map(|intent| (*intent).clone()).collect(),
        consumed: consumed.iter().cloned().collect(),
    };
    let bytes =
        serde_json::to_vec(&file).map_err(|error| StorageFailure::new(Step::Encode, error))?;
    write_atomic(dir, BACKLOG, &bytes, Target::Backlog)
}

/// An accepted, unresolved notification response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseRecord {
    pub notification_request_id: String,
    pub attention_id: String,
    pub received_at_ms: i64,
}

fn record_name(notification_request_id: &str) -> String {
    format!("{notification_request_id}.json")
}

fn record_ids(records_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(records_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| valid_uuid(id))
                .map(str::to_owned)
        })
        .collect()
}

/// Records a response as accepted. Named by its notification request, so
/// recording the same response twice keeps one record.
pub fn write_record(
    dir: &Path,
    notification_request_id: &str,
    attention_id: &str,
) -> Result<Commit, StorageFailure> {
    if !valid_uuid(notification_request_id) {
        return Err(StorageFailure::new(
            Step::Encode,
            "notification request ID is not a UUID",
        ));
    }
    let records_dir = dir.join(RECORDS);
    fs::create_dir_all(&records_dir)
        .map_err(|error| StorageFailure::new(Step::CreateTemporary, error))?;
    let ids = record_ids(&records_dir);
    if ids.len() >= RECORD_LIMIT && !ids.iter().any(|id| id == notification_request_id) {
        return Err(StorageFailure::new(
            Step::Full,
            format!("{RECORD_LIMIT} unresolved responses"),
        ));
    }
    let record = ResponseRecord {
        notification_request_id: notification_request_id.to_owned(),
        attention_id: attention_id.to_owned(),
        received_at_ms: log::now_ms(),
    };
    let bytes =
        serde_json::to_vec(&record).map_err(|error| StorageFailure::new(Step::Encode, error))?;
    write_atomic(
        &records_dir,
        &record_name(notification_request_id),
        &bytes,
        Target::Record,
    )
}

pub fn record_exists(dir: &Path, notification_request_id: &str) -> bool {
    valid_uuid(notification_request_id)
        && dir
            .join(RECORDS)
            .join(record_name(notification_request_id))
            .exists()
}

/// Unresolved responses, oldest first. A malformed record is moved aside
/// and kept; temporary files of interrupted writes are removed.
pub fn records(dir: &Path) -> Vec<ResponseRecord> {
    let records_dir = dir.join(RECORDS);
    let mut found = Vec::new();
    for id in record_ids(&records_dir) {
        remove_temporaries(&records_dir, &record_name(&id));
        let path = records_dir.join(record_name(&id));
        match fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ResponseRecord>(&bytes).ok())
        {
            Some(record) if record.notification_request_id == id => found.push(record),
            _ => {
                let kept_as = format!("{id}.corrupt-{}", log::now_ms());
                let _ = fs::rename(&path, records_dir.join(&kept_as));
                log::error(
                    "RESPONSE_RECORD_QUARANTINED",
                    json!({ "requestId": id, "keptAs": kept_as }),
                );
            }
        }
    }
    found.sort_by_key(|record| record.received_at_ms);
    found
}

/// Retires a record once its intent is committed in the backlog (or there
/// is nothing left to own). A record already gone is success.
pub fn remove_record(dir: &Path, notification_request_id: &str) -> Result<(), StorageFailure> {
    if !valid_uuid(notification_request_id) {
        return Ok(());
    }
    let records_dir = dir.join(RECORDS);
    if let Some(error) = injected(&records_dir, Target::Record, Op::Remove) {
        return Err(StorageFailure::new(Step::Remove, error));
    }
    match fs::remove_file(records_dir.join(record_name(notification_request_id))) {
        Ok(()) => {
            let _ = File::open(&records_dir).and_then(|d| d.sync_all());
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageFailure::new(Step::Remove, error)),
    }
}

/// Where a fault for a target is armed: the backlog's directory or the
/// records directory.
#[cfg(feature = "qualification")]
pub fn fault_dir(dir: &Path, target: Target) -> std::path::PathBuf {
    match target {
        Target::Backlog => dir.to_path_buf(),
        Target::Record => dir.join(RECORDS),
    }
}

/// Accepts one live notification response, received at `received`: records
/// it, then hands it to the writer while it accepts. A response that could
/// not be recorded is still handed over, so the writer can accept it
/// through the backlog; one that is neither recorded nor handed over is
/// reported as not accepted.
pub fn accept(
    dir: &Path,
    writer: &SyncSender<WriterCommand>,
    notification_request_id: String,
    attention_id: String,
    received: Instant,
) {
    let accepting = ACCEPTING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let recorded = match write_record(dir, &notification_request_id, &attention_id) {
        Ok(commit) => {
            log::info(
                "RESPONSE_RECORDED",
                json!({ "requestId": notification_request_id, "attentionId": attention_id, "powerLossConfirmed": commit.power_loss_confirmed }),
            );
            true
        }
        Err(failure) => {
            log::error(
                "RESPONSE_RECORD_FAILED",
                json!({ "requestId": notification_request_id, "failure": failure.json() }),
            );
            false
        }
    };
    if !*accepting {
        log_left(&notification_request_id, recorded, "WRITER_RELEASING");
        return;
    }
    match writer.try_send(WriterCommand::NotificationResponse {
        notification_request_id,
        attention_id,
        received,
        recorded,
    }) {
        Ok(()) => {}
        Err(TrySendError::Full(command) | TrySendError::Disconnected(command)) => {
            if let WriterCommand::NotificationResponse {
                notification_request_id,
                recorded,
                ..
            } = command
            {
                if recorded && let Ok(mut unqueued) = UNQUEUED.lock() {
                    unqueued.push(notification_request_id.clone());
                }
                log_left(&notification_request_id, recorded, "WRITER_BUSY");
            }
        }
    }
}

fn log_left(notification_request_id: &str, recorded: bool, why: &str) {
    if recorded {
        log::info(
            "RESPONSE_LEFT_RECORDED",
            json!({ "requestId": notification_request_id, "reason": why }),
        );
    } else {
        log::error(
            "NOTIFICATION_RESPONSE_NOT_ACCEPTED",
            json!({ "requestId": notification_request_id, "reason": why }),
        );
    }
}

/// Recorded responses the writer has not been handed.
pub fn take_unqueued() -> Vec<String> {
    UNQUEUED
        .lock()
        .map(|mut ids| std::mem::take(&mut *ids))
        .unwrap_or_default()
}

/// A forwarding instance records the response before handing it on, so a
/// failed hand-off leaves it for the next writer.
pub fn record_for_forwarding(dir: &Path, notification_request_id: &str, attention_id: &str) {
    match write_record(dir, notification_request_id, attention_id) {
        Ok(_) => log::info(
            "RESPONSE_RECORDED",
            json!({ "requestId": notification_request_id, "attentionId": attention_id, "reason": "FORWARDED" }),
        ),
        Err(failure) => log::error(
            "RESPONSE_RECORD_FAILED",
            json!({ "requestId": notification_request_id, "reason": "FORWARDED", "failure": failure.json() }),
        ),
    }
}

/// The writer stops taking responses; later ones stay recorded.
pub fn close() {
    *ACCEPTING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
}

/// Waits for any response being recorded or handed over and holds
/// acceptance shut until the process exits.
pub fn seal() -> MutexGuard<'static, bool> {
    ACCEPTING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use threadspace_contracts::projection::IntentSource;

    fn intent(id: &str) -> NativeIntent {
        NativeIntent {
            intent_id: id.to_owned(),
            action: IntentAction::OpenAttention {
                attention_id: Uuid::new_v4().to_string(),
                session_id: Uuid::new_v4().to_string(),
                outstanding: true,
                source: IntentSource::NotificationResponse,
                route: None,
                observation_enabled: false,
            },
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ts-intents-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|_| Uuid::new_v4().to_string()).collect()
    }

    fn commit(
        dir: &Path,
        revision: u64,
        pending: &[NativeIntent],
    ) -> Result<Commit, StorageFailure> {
        let refs: Vec<&NativeIntent> = pending.iter().collect();
        commit_backlog(dir, revision, &refs, &VecDeque::new())
    }

    fn loaded_ids(dir: &Path) -> Vec<String> {
        load_backlog(dir)
            .0
            .pending
            .into_iter()
            .map(|i| i.intent_id)
            .collect()
    }

    fn temporaries(dir: &Path) -> usize {
        fs::read_dir(dir)
            .map(|e| {
                e.flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn a_committed_backlog_reloads_exactly_in_order() {
        let dir = temp_dir();
        assert_eq!(load_backlog(&dir).1, Found::Absent);
        let ids = ids(40);
        let pending: Vec<NativeIntent> = ids.iter().map(|id| intent(id)).collect();
        let commit = commit(&dir, 7, &pending).expect("commit");
        assert!(commit.power_loss_confirmed);
        let (backlog, found) = load_backlog(&dir);
        assert_eq!(found, Found::Loaded);
        assert_eq!(backlog.revision, 7);
        assert_eq!(backlog.pending, pending);
        assert_eq!(temporaries(&dir), 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn every_failure_before_the_rename_keeps_the_previous_backlog() {
        for op in [Op::CreateTemporary, Op::Write, Op::SyncFile, Op::Rename] {
            let dir = temp_dir();
            let first = ids(2);
            let before: Vec<NativeIntent> = first.iter().map(|id| intent(id)).collect();
            commit(&dir, 1, &before).expect("first commit");
            faults::arm(&dir, Target::Backlog, op, 1);
            let mut after = before.clone();
            after.push(intent(&Uuid::new_v4().to_string()));
            let failed = commit(&dir, 2, &after).expect_err("injected failure");
            assert_ne!(failed.step, Step::Encode, "{op:?}");
            assert_eq!(
                loaded_ids(&dir),
                first,
                "{op:?} must leave the previous backlog"
            );
            assert_eq!(load_backlog(&dir).0.revision, 1);
            assert_eq!(
                temporaries(&dir),
                0,
                "{op:?} must remove its temporary file"
            );
            // The next attempt converges.
            commit(&dir, 2, &after).expect("retry");
            assert_eq!(loaded_ids(&dir).len(), 3);
            fs::remove_dir_all(dir).ok();
        }
    }

    #[test]
    fn a_failed_directory_sync_commits_without_power_loss_confirmation() {
        let dir = temp_dir();
        let pending = vec![intent(&Uuid::new_v4().to_string())];
        faults::arm(&dir, Target::Backlog, Op::SyncDirectory, 1);
        let commit = commit(&dir, 1, &pending).expect("rename committed");
        assert!(!commit.power_loss_confirmed);
        assert_eq!(load_backlog(&dir).0.pending, pending);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn leftover_temporaries_are_removed_and_never_read() {
        let dir = temp_dir();
        let pending = vec![intent(&Uuid::new_v4().to_string())];
        commit(&dir, 1, &pending).expect("commit");
        fs::write(dir.join(".pending-intents.json.99999.tmp"), b"{partial").expect("tmp");
        let (backlog, found) = load_backlog(&dir);
        assert_eq!(found, Found::Loaded);
        assert_eq!(backlog.pending, pending);
        assert_eq!(temporaries(&dir), 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_malformed_backlog_is_kept_aside_never_overwritten() {
        let dir = temp_dir();
        fs::write(dir.join(BACKLOG), b"{not json").expect("corrupt");
        let (backlog, found) = load_backlog(&dir);
        assert!(backlog.pending.is_empty());
        let Found::Quarantined { kept_as, .. } = found else {
            panic!("{found:?}");
        };
        assert_eq!(fs::read(dir.join(kept_as)).expect("kept"), b"{not json");
        assert!(!dir.join(BACKLOG).exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_first_store_format_still_loads() {
        let dir = temp_dir();
        let pending = vec![intent(&Uuid::new_v4().to_string())];
        fs::write(
            dir.join(BACKLOG),
            serde_json::to_vec(&pending).expect("json"),
        )
        .expect("v1");
        let (backlog, found) = load_backlog(&dir);
        assert_eq!(found, Found::Loaded);
        assert_eq!(backlog.pending, pending);
        assert_eq!(backlog.revision, 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn consumed_ids_survive_a_reload() {
        let dir = temp_dir();
        let consumed: VecDeque<String> = ids(3).into();
        commit_backlog(&dir, 4, &[], &consumed).expect("commit");
        assert_eq!(load_backlog(&dir).0.consumed, consumed);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn records_are_kept_once_bounded_and_removed_only_on_success() {
        let dir = temp_dir();
        let request = Uuid::new_v4().to_string();
        let attention = Uuid::new_v4().to_string();
        write_record(&dir, &request, &attention).expect("record");
        write_record(&dir, &request, &attention).expect("record again");
        assert_eq!(records(&dir).len(), 1);
        assert!(record_exists(&dir, &request));
        faults::arm(&dir.join(RECORDS), Target::Record, Op::Remove, 1);
        assert_eq!(
            remove_record(&dir, &request).expect_err("injected").step,
            Step::Remove
        );
        assert!(
            record_exists(&dir, &request),
            "a failed removal keeps the owner"
        );
        remove_record(&dir, &request).expect("remove");
        assert!(!record_exists(&dir, &request));
        remove_record(&dir, &request).expect("already gone is success");
        assert!(write_record(&dir, "../escape", &attention).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_record_write_leaves_no_record() {
        for op in [Op::CreateTemporary, Op::Write, Op::SyncFile, Op::Rename] {
            let dir = temp_dir();
            fs::create_dir_all(dir.join(RECORDS)).expect("dir");
            faults::arm(&dir.join(RECORDS), Target::Record, op, 1);
            let request = Uuid::new_v4().to_string();
            assert!(write_record(&dir, &request, &Uuid::new_v4().to_string()).is_err());
            assert!(!record_exists(&dir, &request), "{op:?}");
            assert!(records(&dir).is_empty());
            fs::remove_dir_all(dir).ok();
        }
    }

    #[test]
    fn records_beyond_the_bound_are_refused_before_acceptance() {
        let dir = temp_dir();
        let attention = Uuid::new_v4().to_string();
        for id in ids(RECORD_LIMIT) {
            write_record(&dir, &id, &attention).expect("record");
        }
        let refused =
            write_record(&dir, &Uuid::new_v4().to_string(), &attention).expect_err("full");
        assert_eq!(refused.step, Step::Full);
        assert_eq!(records(&dir).len(), RECORD_LIMIT);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_malformed_record_is_kept_aside() {
        let dir = temp_dir();
        let request = Uuid::new_v4().to_string();
        fs::create_dir_all(dir.join(RECORDS)).expect("dir");
        fs::write(dir.join(RECORDS).join(record_name(&request)), b"{").expect("corrupt");
        assert!(records(&dir).is_empty());
        let kept = fs::read_dir(dir.join(RECORDS))
            .expect("dir")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .count();
        assert_eq!(kept, 1);
        fs::remove_dir_all(dir).ok();
    }
}
