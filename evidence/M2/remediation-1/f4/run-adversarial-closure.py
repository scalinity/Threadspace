#!/usr/bin/env python3
"""Execute the repaired calculator against the independently found failures.

All fixtures and seals here are explicitly synthetic test data. This verifies
calculator rejection predicates; it does not implement or qualify a native
capture census, final producer acknowledgement, or platform clock profile.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
directory = Path(__file__).resolve().parent
repo = directory.parents[3]
tools = repo / "tests/native/tools"
output = directory / "adversarial-closure"
output.mkdir(exist_ok=True)
source_paths = [tools / "m2_latency.py", tools / "test_m2_latency.py"]
source_hashes = {str(path.relative_to(repo)): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_paths}
sys.path.insert(0, str(tools))
from m2_latency import analyze
from test_m2_latency import fixture


def observer_seal(raw):
    return next(seal for seal in raw["populationSeals"] if seal["source"] == "claude.observer")


def drop_tail(raw):
    raw["helperRecords"][1]["receipt"]["results"].pop()
    raw["helperRecords"][2]["records"].pop()
    raw["helperRecords"][2]["totalCaptured"] = 1
    raw["commitRecords"].pop()


def unsealed_producer(raw):
    raw["populationSeals"] = []
    raw["clockQualification"]["qualified"] = False
    raw["clockQualification"]["evidence"] = None


cases = {
    "positive-synthetic-sealed": (True, lambda raw: None),
    "wrong-store-generation": (False, lambda raw: [row.update(storeGeneration="OTHER_STORE") for row in raw["commitRecords"]]),
    "wrong-observer-source-epoch": (False, lambda raw: [row.update(sourceEpoch="OTHER_RUNTIME") for row in raw["commitRecords"] if row["source"] == "claude.observer"]),
    "missing-clock-rate": (False, lambda raw: raw["clockQualification"].pop("maximumRateErrorPpm")),
    "null-dom-identities": (False, lambda raw: raw["domPages"][0]["context"].update(storeGeneration=None, coreGeneration=None, viewEpoch=None)),
    "missing-observer-tail-seal": (False, lambda raw: raw["populationSeals"].remove(observer_seal(raw))),
    "wrong-tail-digest": (False, lambda raw: observer_seal(raw).update(observationIdsSha256="0" * 64)),
    "lost-tail-with-retained-final-census": (False, drop_tail),
    "unsealed-unqualified-producer": (False, unsealed_producer),
}
base = fixture()
assert base["executionKind"] == "SYNTHETIC_UNIT_TEST"
results = []
for name, (expected, change) in cases.items():
    raw = copy.deepcopy(base)
    change(raw)
    actual = analyze(raw)
    assert actual["normalPathPass"] is expected, (name, actual)
    assert actual["nativeExecution"] is False
    assert actual["verdict"] == "INCOMPLETE_OR_FAIL", "synthetic data must never become native qualification"
    if not expected:
        assert actual["errors"], name
    paths = {}
    for suffix, value in (("raw", raw), ("result", actual)):
        path = output / f"{name}.{suffix}.json"
        path.write_text(json.dumps(value, indent=2) + "\n")
        paths[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
    results.append({"case": name, "expectedNormalPathPass": expected, "normalPathPass": actual["normalPathPass"],
                    "nativeExecution": actual["nativeExecution"], "verdict": actual["verdict"], "errors": actual["errors"], "artifactHashes": paths})

argv = [sys.executable, "-m", "unittest", "discover", "-s", "tests/native/tools", "-p", "test_m2_latency.py", "-v"]
completed = subprocess.run(argv, cwd=repo, env=os.environ | {"PYTHONDONTWRITEBYTECODE": "1"},
                           text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=120)
log = output / "calculator-unittest.log"
log.write_text(completed.stdout)
assert completed.returncode == 0, completed.stdout
count = re.search(r"Ran (\d+) tests?", completed.stdout)
assert count and int(count[1]) == 21, completed.stdout
for path in source_paths:
    assert hashlib.sha256(path.read_bytes()).hexdigest() == source_hashes[str(path.relative_to(repo))], "source changed during execution"

summary = {
    "status": "CALCULATOR_COUNTEREXAMPLES_CLOSED_NATIVE_EVIDENCE_INCOMPLETE",
    "pythonVersion": sys.version.split()[0], "sourceHashes": source_hashes,
    "syntheticPositiveCases": 1, "negativeCasesRejected": len(results) - 1,
    "unittest": {"argv": argv, "exitCode": completed.returncode, "testsPassed": int(count[1]), "logSha256": hashlib.sha256(log.read_bytes()).hexdigest()},
    "cases": results,
    "limitations": [
        "The positive fixture's census seals and clock qualification are synthetic test oracles.",
        "This execution does not produce an independent native hook census or acknowledged final observer population.",
        "The current native exporter can verify an observer seal only from its independent close/echo records; the conventional-hook population remains unsealed and native clock evidence remains unqualified. F4 native qualification is incomplete.",
        "The pre-correction observer exporter tail-loss witness is preserved; only the calculator's ability to accept unsealed data is closed here.",
    ],
}
(output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps({"status": summary["status"], "syntheticPositiveCases": 1, "negativeCasesRejected": len(results) - 1,
                  "unitTestsPassed": int(count[1]), "sourceHashes": source_hashes}, indent=2))
