"""Accepted-source admission and fail-closed activation evidence preflight.

The unchanged activation entry cannot supply every required native witness.
Consistent output files are partial evidence, never an activation PASS.
"""
from __future__ import annotations
import hashlib
import json
import subprocess
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
             "activationSha256": ACTIVATION_SHA, "observerSha256": expected["adapterSha256"],
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
    unchanged = {k: v for k, v in reviewed["sources"].items() if k != "m2_activation_native.py"}
    if sources != unchanged:
        raise RuntimeError("changed reviewed controller dependency")
    if binding.get("reviewedControllerSha256") != reviewed["sources"]["m2_activation_native.py"]:
        raise RuntimeError("changed reviewed controller baseline")
    fresh_sources = {"controllerSha256": "m2_activation_native.py", "verdictSha256": "m2_activation_verdict.py",
                     "verdictTestSha256": "test_m2_activation_verdict.py", "projectionControlSha256": "m2_activation_projection_controls.js"}
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


def authority_preflight(activation_raw):
    if digest(activation_raw) != ACTIVATION_SHA:
        raise RuntimeError("changed accepted activation module")
    return {"kind": "NATIVE_EVIDENCE_AUTHORITY_PREFLIGHT", "activationSha256": ACTIVATION_SHA,
            "verdict": "INCOMPLETE", "nativeExecutionAdmitted": False,
            "missingAuthority": [
                "No independently retained original event and downstream result values: the fixed entry projects them out of next.trace and only awaits its result.",
                "next.trace covers links beneath the activation hook only; engine-only completion cannot attribute an upstream canonical observer.",
                "The fixed entry does not retain an independent dispatch/continuation count or a host-owned output-persistence sequence receipt."
            ],
            "supportedPartialEvidence": ["callback file bytes", "host origin/event and downstream trace labels",
                                         "readiness acknowledgement and independent fixture transcript"],
            "resultFilePresenceIsPassAuthority": False}


def require_native_authority(activation_raw):
    verdict = authority_preflight(activation_raw)
    if not verdict["nativeExecutionAdmitted"]:
        raise RuntimeError("NATIVE_EVIDENCE_AUTHORITY_INCOMPLETE: " + "; ".join(verdict["missingAuthority"]))


def assess_partial_outputs(config, artifacts, transcript):
    """Check independently read bytes; cannot manufacture the missing authority.

    artifacts is an ordered list of {label, raw, sha256}. raw is bytes read
    by the controller, not a supplied parsed result.json or a PASS flag.
    This portable adjudicator is not a native execution receipt.
    """
    failures = []
    outputs = {}
    if [a.get("label") for a in artifacts] != ["entry", "forward", "result"]:
        failures.append("missing, duplicate or reordered output readbacks")
    for item in artifacts:
        raw = item.get("raw")
        if not isinstance(raw, bytes) or digest(raw) != item.get("sha256"):
            failures.append("output bytes/hash mismatch")
            continue
        try:
            value = json.loads(raw)
        except (ValueError, UnicodeError):
            failures.append("malformed output bytes")
            continue
        if not isinstance(value, dict):
            failures.append("malformed output object")
            continue
        identity = {"kind": "ACTIVATION_ONLY", **{k: config[k] for k in
                    ("runId", "nonce", "activationSha256", "positiveProbeSha256")}}
        if any(value.get(k) != v for k, v in identity.items()):
            failures.append("foreign or stale source/run/nonce")
        outputs[item["label"]] = value
    expected_stages = {"entry": "CALLBACK_ENTERED", "forward": "DOWNSTREAM_SETTLED", "result": "READINESS_READ_BACK"}
    if any(outputs.get(k, {}).get("stage") != v for k, v in expected_stages.items()):
        failures.append("missing or inconsistent callback stage")
    forward = outputs.get("forward", {})
    if forward.get("origin") != {"plugin": "engine", "tier": "core"} or forward.get("event") != "session.start":
        failures.append("foreign dispatch origin/event")
    trace = forward.get("trace", [])
    if not isinstance(trace, list) or not trace or any(not isinstance(t, dict) for t in trace):
        failures.append("missing host trace")
    else:
        indices = [t.get("index") for t in trace]
        if any(type(i) is not int for i in indices) or indices != sorted(set(indices)):
            failures.append("duplicate or reordered trace")
        if any(t.get("event") != "session.start" or t.get("outcome") not in ("returned", "passed") or "reason" in t for t in trace):
            failures.append("unsettled or bypassed dispatch")
        if len([t for t in trace if t.get("plugin") == "threadspace-observer"]) != 1:
            failures.append("original observer attribution UNVERIFIED")
        if any(t.get("plugin") == "threadspace-observer" and t.get("tier") != "user" for t in trace):
            failures.append("foreign observer tier")
        if trace[-1].get("plugin") != "engine" or trace[-1].get("tier") != "core":
            failures.append("missing engine tail")
        if any(t.get("plugin") not in ("threadspace-observer", "engine") for t in trace):
            failures.append("unexpected trace contributor")
    ack = config["acknowledgement"]
    if outputs.get("result", {}).get("acknowledgement") != ack:
        failures.append("foreign readiness acknowledgement")
    if not isinstance(transcript, list) or any(not isinstance(t, dict) for t in transcript) or [t.get("event") for t in transcript] != ["start", "received", "generated", "answered", "stop"]:
        failures.append("incomplete or reordered native transcript")
    elif (transcript[0].get("identity") != ack or
          any(t.get("attempt") != 1 or t.get("status") != 200 for t in transcript[1:4]) or
          transcript[1].get("nonceMatched") is not True or transcript[2].get("body") != ack or
          any(transcript[-1].get(k) != 1 for k in ("attempts", "generated", "answered", "accepted"))):
        failures.append("inconsistent, replayed or foreign readiness exchange")
    return {"verdict": "INCOMPLETE", "structuralReadbacksConsistent": not failures,
            "failures": failures, "nativeExecutionOrCleanupCertified": False,
            "missingAuthority": authority_preflight((REPO / "tests/native/tools/m2_activation_probe.ts").read_bytes())["missingAuthority"]}
