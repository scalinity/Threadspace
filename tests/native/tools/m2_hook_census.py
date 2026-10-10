#!/usr/bin/env python3
"""Controlled native hook issuance, independently counted before helper output.

This is a deterministic conventional-hook fixture, not Claude inference. Real
provider/observer evidence accompanies it separately. No ordinary hook changes.
The deliberately lost invocation uses a fixture-owned file as an impossible
store directory: both delivery/spool and local telemetry fail, fail-open exit
is retained, and the independently issued denominator stays unchanged.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import time
import uuid


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def issue(helper, root, count, loss):
    session = str(uuid.uuid4())
    ledger = []
    impossible = root / "not-a-directory"
    impossible.write_text("owned double-loss fixture\n")
    ledger_path = root / "issuance.json"
    for index in range(count):
        identity = str(uuid.uuid4())
        row = {"issuanceId": identity, "index": index, "sessionId": session,
               "event": "UserPromptSubmit", "issuedUnixNs": str(time.time_ns()),
               "issuedMonotonicNs": str(time.monotonic_ns()), "doubleLoss": loss == index}
        ledger.append(row)
        # The harness records the expected invocation before launching it.
        # No provider is held waiting for measurement I/O.
        write(ledger_path, {"closed": False, "expectedIssued": count, "records": ledger})
        argv = [str(helper), "hook", "--agent", "ai.scalinity.threadspace.dev.agent",
                "--qualification-latency"]
        if loss == index:
            argv += ["--store-dir", str(impossible)]
        data = json.dumps({"hook_event_name": "UserPromptSubmit", "session_id": session,
                           "prompt": f"THREADSPACE_M2_CENSUS_{index}", "cwd": str(root)})
        started = time.time_ns()
        process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        row["pid"] = process.pid
        row["launchUnixNs"] = str(started)
        try:
            stdout, stderr = process.communicate(data.encode(), timeout=1)
            row["timedOut"] = False
        except subprocess.TimeoutExpired:
            # Only this exact child, acquired from Popen, is killed.
            process.kill()
            stdout, stderr = process.communicate()
            row["timedOut"] = True
        row.update({"exitCode": process.returncode, "stdout": stdout.decode(errors="replace"),
                    "stderr": stderr.decode(errors="replace"), "endedUnixNs": str(time.time_ns()),
                    "endedMonotonicNs": str(time.monotonic_ns())})
        write(ledger_path, {"closed": False, "expectedIssued": count, "records": ledger})
        time.sleep(0.025)
    closed = {"closed": True, "expectedIssued": count, "actualIssued": len(ledger),
              "allChildrenExited": all(r.get("exitCode") == 0 and not r.get("timedOut") for r in ledger),
              "helperSha256": hashlib.sha256(helper.read_bytes()).hexdigest(), "records": ledger}
    write(ledger_path, closed)
    return closed


def verify(ledger, db):
    source = sqlite3.connect(db.as_uri() + "?mode=ro", uri=True, timeout=2)
    session = ledger["records"][0]["sessionId"]
    rows = source.execute("SELECT observation_id,payload_json FROM observations WHERE source_id='claude.hook' AND json_extract(payload_json,'$.sessionKey.nativeSessionId')=?", (session,)).fetchall()
    source.close()
    joins, missing, used = [], [], set()
    for entry in ledger["records"]:
        matches = []
        for obs, payload in rows:
            data = json.loads(payload)
            captures = [e for e in data.get("evidence", []) if e.get("role") == "CAPTURE"]
            at = data["capturedAt"]["wallTimeMs"] * 1_000_000
            if any(e["key"]["pid"] == entry["pid"] for e in captures) and int(entry["launchUnixNs"]) - 1_000_000 <= at <= int(entry["endedUnixNs"]) + 1_000_000:
                matches.append(obs)
        if len(matches) != 1 or matches[0] in used:
            missing.append({"issuanceId": entry["issuanceId"], "index": entry["index"], "matchingUUIDs": matches})
        else:
            used.add(matches[0])
            joins.append({"issuanceId": entry["issuanceId"], "observationId": matches[0], "pid": entry["pid"]})
    return {"source": "claude.hook", "independentExpectedIssued": ledger["expectedIssued"],
            "actualIssued": ledger["actualIssued"], "joinedAccepted": len(joins), "missingInvocations": missing,
            "unexpectedAcceptedUUIDs": sorted(set(r[0] for r in rows) - used), "joins": joins,
            "closed": ledger["closed"] and ledger["allChildrenExited"],
            "complete": ledger["closed"] and ledger["allChildrenExited"] and len(joins) == ledger["expectedIssued"] and len(rows) == len(joins)}


def attach(raw_path, ledger_path, census):
    raw = json.loads(raw_path.read_text())
    ids = sorted(r["observationId"] for r in census["joins"])
    captured = sorted(r["observationId"] for r in raw["helperRecords"] if r.get("kind") == "hook-capture")
    if not census["complete"] or captured != ids or len(ids) != len(set(ids)):
        raise ValueError("independent issued population does not exactly cover native hook telemetry")
    if any(r.get("source") == "claude.hook" for r in raw["populationSeals"]):
        raise ValueError("raw already has a hook seal")
    seal = {"source": "claude.hook", "bootId": raw["bootId"], "boundary": "CLOSED_CAPTURE_WINDOW",
            "sourceBoundary": "ALL_INDEPENDENTLY_ISSUED_CHILDREN_EXITED", "sealed": True,
            "totalCaptured": census["independentExpectedIssued"], "telemetryFailures": 0,
            "observationIdsSha256": hashlib.sha256("\n".join(ids).encode()).hexdigest(),
            "evidence": [{"issuanceLedger": str(ledger_path),
                          "issuanceLedgerSha256": hashlib.sha256(ledger_path.read_bytes()).hexdigest(),
                          "census": census}]}
    raw["populationSeals"].append(seal)
    raw["hookPopulationCensus"] = census
    raw["derivedFrom"] = {"rawSha256": hashlib.sha256(raw_path.read_bytes()).hexdigest(),
                          "change": "join independently issued hook census; clock remains unqualified"}
    output = raw_path.with_name("raw-with-hook-census.json")
    if output.exists():
        raise ValueError("derived raw result already exists; preserve it")
    write(output, raw)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--journal", type=Path, required=True)
    parser.add_argument("--count", type=int, default=20)
    parser.add_argument("--double-loss-index", type=int)
    parser.add_argument("--attach-raw", type=Path)
    args = parser.parse_args()
    if not args.helper.is_absolute() or not args.helper.is_file() or args.count < 1 or args.count > 100:
        raise SystemExit("absolute installed helper and count 1..100 required")
    if args.double_loss_index is not None and not 0 <= args.double_loss_index < args.count:
        raise SystemExit("invalid double-loss index")
    if args.attach_raw:
        ledger = json.loads((args.out / "issuance.json").read_text())
        if ledger["helperSha256"] != hashlib.sha256(args.helper.read_bytes()).hexdigest():
            raise SystemExit("helper changed since independently issued workload")
    else:
        args.out.mkdir(mode=0o700, parents=False, exist_ok=False)
        ledger = issue(args.helper, args.out, args.count, args.double_loss_index)
    census = verify(ledger, args.journal)
    census.update({"fixtureKind": "controlled native conventional-hook issuance; not real inference",
                   "issuanceLedgerSha256": hashlib.sha256((args.out / "issuance.json").read_bytes()).hexdigest()})
    if args.attach_raw:
        attach(args.attach_raw, args.out / "issuance.json", census)
    else:
        write(args.out / "census.json", census)
    print(json.dumps({k: census[k] for k in ["independentExpectedIssued", "actualIssued", "joinedAccepted", "missingInvocations", "complete"]}))
    if args.double_loss_index is not None:
        assert len(census["missingInvocations"]) == 1 and census["missingInvocations"][0]["index"] == args.double_loss_index and not census["complete"]
    else:
        assert census["complete"]


if __name__ == "__main__":
    main()
