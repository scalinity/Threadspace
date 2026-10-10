#!/usr/bin/env python3
"""Discriminate a pre-COMMIT end stamp using the actual SQLite commit hook.

Copies current repository source into a newly acquired temporary directory,
moves only the measurement end stamp before COMMIT in that copy, and requires
the production journal test to fail at its boundary assertion. No shared source
or owner database is changed. Cargo/Rust 1.99.0 must be available on PATH.
"""
import difflib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent / "commit-boundary"
OUT.mkdir(exist_ok=True)
names = subprocess.check_output([
    "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
], cwd=ROOT).decode().split("\0")
selected = sorted({name for name in names if name and not name.startswith(("evidence/", "docs/"))
                   and "__pycache__" not in Path(name).parts and (ROOT / name).is_file()})
consumed = [name for name in selected if name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml")
            or name.startswith(("crates/contracts/src/", "crates/journal/src/", "crates/state-engine/src/", "third_party/"))
            or name in ("crates/contracts/Cargo.toml", "crates/journal/Cargo.toml", "crates/state-engine/Cargo.toml")]
source_hashes = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in consumed}
with tempfile.TemporaryDirectory(prefix="threadspace-f4-commit-mutant-") as temporary:
    work = Path(temporary)
    for name in selected:
        target = work / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / name, target)
    path = work / "crates/journal/src/canonical.rs"
    original = path.read_text()
    old = ('        tx.commit()?;\n'
           '        #[cfg(feature = "qualification")]\n'
           '        { outcome.commit_timing = crate::latency::finish(commit_begin); }')
    new = ('        #[cfg(feature = "qualification")]\n'
           '        { outcome.commit_timing = crate::latency::finish(commit_begin); }\n'
           '        tx.commit()?;')
    if original.count(old) != 1:
        raise SystemExit("expected unique actual post-COMMIT measurement boundary")
    changed = original.replace(old, new)
    path.write_text(changed)
    (OUT / "precommit-end-stamp.patch").write_text("".join(difflib.unified_diff(
        original.splitlines(True), changed.splitlines(True),
        fromfile="a/crates/journal/src/canonical.rs", tofile="b/crates/journal/src/canonical.rs")))
    owned_tmp = work / "owned-test-fixtures"
    owned_tmp.mkdir()
    environment = dict(os.environ, TMPDIR=str(owned_tmp), CARGO_TARGET_DIR=str(work / "target"))
    argv = ["cargo", "+1.99.0", "test", "-p", "threadspace-journal", "--lib",
            "--features", "qualification", "--target", "x86_64-unknown-linux-gnu",
            "--offline", "--locked",
            "latency::tests::commit_end_follows_actual_sqlite_commit_hook_not_writer_receipt", "--", "--nocapture"]
    result = subprocess.run(argv, cwd=work, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = result.stdout.decode(errors="replace")
    (OUT / "precommit-end-stamp.log").write_bytes(result.stdout)
    killed = (result.returncode == 101
              and "COMMIT end must follow SQLite's actual commit hook" in output
              and "1 failed" in output)
    after = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in consumed}
    summary = {
        "nativeMacosExecution": False, "actualSqliteCommitHook": True,
        "mutation": "Move qualification end stamp before actual tx.commit without changing its boundary label.",
        "argv": argv, "exitCode": result.returncode, "killedByBoundaryAssertion": killed,
        "sharedSourceUnchanged": after == source_hashes, "sourceHashes": source_hashes,
        "mutatedCanonicalSha256": hashlib.sha256(changed.encode()).hexdigest(),
        "logSha256": hashlib.sha256(result.stdout).hexdigest(),
    }
    (OUT / "precommit-end-stamp.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({key: value for key, value in summary.items() if key != "sourceHashes"}))
    print(output[-3500:])
    raise SystemExit(0 if killed and summary["sharedSourceUnchanged"] else 1)
