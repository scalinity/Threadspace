#!/usr/bin/env python3
"""Replay 2.1.318 kit controls on disposable mod/config copies only.

Run from the Git worktree root. Results and full copies remain under its
ignored artifacts directory; no live provider configuration is consulted.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--claude', default='/Users/danny/.local/share/claude/versions/2.1.318')
    args = parser.parse_args()
    root = Path.cwd().resolve()
    source = root / 'packages/provider-mod'
    assert (source / 'hooks/register.ts').is_file()
    base = root / 'artifacts' / ('claude-2.1.318-controls-' + uuid.uuid4().hex[:8])
    base.mkdir(parents=True)
    (base / '.gitignore').write_text('*\n')
    env = os.environ.copy()
    for key, directory in [('CLAUDE_CONFIG_DIR', 'config'), ('TMPDIR', 'tmp'),
                           ('XDG_CACHE_HOME', 'cache'), ('XDG_STATE_HOME', 'state')]:
        path = base / directory
        path.mkdir()
        env[key] = str(path)
    env.update(DISABLE_AUTOUPDATER='1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1')
    original = (source / 'hooks/register.ts').read_text()
    variants = {
        'unchanged': (original, None),
        'no-observer': ('export const register = () => {}\n', None),
        'duplicated-observations': (
            original.replace('  const emit = (', '  const emitOnce = (', 1).replace(
                '  const enter = (',
                '  const emit: typeof emitOnce = (...args) => { emitOnce(...args); emitOnce(...args) }\n\n  const enter = (', 1),
            '(fail) tool call preflight is one nested check in one observer epoch'),
        'changed-provider-result': (
            original.replace("    return result\n  }\n\n  on('session.start'",
                             "    return { ...result, text: 'mutated-result' } as R\n  }\n\n  on('session.start'", 1),
            '(fail) middleware contract > provider result returned unchanged'),
        'late-session-context': (
            original.replace('    emit(ctx, phase, resultSequence, payload, reading)',
                             '    emit({ ...ctx, sessionId: ownership.getCurrentScope()?.sessionId }, phase, resultSequence, payload, reading)', 1),
            '(fail) provenance and identity > delayed result uses original identity across a logical session change'),
        'forged-core-settlement': (
            original.replace("last !== undefined && last.plugin === 'engine' && last.tier === 'core' && (last.outcome === 'returned' || last.outcome === 'passed'),", 'true,', 1),
            '(fail) provenance and identity > engine dispatch origin alone does not prove downstream core execution'),
    }
    results = []
    for name, (module, rejected_by) in variants.items():
        mod = base / name
        shutil.copytree(source, mod)
        (mod / 'hooks/register.ts').write_text(module)
        if name == 'no-observer':
            for path in (mod / 'tests').glob('*.test.ts'):
                if path.name != 'kit-behavior.test.ts':
                    path.unlink()
            path = mod / 'tests/kit-behavior.test.ts'
            text = path.read_text()
            # Only omit the observer-specific four-record/epoch assertions.
            # Host order, permission, result and terminal-trace checks remain.
            start = text.index('  const records = helper.records()')
            end = text.index('\n})', start)
            path.write_text(text[:start] + text[end:])
        elif name != 'unchanged':
            assert module != original, name
        command = [args.claude, 'plugin', 'test', str(mod)]
        proc = subprocess.run(command, cwd=base, env=env, capture_output=True, text=True)
        (base / (name + '.stdout')).write_text(proc.stdout)
        (base / (name + '.stderr')).write_text(proc.stderr)
        output = proc.stdout + proc.stderr
        passed = (proc.returncode == 1 and rejected_by in output) if rejected_by else proc.returncode == 0
        results.append({'variant': name, 'command': command, 'exitStatus': proc.returncode,
                        'expectedRejection': rejected_by, 'controlPassed': passed})
        print(name, 'PASS' if passed else 'FAIL', '(CLI exit', str(proc.returncode) + ')')
    (base / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    print('Artifacts:', base)
    return 0 if all(result['controlPassed'] for result in results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
