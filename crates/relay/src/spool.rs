//! The bounded local capture spool (SPEC §8.4):
//!
//! ```text
//! capture-spool/
//!   quota.lock
//!   pending/<observation-uuid>.<writer-nonce>.tmp
//!   ready/<observation-uuid>.<bytes>.json
//!   quarantine/<reason>/<observation-uuid>.json
//!   dropped/<observation-uuid>.<event-or-reason>
//! ```
//!
//! A record is written to a temporary file of its writer's own, synced and
//! closed; then, under the quota lock, `ready/` is recounted, the bounds are
//! checked and the temporary is renamed into `ready/`; the directory is
//! synced after the lock is released. Readers never see a partial record.
//!
//! Every hook invocation is its own process, so the quota is decided across
//! processes: `quota.lock` is an advisory `flock` held only around the
//! recount, the decision and the rename, which makes them one step for every
//! competing publisher. The kernel releases it when its holder exits or
//! dies, so a killed publisher leaves no reservation behind. A publisher
//! waits for it at most [`QUOTA_WAIT`]; one that cannot take it in time does
//! not publish and leaves a `spoolbusy` drop marker.
//!
//! The record bound counts ready records from the listing; the byte bound
//! sums their real sizes on disk (one `lstat` each), not the sizes their
//! names claim. Records are counted first, so a spool at its record bound
//! refuses without the size pass. When a bound is reached the record is not
//! published; a zero-byte marker in `dropped/` names it for saturation
//! diagnostics (itself bounded). Optional activity detail is refused first.
//! Temporaries in `pending/` are never counted. A completed publication
//! survives an ordinary process crash; power-loss durability is not claimed
//! beyond what the platform's fsync provides.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_contracts::limits::capture::{
    OBSERVATION_MAX_BYTES, SPOOL_LOW_PRIORITY_PERCENT, SPOOL_MAX_BYTES, SPOOL_MAX_MARKERS,
    SPOOL_MAX_RECORDS,
};

/// The longest a publisher waits for the quota lock. The hook lives at most
/// `WALL_BUDGET_MS` (250 ms), of which a failed delivery may already have
/// spent `CONNECT_BUDGET_MS + RECEIPT_BUDGET_MS` (95 ms); the lock is held
/// only for one listing, the size pass and a rename (about 2 ms at 1,000
/// ready records), so this covers a burst of concurrent publishers and
/// leaves the rest of the budget for the write, the recount and the exit.
pub const QUOTA_WAIT: Duration = Duration::from_millis(50);

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
    /// The quota lock was not obtained within [`QUOTA_WAIT`].
    Busy,
    InvalidId,
    Io(io::Error),
}

impl std::fmt::Display for SpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("record exceeds the observation bound"),
            Self::Saturated => f.write_str("spool bound reached"),
            Self::Busy => f.write_str("spool quota lock not obtained in time"),
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
    /// The size the record's name claims.
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpoolStats {
    pub ready_records: usize,
    /// Real bytes on disk, as the byte bound counts them.
    pub ready_bytes: u64,
    pub quarantined: usize,
    pub dropped_markers: usize,
    pub pending_files: usize,
}

pub struct Spool {
    root: PathBuf,
    bounds: Bounds,
}

/// The quota decision for one record, made under the quota lock.
enum Admission {
    /// A record with this observation UUID is already published.
    Existing(PathBuf),
    Admit,
    Refuse,
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

/// Creates `path` exclusively, writes `body` and syncs it.
fn write_synced(path: &Path, body: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(body)?;
    file.sync_all()
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

/// The bytes a file holds (`st_size`); zero once it is gone, as when the
/// drainer removed it after the listing.
fn real_size(path: &Path) -> io::Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
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

    /// Ready records and their real bytes on disk.
    fn totals(&self) -> io::Result<(usize, u64)> {
        let records = self.ready(usize::MAX)?;
        let mut bytes = 0;
        for record in &records {
            bytes += real_size(&record.path)?;
        }
        Ok((records.len(), bytes))
    }

    /// Takes the quota lock, waiting at most [`QUOTA_WAIT`]; `None` when it
    /// stays held elsewhere. Dropping the file releases it, and so does the
    /// kernel when this process dies.
    fn lock_quota(&self) -> io::Result<Option<fs::File>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(self.root.join("quota.lock"))?;
        let deadline = Instant::now() + QUOTA_WAIT;
        // Holds are short, so a waiter polls often enough to catch the gaps.
        let mut pause = Duration::from_micros(250);
        loop {
            // SAFETY: `file` is an open descriptor for the call's duration.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Some(file));
            }
            let error = io::Error::last_os_error();
            if !matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) {
                return Err(error);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            std::thread::sleep(pause.min(left));
            pause = (pause * 2).min(Duration::from_millis(1));
        }
    }

    /// Recounts `ready/` and decides whether one more record of `size`
    /// bytes fits within `percent` of the bounds. Call with the quota lock
    /// held. The record count comes from the listing alone; real sizes are
    /// read only when the count admits, and only until the byte bound is
    /// shown to be exceeded.
    fn admission(&self, observation_id: &str, size: u64, percent: u64) -> io::Result<Admission> {
        let mut records = Vec::new();
        let entries = match fs::read_dir(self.dir("ready")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Admission::Admit),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let Some((id, _)) = entry.file_name().to_str().and_then(parse_ready_name) else {
                continue;
            };
            if id == observation_id {
                return Ok(Admission::Existing(entry.path()));
            }
            records.push(entry.path());
        }
        // Compared in u128: the bound times a percentage cannot overflow.
        if (records.len() as u128 + 1) * 100 > self.bounds.max_records as u128 * u128::from(percent) {
            return Ok(Admission::Refuse);
        }
        let limit = u128::from(self.bounds.max_bytes) * u128::from(percent);
        let mut bytes = u128::from(size);
        for path in &records {
            bytes += u128::from(real_size(path)?);
            if bytes * 100 > limit {
                return Ok(Admission::Refuse);
            }
        }
        Ok(if bytes * 100 > limit { Admission::Refuse } else { Admission::Admit })
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

    /// Publishes one envelope atomically under its observation UUID; a
    /// record already published under that UUID is returned as it is.
    pub fn publish(&self, envelope: &ObservationEnvelope) -> Result<PathBuf, SpoolError> {
        if !valid_uuid(&envelope.observation_id) {
            return Err(SpoolError::InvalidId);
        }
        let body = serde_json::to_vec(envelope).map_err(|e| SpoolError::Io(io::Error::other(e)))?;
        if body.len() > OBSERVATION_MAX_BYTES {
            return Err(SpoolError::TooLarge);
        }
        let pending = self.dir("pending");
        let ready = self.dir("ready");
        private_dir(&pending)?;
        private_dir(&ready)?;
        // Written and synced before the quota lock is taken, so the lock
        // covers only the recount, the decision and the rename. The name is
        // this writer's own: a concurrent publisher of the same UUID never
        // touches it.
        let nonce = uuid::Uuid::new_v4().simple();
        let temporary = pending.join(format!("{}.{nonce}.tmp", envelope.observation_id));
        let target = ready.join(format!("{}.{}.json", envelope.observation_id, body.len()));
        let published = write_synced(&temporary, &body)
            .map_err(SpoolError::from)
            .and_then(|()| self.admit(envelope, &temporary, &target, body.len() as u64));
        // Gone once renamed; otherwise this writer's own and never accepted.
        let _ = fs::remove_file(&temporary);
        let path = published?;
        sync_dir(&ready)?;
        Ok(path)
    }

    /// The quota decision and the publication as one step for every
    /// competing publisher: under the quota lock, recount `ready/`, decide,
    /// and rename the synced temporary into place. A refusal's marker is
    /// written after the lock is released, keeping the lock short.
    fn admit(
        &self,
        envelope: &ObservationEnvelope,
        temporary: &Path,
        target: &Path,
        size: u64,
    ) -> Result<PathBuf, SpoolError> {
        let Some(lock) = self.lock_quota()? else {
            self.mark_dropped(&envelope.observation_id, "spool-busy");
            return Err(SpoolError::Busy);
        };
        let percent = match Priority::of(&envelope.native_event) {
            Priority::Essential => 100,
            Priority::Activity => SPOOL_LOW_PRIORITY_PERCENT,
        };
        let admission = self.admission(&envelope.observation_id, size, percent)?;
        if let Admission::Admit = admission {
            fs::rename(temporary, target)?;
        }
        drop(lock);
        match admission {
            Admission::Existing(path) => Ok(path),
            Admission::Admit => Ok(target.to_path_buf()),
            Admission::Refuse => {
                self.mark_dropped(&envelope.observation_id, &envelope.native_event);
                Err(SpoolError::Saturated)
            }
        }
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
    fn the_byte_bound_counts_real_bytes_not_name_claims() {
        let dir = temp();
        let size = serde_json::to_vec(&envelope(2, "Stop")).expect("json").len() as u64;
        let spool = Spool::with_bounds(&dir, Bounds { max_records: 100, max_bytes: 4 * size, max_markers: 100 });
        // A record whose name claims 10 bytes but which holds 4 records' worth.
        let ready = spool.root().join("ready");
        private_dir(&ready).expect("ready");
        fs::write(ready.join(format!("{}.10.json", envelope(1, "Stop").observation_id)), vec![b' '; 4 * size as usize])
            .expect("understated record");
        assert!(matches!(spool.publish(&envelope(2, "Stop")), Err(SpoolError::Saturated)), "the real bytes fill the bound");
        assert_eq!(spool.stats().ready_bytes, 4 * size, "stats report the real bytes");
        // And a name claiming far more than the file holds does not refuse.
        fs::remove_file(ready.join(format!("{}.10.json", envelope(1, "Stop").observation_id))).expect("remove");
        fs::write(ready.join(format!("{}.999999999.json", envelope(1, "Stop").observation_id)), b"{}").expect("overstated record");
        spool.publish(&envelope(2, "Stop")).expect("two real bytes leave room");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_uuid_is_published_once_whatever_the_body() {
        let dir = temp();
        let spool = Spool::at(&dir);
        let path = spool.publish(&envelope(1, "Stop")).expect("publish");
        let mut changed = envelope(1, "SessionStart");
        changed.native_event = "SessionStart".into();
        assert_eq!(spool.publish(&changed).expect("again"), path, "the first record stands");
        assert_eq!(spool.ready(10).expect("ready").len(), 1);
        assert_eq!(fs::read_dir(spool.root().join("pending")).expect("pending").count(), 0, "no temporary left");
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
