"""Archived initialization-only controller; never performs timing conformance.

Run from the repository only after independently creating fresh disposable
gate-plugin/provider-config inputs and reserving the unit with an exclusive
.git/replacement-duration-instrument.lock containing its privateRoot. Do not
reuse the completed run's inputs or interpret this as a positive-run runner.
The probe and owned input setup are described in README.md.
"""
import os, subprocess, json, time, hashlib, ctypes
from pathlib import Path

root = Path(json.loads(Path('.git/replacement-duration-instrument.lock').read_text())['privateRoot'])
provider = Path('/Users/danny/.local/share/claude/versions/2.1.296')
env = os.environ.copy()
env['CLAUDE_CONFIG_DIR'] = str(root / 'provider-config')
env['CLAUDE_CODE_PLUGIN_DIRS'] = str(root / 'gate-plugin')
env['DISABLE_AUTOUPDATER'] = '1'
for key in list(env):
    if any(fragment in key for fragment in ['API_KEY', 'AUTH_TOKEN', 'OAUTH_TOKEN']):
        env.pop(key)
argv = [str(provider), '--print', '--input-format', 'stream-json', '--output-format',
        'stream-json', '--verbose', '--no-session-persistence', '--plugin-dir',
        str(root / 'gate-plugin'), '--settings', str(root / 'provider-config/settings.json'),
        '--setting-sources', '', '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}']
lib = ctypes.CDLL('/usr/lib/libproc.dylib')
lib.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
lib.proc_pidpath.restype = ctypes.c_int
lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                            ctypes.c_void_p, ctypes.c_int]
lib.proc_pidinfo.restype = ctypes.c_int


def identity(pid):
    path = ctypes.create_string_buffer(4096)
    n = lib.proc_pidpath(pid, path, 4096)
    info = ctypes.create_string_buffer(136)
    m = lib.proc_pidinfo(pid, 3, 0, info, 136)
    if n <= 0 or m != 136:
        raise RuntimeError('missing independent native identity')
    import struct
    uid = struct.unpack_from('I', info.raw, 20)[0]
    ppid = struct.unpack_from('I', info.raw, 16)[0]
    birth = struct.unpack_from('QQ', info.raw, 120)
    return {'pid': pid, 'ppid': ppid, 'uid': uid, 'birth': birth,
            'executable': path.value.decode()}


out = (root / 'gate-provider-stdout.log').open('wb')
err = (root / 'gate-provider-stderr.log').open('wb')
child = None
owned = None
actions = []
complete = False
try:
    child = subprocess.Popen(argv, cwd=root, env=env, stdin=subprocess.PIPE,
                             stdout=out, stderr=err, start_new_session=True)
    for attempt in range(30):
        try:
            candidate = identity(child.pid)
            if (candidate['executable'] == str(provider)
                    and candidate['ppid'] == os.getpid()
                    and candidate['uid'] == os.getuid()):
                owned = candidate
                break
        except RuntimeError:
            pass
        time.sleep(.01)
    if owned is None:
        raise RuntimeError('provider ownership not proven')
    child.stdin.write((json.dumps({'type': 'control_request',
                                  'request_id': 'runtime-gate-initialize',
                                  'request': {'subtype': 'initialize'}}) + '\n').encode())
    child.stdin.flush()
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline and child.poll() is None:
        if (root / 'gate-host.json').exists():
            complete = True
            break
        time.sleep(.05)
finally:
    if child and child.poll() is None:
        child.stdin.close()
        actions.append('close owned stdin')
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            if owned is None or identity(child.pid) != owned:
                raise RuntimeError('refuse to signal unverified process')
            child.terminate()
            actions.append('SIGTERM after native identity revalidation')
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                if identity(child.pid) != owned:
                    raise RuntimeError('refuse to kill changed process')
                child.kill()
                actions.append('SIGKILL after native identity revalidation')
                child.wait(timeout=3)
    out.close()
    err.close()
record = {'initializeOnly': True, 'inferenceRequested': False, 'calibrationRun': False,
          'externalDeadlineSeconds': 20, 'ownedProcess': owned, 'complete': complete,
          'exitCode': child.returncode if child else None, 'cleanup': actions,
          'providerReaped': child is not None and child.poll() is not None,
          'stdoutSha256': hashlib.sha256((root / 'gate-provider-stdout.log').read_bytes()).hexdigest(),
          'stderrSha256': hashlib.sha256((root / 'gate-provider-stderr.log').read_bytes()).hexdigest()}
(root / 'gate-controller.json').write_text(json.dumps(record, indent=2) + '\n')
