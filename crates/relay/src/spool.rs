//! The bounded local capture spool (SPEC §8.4):
//!
//! ```text
//! capture-spool/
//!   pending/<observation-uuid>.tmp
//!   ready/<observation-uuid>.<bytes>.json
//!   quarantine/<reason>/<observation-uuid>.json
//!   dropped/<observation-uuid>.<event>
//! ```
//!
//! A record is written to an exclusive temporary file, synced, closed and
//! atomically renamed into `ready/`, then the directory is synced. Readers
//! never see a partial record. The record's size is in its name, so the
//! byte and record bounds are enforced from one directory listing without
//! reading or stat-ing records. When a bound is reached the record is not
//! written; a zero-byte marker in `dropped/` names it for saturation
//! diagnostics (itself bounded). Optional activity detail is refused first.
//! A completed publication survives an ordinary process crash; power-loss
//! durability is not claimed beyond what the platform's fsync provides.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_contracts::limits::capture::{
    OBSERVATION_MAX_BYTES, SPOOL_LOW_PRIORITY_PERCENT, SPOOL_MAX_BYTES, SPOOL_MAX_MARKERS,
    SPOOL_MAX_RECORDS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Identity, outcomes, waits and everything not optional.
    Essential,
    /// High-volume tool activity detail, refused first.
    Activity,
}

impl Priority {
    /// Tool activity is optional detail; everything else is essential.
    pub fn of(native_event: &str) -> Self {
        match native_event {
            "PreToolUse" | "PostToolUse" | "PostToolBatch" | "tool.call" => Self::Activity,
            _ => Self::Essential,
        }
    }
}

#[derive(Debug)]
pub enum SpoolError {
    TooLarge,
    Saturated,
    InvalidId,
    Io(io::Error),
}

impl std::fmt::Display for SpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("record exceeds the observation bound"),
            Self::Saturated => f.write_str("spool bound reached"),
            Self::InvalidId => f.write_str("observation id is not a UUID"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl From<io::Error> for SpoolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Limits a spool enforces; the frozen defaults outside tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub max_records: usize,
    pub max_bytes: u64,
    pub max_markers: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            max_records: SPOOL_MAX_RECORDS,
            max_bytes: SPOOL_MAX_BYTES,
            max_markers: SPOOL_MAX_MARKERS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyRecord {
    pub path: PathBuf,
    pub observation_id: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpoolStats {
    pub ready_records: usize,
    pub ready_bytes: u64,
    pub quarantined: usize,
    pub dropped_markers: usize,
    pub pending_files: usize,
}

pub struct Spool {
    root: PathBuf,
    bounds: Bounds,
}

fn private_dir(path: &Path) -> io::Result<()> {
    match DirBuilder::new().recursive(true).mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_dir(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

fn valid_uuid(text: &str) -> bool {
    text.len() == 36
        && text
            .bytes()
            .enumerate()
            .all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || matches!(b, b'a'..=b'f')
                }
            })
}

/// `<uuid>.<bytes>.json` → (uuid, bytes).
fn parse_ready_name(name: &str) -> Option<(String, u64)> {
    let stem = name.strip_suffix(".json")?;
    let (id, bytes) = stem.split_once('.')?;
    let bytes = bytes.parse().ok()?;
    valid_uuid(id).then(|| (id.to_owned(), bytes))
}

impl Spool {
    pub fn at(store_dir: &Path) -> Self {
        Self::with_bounds(store_dir, Bounds::default())
    }

    pub fn with_bounds(store_dir: &Path, bounds: Bounds) -> Self {
        Self {
            root: store_dir.join("capture-spool"),
            bounds,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Ready records (one directory listing; nothing is opened).
    pub fn ready(&self, limit: usize) -> io::Result<Vec<ReadyRecord>> {
        let mut records = Vec::new();
        let entries = match fs::read_dir(self.dir("ready")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(records),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            if let Some((observation_id, bytes)) = name.to_str().and_then(parse_ready_name) {
                records.push(ReadyRecord {
                    path: entry.path(),
                    observation_id,
                    bytes,
                });
            }
        }
        records.sort_by(|a, b| a.path.cmp(&b.path));
        records.truncate(limit);
        Ok(records)
    }

    fn totals(&self) -> io::Result<(usize, u64)> {
        let records = self.ready(usize::MAX)?;
        Ok((records.len(), records.iter().map(|r| r.bytes).sum()))
    }

    /// Records a capture that could not even be spooled (malformed or
    /// oversized input, an unsafe identifier): saturation/loss diagnostics.
    pub fn record_drop(&self, observation_id: &str, reason: &str) {
        self.mark_dropped(observation_id, reason);
    }

    fn mark_dropped(&self, observation_id: &str, native_event: &str) {
        let dir = self.dir("dropped");
        if private_dir(&dir).is_err() {
            return;
        }
        let markers = fs::read_dir(&dir).map(Iterator::count).unwrap_or(usize::MAX);
        if markers >= self.bounds.max_markers {
            return;
        }
        let event: String = native_event
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(48)
            .collect();
        let _ = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join(format!("{observation_id}.{event}")));
    }

    /// Publishes one envelope atomically under its observation UUID.
    pub fn publish(&self, envelope: &ObservationEnvelope) -> Result<PathBuf, SpoolError> {
        if !valid_uuid(&envelope.observation_id) {
            return Err(SpoolError::InvalidId);
        }
        let body = serde_json::to_vec(envelope).map_err(|e| SpoolError::Io(io::Error::other(e)))?;
        if body.len() > OBSERVATION_MAX_BYTES {
            return Err(SpoolError::TooLarge);
        }
        let (records, bytes) = self.totals()?;
        let size = body.len() as u64;
        // Compared in u128: the bound times a percentage cannot overflow.
        let over = |records: usize, bytes: u64, percent: u64| {
            (records as u128 + 1) * 100 > self.bounds.max_records as u128 * u128::from(percent)
                || (u128::from(bytes) + u128::from(size)) * 100
                    > u128::from(self.bounds.max_bytes) * u128::from(percent)
        };
        let saturated = over(records, bytes, 100)
            || (Priority::of(&envelope.native_event) == Priority::Activity
                && over(records, bytes, SPOOL_LOW_PRIORITY_PERCENT));
        if saturated {
            self.mark_dropped(&envelope.observation_id, &envelope.native_event);
            return Err(SpoolError::Saturated);
        }
        let pending = self.dir("pending");
        let ready = self.dir("ready");
        private_dir(&pending)?;
        private_dir(&ready)?;
        let temporary = pending.join(format!("{}.tmp", envelope.observation_id));
        let target = ready.join(format!("{}.{}.json", envelope.observation_id, body.len()));
        if target.exists() {
            return Ok(target);
        }
        let _ = fs::remove_file(&temporary);
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&body)?;
            file.sync_all()?;
        }
        fs::rename(&temporary, &target)?;
        sync_dir(&ready)?;
        Ok(target)
    }

    /// Reads a ready record (bounded).
    pub fn read(&self, record: &ReadyRecord) -> Result<ObservationEnvelope, SpoolError> {
        let mut body = Vec::new();
        fs::File::open(&record.path)?
            .take(OBSERVATION_MAX_BYTES as u64 + 1)
            .read_to_end(&mut body)?;
        if body.len() > OBSERVATION_MAX_BYTES {
            return Err(SpoolError::TooLarge);
        }
        serde_json::from_slice(&body).map_err(|e| SpoolError::Io(io::Error::other(e)))
    }

    /// Removes a record after its durable journal receipt.
    pub fn remove(&self, record: &ReadyRecord) -> io::Result<()> {
        match fs::remove_file(&record.path) {
            Ok(()) => sync_dir(&self.dir("ready")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Moves a record that cannot be admitted aside, keeping it for diagnosis.
    pub fn quarantine(&self, record: &ReadyRecord, reason: &str) -> io::Result<PathBuf> {
        let reason: String = reason
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .take(48)
            .collect();
        let dir = self.dir("quarantine").join(reason);
        private_dir(&dir)?;
        let target = dir.join(format!("{}.json", record.observation_id));
        fs::rename(&record.path, &target)?;
        Ok(target)
    }

    /// The saturation markers (one per dropped record), left in place until
    /// the loss they stand for is recorded ([`Spool::clear_dropped`]).
    pub fn dropped(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        let entries = match fs::read_dir(self.dir("dropped")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(names),
            Err(error) => return Err(error),
        };
        for entry in entries {
            if let Some(name) = entry?.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Removes markers whose loss has been recorded.
    pub fn clear_dropped(&self, names: &[String]) {
        let dir = self.dir("dropped");
        for name in names {
            let _ = fs::remove_file(dir.join(name));
        }
    }

    /// Ready records older than `max_age`. Once their loss is recorded they
    /// go to `quarantine/expired` ([`Spool::quarantine`]); they are never
    /// delivered.
    pub fn expired(&self, max_age: Duration) -> io::Result<Vec<ReadyRecord>> {
        let now = SystemTime::now();
        Ok(self
            .ready(usize::MAX)?
            .into_iter()
            .filter(|record| {
                fs::metadata(&record.path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > max_age)
            })
            .collect())
    }

    /// Removes temporary files a killed writer abandoned.
    pub fn sweep_pending(&self, older_than: Duration) -> io::Result<usize> {
        let now = SystemTime::now();
        let mut removed = 0;
        let entries = match fs::read_dir(self.dir("pending")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > older_than);
            if stale && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn stats(&self) -> SpoolStats {
        let count = |name: &str| fs::read_dir(self.dir(name)).map(Iterator::count).unwrap_or(0);
        let (ready_records, ready_bytes) = self.totals().unwrap_or((0, 0));
        let quarantined = fs::read_dir(self.dir("quarantine"))
            .map(|dirs| {
                dirs.filter_map(Result::ok)
                    .map(|d| fs::read_dir(d.path()).map(Iterator::count).unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0);
        SpoolStats {
            ready_records,
            ready_bytes,
            quarantined,
            dropped_markers: count("dropped"),
            pending_files: count("pending"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{HookCapture, claude_hook_envelope, local_clock, sample_hook_input};

    fn envelope(index: u32, event: &str) -> ObservationEnvelope {
        claude_hook_envelope(
            &sample_hook_input(event, "session-1"),
            HookCapture {
                observation_id: format!("00000000-0000-4000-8000-{index:012}"),
                profile_ref: "claude-cli:~/.claude".into(),
                clock: local_clock(None, 1, None),
                evidence: Vec::new(),
            },
        )
        .expect("envelope")
    }

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ts-spool-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn publication_is_atomic_and_idempotent_by_uuid() {
        let dir = temp();
        let spool = Spool::at(&dir);
        let record = envelope(1, "Stop");
        let path = spool.publish(&record).expect("publish");
        assert_eq!(spool.publish(&record).expect("again"), path, "same UUID, same record");
        let ready = spool.ready(10).expect("ready");
        assert_eq!(ready.len(), 1);
        assert_eq!(spool.read(&ready[0]).expect("read"), record);
        assert_eq!(fs::read_dir(spool.root().join("pending")).expect("pending").count(), 0);
        spool.remove(&ready[0]).expect("remove");
        assert!(spool.ready(10).expect("ready").is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn bounds_drop_activity_first_and_mark_every_drop() {
        let dir = temp();
        let spool = Spool::with_bounds(&dir, Bounds { max_records: 10, max_bytes: u64::MAX, max_markers: 100 });
        for index in 0..8 {
            spool.publish(&envelope(index, "Stop")).expect("essential");
        }
        assert!(matches!(spool.publish(&envelope(100, "PreToolUse")), Err(SpoolError::Saturated)), "activity refused at 80%");
        spool.publish(&envelope(8, "Stop")).expect("essential still fits");
        spool.publish(&envelope(9, "Stop")).expect("essential up to the bound");
        assert!(matches!(spool.publish(&envelope(10, "Stop")), Err(SpoolError::Saturated)));
        assert_eq!(spool.stats().ready_records, 10, "never above the bound");
        let dropped = spool.dropped().expect("markers");
        assert_eq!(dropped.len(), 2, "{dropped:?}");
        spool.clear_dropped(&dropped);
        assert_eq!(spool.stats().dropped_markers, 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn only_the_canonical_lowercase_uuid_is_spooled() {
        let dir = temp();
        let spool = Spool::at(&dir);
        let mut record = envelope(1, "Stop");
        record.observation_id = record.observation_id.to_ascii_uppercase().replace("0000-4000", "ABCD-4000");
        assert!(matches!(spool.publish(&record), Err(SpoolError::InvalidId)), "receipts name the lowercase form");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_writes_are_invisible_and_swept() {
        let dir = temp();
        let spool = Spool::at(&dir);
        private_dir(&spool.root().join("pending")).expect("pending");
        fs::write(spool.root().join("pending/abandoned.tmp"), b"{\"partial").expect("partial");
        assert!(spool.ready(10).expect("ready").is_empty(), "readers ignore temporaries");
        assert_eq!(spool.sweep_pending(Duration::ZERO).expect("sweep"), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
