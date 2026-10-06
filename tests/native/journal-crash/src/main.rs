//! M0C SQLite crash/receipt and backup/restore qualification runner (G10).
//! Never shipped. It re-executes itself as worker processes that hold the
//! real `WriterLock` and `Journal`, kills them at commit/receipt
//! boundaries, reopens and asserts. See `evidence/M0C/sqlite/README.md`.

mod cases;
mod fixture;
mod harness;
mod worker;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use serde_json::{Value, json};
use threadspace_journal::{CRASH_AT_ENV, CRASH_POINT_ENV};

use crate::cases::Crash;
use crate::fixture::{Flags, Rng, now_ms};
use crate::harness::{Ctx, Outcome};

const USAGE: &str = "usage: threadspace-journal-crash run --out DIR [--runs 25] [--fixed-runs 5] \
                     [--seed N] [--tmp-root /private/tmp/claude-501] [--keep]";
const DEFAULT_SEED: u64 = 0x6d30_6310;
const MAX_RECORDS: u64 = 60;

type FixedCase = fn(&mut Ctx, &mut Outcome, u16) -> Result<(), String>;

const FIXED: [(&str, FixedCase); 7] = [
    ("in-transaction-spill", cases::in_transaction_spill),
    ("wal-replay", cases::wal_replay),
    ("writer-lock", cases::writer_lock),
    ("backup-concurrent", cases::backup_concurrent),
    ("restore", cases::restore_success),
    ("restore-rejected", cases::restore_rejected),
    ("restore-without-lock", cases::restore_without_lock),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]).unwrap_or_else(|detail| {
            eprintln!("journal-crash: {detail}");
            ExitCode::from(2)
        }),
        Some("worker") => worker::main(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(64)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let flags = Flags::parse(args, &["keep"])?;
    let out_dir: PathBuf = flags.get("out", None)?;
    let runs: u16 = flags.get("runs", Some(25))?;
    let fixed_runs: u16 = flags.get("fixed-runs", Some(5))?;
    let seed: u64 = flags.get("seed", Some(DEFAULT_SEED))?;
    let tmp_root: PathBuf =
        flags.get("tmp-root", Some(PathBuf::from("/private/tmp/claude-501")))?;
    if std::env::var_os(CRASH_POINT_ENV).is_some() || std::env::var_os(CRASH_AT_ENV).is_some() {
        return Err(format!(
            "unset {CRASH_POINT_ENV}/{CRASH_AT_ENV}: the parent must never arm a crash point in itself"
        ));
    }

    std::fs::create_dir_all(&out_dir).map_err(|error| format!("{}: {error}", out_dir.display()))?;
    let results_path = out_dir.join("results.jsonl");
    let results = std::fs::File::create(&results_path)
        .map_err(|error| format!("{}: {error}", results_path.display()))?;
    let started = now_ms();
    std::fs::create_dir_all(&tmp_root)
        .map_err(|error| format!("{}: {error}", tmp_root.display()))?;
    let root = tmp_root.join(format!(
        "threadspace-journal-crash-{}-{started}",
        std::process::id()
    ));
    std::fs::create_dir(&root).map_err(|error| format!("{}: {error}", root.display()))?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    eprintln!("journal-crash: stores under {}", root.display());

    let mut ctx = Ctx::new(exe, root.clone(), results);
    let mut rng = Rng::new(seed);
    for kind in Crash::ALL {
        for run in 0..runs {
            let n = rng.range(2, MAX_RECORDS);
            let k = match run {
                0 => 1,
                1 => n,
                _ => rng.range(1, n),
            };
            let mut outcome = Outcome::new(kind.name(), run);
            let result = cases::crash(&mut ctx, &mut outcome, kind, n, k);
            ctx.finish(outcome, result);
        }
    }
    for run in 0..fixed_runs {
        for (name, case) in FIXED {
            let mut outcome = Outcome::new(name, run);
            let result = case(&mut ctx, &mut outcome, run);
            ctx.finish(outcome, result);
        }
    }

    let summary = summary(&ctx, seed, runs, fixed_runs, started);
    let summary_path = out_dir.join("summary.json");
    let text = serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?;
    std::fs::write(&summary_path, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", summary_path.display()))?;
    if flags.switch("keep") {
        eprintln!("journal-crash: kept {}", root.display());
    } else {
        std::fs::remove_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))?;
    }
    println!(
        "{} {}",
        summary["result"].as_str().unwrap_or("FAIL"),
        summary["crashMatrix"]
    );
    Ok(if ctx.violations.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn summary(ctx: &Ctx, seed: u64, runs: u16, fixed_runs: u16, started: i64) -> Value {
    let crash_names: Vec<&str> = Crash::ALL.iter().map(|kind| kind.name()).collect();
    let (mut crash_runs, mut acked, mut lost, mut rows, mut admissions, mut unacked) =
        (0, 0, 0, 0, 0, 0);
    let mut cases = serde_json::Map::new();
    for (name, tally) in &ctx.tallies {
        if crash_names.contains(&name.as_str()) {
            crash_runs += tally.runs;
            acked += tally.acked;
            lost += tally.lost;
            rows += tally.duplicate_rows;
            admissions += tally.duplicate_admissions;
            unacked += tally.unacked_present;
        }
        cases.insert(
            name.clone(),
            json!({
                "runs": tally.runs,
                "passed": tally.passed,
                "ackedRecords": tally.acked,
                "lostAckedRecords": tally.lost,
                "duplicateRows": tally.duplicate_rows,
                "duplicateAdmissions": tally.duplicate_admissions,
                "unackedPresent": tally.unacked_present,
            }),
        );
    }
    json!({
        "runner": "threadspace-journal-crash",
        "result": if ctx.violations.is_empty() { "PASS" } else { "FAIL" },
        "startedAtMs": started,
        "finishedAtMs": now_ms(),
        "seed": seed,
        "runsPerCrashPoint": runs,
        "fixedRuns": fixed_runs,
        "environment": environment(),
        "engine": {
            "sample": ctx.engine_sample,
            "reopenChecks": ctx.engine_checks,
            "engineViolations": ctx.engine_violations,
            "fullfsyncValuesSeen": ctx.fullfsync_seen,
        },
        "durabilityDomain": "Ordinary process death (SIGKILL, no unwinding) on the local APFS volume with \
            journal_mode=WAL, synchronous=FULL, fullfsync off. Power-loss durability is not qualified here (M13).",
        "crashMatrix": {
            "runs": crash_runs,
            "ackedRecords": acked,
            "lostAckedRecords": lost,
            "duplicateRows": rows,
            "duplicateAdmissions": admissions,
            "unackedPresent": unacked,
        },
        "cases": cases,
        "violations": ctx.violations,
    })
}

fn environment() -> Value {
    let read = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    json!({
        "productVersion": read("/usr/bin/sw_vers", &["-productVersion"]),
        "buildVersion": read("/usr/bin/sw_vers", &["-buildVersion"]),
        "machine": read("/usr/bin/uname", &["-m"]),
        "kernel": read("/usr/bin/uname", &["-r"]),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
    })
}
