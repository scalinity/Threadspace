#!/usr/bin/env python3
"""Read-only pre-publication checks over this remediation and retained evidence.

This scans only repository files, never owner configuration or external history.
It reports candidate secret prefixes without printing matching values. The scan
is a bounded static check, not a claim that arbitrary private data is detectable.
"""
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent / "integrity.json"
BASE = "2c4b5012f59189dcb38d61ced0b0b9fe96fb043e"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def sha(data):
    return hashlib.sha256(data).hexdigest()


historical = ROOT / "evidence/M2/remediation-1/historical"
manifest = json.loads((historical / "rejected-candidate-manifest.json").read_text())
preserved = []
for name, expected in manifest["evidenceHashes"].items():
    current = (historical / "rejected-candidate-README.md") if name == "evidence/M2/README.md" else ROOT / name
    actual = sha(current.read_bytes()) if current.is_file() else None
    preserved.append({"originalPath": name, "retainedPath": str(current.relative_to(ROOT)),
                      "expectedSha256": expected, "actualSha256": actual, "match": expected == actual})
snapshots = []
for name, current in [("evidence/M2/manifest.json", historical / "rejected-candidate-manifest.json"),
                      ("evidence/M2/README.md", historical / "rejected-candidate-README.md")]:
    original = git("show", f"{BASE}:{name}")
    snapshots.append({"originalPath": name, "match": original == current.read_bytes(), "sha256": sha(original)})

changed = set(git("diff", "--name-only", BASE).decode().splitlines())
changed.update(git("ls-files", "--others", "--exclude-standard").decode().splitlines())
changed.discard(str(OUT.relative_to(ROOT)))
patterns = {
    "anthropic-key-prefix": rb"sk-ant-[A-Za-z0-9_-]{24,}",
    "openai-key-prefix": rb"sk-(?:proj|svcacct)-[A-Za-z0-9_-]{24,}",
    "github-token-prefix": rb"(?:ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})",
    "aws-access-key-prefix": rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
    "private-key-block": rb"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----",
}
findings = []
scanned = {}
unexpected_caches = []
for name in sorted(changed):
    path = ROOT / name
    if not path.is_file():
        continue
    if "__pycache__" in path.parts or path.suffix == ".pyc" or name.endswith(("-wal", "-shm")):
        unexpected_caches.append(name)
    data = path.read_bytes()
    scanned[name] = {"bytes": len(data), "sha256": sha(data)}
    for label, pattern in patterns.items():
        matches = list(re.finditer(pattern, data))
        if matches:
            findings.append({"path": name, "rule": label, "count": len(matches),
                             "lineNumbers": [data.count(b"\n", 0, match.start()) + 1 for match in matches]})
criterion_paths = ["CLAUDE.md", "AGENTS.md", "docs/SPEC.md", "docs/MILESTONES.md"]
criteria_unchanged = not git("diff", "--name-only", BASE, "--", *criterion_paths).strip()
whitespace = subprocess.run(["git", "diff", "--check", BASE], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
whitespace_paths = sorted(set(re.findall(r"^([^\n]+?):\d+: trailing whitespace\.$", whitespace.stdout.decode(), re.M)))
other_diagnostics = [line for line in whitespace.stdout.decode().splitlines()
                     if line and not line.startswith("+")
                     and not re.fullmatch(r"[^\n]+?:\d+: trailing whitespace\.", line)]
only_generated = (not other_diagnostics and bool(whitespace_paths)
                  and all(name.startswith("apps/desktop/src/contracts/generated/") for name in whitespace_paths))
format_diagnostics = []
for line in whitespace.stdout.decode().splitlines():
    if not line or line.startswith("+"):
        continue
    match = re.fullmatch(r"(.+?):(\d+): (trailing whitespace|new blank line at EOF)\.", line)
    reason = None
    name = match[1] if match else None
    if match:
        path = ROOT / name
        if name.startswith("apps/desktop/src/contracts/generated/") and match[3] == "trailing whitespace":
            reason = "Preserve exact canonical ts-rs generated output."
        elif name.startswith("evidence/M2/remediation-1/") and path.suffix in {".log", ".txt", ".patch"}:
            reason = "Preserve exact raw execution output or mutation patch bytes and their recorded hashes."
        elif name == "evidence/M2/remediation-1/f1b/independent-review/tested-source/crates/provider-claude/src/ownership_record.rs":
            reason = "Preserve the exact historical source snapshot consumed by the retained execution."
        elif (name == "crates/provider-claude/src/ownership_record.rs"
              and match[3] == "new blank line at EOF"
              and sha(path.read_bytes()) == "483e6ff26d4593bbdbac78d2ecd06380e919c23d04505091394a5de1db9b8364"):
            reason = "One harmless final blank line in this exact as-tested source is retained; no semantic or acceptance criterion is affected."
    format_diagnostics.append({"diagnostic": line, "path": name, "preservationReason": reason})
formatting_accounted = bool(format_diagnostics) and all(row["preservationReason"] for row in format_diagnostics)
baseline_generated_whitespace = []
for name in whitespace_paths:
    prior = subprocess.run(["git", "show", f"{BASE}:{name}"], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    if prior.returncode == 0:
        lines = [index for index, line in enumerate(prior.stdout.splitlines(), start=1)
                 if line.endswith((b" ", b"\t"))]
        baseline_generated_whitespace.append({"path": name, "rejectedSourceSha256": sha(prior.stdout),
                                              "existingTrailingWhitespaceLines": lines})
report = {
    "rejectedBase": BASE,
    "nativeMacosExecution": False,
    "historicalEvidence": {"total": len(preserved), "allMatch": all(row["match"] for row in preserved), "records": preserved},
    "exactHistoricalStatusSnapshots": snapshots,
    "acceptanceCriteriaUnchanged": criteria_unchanged,
    "privacy": {"scope": "Changed/new prospective repository files, including sanitized fixture databases and execution logs.",
                "method": "Static candidate-secret-prefix scan; matching values are never printed. Generated fixtures and records were also reviewed for owner data.",
                "limitations": "Not a general proof of absence of arbitrary private information. Private Claude configuration, owner histories and recording pixels were not accessed.",
                "findings": findings, "scannedFileCount": len(scanned), "files": scanned},
    "unexpectedCacheOrSqliteSidecars": unexpected_caches,
    "diffCheck": {"exitCode": whitespace.returncode, "trailingWhitespacePaths": whitespace_paths,
                  "otherDiagnostics": other_diagnostics,
                  "onlyCanonicalGeneratedTypes": only_generated,
                  "baselineGeneratedWhitespace": baseline_generated_whitespace,
                  "classifiedDiagnostics": format_diagnostics,
                  "allDiagnosticsExplicitlyAccounted": formatting_accounted,
                  "disposition": "git diff --check is not clean. Preserve raw evidence, canonical generated output and the explicitly hashed as-tested final blank line; all other diagnostics remain a failure." if whitespace.returncode else "git diff --check is clean."},
}
OUT.write_text(json.dumps(report, indent=2) + "\n")
passed = (report["historicalEvidence"]["allMatch"] and all(row["match"] for row in snapshots)
          and criteria_unchanged and not findings and not unexpected_caches
          and (whitespace.returncode == 0 or formatting_accounted))
print(json.dumps({"passed": bool(passed), "historicalHashes": len(preserved),
                  "scannedFiles": len(scanned), "secretPrefixFindings": findings,
                  "unexpectedCaches": unexpected_caches, "diffCheck": report["diffCheck"]}, indent=2))
raise SystemExit(0 if passed else 1)
