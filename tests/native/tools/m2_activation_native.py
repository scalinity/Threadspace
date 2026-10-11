#!/usr/bin/env python3
"""One activation-only smoke. Never execute the positive timing callback."""
from __future__ import annotations
import ctypes
import re
import signal
from contextlib import contextmanager
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
    reject_retired, adjudicate, CONTRACT, ACTIVATION_SHA)

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
        "activationSha256": file_hash(ACTIVATION), "originalActivationSha256": ACTIVATION_SHA, "fixtureSha256": file_hash(FIXTURE),
        "controllerSha256": file_hash(__file__), "pythonExecutableSha256": file_hash(Path(sys.executable).resolve()), "pythonExecutable": str(Path(sys.executable).resolve()),
        "controlSourceSha256": file_hash(HERE / "m2_activation_controls.js"),
        "testSourceSha256": file_hash(HERE / "test_m2_activation.py"),
        "admittedStagerSha256": file_hash(HERE / "m2_socket_plugins.py"),
        "backgroundSha256": file_hash(HERE / "m2_socket_background.ts"),
        "conformanceSourceIdentities": source_identities(),
        "reviewedControllerSha256": reviewed["m2_activation_native.py"],
        "reviewedDependencyHashes": {k: file_hash(HERE / k) for k in reviewed if k not in ("m2_activation_native.py", "m2_activation_probe.ts")},
        "verdictSha256": file_hash(HERE / "m2_activation_verdict.py"),
        "verdictTestSha256": file_hash(HERE / "test_m2_activation_verdict.py"),
        "readinessControlsSha256": file_hash(HERE / "m2_readiness_controls.js"),
        "readinessTestSha256": file_hash(HERE / "test_m2_readiness.py"),
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
            "hooks/config.ts": ("export const config = " + json.dumps(config, sort_keys=True, allow_nan=False) + ";\n").encode()}
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



def kernel_child(child, executable):
    """Independent signal ownership even if full image/hash acquisition fails."""
    if child.poll() is not None: return None
    lib = ctypes.CDLL('/usr/lib/libproc.dylib')
    path, data = ctypes.create_string_buffer(4096), ctypes.create_string_buffer(136)
    if lib.proc_pidpath(child.pid, path, len(path)) <= 0 or lib.proc_pidinfo(child.pid, 3, 0, data, 136) != 136:
        raise RuntimeError("kernel child ownership unavailable")
    raw = data.raw
    identity = {"pid": child.pid, "uid": int.from_bytes(raw[20:24], 'little'),
        "parentPid": int.from_bytes(raw[16:20], 'little'),
        "startUsec": int.from_bytes(raw[120:128], 'little') * 1000000 + int.from_bytes(raw[128:136], 'little'),
        "imagePath": str(Path(os.fsdecode(path.value)).resolve())}
    if identity['uid'] != os.getuid() or identity['parentPid'] != os.getpid() or identity['imagePath'] != str(Path(executable).resolve()):
        raise RuntimeError("foreign child ownership")
    return identity


def reap_owned(child, identity, executable):
    events = []
    current = kernel_child(child, executable)
    baseline = {k: identity[k] for k in current} if identity and current else current
    if current:
        if current != baseline or kernel_child(child, executable) != baseline:
            raise RuntimeError("cleanup refused changed child incarnation")
        child.send_signal(signal.SIGTERM); events.append('verified-child-SIGTERM')
        try: child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            if kernel_child(child, executable) != baseline: raise RuntimeError("cleanup refused reused child")
            child.kill(); events.append('verified-child-SIGKILL'); child.wait(timeout=3)
    else:
        child.wait(timeout=0); events.append('already-exited')
    if child.stdin is not None: child.stdin.close()
    return {"identity": identity, "signalOwnership": baseline, "events": events,
            "exitCode": child.returncode, "reaped": child.poll() is not None}


@contextmanager
def owned_process(role, argv, executable, records, version=None, **kwargs):
    """Every successfully spawned direct child reaches the cleanup guard."""
    child = None; identity = None
    receipt = {"role": role, "spawned": False, "reaped": False}
    records.append(receipt)
    try:
        child = subprocess.Popen(argv, **kwargs); receipt['spawned'] = True
        try: identity = image_identity(child, executable, version)
        except Exception as error:
            receipt['identityError'] = type(error).__name__ + ': ' + str(error)
            raise
        receipt['identity'] = identity
        yield child, identity
    finally:
        if child is not None:
            try: receipt.update(reap_owned(child, identity, executable))
            except Exception as error:
                receipt['error'] = type(error).__name__ + ': ' + str(error)
                receipt['reaped'] = child.poll() is not None
        if receipt.get('error'): raise RuntimeError('owned cleanup incomplete')


def validate(root, entry, env):
    """One supported non-executing scanner; guard starts at process creation."""
    root = Path(root); verify_staging([entry])
    argv = [str(PROVIDER), '--settings', str(root / 'config/settings.json'), '--setting-sources', '',
        '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--no-session-persistence',
        'plugin', 'validate', entry['root'], '--json']
    save(root / 'scanner-attempt.json', {'entry': entry, 'sources': source_binding(), 'deadlineSeconds': DEADLINE})
    stdout, stderr = root / 'scanner.stdout', root / 'scanner.stderr'
    records = []; expired = False; error = None; identity = None; child = None
    with stdout.open('xb') as out, stderr.open('xb') as err:
        os.chmod(stdout, 0o600); os.chmod(stderr, 0o600)
        until = time.monotonic() + DEADLINE
        try:
            with owned_process('validator', argv, PROVIDER, records, PROVIDER_VERSION, cwd=root, env=env,
                               stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True) as (child, identity):
                try: child.wait(timeout=max(0, until - time.monotonic()))
                except subprocess.TimeoutExpired: expired = True
        except Exception as failure: error = str(failure)
    try: parsed = json.loads(stdout.read_bytes())
    except ValueError: parsed = None
    result = {'kind': 'NON_EXECUTING_INSTALLED_SCANNER', 'argv': argv, 'entry': entry, 'process': identity,
        'deadlineSeconds': DEADLINE, 'deadlineExceeded': expired, 'cleanup': records,
        'stdoutSha256': file_hash(stdout), 'stderrSha256': file_hash(stderr), 'report': parsed,
        'exitCode': child.returncode if child else None, 'controllerError': error, 'callbacksExecuted': False}
    save(root / 'scanner-result.json', result)
    return result


def classify(outputs, diagnostic):
    if outputs.get('error'): return outputs['error'].get('stage', 'CALLBACK_FAILURE')
    if outputs.get('result'): return 'ACTIVATION_OUTPUT_AND_HTTP_RETURNED'
    if outputs.get('forward'): return 'HTTP_READINESS_MISSING_AFTER_FORWARDING'
    if outputs.get('entry'): return 'DOWNSTREAM_OR_OUTPUT_MISSING_AFTER_ENTRY'
    if 'hooks module' in diagnostic and 'failed to load' in diagnostic: return 'REGISTRATION_REFUSAL_REPORTED'
    return 'INITIALIZATION_ONLY_OR_MISSING_CALLBACK_WITNESS'


def require_controls(root):
    gate = json.loads((Path(root) / 'portable-admission.json').read_bytes())
    if gate['sources'] != source_binding() or gate['verdict'] != 'PASS' or not gate['syntheticBaselinePass']:
        raise RuntimeError('portable evidence verifier/source admission incomplete')
    for name, sha in gate['outputs'].items():
        if file_hash(Path(root) / name) != sha: raise RuntimeError('changed source-bound control output')
    return gate


def verify_isolation(owned, entries, config, argv, env):
    verify_staging(entries, config)
    if [e['role'] for e in entries] != ['observer', 'activation'] or [e['name'] for e in entries] != ['threadspace-observer', 'threadspace-socket-duration']:
        raise RuntimeError('changed plugin composition')
    expected = provider_argv(owned, {'plugins': entries}) + ['--debug-file', str(owned / 'provider-debug.log')]
    if argv != expected: raise RuntimeError('changed effective provider arguments')
    if env.get('CLAUDE_CONFIG_DIR') != str(owned / 'config') or any(k.startswith('CLAUDE_CODE_PLUGIN_') for k in env):
        raise RuntimeError('unbounded plugin environment/configuration')
    settings = owned / 'config/settings.json'
    if settings.read_bytes() != json_bytes({'hooks': {}, 'disableAllHooks': False}) or settings.stat().st_mode & 0o777 != 0o600:
        raise RuntimeError('changed private settings')
    if any(p.is_symlink() for p in (owned, owned / 'config', settings)):
        raise RuntimeError('aliased owned configuration')
    return {'entries': entries, 'argv': argv, 'configurationVerified': True, 'loadingSourcesBounded': True,
            'runtimeLoadedNames': [], 'unexpectedContributors': []}


def prepare(root, scanner):
    """Freeze only after final portable controls and exact activation scan."""
    root = Path(root).resolve(strict=True); require_controls(root)
    if file_hash(PROVIDER) != PROVIDER_SHA: raise RuntimeError('provider changed')
    binding = source_binding(); admit_source(binding, read_review())
    if scanner.get('exitCode') != 0 or scanner.get('deadlineExceeded') or scanner.get('controllerError') or scanner.get('report', {}).get('success') is not True or not all(c.get('reaped') for c in scanner['cleanup']):
        raise RuntimeError('fresh scanner not admitted')
    if scanner['entry']['files']['hooks/activation.ts'] != binding['activationSha256'] or scanner['process']['sha256'] != PROVIDER_SHA:
        raise RuntimeError('scanner source/executable mismatch')
    ids = {**binding, 'runId': str(uuid.uuid4()), 'responderId': str(uuid.uuid4()), 'nonce': uuid.uuid4().hex + uuid.uuid4().hex,
        'bootId': subprocess.check_output(['/usr/sbin/sysctl', '-n', 'kern.bootsessionuuid'], text=True).strip().lower()}
    owned = root / 'positive'; owned.mkdir(mode=0o700); fixture = owned / 'readiness'; fixture.mkdir(mode=0o700)
    config = {**ids, 'socketPath': str(fixture / 's.sock'), 'acknowledgement': acknowledgement(ids)}
    config.update({key + 'Output': str(owned / (key + '.json')) for key in ('entry', 'forward', 'result', 'error')})
    config = json.loads(json_bytes(config))
    entries = [stage(owned / 'observer-plugin', 'observer', config), stage(owned / 'activation-plugin', 'activation', config)]
    env = environment(owned)
    argv = provider_argv(owned, {'plugins': entries}) + ['--debug-file', str(owned / 'provider-debug.log')]
    verify_isolation(owned, entries, config, argv, env)
    freeze = {'kind': CONTRACT, 'baseSha': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
        'reviewedGitCommit': REVIEWED_COMMIT, 'reviewRecordSha256': REVIEW_RECORD_SHA, 'deadlineSeconds': DEADLINE,
        'ids': ids, 'config': config, 'smokeEntries': entries, 'argv': argv, 'initializeRequestId': str(uuid.uuid4()),
        'scanner': {'activationSha256': binding['activationSha256'], 'providerSha256': PROVIDER_SHA,
            'success': True, 'exitCode': 0, 'reaped': True, 'resultSha256': canonical_hash(scanner)},
        'osBuild': subprocess.check_output(['/usr/bin/sw_vers', '-buildVersion'], text=True).strip(),
        'originalPositiveModuleExecuted': False, 'timingRows': 0}
    reject_retired(freeze); save(fixture / 'readiness-config.json', ids); save(root / 'activation-freeze.json', freeze)
    return freeze, env


def read_outputs(owned):
    artifacts, outputs = [], {}
    for label in ('entry', 'forward', 'result', 'error'):
        path = owned / (label + '.json')
        if not path.exists(): continue
        if path.is_symlink() or path.stat().st_uid != os.getuid(): raise RuntimeError('foreign output resource')
        os.chmod(path, 0o600)
        raw = path.read_bytes()
        artifacts.append({'label': label, 'raw': raw, 'sha256': hashlib.sha256(raw).hexdigest()})
        try: outputs[label] = json.loads(raw)
        except ValueError: outputs[label] = {'malformed': True}
    return artifacts, outputs


def smoke(root, freeze, env):
    root = Path(root); require_controls(root); reject_retired(freeze)
    if freeze['ids'] != {**source_binding(), **{k: freeze['ids'][k] for k in ('runId', 'responderId', 'nonce', 'bootId')}}:
        raise RuntimeError('changed frozen source binding')
    if (root / 'activation-freeze.json').read_bytes() != json_bytes(freeze): raise RuntimeError('changed frozen run')
    owned = root / 'positive'; fixture = owned / 'readiness'
    composition = verify_isolation(owned, freeze['smokeEntries'], freeze['config'], freeze['argv'], env)
    # Exclusive, durable reservation consumes this attempt even if spawn fails.
    save(owned / 'attempt-started.json', {'runId': freeze['ids']['runId'], 'freezeSha256': canonical_hash(freeze)})
    record = {'domain': 'NATIVE', 'freezeSha256': canonical_hash(freeze), 'attempts': 1,
        'attemptRunId': freeze['ids']['runId'], 'controllerUid': os.getuid(), 'controllerPid': os.getpid(),
        'processes': {}, 'cleanup': [], 'composition': composition, 'scanner': freeze['scanner'],
        'childConfiguration': {'CLAUDE_CONFIG_DIR': env['CLAUDE_CONFIG_DIR'], **{k: v for k, v in env.items() if k.startswith('CLAUDE_CODE_PLUGIN_')}},
        'deadlineExceeded': False, 'timingRows': 0, 'positiveTimingModuleExecuted': False}
    try:
        with owned_process('responder', [freeze['ids']['pythonExecutable'], str(FIXTURE), str(fixture)],
                           freeze['ids']['pythonExecutable'], record['cleanup'], cwd=fixture,
                           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as (responder, identity):
            record['processes']['responder'] = identity
            until = time.monotonic() + 3
            while not (fixture / 's.sock').exists() and responder.poll() is None and time.monotonic() < until: time.sleep(.01)
            if not (fixture / 's.sock').exists() or (fixture / 's.sock').stat().st_mode & 0o777 != 0o600:
                raise RuntimeError('owned readiness bind failed')
            with (owned / 'provider-stdout.log').open('xb') as out, (owned / 'provider-stderr.log').open('xb') as err:
                os.chmod(owned / 'provider-stdout.log', 0o600); os.chmod(owned / 'provider-stderr.log', 0o600)
                deadline = time.monotonic() + DEADLINE
                with owned_process('provider', freeze['argv'], PROVIDER, record['cleanup'], PROVIDER_VERSION,
                                   cwd=owned, env=env, stdin=subprocess.PIPE, stdout=out, stderr=err, start_new_session=True) as (child, identity):
                    record['processes']['provider'] = identity
                    child.stdin.write((json.dumps({'type': 'control_request', 'request_id': freeze['initializeRequestId'], 'request': {'subtype': 'initialize'}}) + '\n').encode()); child.stdin.flush()
                    while time.monotonic() < deadline:
                        complete = False
                        for label in ('result', 'error'):
                            try:
                                value = json.loads((owned / (label + '.json')).read_bytes())
                                complete |= isinstance(value, dict) and value.get('runId') == freeze['ids']['runId'] and bool(value.get('stage'))
                            except (OSError, ValueError): pass
                        if complete or child.poll() is not None: break
                        time.sleep(.02)
                    else: record['deadlineExceeded'] = True
    except Exception as error: record['controllerError'] = type(error).__name__ + ': ' + str(error)
    record['socketRemoved'] = not (fixture / 's.sock').exists()
    transcript = fixture / 'readiness.jsonl'
    try: record['transcript'] = [json.loads(line) for line in transcript.read_text().splitlines()] if transcript.exists() else []
    except ValueError: record['transcript'] = []; record['controllerError'] = 'malformed native transcript'
    stdout = owned / 'provider-stdout.log'
    for line in stdout.read_bytes().splitlines() if stdout.exists() else []:
        try: response = json.loads(line)
        except ValueError: continue
        if response.get('type') == 'control_response' and response.get('response', {}).get('request_id') == freeze['initializeRequestId']:
            if 'initializer' in record: record['controllerError'] = 'duplicate initialize response'
            record['initializer'] = response
    debug = owned / 'provider-debug.log'; diagnostic = debug.read_text() if debug.exists() else ''
    loaded = re.findall(r'hooks module ([^\n]*?) loaded \(([^\n]*?)\); events: ([^\n]*)', diagnostic)
    names = set(); counts = {}
    for label, metadata, events in loaded:
        matches = [e['name'] for e in freeze['smokeEntries'] if e['name'] in label or e['root'] in label]
        if len(matches) == 1 and 'session.start' in events:
            names.add(matches[0]); counts[matches[0]] = counts.get(matches[0], 0) + 1
        elif 'tier builtin' not in metadata: composition['unexpectedContributors'].append('unresolved-module-label')
    composition['runtimeLoadedNames'] = [e['name'] for e in freeze['smokeEntries'] if e['name'] in names]
    composition['loadedModuleCounts'] = counts
    if 'failed to load' in diagnostic or 'refused' in diagnostic: record['controllerError'] = 'loader refusal diagnostic'
    artifacts, outputs = read_outputs(owned)
    record['failureBoundary'] = classify(outputs, diagnostic)
    record['readback'] = [{'label': a['label'], 'sha256': a['sha256'], 'bytes': len(a['raw'])} for a in artifacts]
    verdict = adjudicate(freeze, artifacts, record)
    save(root / 'activation-execution.json', record); save(root / 'readiness-verdict.json', verdict)
    return record, verdict
