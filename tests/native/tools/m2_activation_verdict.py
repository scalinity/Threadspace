"""Accepted-source admission and fail-closed activation evidence preflight.

Versioned readiness is separate from canonical observer qualification.
Only complete independent readback and owned cleanup can establish readiness.
"""
from __future__ import annotations
import hashlib
import json
import subprocess
from m2_activation_readiness import acknowledgement
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
REVIEWED_COMMIT = "e1fa08ec6f94abeaf2b0ef4bb38ba0937bba470e"
REVIEW_RECORD = "evidence/M2/replacement-positive-scanner-repair/source-identities.json"
REVIEW_RECORD_SHA = "30e609c90e6211bcda8a142f36e62d00a2525907f15481618f0b2176a3f34d80"
ACTIVATION_SHA = "ea54a1cbe9253388f6e4a0e2575ee31470746e0c236e2d99ad4e5fc56c273136"
RETIRED = {"runId": "be6eb480-50b5-4dd0-8f1d-92f55e763757",
           "responderId": "316a08d8-2a5e-443c-ba27-aa668050eef3",
           "nonce": "f51d93a7e42e4c19a64c38d24cb5b51cb7a05385e2eb403f82c2e8db043f8762"}


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def read_review():
    """Read the immutable admitted Git object, not a mutable current profile."""
    raw = subprocess.check_output(["git", "show", REVIEWED_COMMIT + ":" + REVIEW_RECORD], cwd=REPO)
    if (REPO / REVIEW_RECORD).read_bytes() != raw:
        raise RuntimeError("changed reviewed source record")
    return raw


def admit_source(binding, raw, reviewed_commit=REVIEWED_COMMIT):
    if reviewed_commit != REVIEWED_COMMIT or digest(raw) != REVIEW_RECORD_SHA:
        raise RuntimeError("unaccepted Git source/review record")
    reviewed = json.loads(raw)["sourceBinding"]
    expected = reviewed["conformanceSourceIdentities"]
    pairs = {"providerPath": reviewed["providerPath"], "providerSha256": reviewed["providerSha256"],
             "providerVersion": reviewed["providerVersion"], "positiveProbeSha256": expected["probeSha256"],
             "originalActivationSha256": ACTIVATION_SHA, "observerSha256": expected["adapterSha256"],
             "backgroundSha256": expected["backgroundSha256"],
             "admittedStagerSha256": expected["pluginStagerSha256"],
             "fixtureSha256": reviewed["sources"]["m2_activation_readiness.py"]}
    if any(binding.get(key) != value for key, value in pairs.items()):
        raise RuntimeError("changed admitted component identity")
    if binding.get("conformanceSourceIdentities") != expected:
        raise RuntimeError("changed reviewed composition/evaluator dependency")
    if binding.get("observerFiles") != expected["pluginComposition"]["observer"]["files"]:
        raise RuntimeError("changed canonical observer package")
    # Only the controller is authorized to change in this unit. Its reviewed
    # baseline and fresh execution hash have separate meanings.
    sources = binding.get("reviewedDependencyHashes", {})
    unchanged = {k: v for k, v in reviewed["sources"].items() if k not in ("m2_activation_native.py", "m2_activation_probe.ts")}
    if sources != unchanged:
        raise RuntimeError("changed reviewed controller dependency")
    if binding.get("reviewedControllerSha256") != reviewed["sources"]["m2_activation_native.py"]:
        raise RuntimeError("changed reviewed controller baseline")
    fresh_sources = {"controllerSha256": "m2_activation_native.py", "verdictSha256": "m2_activation_verdict.py",
                     "verdictTestSha256": "test_m2_activation_verdict.py", "activationSha256": "m2_activation_probe.ts",
                     "readinessControlsSha256": "m2_readiness_controls.js",
                     "readinessTestSha256": "test_m2_readiness.py",
                     "controlSourceSha256": "m2_activation_controls.js", "testSourceSha256": "test_m2_activation.py"}
    for key, name in fresh_sources.items():
        value = binding.get(key)
        if value != digest((REPO / "tests/native/tools" / name).read_bytes()):
            raise RuntimeError("missing or changed fresh controller/verifier identity")
    return {"kind": "ACCEPTED_SOURCE_ADMISSION_ONLY", "reviewedGitCommit": REVIEWED_COMMIT,
            "reviewRecordSha256": REVIEW_RECORD_SHA, "binding": binding,
            "nativeActivation": "NOT_RUN", "sourceAdmission": "PASS"}


def reject_retired(freeze):
    if freeze.get("reviewedGitCommit") != REVIEWED_COMMIT:
        raise RuntimeError("stale activation review binding")
    ids = freeze.get("ids", {})
    for key, value in RETIRED.items():
        if not ids.get(key) or ids[key] == value:
            raise RuntimeError("missing or retired activation " + key)



CONTRACT = "ACTIVATION_READINESS_V1"


def snapshot_supported(value, result=False):
    """Validate the exact declared shape; absent/undefined/null remain distinct."""
    if not isinstance(value, dict): return False
    names = ["cwd"] if result else ["cwd", "surface", "isInteractive"]
    if set(value) != {"schema", "supported", "fields", "unexpected"}: return False
    if value["schema"] != ("SessionStartResult/2.1.295/v1" if result else "SessionStartInput/2.1.295/v1"):
        return False
    if value["supported"] is not True or value["unexpected"] != [] or set(value["fields"]) != set(names): return False
    for name in names:
        field = value["fields"][name]
        if not isinstance(field, dict) or field.get("presence") != "present": return False
        kind = field.get("type")
        if name == "cwd":
            valid = kind == "string" and type(field.get("value")) is str
        elif name == "isInteractive":
            valid = kind == "boolean" and type(field.get("value")) is bool
        else:
            valid = kind == "null" and "value" not in field or kind == "string" and field.get("value") in ("terminal", "desktop", "mobile", "vscode")
        if not valid or set(field) != ({"presence", "type"} if kind == "null" else {"presence", "type", "value"}): return False
    return True


def compare(left, right, result=False):
    supported = snapshot_supported(left, result) and snapshot_supported(right, result)
    names = ["cwd"] if result else ["cwd", "surface", "isInteractive"]
    mismatches = [n for n in names if left.get("fields", {}).get(n) != right.get("fields", {}).get(n)] if supported else []
    return {"supported": supported, "equal": supported and not mismatches, "mismatchFields": mismatches}


def adjudicate(freeze, artifacts, execution, *, synthetic=False):
    """Actual acceptance predicate. Operands stay private; output is value-free."""
    failures, missing = [], []
    def require(condition, label, absent=False):
        if not condition: (missing if absent else failures).append(label)
    ids = freeze.get("ids", {}); cfg = freeze.get("config", {})
    require(freeze.get("kind") == CONTRACT and freeze.get("deadlineSeconds") == 20, "changed readiness contract/deadline")
    require(freeze.get("reviewRecordSha256") == REVIEW_RECORD_SHA, "changed immutable review binding")
    require(all(cfg.get(k) == v for k, v in ids.items()), "configuration/source run identity inconsistent")
    require(execution.get("domain") == ("SYNTHETIC" if synthetic else "NATIVE"), "wrong execution domain")
    try:
        reject_retired(freeze); admit_source(ids, read_review())
    except (ValueError, KeyError, RuntimeError) as error:
        failures.append("source admission: " + str(error))
    require(execution.get("freezeSha256") == digest(json.dumps(freeze, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()), "changed frozen run")
    require(execution.get("attempts") == 1 and execution.get("attemptRunId") == ids.get("runId"), "missing or repeated attempt")
    require(execution.get("deadlineExceeded") is False, "external deadline exceeded")
    require(not execution.get("controllerError"), "controller failure")
    require(execution.get("timingRows") == 0 and execution.get("positiveTimingModuleExecuted") is False, "timing path entered")
    processes = execution.get("processes", {})
    cleanup = execution.get("cleanup", [])
    for role, path, sha in (("provider", ids.get("providerPath"), ids.get("providerSha256")),
                            ("responder", ids.get("pythonExecutable"), ids.get("pythonExecutableSha256"))):
        process = processes.get(role, {})
        require(bool(process), role + " process missing", True)
        require(process.get("kernelVerified") is True and process.get("imagePath") == path and process.get("sha256") == sha,
                role + " image identity inconsistent")
        require(process.get("uid") == execution.get("controllerUid") and process.get("parentPid") == execution.get("controllerPid")
                and type(process.get("startUsec")) is int and process.get("startUsec", 0) > 0, role + " ownership/incarnation inconsistent")
        matches = [c for c in cleanup if c.get("role") == role]
        require(len(matches) == 1 and matches[0].get("identity") == process and matches[0].get("reaped") is True
                and not matches[0].get("error"), role + " cleanup incomplete")
    require(len(cleanup) == 2 and execution.get("socketRemoved") is True, "cleanup accounting/socket incomplete")
    init = execution.get("initializer", {})
    response = init.get("response", {})
    require(init.get("type") == "control_response" and response.get("subtype") == "success" and
            response.get("request_id") == freeze.get("initializeRequestId"), "initializer acknowledgement missing", True)
    composition = execution.get("composition", {})
    require(composition.get("entries") == freeze.get("smokeEntries") and composition.get("argv") == freeze.get("argv")
            and composition.get("configurationVerified") is True and composition.get("loadingSourcesBounded") is True
            and composition.get("unexpectedContributors") == [], "plugin composition/isolation incomplete")
    # Independently re-read staged/configuration bytes. Producer flags are not authority.
    try:
        from m2_activation_native import verify_isolation
        owned = Path(cfg["entryOutput"]).parent
        verify_isolation(owned, freeze["smokeEntries"], cfg, freeze["argv"], execution["childConfiguration"])
    except (KeyError, ValueError, RuntimeError, OSError):
        failures.append("independent staged/configuration readback failed")
    # Loading is bounded independently by source, roots, arguments and isolated
    # configuration. Available pre-admission logs are supporting observations,
    # not a newly required exhaustive post-admission receipt.
    require(all(name in ("threadspace-observer", "threadspace-socket-duration") and count == 1
                for name, count in composition.get("loadedModuleCounts", {}).items()), "duplicate or unexpected module loading")
    scan = execution.get("scanner", {})
    require(scan.get("activationSha256") == ids.get("activationSha256") and scan.get("providerSha256") == ids.get("providerSha256")
            and scan.get("success") is True and scan.get("exitCode") == 0 and scan.get("reaped") is True, "fresh scanner admission missing")
    require([a.get("label") for a in artifacts] == ["entry", "forward", "result"], "missing duplicate or reordered output")
    output = {}
    for item in artifacts:
        raw = item.get("raw")
        if not isinstance(raw, bytes) or digest(raw) != item.get("sha256"):
            failures.append("output bytes/hash inconsistent"); continue
        try: value = json.loads(raw)
        except (ValueError, UnicodeError): failures.append("malformed output"); continue
        if not isinstance(value, dict): failures.append("malformed output object"); continue
        require(value.get("kind") == CONTRACT and all(value.get(k) == ids.get(k) for k in ("runId", "nonce", "activationSha256", "positiveProbeSha256")), "foreign output source/run/nonce")
        output[item["label"]] = value
    for n, label in enumerate(("entry", "forward", "result"), 1):
        stages = {"entry": "CALLBACK_ENTERED", "forward": "DOWNSTREAM_SETTLED", "result": "READINESS_READ_BACK"}
        require(output.get(label, {}).get("sequence") == n and output.get(label, {}).get("stage") == stages[label], "missing or reordered " + label, label not in output)
    entry, forward, result = (output.get(k, {}) for k in ("entry", "forward", "result"))
    for item in (entry, forward):
        require(item.get("origin") == {"plugin": "engine", "tier": "core"} and item.get("event") == "session.start", "wrong dispatch origin/event")
    trace = forward.get("trace", [])
    trace_valid = isinstance(trace, list) and bool(trace) and all(isinstance(t, dict) for t in trace)
    require(trace_valid, "settled trace missing", True)
    boundary, observer = {}, "CANONICAL_OBSERVER_PARTICIPATION_UNVERIFIED"
    if trace_valid:
        indices = [t.get("index") for t in trace]
        require(all(type(i) is int and i >= 0 for i in indices) and indices == sorted(set(indices)), "trace reordered or duplicated")
        require(all(t.get("event") == "session.start" and t.get("outcome") in ("returned", "passed") and "reason" not in t
                    and (t.get("plugin") in ("threadspace-observer", "engine") or t.get("tier") == "builtin") for t in trace), "unsettled self-answer or unexpected trace")
        require(trace[-1].get("plugin") == "engine" and trace[-1].get("tier") == "core" and
                sum(t.get("plugin") == "engine" for t in trace) == 1, "valid engine tail missing")
        # Host ordering is nearest beneath first. Never search for a matching value.
        boundary = trace[0]
        require(all(snapshot_supported(t.get("received")) and snapshot_supported(t.get("returned"), True) for t in trace), "unsupported or missing trace operands", True)
        observed = [t for t in trace if t.get("plugin") == "threadspace-observer"]
        if observed:
            require(len(observed) == 1 and observed[0].get("tier") == "user", "invalid observer trace attribution")
            if len(observed) == 1 and observed[0].get("tier") == "user" and observed[0].get("outcome") in ("returned", "passed"):
                observer = "OBSERVED DOWNSTREAM"
    comparisons = {"originalToReceived": compare(entry.get("original", {}), boundary.get("received", {})),
                   "resolvedToReturned": compare(forward.get("result", {}), boundary.get("returned", {}), True)}
    for label, comparison in comparisons.items():
        require(comparison["supported"], label + " operands unsupported/missing", True)
        if comparison["supported"]: require(comparison["equal"], label + " value mismatch")
    ack = cfg.get("acknowledgement")
    try: expected_ack = acknowledgement(ids)
    except KeyError: expected_ack = None
    require(expected_ack is not None and ack == expected_ack, "acknowledgement does not bind source/run/responder/boot")
    require(result.get("acknowledgement") == ack and result.get("httpStatus") == 200, "readiness acknowledgement inconsistent")
    transcript = execution.get("transcript", [])
    complete = isinstance(transcript, list) and all(isinstance(t, dict) for t in transcript) and [t.get("event") for t in transcript] == ["start", "received", "generated", "answered", "stop"]
    require(complete, "incomplete duplicate reordered or extra native transcript", True)
    if complete:
        require(transcript[0].get("identity") == ack and transcript[0].get("pid") == processes.get("responder", {}).get("pid")
                and transcript[0].get("uid") == execution.get("controllerUid"), "foreign responder transcript")
        require(all(t.get("attempt") == 1 and t.get("status") == 200 for t in transcript[1:4]) and
                transcript[1].get("nonceMatched") is True and transcript[1].get("peerUid") == execution.get("controllerUid") and transcript[2].get("body") == ack and
                all(transcript[-1].get(k) == 1 for k in ("attempts", "generated", "answered", "accepted")), "forged replayed or incomplete HTTP exchange")
    return {"contract": CONTRACT, "domain": "SYNTHETIC" if synthetic else "NATIVE",
            "verdict": "FAIL" if failures else "INCOMPLETE" if missing else "PASS", "failures": failures, "missing": missing,
            "comparisons": comparisons, "canonicalObserver": observer, "timingQualified": False}
