#!/usr/bin/env python3
"""F1B portable qualification, without replacing any product or oracle source.

The synthetic package's manifest pulls native-only executables into Linux
builds. An acquired disposable wrapper points at its exact original library,
test and permutation-generator files and omits those unused dependencies.
All dependency version/source/checksum tuples must match repository Cargo.lock.
No native macOS execution or end-to-end acceptance is claimed by this runner.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import tomllib


REPO = Path(__file__).resolve().parents[3]
REGRESSION_TESTS = ["scenarios", "sqlite", "reducer_upgrade", "reducer2_upgrade", "checkpoint_tail_upgrade"]


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sources(scope: str = "core") -> dict[str, str]:
    files = {REPO / "Cargo.toml", REPO / "Cargo.lock", Path(__file__).resolve()}
    for directory in ["crates/contracts", "crates/state-engine", "crates/journal", "tests/synthetic"]:
        files.update((REPO / directory).rglob("*.rs"))
        files.update((REPO / directory).glob("Cargo.toml"))
    if scope == "core":
        for file in ["crates/provider-claude/src/profiles.rs", "crates/provider-claude/src/observer.rs", "crates/provider-claude/src/ownership_record.rs", "crates/relay/src/modbatch.rs"]:
            files.add(REPO / file)
        # The journal qualification test includes these production modules
        # by path. Fingerprint the complete include chain.
        files.update((REPO / "crates/relay/src").glob("latency*.rs"))
        files.add(REPO / "evidence/M2/remediation-1/f1a/observer-witnesses.json")
        files.add(REPO / "evidence/M2/remediation-1/f1b/reducer-3-store/journal.sqlite3")
        files.add(REPO / "evidence/M2/remediation-1/f1b/reducer-3-native-only-store/journal.sqlite3")
    elif scope == "synthetic":
        # A dependency's integration tests are not compiled by this wrapper;
        # in particular it does not consume F1A or relay latency modules.
        files = {path for path in files if not any(str(path.relative_to(REPO)).startswith(f"crates/{crate}/tests/") for crate in ["contracts", "state-engine", "journal"])}
    else:
        raise ValueError(f"unsupported source scope: {scope}")
    vendor = REPO / "third_party/libsqlite3-sys"
    files.add(vendor / "Cargo.toml")
    for suffix in ["*.rs", "*.c", "*.h"]:
        files.update(vendor.rglob(suffix))
    for directory in ["fixtures/m1/reducer-1-store", "fixtures/m2/reducer-2-store"]:
        files.update(path for path in (REPO / directory).rglob("*") if path.is_file() and path.suffix in {".sqlite3", ".json", ".rs"})
    return {str(path.relative_to(REPO)): sha(path) for path in sorted(files)}


def command(out: Path, name: str, argv: list[str], cwd: Path, extra_env: dict[str, str] | None = None, expected_failure: bool = False, source_scope: str = "core") -> dict[str, object]:
    before = sources(source_scope)
    started = time.time_ns()
    env = os.environ.copy()
    env.update(extra_env or {})
    log = out / f"{name}.log"
    with log.open("w") as stream:
        result = subprocess.run(argv, cwd=cwd, env=env, stdout=stream, stderr=subprocess.STDOUT, check=False)
    after = sources(source_scope)
    text = log.read_text()
    passed = (result.returncode == 0) if not expected_failure else (
        result.returncode == 101 and "NEGATIVE CONTROL: relabeled genuine reducer-3 checkpoint" in text
        and "assertion `left == right` failed" in text and "Some(Restored)" in text
    )
    record = {
        "command": argv, "cwd": str(cwd), "environmentOverrides": extra_env or {},
        "startedUnixNs": started, "endedUnixNs": time.time_ns(), "exitCode": result.returncode,
        "expectedFailure": expected_failure, "pass": passed,
        "sourceHashesBefore": before, "sourceHashesAfter": after,
        "sourceChangedDuringExecution": before != after, "logSha256": sha(log),
        "nativeMacOSExecution": False, "sourceFingerprintScope": source_scope,
    }
    write_json(out / f"{name}.json", record)
    print(f"{name}: exit={result.returncode}, expectedFailure={expected_failure}, sourceStable={before == after}", flush=True)
    if not passed or before != after:
        raise RuntimeError(f"{name}: see {log}; an execution failure or changed source is not a passing qualification")
    return record


def wrapper(root: Path) -> Path:
    package = root / "tests" / "synthetic"
    package.mkdir(parents=True)
    (root / "fixtures").symlink_to(REPO / "fixtures", target_is_directory=True)
    (root / "evidence").symlink_to(REPO / "evidence", target_is_directory=True)
    manifest = f'''[package]
name = "threadspace-synthetic"
version = "0.1.0"
edition = "2024"
rust-version = "1.99"
publish = false
autobins = false
autotests = false

[workspace]
resolver = "3"

[lib]
path = "{REPO}/tests/synthetic/src/lib.rs"

[dependencies]
threadspace-contracts = {{ path = "{REPO}/crates/contracts" }}
threadspace-journal = {{ path = "{REPO}/crates/journal", features = ["qualification"] }}
threadspace-state-engine = {{ path = "{REPO}/crates/state-engine", features = ["synthetic"] }}
rusqlite = {{ version = "=0.40.2", features = ["bundled"] }}
serde = {{ version = "=1.0.229", features = ["derive"] }}
serde_json = "=1.0.151"
sha2 = "=0.11.0"
uuid = {{ version = "=1.27.0", features = ["v4", "serde"] }}
libc = "=0.2.190"

[patch.crates-io]
libsqlite3-sys = {{ path = "{REPO}/third_party/libsqlite3-sys" }}
'''
    for name in REGRESSION_TESTS:
        manifest += f'\n[[test]]\nname = "{name}"\npath = "{REPO}/tests/synthetic/tests/{name}.rs"\n'
    manifest += '\n[[bin]]\nname = "f1b-permutations"\npath = "permutations-main.rs"\n'
    (package / "Cargo.toml").write_text(manifest)
    shutil.copyfile(REPO / "Cargo.lock", package / "Cargo.lock")
    (package / "permutations-main.rs").write_text(f'''#[allow(dead_code)]
#[path = "{REPO}/tests/synthetic/src/bin/m1/evidence.rs"]
mod evidence;
#[path = "{REPO}/tests/synthetic/src/bin/m1/permutations.rs"]
mod permutations;
fn main() {{
    let output = std::env::args_os().nth(1).expect("explicit qualification output");
    let result = permutations::run_all(std::path::Path::new(&output), 20_000).expect("campaign");
    println!("{{}}", serde_json::to_string_pretty(&result).expect("summary"));
    assert_eq!(result["pass"], true, "unchanged generator and oracle must pass");
}}
''')
    return package


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--phase", choices=["all", "core", "synthetic"], default="all", help="Separate independent source-bound phases; a partial campaign never reports full PASS")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    cargo = [args.cargo, "+1.99.0"]
    target = ["--target", "x86_64-unknown-linux-gnu", "--locked", "--offline"]
    if args.phase in {"all", "core"}:
        command(out, "portable-core", cargo + ["test", "-p", "threadspace-contracts", "-p", "threadspace-state-engine", "-p", "threadspace-journal", "--features", "threadspace-state-engine/synthetic,threadspace-journal/qualification"] + target, REPO, {"THREADSPACE_F1B_EVIDENCE_DIR": str(out / "records")})
        command(out, "migration-negative-control", cargo + ["test", "-p", "threadspace-journal", "--test", "observer_ownership"] + target + ["f1b_genuine", "--", "--nocapture"], REPO, {"THREADSPACE_F1B_NEGATIVE_SKIP_UPGRADE": "1"}, expected_failure=True)
    if args.phase in {"all", "synthetic"}:
        synthetic_phase(out, cargo, target)
    phase_paths = [out / f"{name}.json" for name in ["portable-core", "migration-negative-control", "retained-migration-and-scenarios", "permutation-campaign"]]
    complete = all(path.exists() for path in phase_paths)
    current = complete and all(
        record["pass"] and not record["sourceChangedDuringExecution"] and record["sourceHashesAfter"] == sources(record["sourceFingerprintScope"])
        for record in (json.loads(path.read_text()) for path in phase_paths)
    )
    permutation_summary = out / "permutations" / "summary.json"
    write_json(out / "portable-summary.json", {
        "pass": bool(current), "completeCampaign": complete, "requestedPhase": args.phase,
        "nativeMacOSExecution": False,
        "remainingGate": "New source-matched macOS proof producer and unchanged-ID reload/new-Turn smoke",
        "reducerVersion": 4, "factPayloadVersion": 2, "journalPayloadVersion": 2,
        "permutations": json.loads(permutation_summary.read_text()) if permutation_summary.exists() else None,
    })


def synthetic_phase(out: Path, cargo: list[str], target: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="threadspace-f1b-portable-") as owned:
        package = wrapper(Path(owned))
        subprocess.run(cargo + ["generate-lockfile", "--offline"], cwd=package, check=True)
        original = tomllib.loads((REPO / "Cargo.lock").read_text())["package"]
        selected = tomllib.loads((package / "Cargo.lock").read_text())["package"]
        identity = lambda item: (item["name"], item["version"], item.get("source"), item.get("checksum"))
        original_keys = {identity(item) for item in original}
        assert all(identity(item) in original_keys for item in selected), "wrapper may not alter dependency pins"
        for name in ["Cargo.toml", "Cargo.lock", "permutations-main.rs"]:
            shutil.copyfile(package / name, out / ("portable-wrapper-" + name))
        write_json(out / "portable-wrapper.json", {"allDependencyPinsMatchRepository": True, "packageCount": len(selected), "manifestSha256": sha(package / "Cargo.toml"), "qualification": "Original Rust source and test assertions; only unused native dependencies omitted"})
        argv = cargo + ["test"] + target
        for test in REGRESSION_TESTS:
            argv += ["--test", test]
        command(out, "retained-migration-and-scenarios", argv, package, source_scope="synthetic")
        command(out, "permutation-campaign", cargo + ["run", "--bin", "f1b-permutations"] + target + ["--", str(out)], package, source_scope="synthetic")


if __name__ == "__main__":
    main()
