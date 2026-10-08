//! `threadspace-m1`: the M1 qualification runner. Each subcommand writes its
//! evidence under `evidence/M1/<area>/` and prints its summary. No model
//! calls, no installed identity's store: synthetic histories, disposable
//! stores and the real capture executable.
//!
//! ```text
//! threadspace-m1 fixtures                 # fixture catalog + fixtures/m1/*.jsonl
//! threadspace-m1 replay                   # exact replay digests per scenario
//! threadspace-m1 permutations [count]     # >= count seeded valid permutations (default 10000)
//! threadspace-m1 crash                    # 100 commit/ACK crash injections
//! threadspace-m1 capture <hook> [runs]    # native capture timing + fail-open + saturation
//! threadspace-m1 migration                # M0 store upgrade, future-schema refusal
//! threadspace-m1 contracts                # versions, digests, schema validation
//! threadspace-m1 verify <journal.jsonl>   # replay a journal export from genesis
//! ```

mod capture;
mod contracts;
mod crash;
mod evidence;
mod migration;
mod permutations;
mod replay;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn repo_root() -> Result<PathBuf, String> {
    let dir = std::env::current_dir().map_err(|e| e.to_string())?;
    if !dir.join("docs/MILESTONES.md").exists() {
        return Err("run from the repository root".into());
    }
    Ok(dir)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = (|| -> Result<serde_json::Value, String> {
        if args.get(1).map(String::as_str) == Some("crash-worker") {
            let store = args.get(2).ok_or("store")?;
            let acks = args.get(3).ok_or("acks")?;
            crash::worker(Path::new(store), Path::new(acks))?;
            return Ok(serde_json::json!({ "worker": "finished without a crash" }));
        }
        let repo = repo_root()?;
        let root = repo.join("evidence/M1");
        match args.get(1).map(String::as_str) {
            Some("fixtures") => replay::catalog_fixtures(&repo, &root),
            Some("replay") => replay::replay_hashes(&root),
            Some("permutations") => {
                let count = args.get(2).and_then(|n| n.parse().ok()).unwrap_or(10_000);
                permutations::run_all(&root, count)
            }
            Some("crash") => {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                crash::matrix(&root, &exe)
            }
            Some("capture") => {
                let hook = args.get(2).ok_or("capture <hook executable> [runs]")?;
                let runs = args.get(3).and_then(|n| n.parse().ok()).unwrap_or(1000);
                capture::measure(&root, Path::new(hook), runs)
            }
            Some("migration") => migration::run(&repo, &root),
            Some("contracts") => contracts::run(&repo, &root),
            Some("verify") => replay::verify(Path::new(args.get(2).ok_or("verify <journal.jsonl>")?)),
            _ => Err("usage: threadspace-m1 fixtures|replay|permutations|crash|capture|migration|contracts|verify".into()),
        }
    })();
    match result {
        Ok(summary) => {
            println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            if summary.get("pass") == Some(&serde_json::Value::Bool(false)) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("threadspace-m1: {error}");
            ExitCode::FAILURE
        }
    }
}
