#!/usr/bin/env python3
"""Disposable, PID-addressed Ghostty capability qualification, never a session route PASS."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import plistlib
import stat
import subprocess
import time

from m2_activation_process import kernel_snapshot

HERE = Path(__file__).resolve().parent


def command(argv, timeout=10):
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"{Path(argv[0]).name} failed: {result.stderr[:600]}")
    return result.stdout


def processes(executable):
    matches = []
    for line in command(['/bin/ps', '-axo', 'pid=,comm=']).splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) == 2 and fields[1] == str(executable):
            current = kernel_snapshot(int(fields[0]))
            if current['imagePath'] == str(executable) and current['uid'] == os.getuid():
                matches.append(current)
    return matches


def call(identity, operation, *arguments):
    if kernel_snapshot(identity['pid']) != identity:
        raise RuntimeError('Ghostty process incarnation changed')
    output = command(['/usr/bin/osascript', '-l', 'JavaScript', str(HERE / 'ghostty_fixture.js'),
                      str(identity['pid']), operation, *arguments])
    if operation != 'close' and kernel_snapshot(identity['pid']) != identity:
        raise RuntimeError('Ghostty process changed during operation')
    return json.loads(output)


def terminals(inventory):
    return [{**s, 'windowId': w['id'], 'tabId': t['id']}
            for w in inventory['windows'] for t in w['tabs'] for s in t['terminals']]


def device_proof(terminal, application):
    # Ghostty's PID may name its root-owned login wrapper. It is diagnostic
    # only: the join uses the actual known fixture process and native TTY.
    tty = os.stat(terminal['tty'])
    if not stat.S_ISCHR(tty.st_mode):
        raise RuntimeError('native tty is not a character device')
    rows = {}
    for line in command(['/bin/ps', '-axo', 'pid=,ppid=,comm=']).splitlines():
        fields = line.strip().split(None, 2)
        if len(fields) == 3:
            rows[int(fields[0])] = (int(fields[1]), fields[2])
    def owned_descendant(pid):
        seen = set()
        while pid in rows and pid not in seen:
            if pid == application['pid']:
                return True
            seen.add(pid)
            pid = rows[pid][0]
        return False
    matches = []
    lib = ctypes.CDLL('/usr/lib/libproc.dylib')
    for pid, (_, image) in rows.items():
        if image.lstrip('-') not in ('/bin/sleep', '/usr/bin/sleep') or not owned_descendant(pid):
            continue
        before = kernel_snapshot(pid)
        if before['imagePath'] not in ('/bin/sleep', '/usr/bin/sleep') or before['uid'] != os.getuid():
            continue
        # Darwin proc_bsdinfo: verified structure length, e_tdev at offset 108.
        info = ctypes.create_string_buffer(136)
        if lib.proc_pidinfo(pid, 3, 0, info, len(info)) != len(info):
            raise RuntimeError('short BSD process sample')
        device = int.from_bytes(info.raw[108:112], 'little')
        if kernel_snapshot(pid) != before:
            raise RuntimeError('fixture child changed during tty proof')
        if device == tty.st_rdev:
            matches.append(before)
    if len(matches) != 1 or kernel_snapshot(application['pid']) != application:
        census = {pid: value for pid, value in rows.items() if owned_descendant(pid)}
        raise RuntimeError(f'expected one owned fixture process on native tty, found {len(matches)}; descendants={census}')
    return {'terminalId': terminal['id'], 'tty': terminal['tty'], 'rdev': tty.st_rdev,
            'eTdev': tty.st_rdev, 'process': matches[0], 'ghosttyReportedPid': terminal['pid']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True, help='new private evidence directory')
    args = parser.parse_args()
    app = args.app.resolve(strict=True)
    executable = app / 'Contents/MacOS/ghostty'
    if processes(executable):
        raise RuntimeError('candidate already running; no existing application is adopted')
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = {'scope': 'Ghostty native capability; provider identity and Threadspace Return NOT_RUN',
              'state': 'FAIL', 'checks': {}, 'app': str(app)}
    identity = None
    owned = None
    preserved = []
    try:
        command(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(app)], 30)
        command(['/usr/sbin/spctl', '--assess', '--type', 'execute', str(app)], 30)
        info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
        report.update(version=info['CFBundleShortVersionString'], build=info['CFBundleVersion'],
                      binarySha256=hashlib.sha256(executable.read_bytes()).hexdigest(),
                      dictionarySha256=hashlib.sha256((app / 'Contents/Resources/Ghostty.sdef').read_bytes()).hexdigest())
        report['checks']['signatureAndNotarization'] = 'PASS'
        # Capture other Ghostty processes by kernel image, never choose one by
        # bundle ID or foreground status. Preserve their exact terminal ID sets.
        for line in command(['/bin/ps', '-axo', 'pid=,comm=']).splitlines():
            fields = line.strip().split(None, 1)
            if len(fields) == 2 and fields[1].endswith('/Ghostty.app/Contents/MacOS/ghostty'):
                original = kernel_snapshot(int(fields[0]))
                preserved.append({'identity': original, 'inventory': call(original, 'ids')})
        report['preserved'] = preserved
        command(['/usr/bin/open', '-n', '-g', '-a', str(app), '--args',
                 '--config-default-files=false', '--window-save-state=never', '--initial-window=false',
                 '--auto-update=off', '--quit-after-last-window-closed=false',
                 '--confirm-close-surface=false', '--shell-integration=none'])
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            found = processes(executable)
            if len(found) > 1:
                raise RuntimeError('ambiguous candidate launch')
            if found:
                identity = found[0]
                break
            time.sleep(.05)
        if identity is None:
            raise RuntimeError('candidate launch missing')
        report['identity'] = identity
        initial = call(identity, 'inventory')
        if initial['windows']:
            raise RuntimeError('candidate restored unexpected windows; no cleanup authority')
        owned = []  # Exact new empty application is now acquired.
        try:
            inventory = call(identity, 'create')
        finally:
            # A partially completed creation still belongs to this acquired
            # empty process. Record every resulting native ID before cleanup.
            inventory = call(identity, 'inventory')
            owned = [s['id'] for s in terminals(inventory)]
        report['inventory'] = inventory
        surfaces = terminals(inventory)
        if len(inventory['windows']) != 2 or len(surfaces) != 4 or len(set(owned)) != 4:
            raise RuntimeError('fixture needs two windows, three tabs, four distinct terminals')
        # A new surface can expose its short-lived login launcher before the
        # configured command owns the terminal. Re-enumerate during startup;
        # never reuse a stale PID or treat an unreadable process as proof.
        deadline = time.monotonic() + 5
        report['startupObservations'] = []
        while True:
            try:
                report['devices'] = [device_proof(s, identity) for s in surfaces]
                break
            except RuntimeError as error:
                report['startupObservations'].append({'terminals': surfaces, 'error': str(error)})
                if time.monotonic() >= deadline:
                    raise
                time.sleep(.1)
                inventory = call(identity, 'inventory')
                surfaces = terminals(inventory)
                if sorted(s['id'] for s in surfaces) != sorted(owned):
                    raise RuntimeError('fixture surfaces changed during startup')
                report['inventory'] = inventory
        report['checks']['nativeTTYToKernelDevice'] = 'PASS'
        results = []
        report['focus'] = results
        for target in surfaces + list(reversed(surfaces)):
            start = time.monotonic()
            readback = call(identity, 'focus', target['id'])
            elapsed = (time.monotonic() - start) * 1000
            results.append({'target': target, 'readback': readback, 'elapsedMs': elapsed})
            if (not readback.get('frontmost') or not readback.get('tabSelected')
                    or any(readback.get(k) != target[k] for k in ['windowId', 'tabId'])
                    or readback.get('terminalId') != target['id']):
                raise RuntimeError('native focus readback mismatch')
        report['checks']['exactFocusReadback'] = 'PASS'
        absent = call(identity, 'focus', '00000000-0000-0000-0000-000000000000')
        if absent != {'focused': False, 'matches': 0}:
            raise RuntimeError('absent native ID was not refused')
        report['checks']['absentTarget'] = 'PASS'
        report['state'] = 'PASS'
    except Exception as error:
        report['error'] = f'{type(error).__name__}: {error}'
    finally:
        if identity is not None and owned is not None:
            try:
                report['cleanup'] = call(identity, 'close', json.dumps(owned))
                deadline = time.monotonic() + 5
                while processes(executable) and time.monotonic() < deadline:
                    time.sleep(.05)
                if processes(executable):
                    raise RuntimeError('candidate process still running')
                report['checks']['ownedCleanup'] = 'PASS'
            except Exception as error:
                report['state'] = 'FAIL'
                report['cleanupError'] = str(error)
        try:
            for original in preserved:
                if call(original['identity'], 'ids') != original['inventory']:
                    raise RuntimeError('existing Ghostty terminal IDs changed')
            report['checks']['existingSessionsPreserved'] = 'PASS'
        except Exception as error:
            report['state'] = 'FAIL'
            report['preservationError'] = str(error)
        (args.output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'state': report['state'], 'checks': report['checks'],
                      'error': report.get('error'), 'result': str(args.output / 'result.json')}, indent=2))
    return 0 if report['state'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
