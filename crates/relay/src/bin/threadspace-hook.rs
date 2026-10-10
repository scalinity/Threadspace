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
const OWNERSHIP_PROOF_BUDGET_MS: u64 = 2_000;

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

/// The parent that ran this process, the provider, as mod-batch evidence (a
/// kernel sample, never the mod's claim). This process's own sample changes
/// with every invocation, so it is not part of a record that may be retried.
fn provider_evidence(boot_id: &str) -> Vec<ProcessSample> {
    let mut samples = own_evidence_links(boot_id, 2);
    samples.retain_mut(|sample| {
        let parent = sample.role == ProcessRole::Ancestor;
        sample.role = ProcessRole::Provider;
        parent
    });
    samples
}

/// Off-provider-path ownership probe. The native helper creates the token
/// only after the actual parent passes the independent inventory/kernel
/// bracket, and publishes the proof before returning its correlation token.
/// The observer's seal and later native TurnStart are separate observations.
fn observer_proof(args: &[String]) -> Option<Vec<u8>> {
    use threadspace_provider_claude::inventory::ClaudeCli;
    use threadspace_provider_claude::ownership::{self, ProbeRequest};

    let store = store_dir(args)?;
    let home = arg(args, "--home").map(PathBuf::from).or_else(home_dir)?;
    let stdin = read_stdin(4 * 1024)?;
    let request: ProbeRequest = serde_json::from_slice(&stdin).ok()?;
    if !request.valid() {
        return None;
    }
    let boot = process::boot_session_id().ok()?;
    let provider = provider_evidence(&boot).into_iter().next()?;
    let image = provider.executable.as_deref()?;
    let binary = PathBuf::from(image.split_once('#').map_or(image, |(path, _)| path));
    let inventory = ClaudeCli {
        binary,
        home: home.clone(),
        timeout: Duration::from_millis(700),
        now_ms,
    };
    let (_, start, end) = ownership::corroborate(&inventory, &ancestry::KernelSampler, &provider, &request).ok()?;
    let token = uuid::Uuid::new_v4().hyphenated().to_string();
    let proof = ownership::envelope(
        &request,
        provider,
        claude_profile_ref(&home),
        local_clock(Some(boot), now_ms(), monotonic_ns()),
        uuid::Uuid::new_v4().hyphenated().to_string(),
        token.clone(),
        (start, end),
    );
    let committed = deliver(&store, std::slice::from_ref(&proof), Instant::now())
        .is_some_and(|receipts| receipts.iter().any(|r| {
            r.observation_id == proof.observation_id
                && matches!(r.status, RecordStatus::Committed | RecordStatus::AlreadyCommitted)
        }));
    let status = if committed {
        "COMMITTED"
    } else if Spool::at(&store).publish(&proof).is_ok() {
        "LOCAL_SPOOLED"
    } else {
        return None;
    };
    serde_json::to_vec(&serde_json::json!({
        "protocolVersion": ownership::PROTOCOL_VERSION,
        "status": status,
        "proofToken": token,
        "sourceEpoch": request.source_epoch,
        "sessionGeneration": request.session_generation,
        "sessionId": request.session_id,
    })).ok()
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
    #[cfg(feature = "qualification")]
    if let Some(measurement_store) = latency_store(args) {
        // Optional measurement I/O follows the real delivery/spool attempt;
        // it cannot spend that attempt's budget before durability is tried.
        let _ = threadspace_relay::latency::save_hook_capture(&measurement_store, &envelope);
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

#[cfg(feature = "qualification")]
fn latency_store(args: &[String]) -> Option<PathBuf> {
    // No arbitrary path and no production identity for this optional sink.
    if arg(args, "--agent").as_deref() != Some("ai.scalinity.threadspace.dev.agent")
        || arg(args, "--store-dir").is_some()
        || !args.iter().any(|arg| arg == "--qualification-latency") { return None; }
    store_dir(args)
}

/// The answer and the exit status to leave with.
fn mod_batch(args: &[String], started: Instant) -> (Option<Vec<u8>>, i32) {
    // Dev-only phase evidence; ordinary capture and release artifacts do not
    // depend on these stamps or on optional measurement persistence.
    #[cfg(feature = "qualification")]
    let mut phases = serde_json::Map::new();
    #[cfg(feature = "qualification")]
    macro_rules! phase {
        ($name:literal) => {
            if arg(args, "--agent").as_deref() == Some("ai.scalinity.threadspace.dev.agent")
                && arg(args, "--store-dir").is_none()
                && args.iter().any(|arg| arg == "--qualification-phases") {
                if let Some(ns) = monotonic_ns() {
                    phases.insert($name.into(), serde_json::json!(ns.to_string()));
                }
            }
        };
    }
    #[cfg(not(feature = "qualification"))]
    macro_rules! phase { ($name:literal) => {}; }
    phase!("modBatchEntryNs");
    let budget = arg(args, "--budget-ms")
        .and_then(|ms| ms.parse::<u64>().ok())
        .map_or(WALL_BUDGET_MS, |ms| ms.min(WALL_BUDGET_MS));
    let fault = fault();
    let late = || std::thread::sleep(Duration::from_millis(budget + 50));
    if fault.as_deref() == Some("slow") {
        late();
    }
    let Some(store) = store_dir(args) else { return (None, 0) };
    phase!("storeResolvedNs");
    let Some(stdin) = read_stdin(2 * FRAME_MAX_BYTES) else { return (None, 0) };
    phase!("stdinReadNs");
    let home = arg(args, "--home").map(PathBuf::from).or_else(home_dir);
    let Some(home) = home else { return (None, 0) };
    let boot = process::boot_session_id().ok();
    let context = BatchContext {
        profile_ref: claude_profile_ref(&home),
        evidence: provider_evidence(boot.as_deref().unwrap_or_default()),
        boot_id: boot,
    };
    phase!("providerContextNs");
    let socket = locator::read(&store.join("runtime-locator.json"))
        .ok()
        .and_then(|locator| locator.events_socket)
        .map(PathBuf::from);
    phase!("locatorReadNs");
    #[cfg(feature = "qualification")]
    let progress = |stamps: &serde_json::Map<String, serde_json::Value>, stage: &str| {
        if stamps.is_empty() { return; }
        let Some(boot) = context.boot_id.as_deref() else { return; };
        let Some(at) = monotonic_ns() else { return; };
        let Ok(request) = serde_json::from_slice::<modbatch::ModBatchRequest>(&stdin) else { return; };
        // An independent, best-effort diagnostic file survives a lost host
        // response. It has no admission or census authority and is not synced.
        let ids: Vec<_> = request.records.iter().take(64)
            .filter_map(|record| record["observationId"].as_str()).collect();
        let _ = threadspace_relay::latency::save_phase_probe(&store, &serde_json::json!({
            "kind": "helper-phase-probe", "schemaVersion": 1, "clock": "CLOCK_UPTIME_RAW",
            "bootId": boot, "monotonicNs": at.to_string(), "runtimeId": request.source_epoch,
            "helperPid": std::process::id(), "stage": stage, "observationIds": ids,
            "helperPhaseStamps": stamps, "helperElapsedMs": started.elapsed().as_secs_f64() * 1000.0,
        }));
    };
    #[cfg(feature = "qualification")]
    progress(&phases, "initialized");
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
    phase!("receiptReceivedNs");
    #[cfg(feature = "qualification")]
    progress(&phases, "receipt-received");
    match fault.as_deref() {
        Some("malformed") => (Some(br#"{"receiptVersion":1,"results":"malformed"}"#.to_vec()), 0),
        Some("exit1") => (serde_json::to_vec(&receipt).ok(), 1),
        _ => {
            #[cfg(feature = "qualification")]
            let mut response = serde_json::to_vec(&receipt).ok();
            #[cfg(not(feature = "qualification"))]
            let response = serde_json::to_vec(&receipt).ok();
            phase!("measurementBeginNs");
            #[cfg(feature = "qualification")]
            if !args.iter().any(|arg| arg == "--qualification-without-receipt-telemetry")
                && let (Some(store), Some(boot), Some(stamp)) = (latency_store(args), context.boot_id.as_deref(), monotonic_ns())
                && let Some(annotated) = threadspace_relay::latency::annotate_receipt_with_phases(
                    &store, &receipt, &stdin, boot, stamp,
                    (!phases.is_empty()).then_some(&phases),
                ) {
                response = Some(annotated);
            }
            phase!("measurementFinishedNs");
            #[cfg(feature = "qualification")]
            if !phases.is_empty() && let Some(bytes) = &response
                && let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) {
                phase!("answerReadyNs");
                value["qualificationPhases"] = serde_json::json!({
                    "clock": "CLOCK_UPTIME_RAW", "bootId": context.boot_id,
                    "helperPid": std::process::id(), "stamps": phases,
                    "helperElapsedMsAtAnswer": started.elapsed().as_secs_f64() * 1000.0,
                    "measurementEnabled": latency_store(args).is_some()
                        && !args.iter().any(|arg| arg == "--qualification-without-receipt-telemetry"),
                });
                response = serde_json::to_vec(&value).ok();
            }
            (response, 0)
        }
    }
}

fn main() {
    let started = Instant::now();
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().collect();
    let budget = if args.get(1).map(String::as_str) == Some("observer-proof") {
        OWNERSHIP_PROOF_BUDGET_MS
    } else {
        WALL_BUDGET_MS
    };
    watchdog(Duration::from_millis(budget));
    match args.get(1).map(String::as_str) {
        #[cfg(feature = "qualification")]
        Some("latency-census") => {
            let response = std::panic::catch_unwind(|| {
                let store = latency_store(&args)?;
                let bytes = read_stdin(64 * 1024)?;
                let boot = process::boot_session_id().ok()?;
                threadspace_relay::latency::answer_census(&store, &bytes, &boot, monotonic_ns()?, monotonic_ns)
            }).ok().flatten();
            let refused = br#"{"accepted":false}"#.as_slice();
            let bytes = response.as_deref().filter(|bytes| bytes.len() <= 4096).unwrap_or(refused);
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(bytes).and_then(|()| stdout.flush());
        }
        #[cfg(feature = "qualification")]
        Some("latency-samples") => {
            let accepted = std::panic::catch_unwind(|| {
                let store = latency_store(&args)?;
                let bytes = read_stdin(64 * 1024)?;
                Some(threadspace_relay::latency::save_observer(&store, &bytes))
            }).ok().flatten().unwrap_or(false);
            let response = if accepted { br#"{"accepted":true}"#.as_slice() } else { br#"{"accepted":false}"#.as_slice() };
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(response).and_then(|()| stdout.flush());
        }
        Some("observer-proof") => {
            let response = std::panic::catch_unwind(|| observer_proof(&args)).ok().flatten();
            if let Some(response) = response.filter(|text| text.len() <= RECEIPT_MAX_BYTES) {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(&response).and_then(|()| stdout.flush());
            }
        }
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
