#!/usr/bin/env python3
"""One activation-only smoke. Never execute the positive timing callback."""
from __future__ import annotations
import hashlib
import http.client
import json
import os
import socket
import subprocess
import sys
import time
import uuid
from pathlib import Path
from m2_socket_plugins import OBSERVER_FILES, OBSERVER, write_private, json_bytes, package_files
from m2_socket_runner import PROVIDER, PROVIDER_VERSION, PROVIDER_SHA, image_identity, cleanup, provider_argv
from m2_socket_duration import HERE, canonical_hash, source_identities, file_hash
from m2_activation_readiness import acknowledgement
from m2_activation_verdict import (REVIEWED_COMMIT, REVIEW_RECORD_SHA, read_review, admit_source,
    reject_retired, require_native_authority)

DEADLINE = 20
ACTIVATION = HERE / "m2_activation_probe.ts"
FIXTURE = HERE / "m2_activation_readiness.py"


def save(path, value):
    write_private(path, json_bytes(value))


def source_binding():
    reviewed = json.loads(read_review())["sourceBinding"]["sources"]
    return {"providerPath": str(PROVIDER), "providerSha256": PROVIDER_SHA, "providerVersion": PROVIDER_VERSION,
        "observerSha256": source_identities()["adapterSha256"],
        "positiveProbeSha256": file_hash(HERE / "m2_socket_probe.ts"),
        "activationSha256": file_hash(ACTIVATION), "fixtureSha256": file_hash(FIXTURE),
        "controllerSha256": file_hash(__file__), "pythonExecutableSha256": file_hash(Path(sys.executable).resolve()),
        "controlSourceSha256": file_hash(HERE / "m2_activation_controls.js"),
        "testSourceSha256": file_hash(HERE / "test_m2_activation.py"),
        "admittedStagerSha256": file_hash(HERE / "m2_socket_plugins.py"),
        "backgroundSha256": file_hash(HERE / "m2_socket_background.ts"),
        "conformanceSourceIdentities": source_identities(),
        "reviewedControllerSha256": reviewed["m2_activation_native.py"],
        "reviewedDependencyHashes": {k: file_hash(HERE / k) for k in reviewed if k != "m2_activation_native.py"},
        "verdictSha256": file_hash(HERE / "m2_activation_verdict.py"),
        "verdictTestSha256": file_hash(HERE / "test_m2_activation_verdict.py"),
        "projectionControlSha256": file_hash(HERE / "m2_activation_projection_controls.js"),
        "observerFiles": {name: file_hash(OBSERVER / name) for name in OBSERVER_FILES}}


def staging_files(role, config):
    observer, positive = package_files(config)
    if role == "observer": files = observer
    elif role == "positive": files = positive
    elif role == "duplicate":
        files = dict(positive)
        files["hooks/register.ts"] = (HERE.parents[2] / "evidence/M2/replacement-activation-repair/original-combined-probe.ts").read_bytes()
        for name in ("register.ts", "delivery.ts", "ownership.ts"):
            files["hooks/observer.ts" if name == "register.ts" else "hooks/" + name] = (OBSERVER / "hooks" / name).read_bytes()
    elif role == "activation":
        files = {".claude-plugin/plugin.json": json_bytes({"name": "threadspace-socket-duration", "version": "2.0.0",
            "description": "Qualification-only activation smoke; no timing measurements"}),
            "hooks/hooks.json": json_bytes({"modules": ["./activation.ts"]}),
            "hooks/activation.ts": ACTIVATION.read_bytes(),
            "hooks/config.ts": ("export const config = " + json.dumps(config, allow_nan=False) + ";\n").encode()}
    else: raise ValueError("unknown staging role")
    return files


def stage(root, role, config):
    root.mkdir(mode=0o700)
    files = staging_files(role, config)
    for directory in sorted({str(Path(name).parent) for name in files}): (root / directory).mkdir(mode=0o700)
    for name, data in files.items(): write_private(root / name, data)
    manifest = json.loads(files[".claude-plugin/plugin.json"])
    return {"role": role, "root": str(root.resolve()), "name": manifest["name"],
        "modules": json.loads(files["hooks/hooks.json"])["modules"], "files": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}


def verify_staging(entries, config=None):
    if len({entry["root"] for entry in entries}) != len(entries) or len({entry["name"] for entry in entries}) != len(entries): raise RuntimeError("duplicate plugin root/name")
    for entry in entries:
        root = Path(entry["root"])
        expected = staging_files(entry["role"], config)
        # A metadata/file substitution cannot relabel canonical package bytes.
        # Full configuration admission additionally requires its frozen config.
        if config is None: expected.pop("hooks/config.ts", None)
        allowed = set(expected) | ({"hooks/config.ts"} if config is None else set())
        if set(entry["files"]) - allowed:
            raise RuntimeError("unexpected canonical staged resource")
        if any(entry["files"].get(name) != hashlib.sha256(raw).hexdigest() for name, raw in expected.items()):
            raise RuntimeError("changed canonical staged composition/configuration")
        if root.is_symlink() or root.stat().st_uid != os.getuid() or root.stat().st_mode & 0o777 != 0o700: raise RuntimeError("unowned staged root")
        actual = {str(p.relative_to(root)) for p in root.rglob("*") if p.is_file()}
        if actual != set(entry["files"]): raise RuntimeError("missing or unexpected staged file")
        for path in root.rglob("*"):
            if path.is_symlink() or path.stat().st_uid != os.getuid() or path.stat().st_mode & 0o777 != (0o700 if path.is_dir() else 0o600): raise RuntimeError("unprotected staged resource")
        if any(file_hash(root / name) != digest for name, digest in entry["files"].items()): raise RuntimeError("changed staged source")
        if json.loads((root / ".claude-plugin/plugin.json").read_text())["name"] != entry["name"] or json.loads((root / "hooks/hooks.json").read_text())["modules"] != entry["modules"]: raise RuntimeError("changed plugin name/module identity")


def environment(root):
    cfg = root / "config"; cfg.mkdir(mode=0o700)
    save(cfg / "settings.json", {"hooks": {}, "disableAllHooks": False})
    env = {key: value for key, value in os.environ.items() if not key.startswith("CLAUDE_CODE_PLUGIN_")}
    env["CLAUDE_CONFIG_DIR"] = str(cfg)
    return env


def validate(root, entry, env):
    """Supported CLI scanner only: no register evaluation, callback or inference."""
    verify_staging([entry])
    prefix = [str(PROVIDER), "--settings", str(root / "config/settings.json"), "--setting-sources", "",
        "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--no-session-persistence"]
    argv = prefix + ["plugin", "validate", entry["root"], "--json"]
    stdout = root / (entry["role"] + "-validation.stdout")
    stderr = root / (entry["role"] + "-validation.stderr")
    with stdout.open("xb") as out, stderr.open("xb") as err:
        os.chmod(stdout, 0o600); os.chmod(stderr, 0o600)
        child = subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True)
        identity = image_identity(child, PROVIDER, PROVIDER_VERSION)
        expired = False
        try: child.wait(timeout=DEADLINE)
        except subprocess.TimeoutExpired: expired = True
        cleaned = cleanup(child, identity)
    try: result = json.loads(stdout.read_bytes())
    except ValueError: result = None
    return {"kind": "NON_EXECUTING_INSTALLED_SCANNER", "argv": argv, "entry": entry, "process": identity,
        "deadlineExceeded": expired, "cleanup": cleaned, "stdoutSha256": file_hash(stdout),
        "stderrSha256": file_hash(stderr), "exitCode": child.returncode, "report": result}


def classify(outputs, diagnostic):
    if outputs.get("error"):
        return outputs["error"].get("stage", "CALLBACK_FAILURE")
    if outputs.get("result"): return "ACTIVATION_OUTPUT_AND_HTTP_RETURNED"
    if outputs.get("forward"): return "HTTP_READINESS_MISSING_AFTER_FORWARDING"
    if outputs.get("entry"): return "DOWNSTREAM_OR_OUTPUT_MISSING_AFTER_ENTRY"
    if "hooks module" in diagnostic and "failed to load" in diagnostic: return "REGISTRATION_REFUSAL_REPORTED"
    return "INITIALIZATION_ONLY_OR_MISSING_CALLBACK_WITNESS"


def prepare(root):
    root = Path(root).resolve(strict=True)
    if file_hash(PROVIDER) != PROVIDER_SHA: raise RuntimeError("required provider bytes changed")
    binding = source_binding()
    admit_source(binding, read_review())
    # No freeze/nonce/process can be created while mandatory preflight B is
    # unsatisfied. File existence and classify() are not acceptance authority.
    require_native_authority(ACTIVATION.read_bytes())
    ids = {**binding, "runId": str(uuid.uuid4()), "responderId": str(uuid.uuid4()), "nonce": uuid.uuid4().hex + uuid.uuid4().hex,
        "bootId": subprocess.check_output(["/usr/sbin/sysctl", "-n", "kern.bootsessionuuid"], text=True).strip().lower()}
    validation = root / "validation"; validation.mkdir(mode=0o700); env = environment(validation)
    dummy = {"kind": "NON_EXECUTING_VALIDATION_ONLY", "plan": [], "delays": [], "runtimeId": str(uuid.uuid4())}
    entries = [stage(validation / role, role, dummy) for role in ("duplicate", "observer", "positive")]
    owned = root / "positive"; owned.mkdir(mode=0o700); fixture = owned / "readiness"; fixture.mkdir(mode=0o700)
    config = {**ids, "socketPath": str(fixture / "s.sock"), "acknowledgement": acknowledgement(ids)}
    config.update({key + "Output": str(owned / (key + ".json")) for key in ("entry", "forward", "result", "error")})
    smoke_entries = [stage(owned / "observer-plugin", "observer", config), stage(owned / "duration-plugin", "activation", config)]
    positive_env = environment(owned)
    argv = provider_argv(owned, {"plugins": smoke_entries}) + ["--debug-file", str(owned / "provider-debug.log")]
    freeze = {"kind": "ACTIVATION_ONLY_ONE_POSITIVE", "baseSha": REVIEWED_COMMIT,
        "reviewedGitCommit": REVIEWED_COMMIT, "reviewRecordSha256": REVIEW_RECORD_SHA, "deadlineSeconds": DEADLINE,
        "ids": ids, "config": config, "validationEntries": entries, "smokeEntries": smoke_entries, "argv": argv,
        "osBuild": subprocess.check_output(["/usr/bin/sw_vers", "-buildVersion"], text=True).strip(),
        "originalPositiveModuleExecuted": False, "timingRows": 0}
    save(fixture / "readiness-config.json", ids)
    save(root / "activation-freeze.json", freeze)
    return freeze, env, positive_env


def protocol_control(root, ids):
    """One independently owned fixture-only control; no provider or measurements."""
    root = Path(root); root.mkdir(mode=0o700)
    config = {**ids, "runId": str(uuid.uuid4()), "responderId": str(uuid.uuid4()), "nonce": uuid.uuid4().hex + uuid.uuid4().hex}
    save(root / "readiness-config.json", config)
    child = subprocess.Popen([str(Path(sys.executable).resolve()), str(FIXTURE), str(root)], cwd=root,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    identity = image_identity(child, Path(sys.executable).resolve())
    results = []
    try:
        until = time.monotonic() + 3
        while not (root / "s.sock").exists() and child.poll() is None and time.monotonic() < until: time.sleep(.01)
        route = "/" + "/".join(("ready", config["runId"], config["responderId"], config["nonce"], config["activationSha256"]))
        requests = [("forged-nonce", route.replace(config["nonce"], "foreign"), 403),
            ("control-acknowledgement", route, 200), ("replay", route, 409),
            ("foreign-responder", route.replace(config["responderId"], "foreign"), 403)]
        for label, path, expected in requests:
            connection = http.client.HTTPConnection("localhost", timeout=2)
            connection.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); connection.sock.settimeout(2); connection.sock.connect(str(root / "s.sock"))
            connection.request("GET", path); response = connection.getresponse(); body = json.loads(response.read()); connection.close()
            if response.status != expected or expected == 200 and body != acknowledgement(config): raise RuntimeError("protocol control failed: " + label)
            results.append({"control": label, "status": response.status, "body": body})
        child.wait(timeout=3)
    finally: cleaned = cleanup(child, identity)
    transcript = [json.loads(line) for line in (root / "readiness.jsonl").read_text().splitlines()]
    if transcript[-1] != {"event": "stop", "attempts": 4, "generated": 4, "answered": 4, "accepted": 1}: raise RuntimeError("incomplete fixture control accounting")
    record = {"kind": "OWNED_NATIVE_READINESS_PROTOCOL_CONTROL", "purpose": "Reject a forged/replayed nonce on the exact socket server; no provider is invoked",
        "sourceIdentity": source_binding(), "config": config, "process": identity, "results": results,
        "transcript": transcript, "cleanup": cleaned, "socketRemoved": not (root / "s.sock").exists()}
    save(root / "protocol-control.json", record)
    return record


def smoke(root, freeze, env):
    reject_retired(freeze)
    admit_source(source_binding(), read_review())
    require_native_authority(ACTIVATION.read_bytes())
    root = Path(root); owned = root / "positive"; fixture = owned / "readiness"
    if (owned / "attempt-started.json").exists(): raise RuntimeError("positive activation attempt already consumed")
    if source_binding() != {key: freeze["ids"][key] for key in source_binding()}: raise RuntimeError("frozen activation sources changed")
    if canonical_hash(freeze) != canonical_hash(json.loads((root / "activation-freeze.json").read_text())): raise RuntimeError("changed frozen activation configuration")
    verify_staging(freeze["smokeEntries"], freeze["config"])
    save(owned / "attempt-started.json", {"kind": freeze["kind"], "runId": freeze["ids"]["runId"], "freezeSha256": canonical_hash(freeze)})
    responder = subprocess.Popen([str(Path(sys.executable).resolve()), str(FIXTURE), str(fixture)], cwd=fixture,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    responder_id = image_identity(responder, Path(sys.executable).resolve())
    child = None; provider_id = None; report = {"kind": freeze["kind"], "freezeSha256": canonical_hash(freeze),
        "processes": {"responder": responder_id}, "cleanup": [], "outputs": {}, "deadlineExceeded": False}
    try:
        until = time.monotonic() + 3
        while not (fixture / "s.sock").exists() and responder.poll() is None and time.monotonic() < until: time.sleep(.01)
        if not (fixture / "s.sock").exists() or (fixture / "s.sock").stat().st_mode & 0o777 != 0o600: raise RuntimeError("readiness bind/ownership failed")
        with (owned / "provider-stdout.log").open("xb") as out, (owned / "provider-stderr.log").open("xb") as err:
            os.chmod(owned / "provider-stdout.log", 0o600); os.chmod(owned / "provider-stderr.log", 0o600)
            started = time.monotonic()
            child = subprocess.Popen(freeze["argv"], cwd=owned, env=env, stdin=subprocess.PIPE, stdout=out, stderr=err, start_new_session=True)
            provider_id = image_identity(child, PROVIDER, PROVIDER_VERSION); report["processes"]["provider"] = provider_id
            child.stdin.write((json.dumps({"type": "control_request", "request_id": str(uuid.uuid4()), "request": {"subtype": "initialize"}}) + "\n").encode()); child.stdin.flush()
            while time.monotonic() < started + DEADLINE:
                if (owned / "result.json").exists() or (owned / "error.json").exists() or child.poll() is not None: break
                time.sleep(.02)
            else: report["deadlineExceeded"] = True
    except Exception as error:
        report["controllerError"] = str(error)
    finally:
        if child is not None and provider_id is not None: report["cleanup"].append(cleanup(child, provider_id))
        report["cleanup"].append(cleanup(responder, responder_id))
        report["socketRemoved"] = not (fixture / "s.sock").exists()
        for label in ("entry", "forward", "result", "error"):
            path = owned / (label + ".json")
            if path.exists():
                raw = path.read_bytes()
                try: value = json.loads(raw)
                except ValueError: value = {"malformed": True}
                report["outputs"][label] = value
                report.setdefault("readback", {})[label] = {"sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}
        report["transcript"] = [json.loads(line) for line in (fixture / "readiness.jsonl").read_text().splitlines()]
        debug = owned / "provider-debug.log"
        report["failureBoundary"] = classify(report["outputs"], debug.read_text() if debug.exists() else "")
        save(root / "activation-execution.json", report)
    return report
