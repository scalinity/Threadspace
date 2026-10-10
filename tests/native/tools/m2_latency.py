#!/usr/bin/env python3
"""F4 independent calculation of conservative same-boot latency bounds.

Input is retained metadata, not a PASS manifest: hook capture candidates,
observer/process.run brackets, independently saved native helper stamps,
actual post-SQLite-COMMIT log records, paged DOM marks and native/UI clock
calibration brackets. See evidence/M2/remediation-1/f4/README.md.
No wall clock participates in any interval calculation.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from collections import defaultdict
from pathlib import Path

SOURCES = ("claude.hook", "claude.observer")
TARGETS = {"captureToCommitMs": 100, "commitToDomMs": 100, "eventToDomMs": 250}


def percentiles(values):
    values = sorted(values)
    at = lambda q: values[math.ceil(len(values) * q) - 1] if values else None
    return {"n": len(values), "p50": at(.50), "p95": at(.95), "max": values[-1] if values else None}


def number(value):
    if isinstance(value, bool):
        raise ValueError("boolean timestamp")
    result = float(value)
    if not math.isfinite(result) or result < 0:
        raise ValueError("invalid timestamp")
    return result


def integer(value):
    if isinstance(value, bool) or not str(value).isdigit():
        raise ValueError("invalid decimal integer")
    return int(value)


def translated_interval(target_ms, remote_ms, native_lower, native_upper, rate, precision):
    """Translate using an independently bracketed point and a qualified
    rate-error bound. Signed deltas reverse which scale is conservative."""
    delta = (target_ms - remote_ms) * 1_000_000
    low_delta, high_delta = sorted((delta * (1 - rate), delta * (1 + rate)))
    return math.floor(native_lower + low_delta - precision), math.ceil(native_upper + high_delta + precision)


def analyze(raw):
    errors, exclusions, joined = [], [], []
    profile = raw.get("clockQualification", {})
    boot = raw.get("bootId")
    rate = number(profile.get("maximumRateErrorPpm") or 0) / 1_000_000
    precision = number(profile.get("precisionNs") or 0)
    if not boot or profile.get("nativeClock") != "CLOCK_UPTIME_RAW" or profile.get("qualified") is not True:
        errors.append("clock profile is not positively qualified on the recorded native boot")
    if "maximumRateErrorPpm" not in profile or profile["maximumRateErrorPpm"] is None or not 0 <= rate < 1 or precision <= 0 or not profile.get("evidence"):
        errors.append("missing/invalid rate, resolution or clock qualification evidence")
    clock_profile_qualified = not errors
    errors.extend(str(error) for error in raw.get("collectorErrors", []))
    if raw.get("schemaVersion") != 2:
        errors.append("wrong raw measurement schema")

    candidates = {}
    observer_runtime_records = defaultdict(set)
    observer_totals = defaultdict(int)
    native = {}
    for item in raw.get("helperRecords", []):
        if item.get("kind") == "helper-receipt":
            token = item.get("token")
            if token in native:
                errors.append("duplicate independent native token")
            native[token] = item

    def capture(observation_id, source, interval, scope):
        if not observation_id or source not in SOURCES:
            errors.append("invalid capture identity/source")
            return
        previous = candidates.get(observation_id)
        if previous:
            if previous["source"] != source or previous["scope"] != scope:
                errors.append(f"conflicting capture identity: {observation_id}")
                return
            if interval is not None:
                if previous["capture"] is None:
                    previous["capture"] = interval
                else:
                    # Retries retain the same capture, and independently
                    # bounded observations of it may narrow the interval.
                    low, high = max(previous["capture"][0], interval[0]), min(previous["capture"][1], interval[1])
                    if low > high:
                        errors.append(f"incompatible retry clocks: {observation_id}")
                    else:
                        previous["capture"] = (low, high)
            return
        candidates[observation_id] = {"source": source, "capture": interval, "scope": scope}

    for item in raw.get("helperRecords", []):
        if item.get("kind") != "hook-capture":
            continue
        try:
            clock = item["capture"]
            if clock.get("bootId") != boot or clock.get("clockQuality") != "LOCAL_MONOTONIC":
                raise ValueError("hook boot/clock mismatch")
            at = integer(clock["monotonicNs"])
            if not item.get("sourceEpoch"):
                raise ValueError("hook source epoch missing")
            capture(item["observationId"], item["source"], (at, at), item["sourceEpoch"])
        except (KeyError, TypeError, ValueError) as error:
            errors.append(f"invalid hook candidate: {error}")

    for item in raw.get("helperRecords", []):
        if item.get("kind") != "observer-clock-bracket":
            continue
        runtime = item.get("runtimeId")
        try:
            begin, end = number(item["beginMs"]), number(item["endMs"])
            if item.get("clock") != "performance.now" or not runtime or end < begin:
                raise ValueError("observer runtime/clock/bracket mismatch")
            observer_totals[runtime] = max(observer_totals[runtime], integer(item["totalCaptured"]))
            if integer(item["overflow"]) or integer(item["failedExports"]):
                errors.append(f"observer telemetry loss in runtime {runtime}")
            stamp = native.get(item.get("nativeToken"))
            stamp_ok = stamp and stamp.get("bootId") == boot and stamp.get("clock") == "CLOCK_UPTIME_RAW"
            actual = {row["observationId"]: row["status"] for row in stamp.get("receipt", {}).get("results", [])} if stamp_ok else {}
            expected_ids = {row["observationId"] for row in item["records"]}
            if set(actual) != expected_ids:
                stamp_ok = False
            for record in item["records"]:
                observation_id = record["observationId"]
                observer_runtime_records[runtime].add(observation_id)
                interval = None
                if stamp_ok and actual.get(observation_id) == record.get("status") and record.get("capturedMs") is not None:
                    at = number(record["capturedMs"])
                    if at > begin:
                        raise ValueError("capture follows batch launch")
                    point = integer(stamp["monotonicNs"])
                    # The native helper stamp lies within [begin,end] of
                    # this very call. Do not subtract unrelated origins.
                    lower = translated_interval(at, end, point, point, rate, precision)[0]
                    upper = translated_interval(at, begin, point, point, rate, precision)[1]
                    interval = (lower, upper)
                capture(observation_id, "claude.observer", interval, runtime)
        except (KeyError, TypeError, ValueError) as error:
            errors.append(f"invalid observer bracket: {error}")
    for runtime, total in observer_totals.items():
        if total != len(observer_runtime_records[runtime]):
            errors.append(f"observer captured population not reconciled: {runtime}: {total} versus {len(observer_runtime_records[runtime])}")

    # A prior successful export cannot close the tail of a measurement
    # window. Each source needs an independently retained final census.
    # A missing final seal/ack is INCOMPLETE, even if all earlier rows match.
    seals = raw.get("populationSeals", [])
    for source in SOURCES:
        source_seals = [seal for seal in seals if seal.get("source") == source]
        ids = sorted(observation_id for observation_id, item in candidates.items() if item["source"] == source)
        digest = hashlib.sha256("\n".join(ids).encode()).hexdigest()
        if len(source_seals) != 1:
            errors.append(f"missing/ambiguous closed capture population seal for {source}")
            continue
        seal = source_seals[0]
        if seal.get("boundary") != "CLOSED_CAPTURE_WINDOW" or seal.get("sealed") is not True or seal.get("bootId") != boot or not seal.get("evidence"):
            errors.append(f"unqualified capture population seal for {source}")
        if seal.get("totalCaptured") != len(ids) or seal.get("observationIdsSha256") != digest or seal.get("telemetryFailures") != 0:
            errors.append(f"final captured population/loss mismatch for {source}")
        if source == "claude.observer" and sorted(seal.get("runtimeIds", [])) != sorted(observer_totals):
            errors.append("observer epoch population missing from final seal")

    pages = raw.get("domPages", [])
    dom_runtime, dom_context, dom = None, None, []
    seen_offsets = set()
    for page in pages:
        if dom_runtime is None:
            dom_runtime, dom_context = page.get("runtimeId"), page.get("context")
        if page.get("runtimeId") != dom_runtime or page.get("context") != dom_context or page.get("clock") != "performance.now":
            errors.append("DOM runtime/context/clock changed")
        if page.get("overflow") != 0 or page.get("invalid") != 0 or page.get("contextChanged") is not False:
            errors.append("DOM sample overflow, invalid cursor or epoch change")
        if page.get("offset") in seen_offsets:
            errors.append("repeated DOM page")
        seen_offsets.add(page.get("offset"))
        dom.extend(page.get("marks", []))
    if not pages or not dom_context or dom_context.get("phase") != "live" or not all(isinstance(dom_context.get(key), str) and dom_context[key] for key in ("coreGeneration", "storeGeneration", "viewEpoch")):
        errors.append("missing hydrated DOM measurement population")
    elif any(page.get("retainedMarks") != len(dom) or page.get("totalApplied") != len(dom) for page in pages):
        errors.append("incomplete DOM page population")
    if len({mark.get("cursor") for mark in dom}) != len(dom):
        errors.append("duplicate DOM cursors")
    if pages:
        expected_offsets = set(range(0, len(dom), 100)) or {0}
        if seen_offsets != expected_offsets:
            errors.append("missing DOM page offset")

    calibrations = []
    for bracket in raw.get("uiCalibrations", []):
        try:
            reply = bracket["reply"]
            if bracket.get("bootId") != boot or bracket.get("clock") != "CLOCK_UPTIME_RAW" or reply.get("runtimeId") != dom_runtime or reply.get("clock") != "performance.now":
                raise ValueError("UI calibration boot/runtime/clock mismatch")
            low, high = integer(bracket["nativeBeforeNs"]), integer(bracket["nativeAfterNs"])
            if low > high:
                raise ValueError("reversed native calibration")
            calibrations.append((number(reply["monotonicMs"]), low, high))
        except (KeyError, TypeError, ValueError) as error:
            errors.append(f"invalid UI calibration: {error}")
    calibrations.sort()

    def dom_interval(mark):
        if mark.get("domMonotonicMs") is None or mark.get("hydratedVisible") is not True:
            raise ValueError("no actual hydrated visible DOM mark")
        at = number(mark["domMonotonicMs"])
        if at < number(mark["appliedMonotonicMs"]) or integer(mark["domCursor"]) < integer(mark["cursor"]):
            raise ValueError("DOM precedes application/cursor")
        if len(calibrations) < 2 or not calibrations[0][0] <= at <= calibrations[-1][0]:
            raise ValueError("DOM outside positively bracketed calibration span")
        bounds = [translated_interval(at, remote, low, high, rate, precision) for remote, low, high in calibrations]
        low, high = max(bound[0] for bound in bounds), min(bound[1] for bound in bounds)
        if low > high:
            raise ValueError("incompatible cross-runtime monotonic calibration")
        return low, high

    commits = defaultdict(list)
    for item in raw.get("commitRecords", []):
        if item.get("event") == "M2_COMMITTED_MEASUREMENT" and item.get("source") in SOURCES:
            commits[item.get("observationId")].append(item)
            if item.get("observationId") not in candidates:
                errors.append(f"durable record missing capture candidate: {item.get('observationId')}")

    counts = {source: {key: 0 for key in ("totalCaptured", "durablyAccepted", "deduplicatedAttempts", "rejected", "projectionChange", "noProjectionChange", "matchedDom", "unmatchedDom", "missingTelemetry")} for source in SOURCES}
    samples = {source: {metric: [] for metric in TARGETS} for source in SOURCES}
    for observation_id, item in sorted(candidates.items()):
        source, capture_interval = item["source"], item["capture"]
        count = counts[source]
        count["totalCaptured"] += 1
        attempts = commits.get(observation_id, [])
        count["deduplicatedAttempts"] += sum(row.get("status") == "ALREADY_COMMITTED" for row in attempts)
        accepted = [row for row in attempts if row.get("status") == "COMMITTED"]
        if len(accepted) > 1:
            errors.append(f"duplicate original commit: {observation_id}")
        if not accepted:
            if attempts and all(row.get("status") == "NOT_ACCEPTED" for row in attempts):
                count["rejected"] += 1
                exclusions.append({"observationId": observation_id, "reason": "explicit journal rejection", "metrics": list(TARGETS)})
            else:
                count["missingTelemetry"] += 1
                errors.append(f"no original durable commit in sample window: {observation_id}")
            continue
        count["durablyAccepted"] += 1
        row = accepted[0]
        try:
            if row.get("source") != source or row.get("bootId") != boot or capture_interval is None or row.get("sourceEpoch") != item["scope"]:
                raise ValueError("capture/commit identity or monotonic capture missing")
            if not dom_context or row.get("storeGeneration") != dom_context.get("storeGeneration") or not row.get("storeGeneration"):
                raise ValueError("commit belongs to a different/unknown canonical store")
            timing = row["commit"]
            if timing.get("boundary") != "SQLITE_COMMIT_CALL" or timing.get("clock") != "CLOCK_UPTIME_RAW":
                raise ValueError("not the actual SQLite COMMIT bracket")
            commit_low, commit_high = integer(timing["beginNs"]), integer(timing["endNs"])
            if commit_low > commit_high or commit_high < capture_interval[0]:
                raise ValueError("invalid/reversed capture-to-commit causal interval")
            metrics = {"captureToCommitMs": (commit_high - capture_interval[0]) / 1_000_000}
            samples[source]["captureToCommitMs"].append(metrics["captureToCommitMs"])
            patch = row.get("patchCursor")
            if patch is None:
                count["noProjectionChange"] += 1
                exclusions.append({"observationId": observation_id, "reason": "transaction produced no public projection change", "metrics": ["commitToDomMs", "eventToDomMs"]})
            else:
                count["projectionChange"] += 1
                marks = sorted((mark for mark in dom if integer(mark["cursor"]) >= integer(patch)), key=lambda mark: integer(mark["cursor"]))
                if not marks:
                    count["unmatchedDom"] += 1
                    raise ValueError("committed projection has no DOM cursor at or after its patch")
                try:
                    shown = dom_interval(marks[0])
                except ValueError:
                    count["unmatchedDom"] += 1
                    raise
                if shown[1] < commit_low:
                    raise ValueError("DOM causally precedes commit")
                metrics["commitToDomMs"] = (shown[1] - commit_low) / 1_000_000
                metrics["eventToDomMs"] = (shown[1] - capture_interval[0]) / 1_000_000
                count["matchedDom"] += 1
                for metric in ("commitToDomMs", "eventToDomMs"):
                    samples[source][metric].append(metrics[metric])
            joined.append({"observationId": observation_id, "source": source, "cursor": row["cursor"], "patchCursor": patch,
                           "captureIntervalNs": list(map(str, capture_interval)), "commitIntervalNs": [str(commit_low), str(commit_high)], "upperBoundsMs": metrics})
        except (KeyError, TypeError, ValueError) as error:
            count["missingTelemetry"] += 1
            errors.append(f"incomplete sample {observation_id}: {error}")
    by_source = {source: {**{metric: percentiles(values) for metric, values in samples[source].items()}, "population": counts[source]} for source in SOURCES}
    thresholds = all(by_source[source][metric]["n"] > 0 and by_source[source][metric]["p95"] <= target for source in SOURCES for metric, target in TARGETS.items())
    measurement_qualified = clock_profile_qualified and not errors
    if not thresholds:
        errors.append("one or more mandatory source/metric populations is empty or exceeds its p95 target")
    return {"schemaVersion": 2,
            "calculation": "nearest-rank p95 of conservative upper bounds in milliseconds" if measurement_qualified else "UNQUALIFIED diagnostic percentiles; not certified conservative upper bounds",
            "metricsQualified": measurement_qualified,
            "qualifiedSampleCounts": {source: {metric: len(values) if measurement_qualified else 0 for metric, values in samples[source].items()} for source in SOURCES},
            "diagnosticClockFallback": None if clock_profile_qualified else {"maximumRateErrorPpm": rate * 1_000_000, "precisionNs": precision, "certified": False},
            "targets": TARGETS,
            "bySource": by_source, "joinedSamples": joined, "exclusions": exclusions, "errors": errors,
            "normalPathPass": thresholds and not errors,
            "nativeExecution": raw.get("executionKind") == "NATIVE",
            "verdict": "PASS" if thresholds and not errors and raw.get("executionKind") == "NATIVE" else "INCOMPLETE_OR_FAIL"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = analyze(json.loads(args.raw.read_text()))
    except (KeyError, TypeError, ValueError) as error:
        result = {"normalPathPass": False, "verdict": "INCOMPLETE_OR_FAIL", "errors": [f"malformed raw evidence: {error}"]}
    text = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")
    return 0 if result["normalPathPass"] and result.get("nativeExecution") else 1


if __name__ == "__main__":
    raise SystemExit(main())
