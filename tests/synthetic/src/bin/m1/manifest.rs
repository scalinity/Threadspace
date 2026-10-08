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
    let guard = std::fs::read_to_string(root.join("remediation/d0008-guard/summary.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let generator_orders = std::fs::read_to_string(root.join("remediation-2/generator-orders.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let evidence = |path: &str| {
        std::fs::read_to_string(root.join(path))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null)
    };
    // The reducer-1 upgrade wherever its newest checkpoint sits.
    let tail = evidence("remediation-3/checkpoint-tail.json");
    let pair = evidence("remediation-3/unordered-pair.json");
    let tail_rows: Vec<Value> = tail["variants"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|v| {
                    json!({
                        "variant": v["variant"], "newestCheckpointBefore": v["checkpointsBefore"].as_array().and_then(|c| c.last()),
                        "suffixEntriesReplayed": v["suffixEntriesReplayed"], "outbox": v["counts"],
                        "stateSha256": v["digest"]["stateSha256"], "tablesSha256": v["digest"]["tablesSha256"],
                        "semanticSha256": v["digest"]["semanticSha256"],
                        "stateDifferencesFromA": v["stateDifferencesFromA"].as_array().map(Vec::len),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let tail_pass = tail_rows.len() == 3
        && tail_rows.iter().all(|v| {
            v["outbox"] == json!({ "PENDING": 1, "HELD": 4, "SUPPRESSED": 1 })
                && v["stateDifferencesFromA"] == json!(0)
                && v["stateSha256"] == tail_rows[0]["stateSha256"]
        })
        && pair["stateDifferencesXY"] == json!([]);
    let oracle: Vec<Value> = std::fs::read_to_string(root.join("remediation/c04-oracle/retained-runs.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    // The C-04 run native.json names must pass the retired-native oracle.
    let c04_oracle = oracle
        .iter()
        .find(|row| native["c04"]["run"].as_str().is_some_and(|run| row["run"].as_str() == Some(run)))
        .cloned()
        .unwrap_or(Value::Null);
    let areas_pass = [&contracts, &replay, &permutations, &crash, &capture, &migration, &sanitization, &guard, &c04_oracle]
        .iter()
        .all(|summary| summary["pass"] == true)
        && tail_pass;
    let manifest = json!({
        "schema": 1,
        "milestone": "M1",
        "title": "Journal, Contracts and Deterministic Synthetic Harness",
        "verdict": if areas_pass && native["pass"] == true { "M1 REMEDIATION CANDIDATE — pending independent re-review" } else { "M1 NOT READY" },
        "remediation": {
            "base": "d93b0fb2f7fafd97a0a7fc9ad5a19267800c51fd", "record": "evidence/M1/remediation/README.md", "d0008Guard": guard, "c04RetiredNative": c04_oracle,
            "second": {
                "base": "f7e9a6ce2ce034e04abff03bf8a358dc98a98e01",
                "record": "evidence/M1/remediation-2/README.md",
                "groups": ["2: wait owner coverage and semantic equality", "5: WAL store without -shm refused without creating a sidecar"],
                "negativeControls": "evidence/M1/remediation-2/negative-controls/",
                "previousEvidence": "evidence/M1/history/f7e9a6c/",
                "generatorOrders": generator_orders,
                "evidenceSources": {
                    "regenerated": { "areas": ["contracts", "fixtures", "replay", "permutations", "crash", "migration"], "sourceCommit": "05a9a2eaaab72c0ed08ea2ac9e5cfb5f31bd9f9a" },
                    "retained": { "areas": ["capture", "sanitization"], "sourceCommit": "e52c281f8cc1cb998ea64bcdf8010afeae6400e7", "reason": "hook, relay, spool and companion code unchanged since; the journal changes (wait reduction, which no hook event reaches; the preflight branch for a WAL without -shm; the reducer-upgrade load path) are not reached by those workloads" },
                    "native": { "m0b": "build 5944c81, not reinstalled: a reducer-2 build would upgrade the owner's live store", "c04": "build 73636ec" },
                },
            },
            "third": {
                "base": "85d188e221c24a4e91a533d0ab743fa0e1184c80",
                "record": "evidence/M1/remediation-3/README.md",
                "defect": "a reducer-1 store whose newest checkpoint was followed by journal entries upgraded with fresh PENDING intents for attention reducer 2 first derived while replaying them",
                "repair": "6ed18f0b11856120c956b42f620001127fa3b66b",
                "tests": "2e42561c339ea42955151af4219c295fb5984db3",
                "checkpointTail": { "pass": tail_pass, "variants": tail_rows, "unorderedPair": { "X": pair["X"]["outbox"], "Y": pair["Y"]["outbox"], "stateDifferencesXY": pair["stateDifferencesXY"] } },
                "negativeControls": "evidence/M1/remediation-3/negative-controls/",
                "evidenceSources": {
                    "regenerated": { "areas": ["migration"], "sourceCommit": "2e42561c339ea42955151af4219c295fb5984db3", "note": "every field reproduced except the per-run file SHA-256 of fresh stores" },
                    "reproduced": { "areas": ["contracts", "fixtures", "replay", "crash"], "sourceCommit": "2e42561c339ea42955151af4219c295fb5984db3", "note": "files byte-identical to those regenerated at 05a9a2e" },
                    "retained": { "areas": ["permutations"], "sourceCommit": "05a9a2eaaab72c0ed08ea2ac9e5cfb5f31bd9f9a", "reason": "reducer and admission unchanged; every permutation store is written by reducer 2, so its load takes the unchanged same-reducer branch" },
                },
            },
        },
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
            "target/aarch64-apple-darwin/release/threadspace-m1 fixtures|replay|permutations 20000|crash|migration|contracts",
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
            "C-04 closes under D-0008, a containment scoped to tao 0.37.0: any Tauri, tao or Wry update first removes it and requalifies view recovery without it.",
            "C-04's memory criterion is bounded-run evidence (second-half footprint slope over 60 recoveries): it does not exclude a leak that starts late, grows intermittently or stays under the bound.",
            "Three fields stay last-observation until M2 converts them (D-0007 §10): execution attach mode/presence, the human follow-up frontier and the observer link state; no M1 producer emits conflicting observations of them.",
            "Near the spool's record bound one publication's listing and size pass take hundreds of milliseconds, so a capture can reach the 250 ms watchdog and be lost without a marker; mod-batch publishes its records in sequence and a large batch can do the same (the observer mod, M2, sizes its batches).",
            "In a simultaneous burst of 16 capture processes about 1% were refused as spoolbusy (debug build, loaded machine): a recorded loss, not a stall.",
            "A store left with a hot rollback journal is opened writable by the preflight, so SQLite rolls it back even if it is then refused as too new; the rollback restores its last committed bytes. This is an accepted exception, not a necessity: refusing such a store from a private copy would also be possible.",
            "G08's full Terminal.app restart stays BLOCKED on this owner machine (D-0006 C-08, M15).",
            "On build 5944c81, 9 of G08's 10 selection-readback-race routes over two runs were refused conservatively (READBACK_FAILED; Terminal answered the focus script's first AppleEvent with -600 while the harness activated Terminal) and 1 was focused exactly: 0 wrong targets, and ordinary exact Return passed. The race setup and its overlap with each route are not fully attested; the cause is not established; M5 owns the investigation.",
            "A reducer-1 wait owner decision kept only whether it covered positives without a causal point; the upgrade reads it as covering one, in the checkpoint or replayed after it, so an item whose owner covered two or more such positives in one reducer-1 decision is shown again after the upgrade.",
            "G08's fullscreen-space-then-return case recorded null fullscreen entry and exit witnesses on build 5944c81: it is not a fresh fullscreen-transition qualification; the accepted M0C H-10 evidence stays authoritative.",
        ],
    });
    area.json("manifest.json", &manifest)?;
    Ok(manifest)
}
