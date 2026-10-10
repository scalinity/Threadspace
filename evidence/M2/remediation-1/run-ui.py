#!/usr/bin/env python3
"""Run the actual desktop checks; keep raw output and exact consumed source hashes.

Requires the repository's npm dependencies and Node on PATH. This performs no
native execution and does not install or start the application.
"""
import datetime
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent / "ui"
OUT.mkdir(exist_ok=True)


def hashes():
    names = subprocess.check_output([
        "git", "ls-files", "--cached", "--others", "--exclude-standard",
        "apps/desktop/src", "apps/desktop/package.json", "apps/desktop/tsconfig.json",
        "packages/scene", "package.json", "package-lock.json",
    ], cwd=ROOT, text=True).splitlines()
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
            for name in sorted(set(names)) if (ROOT / name).is_file()}


before = hashes()
results = []
for name, argv in [
    ("typecheck", ["npm", "run", "typecheck", "--workspace", "@threadspace/desktop"]),
    ("vitest", ["npm", "test", "--workspace", "@threadspace/desktop"]),
]:
    start = time.monotonic()
    result = subprocess.run(argv, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (OUT / f"{name}.log").write_bytes(result.stdout)
    results.append({"name": name, "argv": argv, "exitCode": result.returncode,
                    "elapsedSeconds": time.monotonic() - start,
                    "log": f"evidence/M2/remediation-1/ui/{name}.log"})
after = hashes()
summary = {
    "executedAtUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "platform": platform.platform(), "nativeMacosExecution": False,
    "node": subprocess.check_output(["node", "--version"], text=True).strip(),
    "headAtExecution": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
    "sourceIdentity": "Uncommitted remediation source is identified by exact file hashes below.",
    "sourceHashesBefore": before, "sourceHashesAfter": after,
    "sourceStable": before == after, "results": results,
}
(OUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps({"sourceStable": before == after, "results": results}))
raise SystemExit(0 if before == after and all(row["exitCode"] == 0 for row in results) else 1)
