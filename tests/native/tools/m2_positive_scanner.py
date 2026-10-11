#!/usr/bin/env python3
"""Freeze one source candidate and scan packages without callback execution."""
from __future__ import annotations
import json
import os
import subprocess
import time
from pathlib import Path
from m2_socket_duration import HERE, source_identities, file_hash, canonical_hash, ADAPTER_SHA
from m2_socket_plugins import composition_identity, verify_plugins, json_bytes, write_private
from m2_socket_runner import PROVIDER, PROVIDER_SHA, PROVIDER_VERSION, prepare_provider, image_identity, cleanup
from m2_activation_native import stage, verify_staging, environment

BASE = '445794e778e27ed071f8e053aadce8b401c64ba4'
DEADLINE = 20
DEPENDENCIES = ('m2_positive_scanner.py', 'm2_positive_scanner_controls.js', 'test_m2_positive_scanner.py',
    'm2_socket_probe.ts', 'm2_socket_background.ts', 'm2_socket_plugins.py', 'm2_socket_runner.py',
    'm2_socket_duration.py', 'm2_composed_duration.py', 'm2_socket_policy.json',
    'm2_activation_probe.ts', 'm2_activation_native.py', 'm2_activation_readiness.py')


def source_binding():
    source = (HERE / 'm2_socket_probe.ts').read_text()
    local = source[source.index('const startBackground ='):source.index('\n\nexport const register')]
    return {'conformanceSourceIdentities': source_identities(),
        'sources': {name: file_hash(HERE / name) for name in DEPENDENCIES},
        'localBackgroundSha256': canonical_hash({'source': local}),
        'localBackgroundHashEncoding': 'canonical JSON object containing exact declaration bytes',
        'providerPath': str(PROVIDER.resolve(strict=True)), 'providerSha256': file_hash(PROVIDER),
        'providerVersion': PROVIDER_VERSION}


def save(path, value): write_private(path, json_bytes(value))


def prepare(root):
    root = Path(root).resolve(strict=True)
    if root.stat().st_uid != os.getuid() or root.stat().st_mode & 0o777 != 0o700:
        raise RuntimeError('scanner root is not private and owned')
    binding = source_binding()
    if binding['providerSha256'] != PROVIDER_SHA or binding['conformanceSourceIdentities']['adapterSha256'] != ADAPTER_SHA:
        raise RuntimeError('required provider or canonical observer changed')
    # The stager emits config object key order verbatim; match frozen JSON readback.
    config = json.loads(json_bytes({'kind': 'NON_EXECUTING_SCANNER_INPUT_ONLY', 'plan': [], 'delays': [], 'runtimeId': 'not-a-runtime'}))
    positive = root / 'positive'; positive.mkdir(mode=0o700)
    prepared = prepare_provider(positive, config, composition_identity())
    entry = {**prepared['plugins']['plugins'][1], 'role': 'positive'}
    activation = stage(root / 'activation', 'activation', config)
    environment(root)
    freeze = {'kind': 'SINGLE_CANDIDATE_NON_EXECUTING_SCANNER_INPUTS', 'baseSha': BASE,
        'sourceBinding': binding, 'config': config, 'positiveStaging': prepared['plugins'],
        'entries': {'positive': entry, 'activation': activation}, 'deadlineSeconds': DEADLINE,
        'settingsSha256': file_hash(root / 'config/settings.json'),
        'callbacksExecuted': False, 'positiveActivationAttempts': 0, 'timingAttempts': 0}
    save(root / 'scanner-input-freeze.json', freeze)
    verify(root, freeze)
    return freeze


def verify(root, freeze):
    root = Path(root)
    if root.is_symlink() or root.stat().st_uid != os.getuid() or root.stat().st_mode & 0o777 != 0o700:
        raise RuntimeError('scanner root ownership changed')
    if (root / 'scanner-input-freeze.json').read_bytes() != json_bytes(freeze) or source_binding() != freeze['sourceBinding']:
        raise RuntimeError('frozen candidate sources changed')
    if freeze['deadlineSeconds'] != DEADLINE or freeze['config'] != {'kind': 'NON_EXECUTING_SCANNER_INPUT_ONLY', 'plan': [], 'delays': [], 'runtimeId': 'not-a-runtime'}:
        raise RuntimeError('changed non-executing scanner configuration')
    if freeze['entries']['positive'] != {**freeze['positiveStaging']['plugins'][1], 'role': 'positive'}:
        raise RuntimeError('scanner entry differs from the source-bound timing plugin')
    verify_plugins(root / 'positive', freeze['config'], composition_identity(), freeze['positiveStaging'])
    verify_staging([freeze['entries']['activation']])
    cfg = root / 'config'; settings = cfg / 'settings.json'
    if cfg.is_symlink() or cfg.stat().st_uid != os.getuid() or cfg.stat().st_mode & 0o777 != 0o700:
        raise RuntimeError('scanner configuration ownership changed')
    if settings.is_symlink() or settings.stat().st_uid != os.getuid() or settings.stat().st_mode & 0o777 != 0o600 or file_hash(settings) != freeze['settingsSha256']:
        raise RuntimeError('scanner settings changed')


def validate(root, freeze, role):
    root = Path(root); verify(root, freeze)
    if role not in ('positive', 'activation'): raise ValueError('unsupported scanner role')
    if role == 'activation':
        preceding = json.loads((root / 'positive-result.json').read_text())
        if preceding['freezeSha256'] != canonical_hash(freeze) or preceding['exitCode'] != 0 or preceding['deadlineExceeded'] or preceding['report'].get('success') is not True:
            raise RuntimeError('positive package admission is a mandatory prerequisite')
    entry = freeze['entries'][role]
    save(root / (role + '-scan-started.json'), {'kind': freeze['kind'], 'freezeSha256': canonical_hash(freeze), 'role': role})
    env = {k: v for k, v in os.environ.items() if not k.startswith('CLAUDE_CODE_PLUGIN_')}
    env['CLAUDE_CONFIG_DIR'] = str(root / 'config')
    argv = [str(PROVIDER), '--settings', str(root / 'config/settings.json'), '--setting-sources', '',
        '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--no-session-persistence',
        'plugin', 'validate', entry['root'], '--json']
    stdout, stderr = root / (role + '-validation.stdout'), root / (role + '-validation.stderr')
    with stdout.open('xb') as out, stderr.open('xb') as err:
        os.chmod(stdout, 0o600); os.chmod(stderr, 0o600)
        deadline = time.monotonic() + DEADLINE
        child = subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=out, stderr=err, start_new_session=True)
        identity = image_identity(child, PROVIDER, PROVIDER_VERSION)
        expired = False
        try: child.wait(timeout=max(0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired: expired = True
        finally: cleaned = cleanup(child, identity)
    try: report = json.loads(stdout.read_bytes())
    except ValueError: report = None
    result = {'kind': 'NON_EXECUTING_INSTALLED_SCANNER', 'freezeSha256': canonical_hash(freeze),
        'sourceBinding': freeze['sourceBinding'], 'argv': argv, 'entry': entry, 'process': identity,
        'deadlineSeconds': DEADLINE, 'deadlineExceeded': expired, 'cleanup': cleaned,
        'stdoutSha256': file_hash(stdout), 'stderrSha256': file_hash(stderr),
        'exitCode': child.returncode, 'report': report, 'callbacksExecuted': False}
    save(root / (role + '-result.json'), result)
    return result
