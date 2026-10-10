#!/usr/bin/env python3
"""Generate the final evidence index from immutable committed Git blobs.

Run after the source/evidence commit. The generated manifest and provenance
are then committed separately; they do not change product or test semantics.
There is no self-referential final-commit or manifest hash.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
BASE = "2c4b5012f59189dcb38d61ced0b0b9fe96fb043e"
MAIN = "af9b285da529890bc441ea00f1a92e73e39902a8"
OLD_NATIVE = "f4ef5f156f7086962bafa92728cc4cace5a58c3c"
MANIFEST = "evidence/M2/manifest.json"
PROVENANCE = "evidence/M2/remediation-1/provenance.json"


def git(*args: str) -> bytes:
    return subprocess.check_output(["git", *args], cwd=ROOT)


def blob(revision: str, name: str) -> bytes:
    return git("show", f"{revision}:{name}")


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def category(name: str) -> str:
    if name.startswith("evidence/"):
        return "evidence_and_reproduction"
    if name.startswith("docs/"):
        return "decision_documentation"
    if "/contracts/generated/" in name or "/schemas/" in name:
        return "generated_contracts"
    if name.startswith("tests/") or "/tests/" in name or ".test." in name:
        return "harness_and_tests"
    if name.startswith("apps/desktop/src/qualification/") or "/latency" in name:
        return "qualification_instrumentation"
    return "product_and_build"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source_commit", help="Committed source/evidence revision; must be current HEAD on first execution")
    args = parser.parse_args()
    source = git("rev-parse", f"{args.source_commit}^{{commit}}").decode().strip()
    if git("rev-parse", "HEAD").decode().strip() != source:
        raise SystemExit("Refusing: generation must run from the exact committed source HEAD.")
    subprocess.run(["git", "merge-base", "--is-ancestor", BASE, source], cwd=ROOT, check=True)
    subprocess.run(["git", "merge-base", "--is-ancestor", MAIN, source], cwd=ROOT, check=True)
    names = git("ls-tree", "-r", "--name-only", source).decode().splitlines()
    changed = git("diff", "--name-only", BASE, source).decode().splitlines()
    # All hashes below come from committed bytes, never the mutable worktree.
    evidence = {name: sha(blob(source, name)) for name in names
                if name.startswith("evidence/M2/remediation-1/") and name != PROVENANCE}
    historical = json.loads(blob(source, "evidence/M2/remediation-1/historical/rejected-candidate-manifest.json"))
    historical_checks = []
    for name, expected in historical["evidenceHashes"].items():
        retained = "evidence/M2/remediation-1/historical/rejected-candidate-README.md" if name == "evidence/M2/README.md" else name
        actual = sha(blob(source, retained))
        historical_checks.append({"originalPath": name, "retainedPath": retained,
                                  "sha256": actual, "matchesRejectedManifest": actual == expected})
        evidence[retained] = actual
    if not all(row["matchesRejectedManifest"] for row in historical_checks):
        raise SystemExit("Refusing: a preserved historical evidence digest differs.")
    source_paths = [name for name in changed if name in names and not name.startswith(("evidence/", "docs/"))]
    source_hashes = {name: sha(blob(source, name)) for name in source_paths}
    groups: dict[str, list[str]] = {}
    for name in changed:
        groups.setdefault(category(name), []).append(name)
    provenance = {
        "schemaVersion": 1, "status": "M2 REMEDIATION INCOMPLETE", "branch": "m2",
        "acceptedMain": MAIN, "rejectedCandidate": BASE,
        "sourceCommit": source, "harnessCommit": source,
        "sourceTree": git("rev-parse", f"{source}^{{tree}}").decode().strip(),
        "currentNativeBuild": {"status": "NOT_BUILT_OR_EXECUTED", "sourceCommit": None,
                               "bundleId": "ai.scalinity.threadspace.dev", "executableHashes": None},
        "historicalNativeBuildSource": OLD_NATIVE,
        "changedFilesByCategory": groups,
        "changedSourceHashesFromCommittedBlobs": source_hashes,
        "preservedHistoricalEvidence": historical_checks,
        "evidenceHashMethod": "SHA-256 of git show SOURCE:path for committed source/evidence revision; no mutable file or prose-derived digest.",
        "evidenceFileCount": len(evidence),
        "finalCommitPolicy": {
            "allowedChangesAfterSource": [MANIFEST, PROVENANCE],
            "requiredVerification": "The final commit must differ from source only at these two evidence index paths. Report final HEAD and remote equality separately.",
            "finalCommitSha": "Read from git rev-parse HEAD after committing this provenance; intentionally not self-referential.",
        },
        "decisions": {"D-0009": "PROPOSED — pending independent re-review",
                      "D-0010": "PROPOSED — pending independent re-review"},
        "limitations": [
            "Portable execution and macOS-target typecheck are not native application qualification.",
            "Changed ownership/integration/measurement paths have no new source-matched Dev build or native result.",
            "F3 has zero current positive repetitions; five within 2000 ms remain mandatory.",
            "F4 lacks an independent hook census, qualified native clocks and complete native sample populations.",
        ],
    }
    manifest = json.loads(blob(source, MANIFEST))
    manifest["sourceCommit"] = source
    manifest["harnessCommit"] = source
    manifest["sourceIdentityNote"] = "This source/harness commit contains the tested implementation and evidence. The final provenance-only commit changes only this manifest and remediation-1/provenance.json; native build remains unexecuted."
    manifest["reducer"]["journalPayloadVersion"] = 2
    manifest["evidenceHashes"] = dict(sorted(evidence.items()))
    (ROOT / PROVENANCE).write_text(json.dumps(provenance, indent=2, ensure_ascii=False) + "\n")
    (ROOT / MANIFEST).write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({"sourceCommit": source, "changedSourceFiles": len(source_hashes),
                      "evidenceFiles": len(evidence), "historicalHashesMatched": len(historical_checks),
                      "nativeBuild": "NOT_BUILT_OR_EXECUTED"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
