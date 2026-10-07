//! C-02 remediation (G09; SPEC §18.9, §19.5): capture admission requires
//! positive login-item supervision, and the saved preference alone never
//! opens it. Native cases against the installed identity, each proving its
//! precondition before its assertion:
//!
//! - A: the preference is enabled, the login item is unregistered (macOS
//!   reports the background item unavailable) and a notification click
//!   cold-starts the companion through LaunchServices.
//! - B: the login item is registered again and claims the store from that
//!   instance; only the login item's companion becomes the observer.
//! - C: with no UI and no provider hook, the observer is killed and launchd
//!   relaunches the login item, which alone resumes capture.
//! - D: the original regression, stop → notification cold start → enable →
//!   crash → supervised relaunch.
//! - E: absent, shell-default and forged launch labels, started directly by
//!   the harness: unknown provenance, never admitted.
//! - F: across all cases, one writer and one effective observer at a time,
//!   and no acknowledged record or outstanding attention lost.
//!
//! `cases` selects a subset (`A,E` runs a negative control against a build
//! that predates the repair). The runner always restores the registered,
//! enabled login item and closes its own windows and processes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_harness::companion::LogCursor;
use threadspace_harness::evidence::Run;
use threadspace_harness::procs::{self, Incarnation};
use threadspace_harness::run::run;
use threadspace_harness::service;
use threadspace_relay::client::ClientError;

use crate::bridge_gates::{companion_snapshot, compare_projection};
use crate::ctx::Ctx;
use crate::service_gates::{kill_companion, writer_lock_free};
use crate::terminal_gates::{StartedClaude, start_claude};

const SAMPLE_MS: u64 = 200;

/// Companion log lines since the run started, kept in order.
struct Log {
    cursor: LogCursor,
    seen: Vec<Value>,
}

impl Log {
    fn find(&self, event: &str, matches: &dyn Fn(&Value) -> bool) -> Option<Value> {
        self.seen
            .iter()
            .find(|line| line["event"] == event && matches(line))
            .cloned()
    }

    /// The first matching line, already read or arriving within `timeout`.
    fn wait(
        &mut self,
        event: &str,
        matches: &dyn Fn(&Value) -> bool,
        timeout_s: u64,
    ) -> Option<Value> {
        if let Some(hit) = self.find(event, matches) {
            return Some(hit);
        }
        self.cursor.wait_for(
            event,
            matches,
            Duration::from_secs(timeout_s),
            &mut self.seen,
        )
    }

    fn drain(&mut self) {
        let lines = self.cursor.read_new();
        self.seen.extend(lines);
    }

    fn of_pid(&self, pid: i32, event: &str) -> Vec<Value> {
        self.seen
            .iter()
            .filter(|line| line["pid"] == pid && line["event"] == event)
            .cloned()
            .collect()
    }
}

fn is_pid(pid: i32) -> impl Fn(&Value) -> bool {
    move |line: &Value| line["pid"] == pid
}

/// The companion's own launch evidence, read independently from the kernel:
/// its launchd job label and its parent.
fn launch_witness(pid: i32) -> Value {
    let env = run(
        "/bin/ps",
        &["eww", "-o", "command=", "-p", &pid.to_string()],
        Duration::from_secs(3),
    );
    let label = env
        .stdout
        .split_whitespace()
        .find_map(|token| token.strip_prefix("XPC_SERVICE_NAME="))
        .map(str::to_owned);
    let parent = run(
        "/bin/ps",
        &["-o", "ppid=", "-p", &pid.to_string()],
        Duration::from_secs(3),
    );
    json!({ "pid": pid, "xpcServiceName": label, "parentPid": parent.stdout.trim().parse::<i32>().ok() })
}

/// A request through the qualification client, keeping a refusal's code.
fn ask(ctx: &Ctx, body: ControlRequestBody) -> Value {
    let mut client = match ctx.companion().client(Duration::from_secs(5)) {
        Ok(client) => client,
        Err(error) => return json!({ "ok": false, "connect": error }),
    };
    match client.request(body) {
        Ok(ControlResponseBody::Admitted { status, cursor, .. }) => {
            json!({ "ok": true, "admitted": true, "status": status, "cursor": cursor })
        }
        Ok(reply) => {
            json!({ "ok": true, "reply": format!("{reply:?}").chars().take(160).collect::<String>() })
        }
        Err(ClientError::Rejected(error)) => {
            json!({ "ok": false, "code": error.code, "detail": error.detail })
        }
        Err(other) => json!({ "ok": false, "error": other.to_string() }),
    }
}

fn admit(ctx: &Ctx, observation_id: &str, captured_wall_ms: i64) -> Value {
    ask(
        ctx,
        ControlRequestBody::QualifyAdmit {
            observation_id: observation_id.to_owned(),
            captured_wall_ms,
        },
    )
}

fn diagnostics(ctx: &Ctx) -> Value {
    match ctx.companion().diagnostics() {
        Ok(d) => json!({
            "pid": d["process"]["pid"],
            "observationEnabled": d["observationEnabled"],
            "launchProvenance": d["launchProvenance"],
            "admissionOpen": d["admissionOpen"],
            "maintenancePhase": d["maintenancePhase"],
            "coreGeneration": d["coreGeneration"],
            "storeGeneration": d["storeGeneration"],
        }),
        Err(error) => json!({ "error": error }),
    }
}

/// Every fact the invariants need at one moment.
fn checkpoint(ctx: &Ctx, label: &str) -> Value {
    json!({
        "label": label,
        "atMs": threadspace_harness::now_ms(),
        "companionProcesses": ctx.companion().processes(),
        "locatorIncarnation": ctx.companion().incarnation(),
        "loginItemPid": service::login_item_pid(&ctx.id),
        "serviceStatus": service::status(&ctx.id),
        "writerLockFree": writer_lock_free(ctx),
        "uiProcesses": ctx.app().processes(),
        "diagnostics": diagnostics(ctx),
    })
}

/// One live companion, the locator's, holding the lock.
fn single_writer(point: &Value) -> bool {
    let processes = point["companionProcesses"].as_array().map_or(0, Vec::len);
    processes == 1
        && point["writerLockFree"] == false
        && point["companionProcesses"][0]["pid"] == point["locatorIncarnation"]["pid"]
}

/// The login item's job is the locator's process.
fn login_item_owns(point: &Value) -> bool {
    point["loginItemPid"].as_u64().is_some()
        && point["loginItemPid"].as_u64() == point["locatorIncarnation"]["pid"].as_u64()
}

fn raise(ctx: &Ctx, label: &str) -> Result<(String, String), String> {
    match ctx.companion().request(
        ControlRequestBody::QualifyRaiseAttention {
            label: label.to_owned(),
            session_id: None,
        },
        Duration::from_secs(10),
    )? {
        ControlResponseBody::AttentionRaised {
            attention_id,
            notification_request_id,
            ..
        } => Ok((attention_id, notification_request_id)),
        other => Err(format!("unexpected {other:?}")),
    }
}

fn attention_state(ctx: &Ctx, attention_id: &str) -> Value {
    match companion_snapshot(ctx) {
        Ok((_, snapshot)) => snapshot
            .attention
            .iter()
            .find(|a| a.attention_id == attention_id)
            .map(|a| json!({ "present": true, "acknowledgedAtMs": a.acknowledged_at_ms, "resolvedAtMs": a.resolved_at_ms }))
            .unwrap_or_else(|| json!({ "present": false })),
        Err(error) => json!({ "error": error }),
    }
}

fn outstanding(state: &Value) -> bool {
    state["present"] == true
        && state["acknowledgedAtMs"].is_null()
        && state["resolvedAtMs"].is_null()
}

fn session_journaled(ctx: &Ctx, native_session_id: &str) -> Value {
    match companion_snapshot(ctx) {
        Ok((cursor, snapshot)) => json!({
            "cursor": cursor,
            "journaled": snapshot.sessions.iter().any(|s| s.native_session_id == native_session_id),
        }),
        Err(error) => json!({ "error": error }),
    }
}

fn press(ctx: &Ctx, label: &str) -> Value {
    let _gui = ctx.gui("c02 notification press");
    ctx.native.json(&["notification", "press", label, "20"])
}

fn wait_new_companion(ctx: &Ctx, known: &[i32], timeout_s: u64) -> Option<Incarnation> {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(timeout_s) {
        if let Some(fresh) = ctx
            .companion()
            .processes()
            .into_iter()
            .find(|p| !known.contains(&p.pid))
        {
            return Some(fresh);
        }
        threadspace_harness::pause_ms(100);
    }
    None
}

/// Waits until the locator names the login item's own job.
fn wait_login_item_owner(ctx: &Ctx, timeout_s: u64) -> Option<u64> {
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(timeout_s) {
        if let (Some(job), Some(locator)) = (
            service::login_item_pid(&ctx.id),
            ctx.companion().incarnation(),
        ) && job == locator.pid as u32
            && ctx.companion().client(Duration::from_secs(2)).is_ok()
        {
            return Some(started.elapsed().as_millis() as u64);
        }
        threadspace_harness::pause_ms(200);
    }
    None
}

/// Samples the live companion processes every `SAMPLE_MS` until stopped.
struct Sampler {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<(i64, Vec<i32>)>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    fn start(executable: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::new()));
        let (thread_stop, thread_samples) = (Arc::clone(&stop), Arc::clone(&samples));
        let handle = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let pids: Vec<i32> = procs::with_executable(&executable)
                    .into_iter()
                    .map(|p| p.pid)
                    .collect();
                if let Ok(mut list) = thread_samples.lock() {
                    list.push((threadspace_harness::now_ms(), pids));
                }
                std::thread::sleep(Duration::from_millis(SAMPLE_MS));
            }
        });
        Self {
            stop,
            samples,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> Vec<(i64, Vec<i32>)> {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.samples.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// F: per-process writer and observer intervals from the companion's own log
/// and the sampler, and their overlaps.
fn invariants(log: &Log, samples: &[(i64, Vec<i32>)]) -> Value {
    #[derive(Default)]
    struct Proc {
        provenance: Option<Value>,
        writer_since: Option<i64>,
        admission_open: Option<bool>,
        forwarder: bool,
        first_seen: Option<i64>,
        last_seen: Option<i64>,
    }
    let mut procs_by_pid: BTreeMap<i64, Proc> = BTreeMap::new();
    for line in &log.seen {
        let Some(pid) = line["pid"].as_i64() else {
            continue;
        };
        let entry = procs_by_pid.entry(pid).or_default();
        let ts = line["ts"].as_i64();
        match line["event"].as_str() {
            Some("CORE_START") => entry.provenance = Some(line["launchProvenance"].clone()),
            Some("WRITER_LOCK_ACQUIRED") | Some("WRITER_CLAIMED") => {
                entry.writer_since = entry.writer_since.or(ts)
            }
            Some("WRITER_LOCK_HELD") => entry.forwarder = true,
            Some("OBSERVATION_STATE") => entry.admission_open = line["admissionOpen"].as_bool(),
            _ => {}
        }
    }
    for (at, pids) in samples {
        for pid in pids {
            let entry = procs_by_pid.entry(i64::from(*pid)).or_default();
            entry.first_seen = Some(entry.first_seen.map_or(*at, |v| v.min(*at)));
            entry.last_seen = Some(entry.last_seen.map_or(*at, |v| v.max(*at)));
        }
    }
    let writers: Vec<(i64, i64, i64)> = procs_by_pid
        .iter()
        .filter_map(|(pid, p)| Some((*pid, p.writer_since?, p.last_seen.unwrap_or(i64::MAX))))
        .collect();
    let mut max_writer_overlap_ms = 0;
    let mut max_observer_overlap_ms = 0;
    for (i, a) in writers.iter().enumerate() {
        for b in writers.iter().skip(i + 1) {
            let overlap = a.2.min(b.2) - a.1.max(b.1);
            max_writer_overlap_ms = max_writer_overlap_ms.max(overlap);
            let both_observe = [a.0, b.0]
                .iter()
                .all(|pid| procs_by_pid.get(pid).and_then(|p| p.admission_open) == Some(true));
            if both_observe {
                max_observer_overlap_ms = max_observer_overlap_ms.max(overlap);
            }
        }
    }
    let unsupervised_observers: Vec<i64> = procs_by_pid
        .iter()
        .filter(|(_, p)| {
            p.admission_open == Some(true)
                && p.provenance.as_ref().and_then(Value::as_str) != Some("LOGIN_ITEM")
        })
        .map(|(pid, _)| *pid)
        .collect();
    let max_simultaneous = samples
        .iter()
        .map(|(_, pids)| pids.len())
        .max()
        .unwrap_or(0);
    let per_process: Vec<Value> = procs_by_pid
        .iter()
        .map(|(pid, p)| {
            json!({
                "pid": pid,
                "launchProvenance": p.provenance,
                "writerSinceMs": p.writer_since,
                "forwarder": p.forwarder,
                "admissionOpen": p.admission_open,
                "firstSeenMs": p.first_seen,
                "lastSeenMs": p.last_seen,
            })
        })
        .collect();
    // Overlap within one sampling period is the sampler's resolution, not
    // two holders: the writer lock is an exclusive flock.
    let pass = max_writer_overlap_ms <= SAMPLE_MS as i64
        && max_observer_overlap_ms <= SAMPLE_MS as i64
        && unsupervised_observers.is_empty();
    json!({
        "pass": pass,
        "samples": samples.len(),
        "sampleIntervalMs": SAMPLE_MS,
        "maxSimultaneousCompanionProcesses": max_simultaneous,
        "maxWriterOverlapMs": max_writer_overlap_ms,
        "maxObserverOverlapMs": max_observer_overlap_ms,
        "unsupervisedObservers": unsupervised_observers,
        "processes": per_process,
    })
}

/// Restores the registered, enabled login item as the store's only writer.
fn restore(ctx: &Ctx, log: &mut Log) -> Value {
    let mut steps = Vec::new();
    if service::status(&ctx.id) != "ENABLED" {
        steps.push(json!({ "register": service::bootstrap(&ctx.id, "register") }));
    }
    if wait_login_item_owner(ctx, 45).is_none() {
        // A build without the claim never yields: end the unsupervised
        // incumbent, proven by its executable, birth and launch label.
        if let Some(incumbent) = ctx.companion().incarnation() {
            let witness = launch_witness(incumbent.pid);
            if witness["xpcServiceName"].as_str() != Some(ctx.id.agent_identifier.as_str()) {
                procs::signal(incumbent.pid, libc::SIGKILL);
                let exited = procs::wait_exit(&incumbent, Duration::from_secs(10));
                steps.push(json!({ "endedUnsupervisedIncumbent": incumbent, "witness": witness, "exitedAfterMs": exited }));
            }
        }
        steps.push(json!({ "loginItemOwnerAfterMs": wait_login_item_owner(ctx, 60) }));
    }
    let enabled = diagnostics(ctx)["observationEnabled"] == true;
    if !enabled {
        steps.push(json!({ "enable": service::bootstrap(&ctx.id, "enable") }));
    }
    log.drain();
    json!({ "steps": steps, "after": checkpoint(ctx, "restored") })
}

fn record(
    run_dir: &Run,
    cases: &mut Vec<Value>,
    case: &str,
    pass: bool,
    detail: Value,
) -> Result<(), String> {
    let entry = json!({ "case": case, "pass": pass, "detail": detail });
    run_dir
        .append("cases.jsonl", &entry)
        .map_err(|e| e.to_string())?;
    cases.push(entry);
    Ok(())
}

pub fn c02(ctx: &Ctx, selection: &str) -> Result<Value, String> {
    let tokens: Vec<String> = selection
        .split(',')
        .map(|c| c.trim().to_uppercase())
        .collect();
    // `legacy` marks a negative control against a build that predates the
    // repair: its older diagnostics cannot be parsed, so only the start
    // precondition's preference read is skipped. No case check changes.
    let negative_control = tokens.iter().any(|t| t == "LEGACY");
    let wanted: Vec<String> = if tokens.iter().any(|t| t == "ALL") {
        ["A", "B", "C", "D", "E"]
            .iter()
            .map(|c| (*c).to_owned())
            .collect()
    } else {
        tokens.into_iter().filter(|t| t != "LEGACY").collect()
    };
    if wanted.iter().any(|c| c == "B") && !wanted.iter().any(|c| c == "A") {
        return Err("case B needs case A's unsupervised incumbent".into());
    }
    let run_dir = Run::create(
        &ctx.evidence_root(),
        "remediation/c02-supervision",
        ctx.channel_name(),
    )
    .map_err(|e| e.to_string())?;
    let mut log = Log {
        cursor: ctx.companion().log(),
        seen: Vec::new(),
    };
    let root = PathBuf::from(format!(
        "/private/tmp/ts-m0c-c02-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    ));
    let mut cases: Vec<Value> = Vec::new();
    let mut windows: Vec<StartedClaude> = Vec::new();
    let mut retained: Vec<(String, String, Value)> = Vec::new(); // admitted records
    let mut open_attention: Vec<String> = Vec::new();
    let sampler = Sampler::start(ctx.id.companion_executable.clone());
    let environment = ctx.environment();
    run_dir
        .write_json("environment.json", &environment)
        .map_err(|e| e.to_string())?;
    // Banner presses and Terminal windows take focus: start only once the owner is idle.
    let gate =
        threadspace_harness::idle::wait_for_idle(&ctx.native, 10.0, Duration::from_secs(1800));
    run_dir
        .write_json("idle-gate.json", &json!(gate))
        .map_err(|e| e.to_string())?;

    // Precondition for every case: the login item's enabled companion owns the store.
    let start = checkpoint(ctx, "start");
    let start_ok = single_writer(&start)
        && login_item_owns(&start)
        && (negative_control || start["diagnostics"]["observationEnabled"] == true);
    run_dir
        .append("checkpoints.jsonl", &start)
        .map_err(|e| e.to_string())?;
    if !start_ok {
        let _ = sampler.finish();
        return Err(format!(
            "precondition: the login item's enabled companion must own the store: {start}"
        ));
    }

    let outcome = (|| -> Result<(), String> {
        if wanted.iter().any(|c| c == "A") {
            case_a_b(
                ctx,
                &run_dir,
                &mut log,
                &mut cases,
                &mut windows,
                &mut open_attention,
                &mut retained,
                &root,
                wanted.iter().any(|c| c == "B"),
            )?;
        }
        if wanted.iter().any(|c| c == "C") {
            case_c(ctx, &run_dir, &mut log, &mut cases, &mut retained)?;
        }
        if wanted.iter().any(|c| c == "D") {
            case_d(ctx, &run_dir, &mut log, &mut cases, &mut open_attention)?;
        }
        if wanted.iter().any(|c| c == "E") {
            case_e(ctx, &run_dir, &mut log, &mut cases)?;
        }
        Ok(())
    })();
    let outcome_error = outcome.err();

    let restored = restore(ctx, &mut log);
    run_dir
        .write_json("restore.json", &restored)
        .map_err(|e| e.to_string())?;
    // Nothing acknowledged was lost: each admitted record replays as
    // ALREADY_COMMITTED at its original cursor; outstanding items stay open.
    let durability: Vec<Value> = retained
        .iter()
        .map(|(id, cursor, captured)| {
            let again = admit(ctx, id, captured.as_i64().unwrap_or(0));
            json!({ "observationId": id, "originalCursor": cursor, "replay": again,
                    "preserved": again["status"] == "ALREADY_COMMITTED" && again["cursor"].as_str() == Some(cursor.as_str()) })
        })
        .collect();
    let attention: Vec<Value> = open_attention
        .iter()
        .map(|id| {
            let state = attention_state(ctx, id);
            json!({ "attentionId": id, "state": state, "outstanding": outstanding(&state) })
        })
        .collect();
    {
        let _gui = ctx.gui("c02 cleanup");
        for window in &windows {
            let _ = run_dir.append("cleanup.jsonl", &window.tab.close());
        }
    }
    let _ = crate::cleanup::resolve_qualification(ctx, "c02 qualification cleanup");
    let _ = crate::cleanup::clear_notifications(ctx);
    log.drain();
    let samples = sampler.finish();
    let f = invariants(&log, &samples);
    let durable_ok = durability.iter().all(|d| d["preserved"] == true)
        && attention.iter().all(|a| a["outstanding"] == true);
    let f_ran = wanted.len() >= 4;
    if f_ran {
        record(
            &run_dir,
            &mut cases,
            "F-single-writer-and-observer",
            f["pass"] == true && durable_ok,
            json!({ "invariants": f, "durability": durability, "outstandingAttention": attention }),
        )?;
    } else {
        run_dir.write_json("invariants.json", &json!({ "invariants": f, "durability": durability, "outstandingAttention": attention })).map_err(|e| e.to_string())?;
    }
    run_dir
        .write_text(
            "companion-log.jsonl",
            &log.seen
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .map_err(|e| e.to_string())?;
    let passed = cases.iter().filter(|c| c["pass"] == true).count();
    let summary = json!({
        "issue": "C-02",
        "gate": "G09",
        "selection": wanted,
        "negativeControl": negative_control,
        "pass": outcome_error.is_none() && passed == cases.len() && !cases.is_empty(),
        "error": outcome_error,
        "cases": cases.iter().map(|c| json!({ "case": c["case"], "pass": c["pass"] })).collect::<Vec<_>>(),
        "companionSha256": environment["companionSha256"],
        "executableSha256": environment["executableSha256"],
        "disposableRoot": root.display().to_string(),
    });
    run_dir
        .write_json("summary.json", &summary)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "dir": run_dir.dir }))
}

#[allow(clippy::too_many_arguments)]
fn case_a_b(
    ctx: &Ctx,
    run_dir: &Run,
    log: &mut Log,
    cases: &mut Vec<Value>,
    windows: &mut Vec<StartedClaude>,
    open_attention: &mut Vec<String>,
    retained: &mut Vec<(String, String, Value)>,
    root: &std::path::Path,
    with_b: bool,
) -> Result<(), String> {
    let app = ctx.app();
    app.stop_all();
    let supervised = ctx.companion().incarnation().ok_or("no companion")?;

    // A1. An outstanding item whose banner is on screen.
    let label_a = format!("c02-a-{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let (attention_a, request_a) = raise(ctx, &label_a)?;
    open_attention.push(attention_a.clone());
    let submitted = log.wait(
        "NOTIFICATION_SUBMISSION",
        &|l| l["attentionId"] == attention_a.as_str() && l["state"] == "SUBMITTED",
        20,
    );

    // A2. The background item becomes unavailable with the preference still
    //     enabled: unregistering ends the login item's companion. Only fast
    //     facts are read before the press, while the banner is on screen.
    let unregistered = service::bootstrap(&ctx.id, "unregister");
    let supervised_exit = procs::wait_exit(&supervised, Duration::from_secs(20));
    let unavailable = json!({
        "label": "A-service-unavailable",
        "atMs": threadspace_harness::now_ms(),
        "serviceStatusAfterUnregister": unregistered["statusAfter"],
        "companionProcesses": ctx.companion().processes(),
        "loginItemPid": service::login_item_pid(&ctx.id),
        "writerLockFree": writer_lock_free(ctx),
    });
    run_dir
        .append("checkpoints.jsonl", &unavailable)
        .map_err(|e| e.to_string())?;
    let unavailable_ok = supervised_exit.is_some()
        && unavailable["serviceStatusAfterUnregister"] != "ENABLED"
        && unavailable["loginItemPid"].is_null()
        && unavailable["writerLockFree"] == true
        && unavailable["companionProcesses"]
            .as_array()
            .is_some_and(Vec::is_empty);

    // A3. The click cold-starts the companion through LaunchServices.
    let since = threadspace_harness::now_ms();
    let pressed = press(ctx, &label_a);
    let Some(cold) = wait_new_companion(ctx, &[supervised.pid], 30) else {
        return record(
            run_dir,
            cases,
            "A-enabled-preference-unsupervised-cold-start",
            false,
            json!({
                "error": "the notification did not start a companion",
                "preconditionServiceUnavailable": unavailable_ok, "notificationSubmitted": submitted,
                "press": pressed, "unregister": unregistered,
            }),
        );
    };
    let pid = cold.pid;
    // A4. Real provider activity while this instance holds the store.
    let claude = start_claude(ctx, root.join("a"), true)?;
    let claude_native = claude.native_session_id.clone();
    windows.push(claude);
    let witness = launch_witness(pid);
    let core_start = log.wait("CORE_START", &is_pid(pid), 20);
    let state = log.wait("OBSERVATION_STATE", &is_pid(pid), 20);
    let ready = log.wait("CORE_READY", &is_pid(pid), 20);
    let response = log.wait(
        "NOTIFICATION_RESPONSE",
        &|l| l["pid"] == pid && l["attentionId"] == attention_a.as_str(),
        45,
    );
    let inspector = log.wait(
        "NOTIFICATION_INSPECTOR",
        &|l| l["pid"] == pid && l["attentionId"] == attention_a.as_str(),
        30,
    );
    let intent = app
        .wait_report(
            "notification-intent",
            since,
            |r| r["report"]["attentionId"].as_str() == Some(attention_a.as_str()),
            Duration::from_secs(60),
        )
        .map(|(_, r)| r["report"].clone());
    // A5. Capture stays closed in this instance: the durability stand-in and
    //     a forced discovery pass are refused; provider polling never runs.
    let capture_probe_id = uuid::Uuid::new_v4().to_string();
    let capture_probe = admit(ctx, &capture_probe_id, threadspace_harness::now_ms());
    let refresh_probe = ask(ctx, ControlRequestBody::RefreshEvidence);
    threadspace_harness::pause_ms(20_000);
    log.drain();
    let discovery_passes = log.of_pid(pid, "DISCOVERY_PASS").len();
    let journaled = session_journaled(ctx, &claude_native);
    let point = checkpoint(ctx, "A-unsupervised-instance");
    run_dir
        .append("checkpoints.jsonl", &point)
        .map_err(|e| e.to_string())?;
    let attention_after = attention_state(ctx, &attention_a);
    let truthful = point["diagnostics"]["observationEnabled"] == true
        && point["diagnostics"]["admissionOpen"] == false
        && point["diagnostics"]["launchProvenance"] == "LAUNCH_SERVICES";
    let checks = json!({
        "preconditionServiceUnavailable": unavailable_ok,
        "notificationSubmitted": submitted.is_some(),
        "pressed": pressed["pressed"] == true,
        "launchedThroughLaunchServices": witness["xpcServiceName"].as_str().is_some_and(|n| n.starts_with(&format!("application.{}.", ctx.id.agent_identifier))),
        "classifiedUnsupervised": core_start.as_ref().is_some_and(|l| l["supervised"] == false && l["launchProvenance"] == "LAUNCH_SERVICES"),
        "preferenceKeptEnabled": state.as_ref().is_some_and(|l| l["observationEnabled"] == true),
        "admissionClosedAtStartup": state.as_ref().is_some_and(|l| l["admissionOpen"] == false),
        "inspectorNotReturn": response.as_ref().is_some_and(|l| l["plan"] == "INSPECTOR") && inspector.is_some(),
        "intentAppliedAfterHydration": intent.as_ref().is_some_and(|i| i["appliedAfterHydration"] == true),
        "captureRefused": capture_probe["admitted"] != true && capture_probe["code"] == "NOT_SUPERVISED",
        "discoveryRefused": refresh_probe["ok"] == false && refresh_probe["code"] == "NOT_SUPERVISED",
        "noDiscoveryPass": discovery_passes == 0,
        "providerSessionNotCaptured": journaled["journaled"] == false,
        "singleWriter": single_writer(&point) && point["locatorIncarnation"]["pid"] == pid,
        "diagnosticsTruthful": truthful,
        "attentionOutstanding": outstanding(&attention_after),
    });
    let pass_a = checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true));
    record(
        run_dir,
        cases,
        "A-enabled-preference-unsupervised-cold-start",
        pass_a,
        json!({
            "checks": checks,
            "attentionId": attention_a, "notificationRequestId": request_a,
            "unregister": unregistered, "supervisedBefore": supervised, "supervisedExitedAfterMs": supervised_exit,
            "coldStart": cold, "launchWitness": witness,
            "coreStart": core_start, "observationState": state, "coreReady": ready.map(|l| l["ts"].clone()),
            "notificationResponse": response, "viewIntent": intent,
            "captureProbe": capture_probe, "refreshProbe": refresh_probe,
            "discoveryPassesFromInstance": discovery_passes, "providerSession": { "nativeSessionId": claude_native, "journal": journaled },
            "attentionAfter": attention_after,
        }),
    )?;
    if !with_b {
        return Ok(());
    }

    // B1. A second click lands on the unsupervised instance with the UI
    //     closed, so its intent is still undelivered when the login item
    //     returns; it must reach the UI whichever companion delivers it.
    app.stop_all();
    let label_b = format!("c02-b-{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let (attention_b, _) = raise(ctx, &label_b)?;
    open_attention.push(attention_b.clone());
    let submitted_b = log.wait(
        "NOTIFICATION_SUBMISSION",
        &|l| l["attentionId"] == attention_b.as_str() && l["state"] == "SUBMITTED",
        20,
    );
    let since_b = threadspace_harness::now_ms();
    let pressed_b = press(ctx, &label_b);
    let queued_b = log.wait(
        "NOTIFICATION_INSPECTOR",
        &|l| l["pid"] == pid && l["attentionId"] == attention_b.as_str(),
        30,
    );

    // B2. The login item is registered again; its companion claims the store.
    let registered = service::bootstrap(&ctx.id, "register");
    let owner_after = wait_login_item_owner(ctx, 60);
    let fresh = ctx.companion().incarnation();
    let fresh_pid = fresh.as_ref().map_or(0, |f| f.pid);
    let incumbent_exit = procs::wait_exit(&cold, Duration::from_secs(15));
    let yielded = log.wait("UNSUPERVISED_YIELD", &is_pid(pid), 10);
    let claimed = log.wait("WRITER_CLAIMED", &is_pid(fresh_pid), 10);
    let fresh_start = log.wait("CORE_START", &is_pid(fresh_pid), 10);
    let fresh_state = log.wait("OBSERVATION_STATE", &is_pid(fresh_pid), 10);
    let fresh_witness = launch_witness(fresh_pid);
    // B3. Only now is the provider session captured, by the login item's companion.
    let bound = windows
        .last()
        .and_then(|w| w.bound_session(ctx, Duration::from_secs(90)));
    log.drain();
    let fresh_passes = log.of_pid(fresh_pid, "DISCOVERY_PASS").len();
    let intent_b = app
        .wait_report(
            "notification-intent",
            since_b,
            |r| r["report"]["attentionId"].as_str() == Some(attention_b.as_str()),
            Duration::from_secs(90),
        )
        .map(|(_, r)| r["report"].clone());
    let projection = compare_projection(ctx, "after-claim");
    let admit_id = uuid::Uuid::new_v4().to_string();
    let captured = threadspace_harness::now_ms();
    let admitted = admit(ctx, &admit_id, captured);
    if let Some(cursor) = admitted["cursor"].as_str() {
        retained.push((admit_id.clone(), cursor.to_owned(), json!(captured)));
    }
    let point_b = checkpoint(ctx, "B-login-item-observer");
    run_dir
        .append("checkpoints.jsonl", &point_b)
        .map_err(|e| e.to_string())?;
    let checks_b = json!({
        "secondIntentQueuedByUnsupervised": submitted_b.is_some() && pressed_b["pressed"] == true && queued_b.is_some(),
        "registered": registered["ok"] == true,
        "loginItemOwnsStore": owner_after.is_some() && login_item_owns(&point_b),
        "incumbentYieldedAndExited": yielded.is_some() && incumbent_exit.is_some(),
        "claimedByLoginItem": claimed.is_some() && fresh_start.as_ref().is_some_and(|l| l["launchProvenance"] == "LOGIN_ITEM"),
        "independentLaunchWitness": fresh_witness["xpcServiceName"].as_str() == Some(ctx.id.agent_identifier.as_str()) && fresh_witness["parentPid"] == 1,
        "admissionOpenOnlyNow": fresh_state.as_ref().is_some_and(|l| l["admissionOpen"] == true),
        "providerSessionCapturedBySupervised": bound.is_some() && fresh_passes > 0,
        "intentPreserved": intent_b.as_ref().is_some_and(|i| i["appliedAfterHydration"] == true),
        "uiHydratedAgainstNewCompanion": projection["equal"] == true,
        "captureAdmitted": admitted["admitted"] == true,
        "singleWriter": single_writer(&point_b),
        "diagnostics": point_b["diagnostics"]["launchProvenance"] == "LOGIN_ITEM" && point_b["diagnostics"]["admissionOpen"] == true,
    });
    let pass_b = checks_b
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true));
    record(
        run_dir,
        cases,
        "B-login-item-claims-the-store",
        pass_b,
        json!({
            "checks": checks_b,
            "register": registered, "ownerAfterMs": owner_after,
            "unsupervisedIncumbent": cold, "incumbentExitedAfterMs": incumbent_exit, "yield": yielded,
            "claimant": fresh, "claim": claimed, "coreStart": fresh_start, "observationState": fresh_state, "launchWitness": fresh_witness,
            "inheritedIntents": claimed.as_ref().map(|l| l["inheritedIntents"].clone()),
            "providerSessionBoundAs": bound, "discoveryPassesFromLoginItem": fresh_passes,
            "secondAttentionId": attention_b, "secondIntent": intent_b, "projection": projection, "admit": admitted,
        }),
    )?;
    Ok(())
}

fn case_c(
    ctx: &Ctx,
    run_dir: &Run,
    log: &mut Log,
    cases: &mut Vec<Value>,
    retained: &mut Vec<(String, String, Value)>,
) -> Result<(), String> {
    ctx.app().stop_all();
    threadspace_harness::pause_ms(2000);
    let before = checkpoint(ctx, "C-before-crash");
    run_dir
        .append("checkpoints.jsonl", &before)
        .map_err(|e| e.to_string())?;
    let since = threadspace_harness::now_ms();
    let precondition = single_writer(&before)
        && login_item_owns(&before)
        && before["uiProcesses"].as_array().is_some_and(Vec::is_empty);
    let (old, fresh, waited) = kill_companion(ctx)?;
    let start = log.wait("CORE_START", &is_pid(fresh.pid), 20);
    let state = log.wait("OBSERVATION_STATE", &is_pid(fresh.pid), 20);
    let pass_after_start = log.wait("DISCOVERY_PASS", &is_pid(fresh.pid), 60);
    log.drain();
    // Who connected to the relaunched companion before it was ready: only this harness.
    let clients: Vec<Value> = log
        .of_pid(fresh.pid, "CLIENT_CONNECTED")
        .iter()
        .filter(|l| l["ts"].as_i64().is_some_and(|ts| ts >= since))
        .map(|l| json!({ "ts": l["ts"], "role": l["role"], "peerPid": l["peerPid"] }))
        .collect();
    let foreign_clients = clients
        .iter()
        .filter(|c| c["role"] != "QUALIFICATION")
        .count();
    let admit_id = uuid::Uuid::new_v4().to_string();
    let captured = threadspace_harness::now_ms();
    let admitted = admit(ctx, &admit_id, captured);
    if let Some(cursor) = admitted["cursor"].as_str() {
        retained.push((admit_id.clone(), cursor.to_owned(), json!(captured)));
    }
    let after = checkpoint(ctx, "C-after-relaunch");
    run_dir
        .append("checkpoints.jsonl", &after)
        .map_err(|e| e.to_string())?;
    let checks = json!({
        "preconditionLoginItemOwnsNoUi": precondition,
        "relaunchedBySupervision": login_item_owns(&after) && after["locatorIncarnation"]["pid"] == fresh.pid,
        "noUiDuringRelaunch": after["uiProcesses"].as_array().is_some_and(Vec::is_empty),
        "noUiOrHookClient": foreign_clients == 0,
        "loginItemProvenance": start.as_ref().is_some_and(|l| l["launchProvenance"] == "LOGIN_ITEM"),
        "admissionOpen": state.as_ref().is_some_and(|l| l["admissionOpen"] == true),
        "discoveryResumedInSupervised": pass_after_start.is_some(),
        "captureAdmitted": admitted["admitted"] == true,
        "singleWriter": single_writer(&after),
    });
    let pass = checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true));
    record(
        run_dir,
        cases,
        "C-crash-relaunch-no-ui-no-hook",
        pass,
        json!({
            "checks": checks, "killed": old, "relaunched": fresh, "relaunchWaitMs": waited,
            "coreStart": start, "observationState": state, "discoveryPass": pass_after_start,
            "clientsAfterKill": clients, "admit": admitted,
        }),
    )
}

fn case_d(
    ctx: &Ctx,
    run_dir: &Run,
    log: &mut Log,
    cases: &mut Vec<Value>,
    open_attention: &mut Vec<String>,
) -> Result<(), String> {
    let app = ctx.app();
    app.stop_all();
    let supervised = ctx.companion().incarnation().ok_or("no companion")?;
    let label = format!("c02-d-{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let (attention, _) = raise(ctx, &label)?;
    open_attention.push(attention.clone());
    let submitted = log.wait(
        "NOTIFICATION_SUBMISSION",
        &|l| l["attentionId"] == attention.as_str() && l["state"] == "SUBMITTED",
        20,
    );
    let stopped = service::bootstrap(&ctx.id, "stop");
    let exited = procs::wait_exit(&supervised, Duration::from_secs(20));
    let since = threadspace_harness::now_ms();
    let pressed = press(ctx, &label);
    let cold = wait_new_companion(ctx, &[supervised.pid], 30)
        .ok_or("the notification did not start a companion")?;
    // Read while the cold-started process is alive: enable hands it over.
    let witness = launch_witness(cold.pid);
    let start = log.wait("CORE_START", &is_pid(cold.pid), 20);
    let state = log.wait("OBSERVATION_STATE", &is_pid(cold.pid), 20);
    let response = log.wait(
        "NOTIFICATION_RESPONSE",
        &|l| l["pid"] == cold.pid && l["attentionId"] == attention.as_str(),
        45,
    );
    let intent = app
        .wait_report(
            "notification-intent",
            since,
            |r| r["report"]["attentionId"].as_str() == Some(attention.as_str()),
            Duration::from_secs(60),
        )
        .map(|(_, r)| r["report"].clone());
    let enabled = service::bootstrap(&ctx.id, "enable");
    let owner_after = wait_login_item_owner(ctx, 60);
    let handed = checkpoint(ctx, "D-after-enable");
    run_dir
        .append("checkpoints.jsonl", &handed)
        .map_err(|e| e.to_string())?;
    let incumbent_exit = procs::wait_exit(&cold, Duration::from_secs(15));
    let (_, relaunched, waited) = kill_companion(ctx)?;
    let relaunch_state = log.wait("OBSERVATION_STATE", &is_pid(relaunched.pid), 20);
    let after = checkpoint(ctx, "D-after-crash");
    run_dir
        .append("checkpoints.jsonl", &after)
        .map_err(|e| e.to_string())?;
    let checks = json!({
        "stopped": stopped["ok"] == true && exited.is_some(),
        "notificationColdStart": submitted.is_some() && pressed["pressed"] == true
            && witness["xpcServiceName"].as_str().is_some_and(|n| n.starts_with(&format!("application.{}.", ctx.id.agent_identifier))),
        "coldStartUnsupervisedAndStopped": start.as_ref().is_some_and(|l| l["supervised"] == false) && state.as_ref().is_some_and(|l| l["observationEnabled"] == false && l["admissionOpen"] == false),
        "inspector": response.as_ref().is_some_and(|l| l["plan"] == "INSPECTOR") && intent.as_ref().is_some_and(|i| i["appliedAfterHydration"] == true),
        "enableHandsOverToLoginItem": enabled["ok"] == true && owner_after.is_some() && incumbent_exit.is_some() && single_writer(&handed) && login_item_owns(&handed),
        "crashRelaunchedSupervised": login_item_owns(&after) && after["locatorIncarnation"]["pid"] == relaunched.pid && relaunch_state.as_ref().is_some_and(|l| l["admissionOpen"] == true),
        "singleWriter": single_writer(&after),
    });
    let pass = checks
        .as_object()
        .is_some_and(|m| m.values().all(|v| v == true));
    record(
        run_dir,
        cases,
        "D-stop-cold-start-enable-crash",
        pass,
        json!({
            "checks": checks, "attentionId": attention, "stop": stopped, "coldStart": cold, "launchWitness": witness,
            "coreStart": start, "observationState": state, "notificationResponse": response, "viewIntent": intent,
            "enable": enabled, "ownerAfterMs": owner_after, "relaunched": relaunched, "relaunchWaitMs": waited, "relaunchState": relaunch_state,
        }),
    )
}

fn case_e(ctx: &Ctx, run_dir: &Run, log: &mut Log, cases: &mut Vec<Value>) -> Result<(), String> {
    ctx.app().stop_all();
    let supervised = ctx.companion().incarnation().ok_or("no companion")?;
    let unregistered = service::bootstrap(&ctx.id, "unregister");
    let exited = procs::wait_exit(&supervised, Duration::from_secs(20));
    let unavailable = checkpoint(ctx, "E-service-unavailable");
    run_dir
        .append("checkpoints.jsonl", &unavailable)
        .map_err(|e| e.to_string())?;
    let precondition = exited.is_some()
        && unavailable["writerLockFree"] == true
        && unavailable["loginItemPid"].is_null();
    let label = ctx.id.agent_identifier.clone();
    let variants: [(&str, Option<String>); 3] = [
        ("label-absent", None),
        ("shell-default-label", Some("0".to_owned())),
        ("login-item-label-without-launchd-parent", Some(label)),
    ];
    let mut results = Vec::new();
    for (name, value) in variants {
        let mut command = Command::new(&ctx.id.companion_executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match &value {
            Some(v) => command.env("XPC_SERVICE_NAME", v),
            None => command.env_remove("XPC_SERVICE_NAME"),
        };
        let mut child = command.spawn().map_err(|e| format!("{name}: {e}"))?;
        let pid = child.id() as i32;
        let start = log.wait("CORE_START", &is_pid(pid), 20);
        let state = log.wait("OBSERVATION_STATE", &is_pid(pid), 20);
        let ready = log.wait("CORE_READY", &is_pid(pid), 20);
        let witness = launch_witness(pid);
        let cursor_before = companion_snapshot(ctx).map(|(c, _)| c).ok();
        let capture_probe = admit(
            ctx,
            &uuid::Uuid::new_v4().to_string(),
            threadspace_harness::now_ms(),
        );
        let refresh_probe = ask(ctx, ControlRequestBody::RefreshEvidence);
        let diagnostics_seen = diagnostics(ctx);
        threadspace_harness::pause_ms(10_000);
        log.drain();
        let passes = log.of_pid(pid, "DISCOVERY_PASS").len();
        let cursor_after = companion_snapshot(ctx).map(|(c, _)| c).ok();
        let incarnation = Incarnation::of(pid);
        procs::signal(pid, libc::SIGTERM);
        let ended = incarnation
            .as_ref()
            .and_then(|i| procs::wait_exit(i, Duration::from_secs(10)));
        if ended.is_none() {
            procs::signal(pid, libc::SIGKILL);
        }
        let _ = child.wait();
        let lock_after = writer_lock_free(ctx);
        let checks = json!({
            "launchedByHarness": witness["parentPid"] == std::process::id() as i64,
            "classifiedUnknown": start.as_ref().is_some_and(|l| l["launchProvenance"] == "UNKNOWN" && l["supervised"] == false),
            "preferenceKeptEnabled": state.as_ref().is_some_and(|l| l["observationEnabled"] == true),
            "admissionClosed": state.as_ref().is_some_and(|l| l["admissionOpen"] == false),
            "controlAnswered": ready.is_some() && diagnostics_seen["launchProvenance"] == "UNKNOWN",
            "captureRefused": capture_probe["admitted"] != true && capture_probe["code"] == "NOT_SUPERVISED",
            "discoveryRefused": refresh_probe["code"] == "NOT_SUPERVISED",
            "noDiscoveryPass": passes == 0,
            "journalUnchanged": cursor_before.is_some() && cursor_before == cursor_after,
            "lockReleasedAfter": lock_after == true,
        });
        let pass = checks
            .as_object()
            .is_some_and(|m| m.values().all(|v| v == true));
        results.push(json!({ "variant": name, "pass": pass, "checks": checks, "launchWitness": witness,
            "coreStart": start, "observationState": state, "diagnostics": diagnostics_seen,
            "captureProbe": capture_probe, "refreshProbe": refresh_probe, "journalCursor": { "before": cursor_before, "after": cursor_after },
            "endedAfterMs": ended }));
    }
    let registered = service::bootstrap(&ctx.id, "register");
    let owner_after = wait_login_item_owner(ctx, 60);
    let after = checkpoint(ctx, "E-restored");
    run_dir
        .append("checkpoints.jsonl", &after)
        .map_err(|e| e.to_string())?;
    let pass = precondition
        && results.iter().all(|r| r["pass"] == true)
        && owner_after.is_some()
        && single_writer(&after);
    record(
        run_dir,
        cases,
        "E-unknown-or-malformed-provenance",
        pass,
        json!({
            "preconditionServiceUnavailable": precondition, "unregister": unregistered, "variants": results,
            "register": registered, "loginItemOwnerAfterMs": owner_after,
        }),
    )
}
