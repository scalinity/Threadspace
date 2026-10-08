//! Spool capacity across processes (SPEC §8.4). Every provider hook runs as
//! its own short-lived process, so the spool's bounds must hold between
//! processes: the quota decision and the publication are one step for every
//! competing publisher. The test binary re-executes itself as publisher and
//! lock-holder processes; publishers are released together through their
//! stdin against temporary spools already at the edge of a bound.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use threadspace_contracts::canonical::envelope::ObservationEnvelope;
use threadspace_relay::capture::{HookCapture, claude_hook_envelope, local_clock, sample_hook_input};
use threadspace_relay::spool::{Bounds, Spool};

const ROLE_ENV: &str = "THREADSPACE_SPOOL_CHILD_ROLE";
const STORE_ENV: &str = "THREADSPACE_SPOOL_CHILD_STORE";
const INDEX_ENV: &str = "THREADSPACE_SPOOL_CHILD_INDEX";
const BOUNDS_ENV: &str = "THREADSPACE_SPOOL_CHILD_BOUNDS";
const PUBLISHERS: u32 = 16;
/// The spool's quota lock; the holder child takes it exactly as a publisher does.
const QUOTA_LOCK: &str = "capture-spool/quota.lock";
/// A holder that is never killed ends by itself.
const HOLD_CAP: Duration = Duration::from_secs(20);
/// The hook's wall budget: a refused publication still ends inside it.
const WALL_BUDGET: Duration = Duration::from_millis(250);

fn observation_id(index: u32) -> String {
    format!("00000000-0000-4000-8000-{index:012}")
}

fn envelope(index: u32) -> ObservationEnvelope {
    claude_hook_envelope(
        &sample_hook_input("Stop", "session-1"),
        HookCapture {
            observation_id: observation_id(index),
            profile_ref: "claude-cli:~/.claude".into(),
            clock: local_clock(None, 1, None),
            evidence: Vec::new(),
        },
    )
    .expect("envelope")
}

fn say(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}").and_then(|()| stdout.flush());
}

fn lock_file(store: &Path) -> fs::File {
    let path = store.join(QUOTA_LOCK);
    fs::create_dir_all(path.parent().expect("spool dir")).expect("spool dir");
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .expect("lock file")
}

/// Runs only as a re-executed child of the tests below.
#[test]
fn spool_child() {
    let Ok(role) = std::env::var(ROLE_ENV) else {
        return;
    };
    let store = PathBuf::from(std::env::var(STORE_ENV).expect("store"));
    if role == "hold" {
        let file = lock_file(&store);
        // SAFETY: `file` is an open descriptor for the call's duration.
        assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }, 0, "flock");
        say("@@HELD");
        std::thread::sleep(HOLD_CAP);
        return;
    }
    let index: u32 = std::env::var(INDEX_ENV).expect("index").parse().expect("index");
    let bounds: Vec<u64> = std::env::var(BOUNDS_ENV)
        .expect("bounds")
        .split(',')
        .map(|n| n.parse().expect("bound"))
        .collect();
    let spool = Spool::with_bounds(
        &store,
        Bounds { max_records: bounds[0] as usize, max_bytes: bounds[1], max_markers: bounds[2] as usize },
    );
    let record = envelope(index);
    say("@@READY");
    // The start barrier: one byte on stdin releases every publisher at once.
    let _ = std::io::stdin().read(&mut [0u8; 1]);
    let line = match spool.publish(&record) {
        Ok(path) => format!("@@RESULT Ok {}", path.file_name().and_then(|n| n.to_str()).unwrap_or_default()),
        Err(error) => format!("@@RESULT Err {error:?}"),
    };
    say(&line);
}

struct TempStore(PathBuf);

impl TempStore {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ts-spool-capacity-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("capture-spool/ready")).expect("store");
        Self(dir)
    }

    fn ready_dir(&self) -> PathBuf {
        self.0.join("capture-spool/ready")
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Proc {
    child: Child,
    stdout: BufReader<ChildStdout>,
}

impl Proc {
    fn spawn(role: &str, store: &Path, index: u32, bounds: Bounds) -> Self {
        let mut child = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "spool_child", "--nocapture"])
            .env(ROLE_ENV, role)
            .env(STORE_ENV, store)
            .env(INDEX_ENV, index.to_string())
            .env(BOUNDS_ENV, format!("{},{},{}", bounds.max_records, bounds.max_bytes, bounds.max_markers))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        Self { child, stdout }
    }

    /// Reads until a line carrying `token`; false when the output ends first.
    fn wait_for(&mut self, token: &str) -> bool {
        let mut line = String::new();
        loop {
            line.clear();
            match self.stdout.read_line(&mut line) {
                Ok(0) | Err(_) => return false,
                Ok(_) if line.contains(token) => return true,
                Ok(_) => {}
            }
        }
    }

    fn release(&mut self) {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"g");
        }
    }

    /// The rest of the output and the exit status.
    fn finish(&mut self) -> (String, std::process::ExitStatus) {
        let mut rest = String::new();
        let _ = self.stdout.read_to_string(&mut rest);
        (rest, self.child.wait().expect("wait"))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What a publisher reported: `Ok(file name)` or `Err(error, as Debug)`.
type Reported = Result<String, String>;

fn reported(output: &str) -> Option<Reported> {
    let line = output.lines().find_map(|l| l.find("@@RESULT ").map(|at| &l[at + 9..]))?;
    match line.split_once(' ') {
        Some(("Ok", name)) => Some(Ok(name.to_owned())),
        Some(("Err", error)) => Some(Err(error.to_owned())),
        _ => None,
    }
}

/// One accepted file in `ready/`, as the file system holds it.
struct Accepted {
    id: String,
    name: String,
    real_bytes: u64,
}

fn accepted(store: &TempStore) -> Vec<Accepted> {
    let mut files: Vec<Accepted> = fs::read_dir(store.ready_dir())
        .expect("ready dir")
        .map(|entry| {
            let entry = entry.expect("entry");
            let name = entry.file_name().into_string().expect("name");
            Accepted {
                id: name.split('.').next().unwrap_or_default().to_owned(),
                real_bytes: entry.metadata().expect("metadata").len(),
                name,
            }
        })
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

fn markers(store: &TempStore) -> Vec<String> {
    fs::read_dir(store.0.join("capture-spool/dropped"))
        .map(|entries| entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// Ready records written as the spool writes them (real serialized
/// envelopes under `<uuid>.<bytes>.json`): name → body.
fn prefill(store: &TempStore, indices: std::ops::Range<u32>) -> BTreeMap<String, Vec<u8>> {
    indices
        .map(|index| {
            let body = serde_json::to_vec(&envelope(index)).expect("json");
            let name = format!("{}.{}.json", observation_id(index), body.len());
            fs::write(store.ready_dir().join(&name), &body).expect("prefill");
            (name, body)
        })
        .collect()
}

/// Starts one publisher process per index, releases them together, and
/// returns what each one reported.
fn race(store: &TempStore, bounds: Bounds, indices: std::ops::Range<u32>) -> Vec<(u32, Reported)> {
    let mut publishers: Vec<(u32, Proc)> =
        indices.map(|index| (index, Proc::spawn("publish", &store.0, index, bounds))).collect();
    for (index, publisher) in &mut publishers {
        assert!(publisher.wait_for("@@READY"), "publisher {index} never became ready");
    }
    for (_, publisher) in &mut publishers {
        publisher.release();
    }
    publishers
        .iter_mut()
        .map(|(index, publisher)| {
            let (output, status) = publisher.finish();
            assert!(status.success(), "publisher {index} ended {status:?}");
            (*index, reported(&output).unwrap_or_else(|| panic!("publisher {index} reported nothing: {output}")))
        })
        .collect()
}

fn is_refusal(error: &str) -> bool {
    error.starts_with("Saturated") || error.starts_with("Busy")
}

/// Every report is true of the spool: each publication is in `ready/` with
/// its own envelope, each refusal left no record and a drop marker, nothing
/// accepted before the race is gone or changed, and no UUID appears twice.
fn check_reports(store: &TempStore, outcomes: &[(u32, Reported)], before: &BTreeMap<String, Vec<u8>>) {
    let ready = accepted(store);
    let mut ids: Vec<&str> = ready.iter().map(|r| r.id.as_str()).collect();
    ids.dedup();
    assert_eq!(ids.len(), ready.len(), "one ready file per observation UUID");
    let markers = markers(store);
    let mut published = 0;
    for (index, outcome) in outcomes {
        let id = observation_id(*index);
        let file = ready.iter().find(|r| r.id == id);
        match outcome {
            Ok(name) => {
                published += 1;
                let file = file.unwrap_or_else(|| panic!("reported publication {name} is not in ready/"));
                assert_eq!(&file.name, name, "the reported path is the accepted file");
                let body = fs::read(store.ready_dir().join(name)).expect("read");
                let parsed: ObservationEnvelope = serde_json::from_slice(&body).expect("a whole envelope");
                assert_eq!(parsed, envelope(*index), "the accepted file is the publisher's record");
            }
            Err(error) => {
                assert!(is_refusal(error), "publisher {index}: a refusal is saturation or a busy quota, not {error}");
                assert!(file.is_none(), "publisher {index} was refused but its record is in ready/");
                assert!(
                    markers.iter().any(|m| m.starts_with(&format!("{id}."))),
                    "publisher {index}'s refusal left no drop marker"
                );
            }
        }
    }
    assert_eq!(ready.len(), before.len() + published, "ready/ holds the prefill plus exactly the reported publications");
    for (name, body) in before {
        assert_eq!(
            fs::read(store.ready_dir().join(name)).ok().as_ref(),
            Some(body),
            "accepted file {name} was removed or changed"
        );
    }
}

fn summary(outcomes: &[(u32, Reported)]) -> (usize, usize, usize) {
    let published = outcomes.iter().filter(|(_, o)| o.is_ok()).count();
    let busy = outcomes.iter().filter(|(_, o)| matches!(o, Err(e) if e.starts_with("Busy"))).count();
    (published, busy, outcomes.len() - published - busy)
}

#[test]
fn concurrent_publishers_never_pass_the_record_bound() {
    let store = TempStore::new("records");
    let bounds = Bounds { max_records: 1000, max_bytes: u64::MAX, max_markers: 10_000 };
    let before = prefill(&store, 0..999);
    let outcomes = race(&store, bounds, 1000..1000 + PUBLISHERS);
    let final_count = accepted(&store).len();
    let (published, busy, refused) = summary(&outcomes);
    eprintln!(
        "record bound 1000, 999 ready, {PUBLISHERS} publisher processes: \
         {published} published, {refused} saturated, {busy} busy; ready {final_count}"
    );
    assert!(final_count <= bounds.max_records, "ready/ holds {final_count} records, above the bound");
    assert!(published <= 1, "{published} publishers succeeded with room for one");
    check_reports(&store, &outcomes, &before);

    // The bound is exact: one more record fits only if none was published.
    let next = format!("{:?}", Spool::with_bounds(&store.0, bounds).publish(&envelope(2000)));
    if published == 1 {
        assert!(next.starts_with("Err(Saturated"), "{next}");
    } else {
        assert!(next.starts_with("Ok("), "{next}");
    }
}

#[test]
fn concurrent_publishers_never_pass_the_byte_bound() {
    let store = TempStore::new("bytes");
    let before = prefill(&store, 0..50);
    let used: u64 = accepted(&store).iter().map(|r| r.real_bytes).sum();
    let size = serde_json::to_vec(&envelope(1000)).expect("json").len() as u64;
    // Room for one more record, not two.
    let bounds = Bounds { max_records: 10_000, max_bytes: used + size + size / 2, max_markers: 10_000 };
    // An abandoned temporary bigger than the room left is never counted.
    let pending = store.0.join("capture-spool/pending");
    fs::create_dir_all(&pending).expect("pending");
    fs::write(pending.join(format!("{}.tmp", observation_id(3000))), vec![b'x'; 4 * size as usize]).expect("temporary");

    let outcomes = race(&store, bounds, 1000..1000 + PUBLISHERS);
    let real: u64 = accepted(&store).iter().map(|r| r.real_bytes).sum();
    let (published, busy, refused) = summary(&outcomes);
    eprintln!(
        "byte bound {} (ready {used} + room for one {size}-byte record), {PUBLISHERS} publisher processes: \
         {published} published, {refused} saturated, {busy} busy; ready {real} real bytes",
        bounds.max_bytes
    );
    assert!(real <= bounds.max_bytes, "ready/ holds {real} real bytes, above the {} bound", bounds.max_bytes);
    assert!(published <= 1, "{published} publishers succeeded with room for one");
    check_reports(&store, &outcomes, &before);

    let next = format!("{:?}", Spool::with_bounds(&store.0, bounds).publish(&envelope(2000)));
    if published == 1 {
        assert!(next.starts_with("Err(Saturated"), "{next}");
    } else {
        assert!(next.starts_with("Ok("), "the pending temporary must not count: {next}");
    }
}

#[test]
fn a_publisher_killed_holding_the_quota_lock_leaves_no_reservation() {
    let store = TempStore::new("killed-holder");
    let bounds = Bounds { max_records: 3, max_bytes: u64::MAX, max_markers: 100 };
    let before = prefill(&store, 0..2);
    let spool = Spool::with_bounds(&store.0, bounds);

    let mut holder = Proc::spawn("hold", &store.0, 0, bounds);
    assert!(holder.wait_for("@@HELD"), "the holder took the quota lock");
    let started = Instant::now();
    let blocked = format!("{:?}", spool.publish(&envelope(10)));
    let waited = started.elapsed();
    eprintln!("publish while another process holds the quota lock: {blocked} after {waited:?}");
    assert!(blocked.starts_with("Err(Busy"), "a publisher that cannot take the lock refuses: {blocked}");
    assert!(waited < WALL_BUDGET, "the refusal took {waited:?}");
    assert!(accepted(&store).iter().all(|r| r.id != observation_id(10)), "nothing published without the lock");
    assert!(
        markers(&store).contains(&format!("{}.spoolbusy", observation_id(10))),
        "the busy refusal is recorded as a loss: {:?}",
        markers(&store)
    );

    holder.child.kill().expect("SIGKILL the holder");
    let (_, status) = holder.finish();
    assert_eq!(status.signal(), Some(libc::SIGKILL), "the holder died holding the lock");

    // The kernel released the lock with the process; the whole remaining
    // capacity is there, and the bound still holds after it.
    let started = Instant::now();
    let path = spool.publish(&envelope(11)).expect("a publisher succeeds after the holder died");
    eprintln!("publish after the holder was killed: Ok after {:?}", started.elapsed());
    assert_eq!(spool.publish(&envelope(11)).expect("again"), path, "same UUID, same record");
    let next = format!("{:?}", spool.publish(&envelope(12)));
    assert!(next.starts_with("Err(Saturated"), "the bound holds after the crash: {next}");
    assert_eq!(accepted(&store).len(), before.len() + 1);
}

#[test]
fn a_publisher_killed_waiting_for_the_quota_leaves_nothing_accepted() {
    let store = TempStore::new("killed-waiter");
    let bounds = Bounds { max_records: 10, max_bytes: u64::MAX, max_markers: 100 };
    let before = prefill(&store, 0..2);
    let waiter_id = observation_id(20);

    // This process holds the quota while a publisher process waits for it.
    let held = lock_file(&store.0);
    // SAFETY: `held` is an open descriptor for the call's duration.
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0, "flock");
    let mut waiter = Proc::spawn("publish", &store.0, 20, bounds);
    assert!(waiter.wait_for("@@READY"), "the publisher started");
    waiter.release();
    // Its synced temporary appears before it asks for the quota; it is
    // killed well inside its bounded wait.
    let pending = store.0.join("capture-spool/pending");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut temporary_seen = false;
    while Instant::now() < deadline && !temporary_seen {
        temporary_seen = fs::read_dir(&pending).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|e| e.file_name().to_string_lossy().starts_with(&waiter_id))
        });
        std::thread::sleep(Duration::from_micros(200));
    }
    std::thread::sleep(Duration::from_millis(15));
    waiter.child.kill().expect("SIGKILL the waiter");
    let (output, status) = waiter.finish();
    assert!(temporary_seen, "the publisher wrote its temporary");
    assert_eq!(status.signal(), Some(libc::SIGKILL), "the publisher died before it decided");
    assert!(reported(&output).is_none(), "the killed publisher reported nothing: {output}");
    assert!(accepted(&store).iter().all(|r| r.id != waiter_id), "no partial or undecided record was accepted");
    assert!(markers(&store).iter().all(|m| !m.starts_with(&waiter_id)), "it was killed inside its wait, not after");
    drop(held);

    // Retried under the same UUID: published once, then idempotent; the
    // abandoned temporary is never counted and is swept as stale.
    let spool = Spool::with_bounds(&store.0, bounds);
    let path = spool.publish(&envelope(20)).expect("the retry publishes");
    assert_eq!(spool.publish(&envelope(20)).expect("again"), path, "same UUID, same record");
    let ready = accepted(&store);
    assert_eq!(ready.iter().filter(|r| r.id == waiter_id).count(), 1, "one record for the UUID");
    assert_eq!(ready.len(), before.len() + 1);
    assert_eq!(spool.stats().ready_records, before.len() + 1, "the abandoned temporary is not a record");
    assert_eq!(spool.sweep_pending(Duration::ZERO).expect("sweep"), 1, "the abandoned temporary is swept");
}
