#!/usr/bin/env python3
"""Schema 3 composed causal durations. Never translates JS clock origins.

The old m2_latency.py and schema 2 evidence remain independent. Inputs are
the original COMMIT logs, independent helper receipts, source censuses and
native post-DOM records, with a predeclared installed instrument profile.
"""
from __future__ import annotations
import argparse
from collections import defaultdict
from decimal import Decimal, ROUND_CEILING
import hashlib
import json
import uuid
from pathlib import Path

from m2_latency import integer, number, percentiles, SOURCES, TARGETS
from m2_socket_duration import validate_binding

METHOD = "composed-duration-v1"

def canonical_uuid(value):
    try: return isinstance(value, str) and str(uuid.UUID(value)) == value
    except (ValueError, AttributeError): return False


def analyze(raw):
    errors, exclusions, joined = [], [], []
    profile = raw.get("instrumentProfile", {})
    boot = raw.get("bootId")
    if raw.get("schemaVersion") != 3 or raw.get("method") != METHOD:
        errors.append("wrong composed-duration schema/method")
    if not boot or profile.get("bootId") != boot or profile.get("qualified") is not True:
        errors.append("missing qualified same-boot instrument profile")
    if profile.get("scope") != "INSTALLED_ENGINEERING_DURATION" or profile.get("nativeClock") != "CLOCK_UPTIME_RAW" or profile.get("observerClock") != "performance.now":
        errors.append("unqualified instrument scope/units")
    if profile.get("universalRateBound") is not False or not all(profile.get(key) for key in ("providerSha256", "adapterSha256", "conformanceSha256", "policySha256", "osBuild")):
        errors.append("missing installed instrument identities or uncertainty authority")
    conformance = raw.get("conformance", {})
    if raw.get("executionKind") == "NATIVE" and conformance.get("executionKind") != "NATIVE_INSTALLED_PROVIDER":
        errors.append("native F4 samples lack an installed-provider conformance")
    identity = raw.get("artifactIdentity", {})
    checked, binding_errors = validate_binding(conformance, profile, identity)
    errors.extend(binding_errors)
    try:
        eo = Decimal(str(number(profile["observerAdditiveMs"])))
        en = Decimal(str(number(profile["nativeAdditiveMs"])))
        step = Decimal(str(number(profile["outwardStepMs"])))
        max_duration = number(profile["maximumDurationMs"])
        if eo <= 0 or en <= 0 or step <= 0 or max_duration <= 0:
            raise ValueError("uncertainty must be explicit and positive")
    except (KeyError, ValueError):
        eo = en = step = Decimal("0.001")
        max_duration = 0
        errors.append("missing/invalid explicit duration uncertainty")
    errors.extend(str(error) for error in raw.get("collectorErrors", []))
    if raw.get("suspendDiscontinuity") is not False:
        errors.append("missing same-boot suspend continuity")

    def upper(ms, epsilon):
        return float(((Decimal(str(ms)) + epsilon) / step).to_integral_value(rounding=ROUND_CEILING) * step)

    natives, attempts, candidates = {}, defaultdict(list), {}
    for item in raw.get("helperRecords", []):
        if item.get("kind") == "helper-receipt":
            token = item.get("token")
            if not canonical_uuid(token) or token in natives: errors.append("duplicate/unbound independent native receipt token")
            natives[token] = item
        elif item.get("kind") == "hook-capture":
            oid = item.get("observationId")
            if not canonical_uuid(oid): errors.append("invalid hook UUID")
            if oid in candidates: errors.append("duplicate original hook capture")
            candidates[oid] = {"source": "claude.hook", "epoch": item.get("sourceEpoch"), "capture": item.get("capture")}
    totals = {}
    for item in raw.get("helperRecords", []):
        if item.get("kind") != "observer-duration-attempt": continue
        try:
            epoch = item["runtimeId"]
            if not canonical_uuid(epoch): raise ValueError("invalid observer epoch")
            if epoch not in {r.get("runtimeId") for r in conformance.get("identities", {}).get("runtimes", [])}:
                raise ValueError("observer runtime is outside original conformance")
            begin, end = number(item["beginMs"]), number(item["endMs"])
            if item.get("schemaVersion") != 3 or item.get("clock") != "performance.now" or end < begin: raise ValueError("reset/units/schema mismatch")
            if item.get("overflow") != 0 or item.get("failedExports") != 0: raise ValueError("observer evidence loss")
            totals[epoch] = max(totals.get(epoch, 0), integer(item["totalCaptured"]))
            ids = [row["observationId"] for row in item["records"]]
            if len(ids) != len(set(ids)): raise ValueError("duplicate batch UUID")
            for record in item["records"]:
                oid = record["observationId"]
                if not canonical_uuid(oid): raise ValueError("invalid observation UUID")
                at = number(record["capturedMs"])
                if at > begin or record.get("phaseEntry") is not True: raise ValueError("not an original callback/phase entry reading")
                candidate = {"source": "claude.observer", "epoch": epoch, "capture": at}
                if oid in candidates and candidates[oid] != candidate: raise ValueError("retry changed capture UUID/epoch/reading")
                candidates[oid] = candidate
                attempts[oid].append((item, record))
        except (KeyError, TypeError, ValueError) as error:
            errors.append(f"invalid observer attempt: {error}")

    for epoch, total in totals.items():
        if total != sum(c["epoch"] == epoch and c["source"] == "claude.observer" for c in candidates.values()): errors.append("observer captured denominator mismatch")
    # Final positive seals, never the last successful prefix's counters.
    for source in SOURCES:
        seals = [s for s in raw.get("populationSeals", []) if s.get("source") == source]
        ids = sorted(oid for oid, c in candidates.items() if c["source"] == source)
        digest = hashlib.sha256("\n".join(ids).encode()).hexdigest()
        if len(seals) != 1:
            errors.append(f"missing final census: {source}"); continue
        seal = seals[0]
        if seal.get("boundary") != "CLOSED_CAPTURE_WINDOW" or seal.get("sealed") is not True or seal.get("bootId") != boot or not seal.get("evidence") or seal.get("telemetryFailures") != 0 or seal.get("totalCaptured") != len(ids) or seal.get("observationIdsSha256") != digest:
            errors.append(f"unqualified final census: {source}")
        if source == "claude.observer" and sorted(seal.get("runtimeIds", [])) != sorted(totals): errors.append("missing observer epoch in final census")

    starts, closes, witnesses = [], [], []
    for record in raw.get("domRecords", []):
        # These records are independently retained companion log results.
        if record.get("error") is not None:
            errors.append("rejected native DOM evidence"); continue
        item = record.get("result", {})
        if item.get("schemaVersion") != 3 or item.get("bootId") != boot or item.get("clock") != "CLOCK_UPTIME_RAW":
            errors.append("native DOM clock/boot/schema mismatch"); continue
        kind = item.get("kind")
        if kind == "composed-duration-start": starts.append(item)
        elif kind == "composed-dom-census": closes.append(item)
        elif kind == "composed-dom-receipt": witnesses.append(item)
        else: errors.append("unknown native DOM evidence")
    context = starts[0].get("context") if len(starts) == 1 else None
    keys = ("runId", "domRuntimeId", "nativeIncarnation", "subscriptionId", "viewEpoch", "coreGeneration", "storeGeneration")
    if not context or not all(canonical_uuid(context.get(k)) for k in keys): errors.append("missing unique immutable DOM start")
    if len(closes) != 1 or closes[0].get("closed") is not True or closes[0].get("context") != context or closes[0].get("failures") != 0 or closes[0].get("totalWitnesses") != len(witnesses):
        errors.append("missing/lost/overflowing final DOM census")
    seen = set()
    for item in witnesses:
        try:
            w = item["witness"]
            if not canonical_uuid(w.get("witnessId")): raise ValueError("invalid witness UUID")
            start = starts[0]
            if item.get("context") != context or w.get("context") != context or w.get("schemaVersion") != 3 or w.get("pendingHydration") != 0 or w.get("visible") is not True: raise ValueError("stale or unresolved DOM context/hydration")
            if w.get("fromCursor") != start["cursor"] or w.get("fromStreamSeq") != start["streamSeq"] or integer(w["cursor"]) <= integer(start["cursor"]) or integer(w["streamSeq"]) <= integer(start["streamSeq"]): raise ValueError("noncontiguous DOM coverage baseline")
            if w["witnessId"] in seen: raise ValueError("duplicate witness")
            seen.add(w["witnessId"])
            if integer(item["monotonicNs"]) < integer(start["monotonicNs"]): raise ValueError("native clock reset")
            if closes and integer(item["monotonicNs"]) > integer(closes[0]["monotonicNs"]): raise ValueError("ack follows final census")
            rows = w["revisions"]
            if any(r.get("entity") not in ("SESSION", "ATTENTION") or not canonical_uuid(r.get("id")) or type(r.get("absent")) is not bool for r in rows): raise ValueError("malformed covered entity revision")
            if len({(r["entity"], r["id"]) for r in rows}) != len(rows) or len(rows) > 1024: raise ValueError("ambiguous covered revisions")
        except (KeyError, IndexError, TypeError, ValueError) as error: errors.append(f"invalid DOM witness: {error}")
    if sorted(item.get("ordinal") for item in witnesses) != list(range(1, len(witnesses) + 1)): errors.append("lost native DOM ordinal")

    commits = defaultdict(list)
    for item in raw.get("commitRecords", []):
        if item.get("event") == "M2_COMMITTED_MEASUREMENT" and item.get("source") in SOURCES:
            commits[item.get("observationId")].append(item)
            if item.get("observationId") not in candidates: errors.append("journal admission missing original capture")
    counts = {s: {k: 0 for k in ("totalCaptured", "durablyAccepted", "deduplicatedAttempts", "projectionChange", "noProjectionChange", "matchedDom", "missingTelemetry")} for s in SOURCES}
    samples = {s: {m: [] for m in TARGETS} for s in SOURCES}
    central = {s: {m: [] for m in TARGETS} for s in SOURCES}
    for oid, capture in sorted(candidates.items()):
        source = capture["source"]
        count = counts[source]
        count["totalCaptured"] += 1
        admissions = commits.get(oid, [])
        count["deduplicatedAttempts"] += sum(r.get("status") == "ALREADY_COMMITTED" for r in admissions)
        original = [r for r in admissions if r.get("status") == "COMMITTED"]
        try:
            if len(original) != 1: raise ValueError("missing/ambiguous original successful COMMIT; retry is not a replacement")
            row = original[0]
            if row.get("sourceEpoch") != capture["epoch"] or row.get("bootId") != boot or not context or row.get("storeGeneration") != context["storeGeneration"]: raise ValueError("original admission epoch/store/boot mismatch")
            bracket = row["commit"]
            if bracket.get("boundary") != "SQLITE_COMMIT_CALL" or bracket.get("clock") != "CLOCK_UPTIME_RAW": raise ValueError("not the actual COMMIT bracket")
            b, e = integer(bracket["beginNs"]), integer(bracket["endNs"])
            if b > e: raise ValueError("reversed native COMMIT")
            count["durablyAccepted"] += 1
            chosen_receipt = None
            if source == "claude.observer":
                durable = []
                for attempt, record in attempts[oid]:
                    native = natives.get(attempt.get("nativeToken"), {})
                    actual = native.get("receipt", {}).get("results", [])
                    expected = attempt["records"]
                    native_ids = native.get("requestObservationIds", [])
                    if native.get("runtimeId") != capture["epoch"] or native.get("clock") != "CLOCK_UPTIME_RAW" or native.get("bootId") != boot or type(native.get("receipt", {}).get("receiptVersion")) is not int or native.get("receipt", {}).get("receiptVersion") != 1: continue
                    if native_ids != [r["observationId"] for r in expected] or len(actual) != len(expected) or len({r.get("observationId") for r in actual}) != len(actual): continue
                    if {r["observationId"]: r["status"] for r in actual} != {r["observationId"]: r["status"] for r in expected}: continue
                    if record.get("status") not in ("COMMITTED", "ALREADY_COMMITTED") or integer(native["monotonicNs"]) < e: continue
                    duration = number(attempt["endMs"]) - capture["capture"]
                    if not 0 <= duration <= max_duration: raise ValueError("observer duration outside qualified instrument scope")
                    durable.append((duration, native["token"]))
                if not durable: raise ValueError("no independently bound validated durable receipt")
                cj, chosen_receipt = min(durable)
                ucj = upper(cj, eo)
            else:
                clock = capture["capture"]
                if clock.get("clockQuality") != "LOCAL_MONOTONIC" or clock.get("bootId") != boot: raise ValueError("unqualified native hook capture")
                c = integer(clock["monotonicNs"])
                if c > e: raise ValueError("hook capture follows durable COMMIT")
                cj = Decimal(e - c) / 1_000_000
                ucj = upper(cj, en)
            metrics, base = {"captureToCommitMs": ucj}, {"captureToCommitMs": float(cj)}
            patch = row.get("patchCursor")
            if patch is None:
                if row.get("projectionRequirements") is not None: raise ValueError("contradictory projection exclusion")
                count["noProjectionChange"] += 1
                exclusions.append({"observationId": oid, "reason": "original transaction had no public projection change", "metrics": ["commitToDomMs", "eventToDomMs"]})
            else:
                count["projectionChange"] += 1
                required = row.get("projectionRequirements")
                if not required or required.get("toCursor") != patch: raise ValueError("missing original affected entity revisions")
                matching = []
                for ack in witnesses:
                    w = ack["witness"]
                    if integer(w["fromCursor"]) >= integer(patch) or integer(w["cursor"]) < integer(patch): continue
                    covered = {(r["entity"], r["id"]): r for r in w["revisions"]}
                    requirements = [("SESSION", r["sessionId"], r["revision"], False) for r in required.get("sessionUpserts", [])]
                    requirements += [("ATTENTION", r["attentionId"], r["revision"], r.get("resolvedAtMs") is not None) for r in required.get("attentionUpserts", [])]
                    requirements += [(r["entity"], r["id"], patch, True) for r in required.get("tombstones", [])]
                    if any((entity, entity_id) not in covered or integer(covered[(entity, entity_id)]["revision"]) < integer(rev)
                           or (integer(covered[(entity, entity_id)]["revision"]) == integer(rev) and covered[(entity, entity_id)].get("absent") is not absent) for entity, entity_id, rev, absent in requirements): continue
                    at = integer(ack["monotonicNs"])
                    if at < e: raise ValueError("DOM acknowledgement precedes original COMMIT completion")
                    matching.append((at, w["witnessId"]))
                if not matching: raise ValueError("missing actual relevant hydrated DOM witness")
                a, witness_id = min(matching)
                jd = Decimal(a - b) / 1_000_000  # PRE-COMMIT, never E.
                ujd = upper(jd, en)
                event = upper(Decimal(str(ucj)) + Decimal(str(ujd)), Decimal(0)) if source == "claude.observer" else upper(Decimal(a - c) / 1_000_000, en)
                metrics.update(commitToDomMs=ujd, eventToDomMs=event)
                base.update(commitToDomMs=float(jd), eventToDomMs=float(Decimal(str(cj)) + jd) if source == "claude.observer" else float(Decimal(a - c) / 1_000_000))
                count["matchedDom"] += 1
            for metric, value in metrics.items():
                samples[source][metric].append(value)
                central[source][metric].append(base[metric])
            joined.append({"observationId": oid, "source": source, "originalCommitBeginNs": str(b), "originalCommitEndNs": str(e),
                "receiptToken": chosen_receipt, "upperBoundsMs": metrics, "retainedAttempts": len(attempts[oid])})
        except (KeyError, TypeError, ValueError) as error:
            count["missingTelemetry"] += 1
            errors.append(f"incomplete sample {oid}: {error}")
    qualified = not errors
    by_source = {s: {**{m: percentiles(values) for m, values in samples[s].items()}, "population": counts[s]} for s in SOURCES}
    thresholds = True
    for source in SOURCES:
        for metric, target in TARGETS.items():
            result = by_source[source][metric]
            base = percentiles(central[source][metric])["p95"]
            if result["n"] == 0 or result["p95"] > target:
                thresholds = False
                if base is not None and base <= target < result["p95"]:
                    errors.append(f"uncertainty determines threshold verdict: {source}/{metric}")
    if not thresholds: errors.append("mandatory population empty or conservative p95 does not demonstrate target")
    native = raw.get("executionKind") == "NATIVE"
    return {"schemaVersion": 3, "method": METHOD, "metricsQualified": qualified, "nativeExecution": native,
        "qualifiedSampleCounts": {s: {m: len(samples[s][m]) if qualified else 0 for m in TARGETS} for s in SOURCES},
        "bySource": by_source, "joinedSamples": joined, "exclusions": exclusions, "errors": errors,
        "normalPathPass": qualified and thresholds, "verdict": "PASS" if qualified and thresholds and native else "INCOMPLETE_OR_FAIL"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = analyze(json.loads(args.raw.read_text()))
    except (KeyError, TypeError, ValueError, IndexError) as error:
        result = {"normalPathPass": False, "verdict": "INCOMPLETE_OR_FAIL", "errors": [f"malformed composed evidence: {error}"]}
    text = json.dumps(result, indent=2) + "\n"
    if args.output: args.output.write_text(text)
    else: print(text, end="")
    return 0 if result["verdict"] == "PASS" else 1


if __name__ == "__main__": raise SystemExit(main())
