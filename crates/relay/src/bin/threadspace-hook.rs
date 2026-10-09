//! `threadspace-hook`: the fail-open capture executable (SPEC §2.1, §8.2).
//!
//! ```text
//! threadspace-hook hook      (--agent <bundle id> | --store-dir <dir>) [--home <dir>]
//! threadspace-hook mod-batch (--agent <bundle id> | --store-dir <dir>)
//! ```
//!
//! `hook` is the conventional provider hook: it reads the provider's JSON on
//! stdin, captures a sanitized envelope with a stable observation UUID and
//! its own validated ancestry, delivers it to the companion's event socket
//! and waits for a durable receipt within the receipt budget, and otherwise
//! publishes it to the local spool. It prints nothing, writes no stderr and
//! exits 0 on every path, so capture failure can never become a provider
//! decision. A watchdog ends the process at the wall budget.
//!
//! `mod-batch [--budget-ms <ms>]` reads the observer mod's batch on stdin
//! and prints one typed receipt (`ModBatchReceipt`) for the calling mod, with
//! exactly one result per submitted record (`modbatch`); exit status alone is
//! never acceptance. The mod passes the time its host allows the call, so
//! the receipt is printed before the host stops waiting.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use threadspace_contracts::canonical::capture::{
    CAPTURE_PROTOCOL_VERSION, CaptureBatch, CaptureReply, ModBatchReceipt, RecordReceipt, RecordStatus,
};
use threadspace_contracts::canonical::envelope::{ObservationEnvelope, ProcessRole, ProcessSample};
use threadspace_contracts::limits::capture::{
    CONNECT_BUDGET_MS, FRAME_MAX_BYTES, RAW_INPUT_MAX_BYTES, RECEIPT_BUDGET_MS,
    RECEIPT_MAX_BYTES, WALL_BUDGET_MS,
};
use threadspace_contracts::route::ProcessKey;
use threadspace_relay::capture::{HookCapture, claude_hook_envelope, local_clock, parse_bounded};
use threadspace_relay::events::send;
use threadspace_relay::locator;
use threadspace_relay::modbatch::{self, BatchContext, Sink};
use threadspace_relay::paths::{AgentPaths, claude_profile_ref, home_dir};
use threadspace_relay::spool::Spool;
use threadspace_surfaces_macos::{ancestry, process};

const ANCESTRY_LINKS: usize = 24;

fn quiet_exit() -> ! {
    // SAFETY: `_exit` has no preconditions; it skips destructors and buffered
    // output on purpose, so nothing is printed on the way out.
    unsafe { libc::_exit(0) }
}

/// Ends the process at the wall budget whatever it is doing.
fn watchdog(budget: Duration) {
    let _ = std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            std::thread::sleep(budget);
            quiet_exit();
        });
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn store_dir(args: &[String]) -> Option<PathBuf> {
    if let Some(dir) = arg(args, "--store-dir") {
        return Some(PathBuf::from(dir));
    }
    AgentPaths::for_agent(&arg(args, "--agent")?).map(|paths| paths.store_dir)
}

fn read_stdin(limit: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn monotonic_ns() -> Option<u64> {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `now` is a valid, writable timespec.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut now) };
    (rc == 0).then(|| now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64)
}

/// This process and its validated ancestors, as capture evidence.
fn own_evidence(boot_id: &str) -> Vec<ProcessSample> {
    own_evidence_links(boot_id, ANCESTRY_LINKS)
}

fn own_evidence_links(boot_id: &str, links: usize) -> Vec<ProcessSample> {
    let walk = ancestry::walk(&ancestry::KernelSampler, std::process::id() as i32, links);
    walk.chain
        .iter()
        .enumerate()
        .map(|(index, incarnation)| {
            let sample = &incarnation.sample;
            ProcessSample {
                role: if index == 0 { ProcessRole::Capture } else { ProcessRole::Ancestor },
                key: ProcessKey {
                    endpoint_id: String::new(),
                    boot_id: boot_id.to_owned(),
                    pid: sample.pid.max(0) as u32,
                    start_seconds: sample.start_seconds.to_string(),
                    start_microseconds: sample.start_microseconds,
                },
                parent_pid: Some(sample.ppid.max(0) as u32),
                executable: Some(incarnation.executable.canonical()),
                controlling_device: sample.controlling_device,
            }
        })
        .collect()
}

/// This process and the parent that ran it, the provider, as mod-batch
/// evidence (kernel samples, never the mod's claims).
fn provider_evidence(boot_id: &str) -> Vec<ProcessSample> {
    let mut samples = own_evidence_links(boot_id, 2);
    if let Some(parent) = samples.get_mut(1) {
        parent.role = ProcessRole::Provider;
    }
    samples
}

/// Delivers a batch to the event socket within the receipt budget; `None`
/// when no committed receipt arrived in time.
fn deliver(store: &Path, records: &[ObservationEnvelope], started: Instant) -> Option<Vec<RecordReceipt>> {
    let locator = locator::read(&store.join("runtime-locator.json")).ok()?;
    let socket = PathBuf::from(locator.events_socket?);
    let deadline = started + Duration::from_millis(CONNECT_BUDGET_MS + RECEIPT_BUDGET_MS);
    let batch = CaptureBatch {
        protocol_version: CAPTURE_PROTOCOL_VERSION,
        records: records.to_vec(),
    };
    match send(&socket, &batch, deadline).ok()? {
        CaptureReply::Receipts { receipts, .. } => Some(receipts),
        CaptureReply::Refused { .. } => None,
    }
}

fn hook(args: &[String], started: Instant) {
    let Some(store) = store_dir(args) else { return };
    let spool = Spool::at(&store);
    let observation_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let Some(bytes) = read_stdin(RAW_INPUT_MAX_BYTES) else {
        spool.record_drop(&observation_id, "oversize");
        return;
    };
    let input = match parse_bounded(&bytes) {
        Ok(input) => input,
        Err(error) => {
            spool.record_drop(&observation_id, error.code());
            return;
        }
    };
    let home = arg(args, "--home").map(PathBuf::from).or_else(home_dir);
    let Some(home) = home else { return };
    let boot = process::boot_session_id().ok();
    let capture = HookCapture {
        observation_id: observation_id.clone(),
        profile_ref: claude_profile_ref(&home),
        clock: local_clock(boot.clone(), now_ms(), monotonic_ns()),
        evidence: own_evidence(boot.as_deref().unwrap_or_default()),
    };
    let envelope = match claude_hook_envelope(&input, capture) {
        Ok(envelope) => envelope,
        Err(error) => {
            spool.record_drop(&observation_id, error.code());
            return;
        }
    };
    let committed = deliver(&store, std::slice::from_ref(&envelope), started).is_some_and(|receipts| {
        receipts.iter().any(|r| {
            r.observation_id == observation_id
                && matches!(r.status, RecordStatus::Committed | RecordStatus::AlreadyCommitted)
        })
    });
    if !committed {
        let _ = spool.publish(&envelope);
    }
}

/// The companion's event socket, then the local spool.
struct LiveSink {
    socket: Option<PathBuf>,
    spool: Spool,
}

impl Sink for LiveSink {
    fn deliver(&mut self, frame: &[ObservationEnvelope], deadline: Instant) -> Option<Vec<RecordReceipt>> {
        let socket = self.socket.as_ref()?;
        let batch = CaptureBatch {
            protocol_version: CAPTURE_PROTOCOL_VERSION,
            records: frame.to_vec(),
        };
        match send(socket, &batch, deadline).ok()? {
            CaptureReply::Receipts { receipts, .. } => Some(receipts),
            CaptureReply::Refused { .. } => None,
        }
    }

    fn spool(&mut self, envelope: &ObservationEnvelope) -> bool {
        self.spool.publish(envelope).is_ok()
    }
}

/// A qualification-only delivery fault, armed once:
/// `THREADSPACE_QUALIFY_MOD_BATCH_FAULT=<kind>:<arming file>` fires `kind` on
/// the first batch that finds the arming file and removes it, so the mod's
/// retry meets a healthy helper. `partial` commits the first half then lets
/// the caller time out, `slow` answers after the budget, `malformed` prints
/// an invalid receipt and `exit1` exits 1 after a valid one. Release builds
/// have none.
#[cfg(feature = "qualification")]
fn fault() -> Option<String> {
    let armed = std::env::var("THREADSPACE_QUALIFY_MOD_BATCH_FAULT").ok()?;
    let (kind, file) = armed.split_once(':')?;
    std::fs::remove_file(file).ok()?;
    Some(kind.to_owned())
}

#[cfg(not(feature = "qualification"))]
fn fault() -> Option<String> {
    None
}

/// The answer and the exit status to leave with.
fn mod_batch(args: &[String], started: Instant) -> (Option<Vec<u8>>, i32) {
    let budget = arg(args, "--budget-ms")
        .and_then(|ms| ms.parse::<u64>().ok())
        .map_or(WALL_BUDGET_MS, |ms| ms.min(WALL_BUDGET_MS));
    let fault = fault();
    let late = || std::thread::sleep(Duration::from_millis(budget + 50));
    if fault.as_deref() == Some("slow") {
        late();
    }
    let Some(store) = store_dir(args) else { return (None, 0) };
    let Some(stdin) = read_stdin(2 * FRAME_MAX_BYTES) else { return (None, 0) };
    let home = arg(args, "--home").map(PathBuf::from).or_else(home_dir);
    let Some(home) = home else { return (None, 0) };
    let boot = process::boot_session_id().ok();
    let context = BatchContext {
        profile_ref: claude_profile_ref(&home),
        clock: local_clock(boot.clone(), now_ms(), monotonic_ns()),
        evidence: provider_evidence(boot.as_deref().unwrap_or_default()),
    };
    let socket = locator::read(&store.join("runtime-locator.json"))
        .ok()
        .and_then(|locator| locator.events_socket)
        .map(PathBuf::from);
    let mut sink = LiveSink {
        socket,
        spool: Spool::at(&store),
    };
    // The companion round trips take at most the capture receipt budget, and
    // half the caller's, leaving the rest for the spool.
    let delivery = started + Duration::from_millis((CONNECT_BUDGET_MS + RECEIPT_BUDGET_MS).min(budget / 2));
    let deadline = started + Duration::from_millis(budget.saturating_sub(10));
    if fault.as_deref() == Some("partial") {
        let mut request: serde_json::Value = serde_json::from_slice(&stdin).unwrap_or_default();
        if let Some(records) = request.get_mut("records").and_then(serde_json::Value::as_array_mut) {
            let keep = records.len().div_ceil(2);
            records.truncate(keep);
        }
        let half = serde_json::to_vec(&request).unwrap_or_default();
        let _ = modbatch::answer(&half, &context, &mut sink, delivery, delivery);
        late();
        return (None, 0);
    }
    let receipt: ModBatchReceipt = modbatch::answer(&stdin, &context, &mut sink, delivery, deadline);
    match fault.as_deref() {
        Some("malformed") => (Some(br#"{"receiptVersion":1,"results":"malformed"}"#.to_vec()), 0),
        Some("exit1") => (serde_json::to_vec(&receipt).ok(), 1),
        _ => (serde_json::to_vec(&receipt).ok(), 0),
    }
}

fn main() {
    let started = Instant::now();
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().collect();
    watchdog(Duration::from_millis(WALL_BUDGET_MS));
    match args.get(1).map(String::as_str) {
        Some("mod-batch") => {
            let (text, status) = std::panic::catch_unwind(|| mod_batch(&args, started)).unwrap_or((None, 0));
            if let Some(text) = text.filter(|text| text.len() <= RECEIPT_MAX_BYTES) {
                // Flushed here: the exit below discards buffered output.
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(&text).and_then(|()| stdout.flush());
            }
            if status != 0 {
                // SAFETY: as in `quiet_exit`; only the qualification `exit1`
                // fault leaves with a non-zero status.
                unsafe { libc::_exit(status) }
            }
        }
        _ => {
            // The conventional hook, and any unrecognized invocation: silent.
            let _ = std::panic::catch_unwind(|| hook(&args, started));
        }
    }
    quiet_exit();
}
