#!/usr/bin/env python3
"""Reproduce only the retained pre-correction calculator counterexamples."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
root = Path(__file__).resolve().parent
source = root / "calculator-before.py"
spec = importlib.util.spec_from_file_location("before_calculator", source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
mutations = {
    "wrong_store_generation": lambda raw: [row.update(storeGeneration="OTHER_STORE") for row in raw["commitRecords"]],
    "wrong_observer_source_epoch": lambda raw: [row.update(sourceEpoch="OTHER_RUNTIME") for row in raw["commitRecords"] if row["source"] == "claude.observer"],
    "missing_clock_rate_qualification": lambda raw: raw["clockQualification"].pop("maximumRateErrorPpm"),
    "null_dom_epoch_identity": lambda raw: raw["domPages"][0]["context"].update(storeGeneration=None, coreGeneration=None, viewEpoch=None),
}
results = {}
for label, change in mutations.items():
    raw = json.loads((root / "calculator-fixture-before.json").read_text())
    change(raw)
    result = module.analyze(raw)
    results[label] = {"syntheticWitness": True, "normalPathPass": result["normalPathPass"], "errors": result["errors"]}
    assert result["normalPathPass"], "this script documents the retained pre-correction defect"
output = {"calculatorSha256": hashlib.sha256(source.read_bytes()).hexdigest(), "results": results}
(root / "adversarial-calculator-before-reproduced.json").write_text(json.dumps(output, indent=2) + "\n")
print(json.dumps(output, indent=2))
