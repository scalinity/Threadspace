#!/usr/bin/env python3
"""Retain source-bound macOS-target checks; this never executes native code."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent / "build-checks"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def source_hashes() -> dict[str, str]:
    names = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
    ).decode().split("\0")
    selected = []
    for name in names:
        path = Path(name)
        if name in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml"}:
            selected.append(name)
        elif name.startswith(("crates/", "apps/agent-macos/", "tests/native/harness/", "tests/synthetic/", "packages/provider-mod/")):
            if path.suffix in {".rs", ".toml", ".lock", ".h", ".m", ".c", ".ts", ".json"}:
                selected.append(name)
    return {name: digest((ROOT / name).read_bytes()) for name in sorted(set(selected)) if (ROOT / name).is_file()}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-dir", required=True)
    args = parser.parse_args()
    OUT.mkdir(exist_ok=True)
    before = source_hashes()
    checks = []
    for phase in ("check", "clippy"):
        command = [
            "cargo", "+1.99.0", phase,
            "-p", "threadspace-provider-claude", "-p", "threadspace-relay", "-p", "threadspace-harness",
            "--all-targets", "--features", "threadspace-relay/qualification",
            "--target", "aarch64-apple-darwin", "--target-dir", str(Path(args.target_dir).resolve()),
            "--offline", "--locked",
        ]
        if phase == "clippy":
            command += ["--", "-D", "warnings"]
        start = time.monotonic()
        log = OUT / f"macos-target-{phase}-final.log"
        with log.open("wb") as stream:
            completed = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
        checks.append({"command": command, "exitCode": completed.returncode,
                       "elapsedSeconds": time.monotonic() - start,
                       "log": log.name, "logSha256": digest(log.read_bytes())})
        print(json.dumps({"phase": phase, "exitCode": completed.returncode, "log": str(log)}), flush=True)
    after = source_hashes()
    summary = {
        "schemaVersion": 1,
        "scope": "macOS-target Rust typecheck and Clippy; no macOS link, application build, or native execution",
        "host": platform.platform(), "nativeExecution": False,
        "toolchain": subprocess.check_output(["rustc", "+1.99.0", "--version"], text=True).strip(),
        "target": "aarch64-apple-darwin", "checks": checks,
        "sourceHashesBefore": before, "sourceHashesAfter": after,
        "sourcesUnchanged": before == after,
        "pass": before == after and all(row["exitCode"] == 0 for row in checks),
        "limitations": [
            "Darwin process and scripting paths were not executed on this Linux host.",
            "The outer Tauri application and native Objective-C companion were not built or packaged.",
            "This record does not establish any executable hash or native timing result.",
        ],
    }
    (OUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    return 0 if summary["pass"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
