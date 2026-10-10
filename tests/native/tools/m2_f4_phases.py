#!/usr/bin/env python3
"""Disposable launch-phase control for the qualification-startup feature.

Two acquired executable inodes, first/repeat on each. No provider launch,
live store, cache eviction, signing change, or F4 percentile qualification.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from m2_f4_startup import batch, identity, native_ns, save


def account(parent, diagnostics, boot):
    assert parent["clock"] == "CLOCK_UPTIME_RAW" and parent["bootId"] == boot
    helper, emitted = [json.loads(line) for line in diagnostics.splitlines()]
    assert helper["kind"] == "qualification-startup"
    assert emitted["kind"] == "qualification-startup-emitted"
    assert helper["bootId"] == emitted["bootId"] == boot
    assert helper["clock"] == emitted["clock"] == "CLOCK_UPTIME_RAW"
    stamps = helper["stamps"]
    pairs = {
        "launchToMain": (parent["parentLaunchRequestNs"], stamps["mainEntryNs"]),
        "spawnCall": (parent["parentLaunchRequestNs"], parent["successfulSpawnReturnNs"]),
        "mainToInitialized": (stamps["mainEntryNs"], stamps["initializationCompletedNs"]),
        "initializedToModBatch": (stamps["initializationCompletedNs"], stamps["modBatchEntryNs"]),
        "modBatchToLocator": (stamps["modBatchEntryNs"], stamps["locatorReadNs"]),
        "locatorToReceiptReady": (stamps["locatorReadNs"], stamps["receiptReadyNs"]),
        "receiptReadyToFlush": (stamps["receiptReadyNs"], stamps["stdoutFlushCompletedNs"]),
        "flushToReap": (stamps["stdoutFlushCompletedNs"], parent["parentReapNs"]),
        "diagnosticEmission": (helper["diagnosticEmissionStartedNs"], emitted["diagnosticEmissionCompletedNs"]),
        "launchToReap": (parent["parentLaunchRequestNs"], parent["parentReapNs"]),
    }
    for before, after in pairs.values():
        assert isinstance(before, int) and isinstance(after, int) and after >= before
    assert parent["successfulSpawnReturnNs"] <= parent["parentReapNs"]
    return {"helper": helper, "emission": emitted,
            "phaseMs": {name: (b - a) / 1e6 for name, (a, b) in pairs.items()},
            "finalProcessExitNs": None,
            "exitLimitation": "Parent reap follows child termination and output collection; it is not exact exit."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--private-output", type=Path, required=True)
    args = parser.parse_args()
    root = args.private_output.resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    boot = subprocess.check_output(["sysctl", "-n", "kern.bootsessionuuid"], text=True).strip()
    source = identity(args.helper)
    rows = []
    for copy in range(2):
        helper = root / f"helper-{copy}"
        shutil.copy2(args.helper, helper)
        acquired = identity(helper)
        for call in range(2):
            store = root / f"store-{copy}-{call}"
            store.mkdir(mode=0o700)
            argv = [str(helper), "mod-batch", "--store-dir", str(store), "--budget-ms", "80"]
            request = batch("6d1c7f0e-1111-4aaa-8bbb-000000000001", record=True).encode()
            env = {**os.environ, "THREADSPACE_QUALIFY_STARTUP": "1"}
            requested = native_ns()
            child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, env=env)
            spawned = native_ns()
            stdout, stderr = child.communicate(request, timeout=4)
            reaped = native_ns()
            parent = {"parentLaunchRequestNs": requested, "successfulSpawnReturnNs": spawned,
                      "parentReapNs": reaped, "clock": "CLOCK_UPTIME_RAW", "bootId": boot}
            row = {"copy": copy, "call": call, "firstExecutionOfAcquiredInode": call == 0,
                   "fileIdentity": acquired, "parent": parent, "exitCode": child.returncode,
                   "receipt": json.loads(stdout), "rawDiagnostics": stderr.decode(),
                   "readySpoolRecords": len(list((store / "capture-spool/ready").glob("*.json")))}
            # Preserve raw observations before interpreting or asserting them.
            save(root / f"raw-{copy}-{call}.json", row)
            row.update(account(parent, stderr.decode(), boot))
            assert child.returncode == 0
            assert row["receipt"]["results"][0]["status"] == "LOCAL_SPOOLED"
            assert row["readySpoolRecords"] == 1
            assert identity(helper) == acquired
            rows.append(row)
    assert subprocess.check_output(["sysctl", "-n", "kern.bootsessionuuid"], text=True).strip() == boot
    save(root / "summary.json", {"clock": "CLOCK_UPTIME_RAW", "bootId": boot,
         "sourceExecutable": source, "rows": rows, "cacheEviction": False,
         "population": "Two acquired inodes, first and repeat; global OS caches uncontrolled."})
    print(json.dumps([{key: row[key] for key in ["copy", "call", "phaseMs"]} for row in rows], indent=2))


if __name__ == "__main__":
    main()
