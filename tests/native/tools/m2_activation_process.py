"""Exact-image direct-child ownership and retained wait authority for readiness."""
from __future__ import annotations
from contextlib import contextmanager
import ctypes
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import time

RESPONDER = Path('/opt/homebrew/Cellar/python@3.14/3.14.8/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python')
RESPONDER_SHA = 'b80d260a48404da13a32dd44d7283aadc80f6a6e56ed6e03ed5597ec6f46d48d'
PROVIDER = Path('/Users/danny/.local/share/claude/versions/2.1.295')
PROVIDER_SHA = '0116ee2e0a513900b633d9951367f18747686478e2b462805b8c31609f047f70'


def image_file(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    sha = hashlib.sha256(path.read_bytes()).hexdigest()
    after = path.stat()
    keys = ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns', 'st_uid', 'st_mode')
    if any(getattr(before, key) != getattr(after, key) for key in keys): raise RuntimeError('image changed during hash')
    return {'path': str(path), 'sha256': sha, **{key: getattr(after, key) for key in keys}}


def approved_image(path):
    actual = image_file(path)
    expected = {str(RESPONDER): RESPONDER_SHA, str(PROVIDER): PROVIDER_SHA}
    if actual['path'] not in expected or actual['sha256'] != expected[actual['path']] or not actual['st_mode'] & 0o111:
        raise RuntimeError('unapproved executable')
    return actual


def kernel_snapshot(pid):
    """Bracket kernel image with two matching independent BSD process samples."""
    lib = ctypes.CDLL('/usr/lib/libproc.dylib')
    def info():
        data = ctypes.create_string_buffer(136)
        if lib.proc_pidinfo(pid, 3, 0, data, 136) != 136: raise RuntimeError('kernel birth unavailable')
        raw = data.raw
        return {'pid': int.from_bytes(raw[12:16], 'little'), 'uid': int.from_bytes(raw[20:24], 'little'),
            'parentPid': int.from_bytes(raw[16:20], 'little'),
            'startUsec': int.from_bytes(raw[120:128], 'little') * 1000000 + int.from_bytes(raw[128:136], 'little')}
    first = info(); buf = ctypes.create_string_buffer(4096)
    if lib.proc_pidpath(pid, buf, len(buf)) <= 0: raise RuntimeError('kernel image unavailable')
    second = info()
    if first != second: raise RuntimeError('kernel incarnation changed during sample')
    return {**first, 'imagePath': str(Path(os.fsdecode(buf.value)).resolve(strict=True))}


def observe(child, policy, receipt, phase, baseline=None):
    event = {'phase': phase}; receipt.setdefault('observations', []).append(event)
    try:
        current = kernel_snapshot(child.pid); event['kernel'] = current
        if (current.get('pid') != child.pid or current.get('uid') != os.getuid() or current.get('parentPid') != os.getpid()
                or type(current.get('startUsec')) is not int or current['startUsec'] <= 0
                or current.get('imagePath') != policy['path']): raise RuntimeError('kernel ownership/image mismatch')
        if baseline is not None and current != baseline: raise RuntimeError('changed process incarnation')
        event['admitted'] = True
        return current
    except Exception as error:
        event['error'] = type(error).__name__ + ': ' + str(error)
        raise


def check_file(policy, receipt, phase):
    event = {'phase': phase}; receipt.setdefault('observations', []).append(event)
    try:
        actual = approved_image(policy['path']); event['file'] = actual
        if actual != policy: raise RuntimeError('frozen executable file substituted')
        event['admitted'] = True
    except Exception as error:
        event['error'] = type(error).__name__ + ': ' + str(error); raise


def wait_exact(child, receipt, timeout=0):
    """Only this child's waitpid result, never PID absence or bare ECHILD."""
    prior = receipt.get('wait')
    if prior:
        if prior['pid'] != child.pid: raise RuntimeError('foreign retained wait')
        return prior
    until = time.monotonic() + timeout
    while True:
        try: pid, status = os.waitpid(child.pid, os.WNOHANG)
        except InterruptedError: continue
        except ChildProcessError as error:
            receipt['waitError'] = 'ECHILD without retained exact-child wait'
            raise RuntimeError(receipt['waitError']) from error
        if pid:
            if pid != child.pid or not (os.WIFEXITED(status) or os.WIFSIGNALED(status)):
                raise RuntimeError('invalid exact-child terminal wait')
            code = os.waitstatus_to_exitcode(status); child.returncode = code
            receipt['wait'] = {'pid': pid, 'waitStatus': status, 'exitCode': code, 'reaped': True}
            receipt.setdefault('observations', []).append({'phase': 'terminal-wait', **receipt['wait']})
            return receipt['wait']
        if time.monotonic() >= until: return None
        time.sleep(.01)


def terminate_owned(child, baseline, policy, receipt):
    if wait_exact(child, receipt): return
    if baseline is None: raise RuntimeError('no authoritative launch baseline; signal refused')
    for signum, name in ((signal.SIGTERM, 'SIGTERM'), (signal.SIGKILL, 'SIGKILL')):
        try:
            observe(child, policy, receipt, 'pre-' + name, baseline)
            check_file(policy, receipt, 'file-pre-' + name)
            if wait_exact(child, receipt): return
            observe(child, policy, receipt, 'final-pre-' + name, baseline)
        except Exception:
            if wait_exact(child, receipt): return
            raise
        if wait_exact(child, receipt): return
        try: os.kill(child.pid, signum)
        except ProcessLookupError:
            if wait_exact(child, receipt, 1): return
            raise RuntimeError('signal ESRCH without terminal wait')
        receipt.setdefault('signals', []).append(name)
        receipt.setdefault('observations', []).append({'phase': 'signal', 'signal': name, 'pid': child.pid})
        if wait_exact(child, receipt, 3): return
    raise RuntimeError('owned child did not reach terminal wait')


@contextmanager
def owned_process(role, argv, executable, records, version=None, image_policy=None, **kwargs):
    child = None; baseline = None
    policy = approved_image(executable) if image_policy is None else image_policy
    receipt = {'role': role, 'spawned': False, 'reaped': False, 'approvedImage': policy, 'observations': [], 'signals': []}
    records.append(receipt)
    # Never learn an approved file identity from child-supplied labels.
    check_file(policy, receipt, 'file-before-spawn')
    try:
        child = subprocess.Popen(argv, **kwargs); receipt['spawned'] = True
        receipt['childPid'] = child.pid
        baseline = observe(child, policy, receipt, 'launch-baseline')
        receipt['baseline'] = baseline
        check_file(policy, receipt, 'full-image')
        identity = {**baseline, 'sha256': policy['sha256'], 'kernelVerified': True}
        if version: identity['version'] = version
        receipt['identity'] = identity
        yield child, identity
    except Exception as error:
        receipt['executionError'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if child is not None:
            try:
                terminate_owned(child, baseline, policy, receipt)
                receipt.update({'reaped': True, 'exitCode': receipt['wait']['exitCode'],
                    'events': ['verified-child-' + s for s in receipt['signals']] or ['already-exited']})
                if child.stdin is not None: child.stdin.close()
            except Exception as error:
                receipt['error'] = type(error).__name__ + ': ' + str(error)
                receipt['reaped'] = bool(receipt.get('wait'))
        if receipt.get('error'): raise RuntimeError('owned cleanup incomplete: ' + receipt['error'])


def verify_cleanup(receipt, identity, orderly=False):
    """Check retained observations and wait, independent of producer reaped flags."""
    if receipt.get('error') or receipt.get('executionError') or not identity: return False
    baseline = receipt.get('baseline'); policy = receipt.get('approvedImage', {})
    if not baseline or {k: identity.get(k) for k in baseline} != baseline: return False
    if policy.get('path') != identity.get('imagePath') or policy.get('sha256') != identity.get('sha256'): return False
    observations = receipt.get('observations', []); phases = [o.get('phase') for o in observations]
    if phases[:3] != ['file-before-spawn', 'launch-baseline', 'full-image'] or phases[-1:] != ['terminal-wait']: return False
    for o in observations:
        if 'error' in o or 'kernel' in o and (o['kernel'] != baseline or o.get('admitted') is not True): return False
        if 'file' in o and (o['file'] != policy or o.get('admitted') is not True): return False
    signals = receipt.get('signals', [])
    if signals not in ([], ['SIGTERM'], ['SIGTERM', 'SIGKILL']): return False
    expected = ['file-before-spawn', 'launch-baseline', 'full-image']
    if 'fixture-ready' in phases: expected.append('fixture-ready')
    for name in signals: expected += ['pre-' + name, 'file-pre-' + name, 'final-pre-' + name, 'signal']
    expected.append('terminal-wait')
    # A proven exit can occur during pre-signal checks. A partial next check
    # sequence is valid only when it ends in this child's successful wait.
    if phases != expected:
        next_signal = 'SIGTERM' if not signals else 'SIGKILL'
        partial = ['pre-' + next_signal, 'file-pre-' + next_signal, 'final-pre-' + next_signal]
        if not any(phases == expected[:-1] + partial[:n] + ['terminal-wait'] for n in (1, 2, 3)): return False
    if [o.get('signal') for o in observations if o.get('phase') == 'signal'] != signals: return False
    wait = receipt.get('wait', {})
    if wait != {k: observations[-1].get(k) for k in ('pid', 'waitStatus', 'exitCode', 'reaped')}: return False
    if wait.get('pid') != identity.get('pid') or wait.get('reaped') is not True or receipt.get('reaped') is not True: return False
    try:
        if os.waitstatus_to_exitcode(wait['waitStatus']) != wait['exitCode'] or receipt['exitCode'] != wait['exitCode']: return False
    except (KeyError, ValueError): return False
    if orderly and (signals not in ([], ['SIGTERM']) or wait['exitCode'] != 0): return False
    return True
