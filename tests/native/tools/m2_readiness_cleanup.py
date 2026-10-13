"""Predeclared four-case, provider-free qualification of exact-image cleanup."""
from __future__ import annotations
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import uuid
from m2_activation_process import RESPONDER, owned_process, observe, wait_exact, verify_cleanup
from m2_activation_readiness import acknowledgement
from m2_socket_duration import canonical_hash, file_hash

CASES = {'one-exchange': [200], 'no-request': [], 'partial-header': [], 'four-request': [403, 200, 409, 403]}


def prepare(root):
    from m2_activation_native import save, source_binding
    root = Path(root); root.mkdir(mode=0o700)
    binding = source_binding()
    boot = subprocess.check_output(['/usr/sbin/sysctl', '-n', 'kern.bootsessionuuid'], text=True).strip().lower()
    cases = []
    for name, statuses in CASES.items():
        directory = root / name; directory.mkdir(mode=0o700)
        ids = {**binding, 'runId': str(uuid.uuid4()), 'responderId': str(uuid.uuid4()), 'nonce': uuid.uuid4().hex + uuid.uuid4().hex, 'bootId': boot}
        save(directory / 'readiness-config.json', ids)
        cases.append({'name': name, 'ids': ids, 'statuses': statuses, 'root': str(directory), 'deadlineSeconds': 20})
    freeze = {'kind': 'DIRECT_IMAGE_CLEANUP_QUALIFICATION_V1', 'sources': binding, 'cases': cases,
              'order': list(CASES), 'eachCaseAttempts': 1, 'providerLaunches': 0}
    save(root / 'suite-freeze.json', freeze)
    return freeze


def exchange(path, route):
    connection = http.client.HTTPConnection('localhost', timeout=2)
    connection.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.sock.settimeout(2); connection.sock.connect(str(path))
    try:
        connection.request('GET', route); response = connection.getresponse()
        return {'status': response.status, 'body': json.loads(response.read())}
    finally: connection.close()


def assess(case, result):
    failures = []
    cleanup = result.get('cleanup', [])
    if len(cleanup) != 1 or not verify_cleanup(cleanup[0], result.get('process'), orderly=True): failures.append('owned orderly cleanup not proven')
    if result.get('error') or result.get('deadlineExceeded') or not result.get('socketRemoved'): failures.append('case containment failed')
    rows = result.get('transcript', []); events = [r.get('event') for r in rows]
    name = case['name']; ack = acknowledgement(case['ids'])
    expected_events = ['start'] + (['received', 'generated', 'answered'] * len(case['statuses'])) + ['stop']
    if name == 'partial-header': expected_events = ['start', 'incomplete', 'stop']
    if events != expected_events: failures.append('missing/reordered transcript')
    stop = rows[-1] if rows else {}
    expected_counts = {'one-exchange': [1, 1, 1, 1], 'no-request': [0, 0, 0, 0], 'partial-header': [1, 0, 0, 0], 'four-request': [4, 4, 4, 1]}[name]
    if [stop.get(k) for k in ('attempts', 'generated', 'answered', 'accepted')] != expected_counts: failures.append('incorrect complete counters')
    if not rows or rows[0].get('identity') != ack or rows[0].get('pid') != result.get('process', {}).get('pid'): failures.append('foreign transcript source')
    if [r.get('status') for r in result.get('responses', [])] != case['statuses']: failures.append('wrong responses')
    for event in ('received', 'generated', 'answered'):
        matched = [r for r in rows if r.get('event') == event]
        if [r.get('status') for r in matched] != case['statuses']: failures.append('wrong independent ' + event + ' accounting')
        if [r.get('attempt') for r in matched] != list(range(1, len(case['statuses']) + 1)): failures.append('replayed/reordered ' + event)
    for response in result.get('responses', []):
        if response['status'] == 200 and response['body'] != ack: failures.append('foreign acknowledgement')
    if name == 'four-request' and cleanup and cleanup[0].get('signals') != []: failures.append('signaled natural exit')
    if name != 'four-request' and cleanup and cleanup[0].get('signals') != ['SIGTERM']: failures.append('live cleanup not exercised')
    return {'verdict': 'FAIL' if failures else 'PASS', 'failures': failures}


def execute(root):
    from m2_activation_native import save, source_binding, FIXTURE
    root = Path(root); freeze = json.loads((root / 'suite-freeze.json').read_bytes())
    if source_binding() != freeze['sources'] or freeze['order'] != list(CASES): raise RuntimeError('changed predeclared suite')
    save(root / 'suite-started.json', {'freezeSha256': canonical_hash(freeze), 'attempt': 1})
    results = []
    for case in freeze['cases']:
        directory = Path(case['root']); path = directory / 's.sock'
        result = {'name': case['name'], 'cleanup': [], 'responses': [], 'deadlineExceeded': False}
        deadline = time.monotonic() + case['deadlineSeconds']
        try:
            with owned_process('responder', [str(RESPONDER), str(FIXTURE), str(directory)], RESPONDER, result['cleanup'],
                               image_policy=freeze['sources']['responderImagePolicy'], cwd=directory,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as (child, identity):
                result['process'] = identity; receipt = result['cleanup'][0]
                while not path.exists() and not wait_exact(child, receipt) and time.monotonic() < deadline: time.sleep(.01)
                if not path.exists() or path.stat().st_uid != os.getuid() or path.stat().st_mode & 0o777 != 0o600: raise RuntimeError('private socket bind failed')
                observe(child, receipt['approvedImage'], receipt, 'fixture-ready', receipt['baseline'])
                ids = case['ids']; route = '/' + '/'.join(('ready', ids['runId'], ids['responderId'], ids['nonce'], ids['activationSha256']))
                if case['name'] == 'one-exchange': result['responses'].append(exchange(path, route))
                if case['name'] == 'four-request':
                    for request in (route.replace(ids['nonce'], 'forged'), route, route, route.replace(ids['responderId'], 'foreign')):
                        result['responses'].append(exchange(path, request))
                    if not wait_exact(child, receipt, max(0, deadline - time.monotonic())): raise RuntimeError('natural exit deadline exceeded')
                if case['name'] == 'partial-header':
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                        connection.settimeout(2); connection.connect(str(path)); connection.sendall(b'GET /incomplete HTTP/1.1\r\n')
                        # Fixture has its original one-second header timeout.
                        if connection.recv(1) != b'': raise RuntimeError('partial request unexpectedly answered')
                if time.monotonic() >= deadline: result['deadlineExceeded'] = True
        except Exception as error: result['error'] = type(error).__name__ + ': ' + str(error)
        result['socketRemoved'] = not path.exists()
        transcript = directory / 'readiness.jsonl'
        result['transcript'] = [json.loads(line) for line in transcript.read_text().splitlines()] if transcript.exists() else []
        result['transcriptSha256'] = file_hash(transcript) if transcript.exists() else None
        result['assessment'] = assess(case, result)
        save(directory / 'result.json', result); results.append(result)
        if result['assessment']['verdict'] != 'PASS': break
    qualification = {'kind': freeze['kind'], 'freezeSha256': canonical_hash(freeze), 'sources': freeze['sources'],
                     'cases': results, 'providerLaunches': 0, 'verdict': 'PASS' if len(results) == 4 and all(r['assessment']['verdict'] == 'PASS' for r in results) else 'FAIL'}
    save(root / 'qualification.json', qualification)
    return qualification


def verify_qualification(root):
    from m2_activation_native import source_binding
    root = Path(root); freeze = json.loads((root / 'suite-freeze.json').read_bytes()); record = json.loads((root / 'qualification.json').read_bytes())
    if (record['freezeSha256'] != canonical_hash(freeze) or record['sources'] != source_binding() or freeze['sources'] != record['sources']
            or [r['name'] for r in record['cases']] != list(CASES) or record['providerLaunches'] != 0): raise RuntimeError('changed cleanup suite/source binding')
    for case, result in zip(freeze['cases'], record['cases']):
        path = Path(case['root']) / 'readiness.jsonl'
        if file_hash(path) != result['transcriptSha256'] or [json.loads(line) for line in path.read_text().splitlines()] != result['transcript']:
            raise RuntimeError('changed original cleanup transcript')
        if assess(case, result)['verdict'] != 'PASS' or (Path(case['root']) / 's.sock').exists(): raise RuntimeError('cleanup qualification incomplete')
    return record
