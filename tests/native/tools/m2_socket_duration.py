#!/usr/bin/env python3
"""Fixed Unix-socket conformance evaluator; no clock-origin conversion."""
from __future__ import annotations
import hashlib
import json
import math
from decimal import Decimal, ROUND_FLOOR, ROUND_CEILING
from pathlib import Path
from m2_socket_plugins import composition_identity, validate_manifest

HERE = Path(__file__).resolve().parent
POLICY = json.loads((HERE / "m2_socket_policy.json").read_text())
PROVIDER_SHA = "0116ee2e0a513900b633d9951367f18747686478e2b462805b8c31609f047f70"
PROVIDER_VERSION = "2.1.295"
ADAPTER_SHA = "d461d6d6cb8769dbe65d3e229212f052f5ba0c45a936f92ba76d18b18e88214b"
IDENTITY_KEYS = (
    "providerSha256", "providerVersion", "adapterSha256", "collectorSha256",
    "responderSourceSha256", "responderExecutableSha256", "runnerSha256",
    "evaluatorSha256", "calculatorSha256", "policySha256", "policyFileSha256", "probeSha256",
    "backgroundSha256", "osBuild", "bootId", "nativeClock", "observerClock",
    "clockScope", "responderId", "responderProcess", "runId", "runtimes",
    "pluginComposition", "pluginStagerSha256",
)
MARKERS = ("captureLower", "captureUpper", "endLower", "endUpper")

def canonical_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()

def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def source_identities():
    repo = HERE.parents[2]
    hooks = repo / "packages/provider-mod/hooks"
    return {
        "providerSha256": PROVIDER_SHA, "providerVersion": PROVIDER_VERSION,
        "adapterSha256": canonical_hash({name: file_hash(hooks / name) for name in ("register.ts", "latency.ts", "delivery.ts", "ownership.ts")}),
        "collectorSha256": file_hash(hooks / "latency.ts"),
        "responderSourceSha256": file_hash(HERE / "m2_timestamp_responder.c"),
        "runnerSha256": file_hash(HERE / "m2_socket_runner.py"),
        "evaluatorSha256": file_hash(__file__),
        "calculatorSha256": file_hash(HERE / "m2_composed_duration.py"),
        "policySha256": canonical_hash(POLICY),
        "policyFileSha256": file_hash(HERE / "m2_socket_policy.json"),
        "probeSha256": file_hash(HERE / "m2_socket_probe.ts"),
        "backgroundSha256": file_hash(HERE / "m2_socket_background.ts"),
        "pluginComposition": composition_identity(),
        "pluginStagerSha256": file_hash(HERE / "m2_socket_plugins.py"),
    }

def decimal(value):
    if isinstance(value, bool): raise ValueError("boolean clock")
    v = Decimal(str(value))
    if not v.is_finite() or v < 0: raise ValueError("nonfinite/negative clock")
    return v

def rounded(value, mode):
    step = Decimal(str(POLICY["outwardStepMs"]))
    return (value / step).to_integral_value(rounding=mode) * step

def qualify(report):
    errors, checks = [], []
    ids = report.get("identities", {})
    def require(ok, reason):
        if not ok: errors.append(reason)
    require(report.get("instrument") == POLICY["instrument"] and report.get("policy") == POLICY, "wrong frozen instrument/policy")
    require(report.get("complete") is True and report.get("positiveAttempt") == 1, "incomplete/incorrect attempt accounting")
    require(report.get("deadlineExceeded") is False and report.get("providerLost") is False and report.get("responderLost") is False, "deadline/process continuity")
    require(report.get("socketRemoved") is True and len(report.get("cleanup",[])) == 3 and all(c.get("reaped") is True for c in report.get("cleanup",[])), "incomplete owned cleanup")
    ownership = report.get("socketOwnership", {})
    require(ownership.get("directoryMode") == 0o700 and ownership.get("socketMode") == 0o600, "unprotected Unix socket ownership")
    require(report.get("bootBefore") == report.get("bootAfter") == ids.get("bootId") and report.get("powerDiscontinuity") is False, "boot/sleep continuity")
    require(report.get("freezeSha256") == canonical_hash(report.get("freeze", {})), "changed frozen manifest")
    freeze = report.get("freeze", {})
    require(all(ids.get(k) == v for k, v in source_identities().items()), "changed source/provider/adapter/evaluator/policy/probe")
    require(all(ids.get(k) == v for k, v in freeze.get("identities", {}).items()), "conformance identity differs from frozen source")
    require(all(ids.get(k) not in (None, "", [], {}) for k in IDENTITY_KEYS), "missing original conformance identity")
    if report.get("executionKind") == "NATIVE_INSTALLED_PROVIDER":
        staging = report.get("pluginStaging", [])
        require(len(staging) == 2, "missing original two-plugin staging identities")
        for index, entry in enumerate(staging):
            try:
                manifest = entry["manifest"]
                owned = Path(manifest["plugins"][0]["root"]).parent
                runtime = freeze["runtimePlan"][index]
                config = {**runtime, **{k: ids[k] for k in ("runId", "bootId", "responderId")},
                    "plan": freeze["plan"], "firstSequence": index * 28, "delays": POLICY["requestedDelaysMs"],
                    "socketPath": str(owned.parent / "s.sock"), "freezeSha256": report["freezeSha256"],
                    "output": str(owned / "host-result.json")}
                validate_manifest(manifest, owned, config, ids["pluginComposition"])
                require(entry.get("condition") == runtime["condition"] and entry.get("manifestSha256") == canonical_hash(manifest), "changed original plugin staging digest/condition")
            except (KeyError, ValueError, RuntimeError, TypeError, IndexError) as error:
                errors.append("unbound original plugin staging: " + str(error))
    require(ids.get("nativeClock") == "CLOCK_UPTIME_RAW" and ids.get("observerClock") == "performance.now" and ids.get("clockScope") == POLICY["clockScope"], "clock implementation/scope")
    runtimes = ids.get("runtimes", [])
    require(len(runtimes) == 2 and [r.get("condition") for r in runtimes] == POLICY["conditions"], "missing fresh runtime conditions")
    require(len({r.get("runtimeId") for r in runtimes}) == 2 and len({r.get("sourceEpoch") for r in runtimes}) == 2, "reused runtime/epoch")
    require([{k:v for k,v in r.items() if k != "process"} for r in runtimes] == freeze.get("runtimePlan"), "runtime/epoch differs from frozen plan")
    require(len({r.get("process",{}).get("startUsec") for r in runtimes}) == 2, "provider runtime process reused")
    for runtime in runtimes:
        proc = runtime.get("process", {})
        require(proc.get("sha256") == PROVIDER_SHA and proc.get("version") == PROVIDER_VERSION and proc.get("kernelVerified") is True and proc.get("pid", 0) > 0 and proc.get("startUsec", 0) > 0, "unbound provider process/image")
    rp = ids.get("responderProcess", {})
    require(rp.get("sha256") == ids.get("responderExecutableSha256") and rp.get("kernelVerified") is True and rp.get("pid", 0) > 0 and rp.get("startUsec", 0) > 0, "unbound responder process/image")
    require(ownership.get("uid") == rp.get("uid"), "socket owner differs from verified responder")
    require([c.get("identity") for c in report.get("cleanup",[])] == [r.get("process") for r in runtimes] + [rp], "cleanup identity binding")
    transcript = report.get("nativeTranscript", [])
    require(report.get("transcriptSha256") == canonical_hash(transcript), "original native transcript digest")
    plan = freeze.get("plan", [])
    require(len(plan) == 56 and len({p.get("challenge") for p in plan}) == 56 and [p.get("sequence") for p in plan] == list(range(56)), "incomplete/reordered frozen request plan")
    generated = []
    expected_events = ["start"] + [event for _ in range(56) for event in ("received", "generated", "answered")] + ["stop"]
    require([e.get("event") for e in transcript] == expected_events, "incomplete/reordered request-response transcript")
    if transcript:
        header = transcript[0]
        require(all(header.get(k) == ids.get(k) for k in ("runId", "bootId", "responderId")) and header.get("manifestSha256") == report.get("freezeSha256") and header.get("pid") == rp.get("pid") and header.get("uid") == rp.get("uid"), "foreign native transcript incarnation")
        last = transcript[-1]
        require(last.get("attempts") == last.get("generated") == last.get("answered") == 56, "native complete exchange accounting")
    for index in range(56):
        offset = 1 + index * 3
        if len(transcript) <= offset + 2 or len(plan) <= index: continue
        received, produced, answered = transcript[offset:offset+3]
        response = produced.get("response", {})
        require(received.get("accepted") is True and received.get("attempt") == produced.get("attempt") == answered.get("attempt") == index + 1 and received.get("sequence") == answered.get("sequence") == index, f"exchange {index}: sequence/accounting")
        require(all(response.get(k) == v for k, v in plan[index].items()) and all(response.get(k) == ids.get(k) for k in ("runId", "bootId", "responderId")) and response.get("manifestSha256") == report.get("freezeSha256") and response.get("clock") == "CLOCK_UPTIME_RAW", f"exchange {index}: challenge/identity")
        generated.append(response)
    try:
        native_values = [int(r["nativeNs"]) for r in generated]
        require(all(b >= a for a,b in zip(native_values,native_values[1:])), "native clock reset between markers")
    except (KeyError, ValueError, TypeError):
        errors.append("malformed native timestamp transcript")
    runtime_results = report.get("runtimeResults", [])
    require(len(runtime_results) == 2 and all(r.get("complete") is True and r.get("error") is None for r in runtime_results), "missing/incomplete original provider results")
    require([response for r in runtime_results for response in r.get("responses",[])] == generated, "provider/native response accounting")
    require(all("".join(r.get("wrapper", "").split()) == "()=>guarded(helpers.now)()" for r in runtime_results), "changed guarded runtime clock wrapper")
    rows = report.get("rows", [])
    require(len(rows) == 14, "missing conformance rows")
    previous = {}
    for index, row in enumerate(rows):
        row_errors = []
        item = {"index": index, "condition": row.get("condition"), "requestedDelayMs": row.get("requestedDelayMs"), "pass": False}
        try:
            condition = POLICY["conditions"][index // 7]
            delay = POLICY["requestedDelaysMs"][index % 7]
            runtime = runtimes[index // 7]
            if row.get("condition") != condition or row.get("requestedDelayMs") != delay: row_errors.append("row/stimulus order")
            if any(row.get(k) != runtime.get(k) for k in ("runtimeId", "sourceEpoch")): row_errors.append("runtime/epoch continuity")
            readings = [decimal(v) for v in row["readings"]]
            if len(readings) != 258: raise ValueError("incomplete precision/interval readings")
            if readings[0] < previous.get(condition, Decimal(0)) or any(b < a for a,b in zip(readings,readings[1:])): row_errors.append("clock reset")
            previous[condition] = readings[-1]
            steps = [b-a for a,b in zip(readings[:256],readings[1:256]) if b>a]
            max_step = max(steps, default=Decimal(0))
            if not steps or max_step > POLICY["maximumStepMs"]: row_errors.append("clock precision/coarsening/step")
            c,r = readings[-2:]
            observer = r-c
            markers = [row[name] for name in MARKERS]
            for m, marker in enumerate(markers):
                seq = index*4+m
                if seq >= len(generated) or marker != generated[seq]: row_errors.append(f"native marker {m} correlation")
            ns = [int(m["nativeNs"]) for m in markers]
            if any(str(v) != m["nativeNs"] or v<0 for v,m in zip(ns,markers)) or any(b<a for a,b in zip(ns,ns[1:])): raise ValueError("native reset/units")
            guard = Decimal(str(POLICY["nativeAdditiveMs"]))
            lower = rounded(max(Decimal(0), Decimal(ns[2]-ns[1])/1000000-guard), ROUND_FLOOR)
            upper = rounded(Decimal(ns[3]-ns[0])/1000000+guard, ROUND_CEILING)
            width = rounded(upper-lower, ROUND_CEILING)
            residual = rounded(max(Decimal(0),upper-observer)+POLICY["residualGuardMs"], ROUND_CEILING)
            if width > POLICY["maximumEnclosureMs"]: row_errors.append("native enclosure >10 ms")
            if residual > POLICY["observerAdditiveMs"]: row_errors.append("fixed observer residual >10 ms")
            if observer > upper + POLICY["observerAdditiveMs"]: row_errors.append("observer/native units or duration inconsistent")
            if delay==0 and upper>POLICY["zeroUpperMs"]: row_errors.append("missing near-zero coverage")
            if delay==405 and (lower<POLICY["boundaryLowerMs"] or upper>POLICY["boundaryUpperMs"]): row_errors.append("insufficient 400/425 ms boundary coverage")
            item.update(observerElapsedMs=float(observer),nativeLowerMs=float(lower),nativeUpperMs=float(upper),enclosureMs=float(width),residualMs=float(residual),maximumStepMs=float(max_step))
        except (KeyError, ValueError, TypeError, IndexError, ArithmeticError) as error:
            row_errors.append(str(error))
        item.update(errors=row_errors, **{"pass": not row_errors})
        checks.append(item)  # Failed rows stay in the complete residual denominator.
        errors.extend(f"row {index}: {error}" for error in row_errors)
    background = report.get("background", {})
    require(background.get("ticks") == POLICY["background"]["ticks"] and background.get("operations") == POLICY["background"]["ticks"]*POLICY["background"]["operationsPerTick"] and background.get("complete") is True, "incomplete frozen background fixture")
    return {"qualified": not errors, "errors": errors, "checks": checks,
        "assessedRows": len(checks), "maximumResidualMs": max((c.get("residualMs",0) for c in checks),default=None),
        "observerAdditiveMs": 10, "nativeAdditiveMs": .001, "outwardStepMs": .001,
        "maximumDurationMs": 400, "scope": "INSTALLED_ENGINEERING_DURATION",
        "nativeClock": "CLOCK_UPTIME_RAW", "observerClock": "performance.now", "universalRateBound": False}

def make_profile(report):
    return {**report.get("identities", {}), **qualify(report), "conformanceSha256": canonical_hash(report),
        "transcriptSha256": report.get("transcriptSha256"), "freezeSha256": report.get("freezeSha256")}

def validate_binding(report, profile, artifact):
    checked = qualify(report)
    errors = list(checked["errors"])
    try: expected = make_profile(report)
    except (ValueError, TypeError): return checked, errors + ["noncanonical original conformance"]
    for key in (*IDENTITY_KEYS, "conformanceSha256", "transcriptSha256", "freezeSha256", "observerAdditiveMs", "nativeAdditiveMs", "outwardStepMs", "maximumDurationMs", "scope", "universalRateBound", "qualified"):
        if profile.get(key) != expected.get(key): errors.append(f"profile/conformance identity join: {key}")
    for key, value in artifact_identity(report).items():
        if artifact.get(key) != value: errors.append(f"artifact/original conformance identity join: {key}")
    return checked, errors

def artifact_identity(report):
    return {**report.get("identities",{}), "conformanceSha256":canonical_hash(report),
        "transcriptSha256":report.get("transcriptSha256"), "freezeSha256":report.get("freezeSha256")}
