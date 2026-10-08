//! Composes `evidence/M1/manifest.json` from the areas' own summaries, the
//! repository's git identity and the platform, so no figure is copied by
//! hand. Native areas (C-04 view recovery, M0B regression) are referenced by
//! the run directories their runners wrote.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use crate::evidence::{Area, sha256_file};

fn read(root: &Path, area: &str) -> Value {
    std::fs::read_to_string(root.join(area).join("summary.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

fn output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

pub fn compose(repo: &Path, root: &Path, native: &Value) -> Result<Value, String> {
    let area = Area::new(root, ".")?;
    let contracts = read(root, "contracts");
    let replay = read(root, "replay");
    let permutations = read(root, "permutations");
    let crash = read(root, "crash");
    let capture = read(root, "capture");
    let migration = read(root, "migration");
    let sanitization = read(root, "sanitization");
    let fixtures = std::fs::read_to_string(root.join("fixtures/catalog.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let replay_rows: Vec<Value> = replay["scenarios"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    json!({
                        "scenario": r["scenario"], "journalSha256": r["journal"]["sha256"],
                        "journalRange": [r["journal"]["firstCursor"], r["journal"]["lastCursor"]],
                        "entries": r["journal"]["entries"], "facts": r["journal"]["facts"],
                        "checkpointSha256": r["checkpointSha256"], "stateSha256": r["stateSha256"],
                        "projectionSha256": r["projectionSha256"], "semanticSha256": r["semanticSha256"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let areas_pass = [&contracts, &replay, &permutations, &crash, &capture, &migration, &sanitization]
        .iter()
        .all(|summary| summary["pass"] == true);
    let manifest = json!({
        "schema": 1,
        "milestone": "M1",
        "title": "Journal, Contracts and Deterministic Synthetic Harness",
        "verdict": if areas_pass && native["pass"] == true { "M1 PASS CANDIDATE — pending independent review" } else { "M1 NOT READY" },
        "branch": output("git", &["rev-parse", "--abbrev-ref", "HEAD"]),
        "baseCommit": "cd9e37645adf7e6b5f74ab7f0baa5197d8e08b54",
        "sourceCommit": output("git", &["rev-parse", "HEAD"]),
        "decision": "docs/decisions/D-0007-m1-canonical-engine.md",
        "reducerInvariantCatalog": "evidence/M1/reducer-invariants.md",
        "platform": {
            "os": format!("macOS {} ({})", output("sw_vers", &["-productVersion"]), output("sw_vers", &["-buildVersion"])),
            "hardware": output("sysctl", &["-n", "machdep.cpu.brand_string"]),
            "rustc": output("rustc", &["--version"]),
            "sqlite": threadspace_journal::REQUIRED_SQLITE_VERSION,
        },
        "versions": contracts["versions"],
        "migrations": contracts["migrations"],
        "generatedArtifacts": {
            "typescript": { "files": contracts["generated"]["typescript"]["files"], "aggregateSha256": contracts["generated"]["typescript"]["aggregateSha256"] },
            "jsonSchema": { "files": contracts["generated"]["jsonSchema"]["files"], "aggregateSha256": contracts["generated"]["jsonSchema"]["aggregateSha256"], "freshAgainstRust": contracts["generated"]["jsonSchema"]["freshAgainstRust"] },
            "instanceValidation": contracts["instanceValidation"],
        },
        "fixtures": {
            "scenarios": fixtures["scenarios"],
            "catalog": "evidence/M1/fixtures/catalog.json",
            "m0StoreV2Sha256": sha256_file(&repo.join("fixtures/m1/m0-store-v2/journal.sqlite3")),
            "regressionSeedsSha256": sha256_file(&repo.join("fixtures/m1/regression-seeds.json")),
        },
        "deterministicSeeds": {
            "scheme": permutations["seedScheme"],
            "families": permutations["families"],
            "replaySeed": replay["seed"],
        },
        "replay": { "pass": replay["pass"], "repeats": replay["repeats"], "contract": replay["contract"], "scenarios": replay_rows },
        "propertyTests": {
            "pass": permutations["pass"],
            "requiredFamilyPermutations": permutations["requiredFamilyPermutations"],
            "totalPermutations": permutations["totalPermutations"],
            "sqliteStepwiseCrossChecks": permutations["sqliteCrossChecks"],
            "failures": permutations["failures"],
            "preservedFailingSeeds": "fixtures/m1/regression-seeds.json",
        },
        "crashInjection": {
            "pass": crash["pass"], "injections": crash["injections"], "points": crash["points"],
            "acknowledgedLost": crash["acknowledgedLost"], "duplicateFacts": crash["duplicateFacts"],
        },
        "capture": {
            "pass": capture["pass"], "targets": capture["targets"], "normal": capture["normal"],
            "failurePaths": capture["failurePaths"], "saturation": capture["saturation"], "hookSha256": capture["hookSha256"],
        },
        "migration": migration,
        "sanitization": { "pass": sanitization["pass"], "storeScan": sanitization["storeScan"], "spoolPath": sanitization["spoolPath"] },
        "carryovers": {
            "C-04": native["c04"],
            "C-12": { "status": "CLOSED", "evidence": "evidence/M1/c12/README.md" },
            "C-13": { "status": "CLOSED", "evidence": "evidence/M1/c13/README.md" },
        },
        "m0bCompatibility": native["m0b"],
        "testCommands": [
            "cargo test --workspace --features threadspace-journal/qualification,threadspace-agent/qualification",
            "npx vitest run",
            "THREADSPACE_CHECK_SCHEMAS=1 cargo test -p threadspace-contracts --test schemas",
            "cargo build --release -p threadspace-synthetic --bin threadspace-m1 -p threadspace-relay --bin threadspace-hook",
            "target/aarch64-apple-darwin/release/threadspace-m1 fixtures|replay|permutations 10000|crash|migration|contracts",
            "target/aarch64-apple-darwin/release/threadspace-m1 capture target/aarch64-apple-darwin/release/threadspace-hook 1000",
            "target/aarch64-apple-darwin/release/threadspace-m1 sanitization target/aarch64-apple-darwin/release/threadspace-hook",
        ],
        "nativeBuild": native["build"],
        "knownLimitations": [
            "Canonical attention creates durable outbox intents; their OS delivery is M5 (D-0007 §8).",
            "Claude inventory waiting/waitingFor stay display metadata until M2 maps them to session-scoped wait episodes (D-0007 §8).",
            "Claude conventional hooks are interpreted at the classic-limited profile only; native turn identity and outcomes from the observer mod are M2 (D-0007 §7).",
            "Durability is qualified against ordinary process crashes (SIGKILL) under WAL with synchronous=FULL; power-loss durability is not claimed.",
            "The capture benchmark runs the release hook against the fixture companion (real writer and event socket, disposable store), not against an installed identity's store.",
            "A single-write log record narrows, but does not eliminate, a torn record on a mid-write kill; readers skip and count torn lines (C-13).",
        ],
    });
    area.json("manifest.json", &manifest)?;
    Ok(manifest)
}
