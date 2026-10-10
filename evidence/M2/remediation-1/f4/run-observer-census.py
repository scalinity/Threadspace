#!/usr/bin/env python3
"""Execute real observer census modules on disposable portable fixtures.

No native app, provider session, owner configuration or live store is used.
The Rust wrapper copies the exact relay metadata modules and imports the real
contracts crate solely for its actual ModBatchReceipt/ObservationEnvelope
shape. Positive and mutation runs use locked pinned dependency versions.
"""
import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[4])
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parent/'observer-census')
    parser.add_argument('--target-dir', type=Path, required=True)
    parser.add_argument('--node', required=True)
    args = parser.parse_args()
    repo, output = args.repo.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    sources = [
        'crates/relay/src/latency.rs', 'crates/relay/src/latency_census.rs', 'crates/relay/src/latency_census_tests.rs',
        'crates/relay/src/bin/threadspace-hook.rs', 'crates/relay/Cargo.toml',
        'packages/provider-mod/hooks/latency.ts', 'packages/provider-mod/hooks/delivery.ts',
        'packages/provider-mod/hooks/register.ts', 'packages/provider-mod/hooks/ownership.ts',
        'packages/provider-mod/tests/latency.node.mjs',
        'tests/native/harness/src/bin/m0c/m2_latency.rs', 'tests/native/harness/Cargo.toml',
        'crates/contracts/src/canonical/capture.rs', 'Cargo.lock',
    ]
    source_hashes = {name: sha((repo/name).read_bytes()) for name in sources}
    manifest = f'''[package]
name = "threadspace-observer-census-portable"
version = "0.0.0"
edition = "2024"
rust-version = "1.99"
[workspace]
[dependencies]
threadspace-contracts = {{ path = "{repo}/crates/contracts" }}
serde = {{ version = "=1.0.229", features = ["derive"] }}
serde_json = "=1.0.151"
uuid = {{ version = "=1.27.0", features = ["v4", "serde"] }}
libc = "=0.2.190"
sha2 = "=0.11.0"
[lints.rust]
unsafe_op_in_unsafe_fn = "deny"
unused_must_use = "deny"
rust_2018_idioms = {{ level = "deny", priority = -1 }}
[lints.clippy]
all = {{ level = "deny", priority = -1 }}
unwrap_used = "deny"
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
'''
    records = []
    with tempfile.TemporaryDirectory(prefix='threadspace-census-') as disposable:
        root = Path(disposable)
        identity = (root.stat().st_dev, root.stat().st_ino)
        crate = root/'rust'
        (crate/'src').mkdir(parents=True)
        for name in ('latency.rs', 'latency_census.rs', 'latency_census_tests.rs'):
            shutil.copyfile(repo/'crates/relay/src'/name, crate/'src'/name)
            assert sha((crate/'src'/name).read_bytes()) == source_hashes[f'crates/relay/src/{name}']
        (crate/'Cargo.toml').write_text(manifest)
        (crate/'src/lib.rs').write_text('pub mod latency;\n')
        shutil.copytree(repo/'packages/provider-mod', root/'provider-mod', ignore=shutil.ignore_patterns('node_modules', '.git'))
        (root/'fixtures').mkdir()
        env = os.environ | {'CARGO_TARGET_DIR': str(args.target_dir.resolve()), 'CARGO_TERM_COLOR': 'never',
            'TMPDIR': str(root/'fixtures'), 'PYTHONDONTWRITEBYTECODE': '1'}

        def run(label, argv, cwd=crate, expected_failure=None):
            artifacts = root/label
            artifacts.mkdir()
            completed = subprocess.run(argv, cwd=cwd, env=env | {'THREADSPACE_CENSUS_EVIDENCE_DIR': str(artifacts)},
                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=600)
            text = completed.stdout.replace(str(root), '<disposable>')
            (output/f'{label}.log').write_text(text)
            result = {'label': label, 'argv': argv, 'exitCode': completed.returncode,
                'expectedFailure': expected_failure is not None,
                'passedTests': re.findall(r'^test (\S+) \.\.\. ok$', text, re.M),
                'failedTests': re.findall(r'^test (\S+) \.\.\. FAILED$', text, re.M),
                'outputSha256': sha(text.encode())}
            if expected_failure is None:
                assert completed.returncode == 0, f'{label}: see retained output'
            else:
                assert completed.returncode != 0 and expected_failure in text and ('FAILED' in text or 'not ok ' in text), f'{label}: mutation must fail its intended assertion, not only compilation'
            artifact_hashes = {}
            if list(artifacts.iterdir()):
                destination = output/label
                destination.mkdir(exist_ok=True)
                for path in sorted(artifacts.iterdir()):
                    shutil.copyfile(path, destination/path.name)
                    artifact_hashes[path.name] = sha(path.read_bytes())
            result['artifacts'] = artifact_hashes
            records.append(result)
            print(json.dumps({key: value for key, value in result.items() if key not in ('passedTests', 'failedTests')}), flush=True)
            return result

        run('portable-lock', ['cargo', '+1.99.0', 'generate-lockfile', '--offline'])
        shutil.copyfile(crate/'Cargo.lock', output/'portable-Cargo.lock')
        (output/'portable-Cargo.toml').write_text(manifest)
        (output/'portable-lib.rs').write_text('pub mod latency;\n')
        cargo = ['cargo', '+1.99.0', 'test', '--offline', '--locked', '--target', 'x86_64-unknown-linux-gnu']
        baseline = run('portable-positive', cargo)
        assert len(baseline['passedTests']) == 16, baseline['passedTests']
        run('portable-clippy', ['cargo', '+1.99.0', 'clippy', '--offline', '--locked', '--target', 'x86_64-unknown-linux-gnu', '--all-targets', '--', '-D', 'warnings'])
        node = run('node-positive', [args.node, '--test', '--test-reporter=tap', 'tests/latency.node.mjs'], root/'provider-mod')
        text = (output/'node-positive.log').read_text()
        assert '# pass 20' in text and '# fail 0' in text

        source = crate/'src/latency_census.rs'
        original = source.read_text()
        marker = '        if confirms.len() != 1 {'
        assert original.count(marker) == 1
        unconfirmed = original.replace(marker, '        if confirms.is_empty() { return Ok(json!({"sealed": true})); }\n'+marker, 1)
        predicate = '&& digest(&ids) == close.observation_ids_sha256'
        assert original.count(predicate) == 1
        digestless = original.replace(predicate, '', 1)
        mutations = [
            ('mutation-prefix-without-confirmation', unconfirmed, 'complete_paged_ledger_requires_positive_close_receipt_confirmation', 'unconfirmed close'),
            ('mutation-omit-ledger-digest', digestless, 'changed_count_digest_order_duplicate_or_overflow_refuses_native_close', 'digest'),
        ]
        for label, changed, test_filter, expected in mutations:
            source.write_text(changed)
            (output/f'{label}.patch').write_text(''.join(difflib.unified_diff(original.splitlines(keepends=True), changed.splitlines(keepends=True), fromfile='repaired/latency_census.rs', tofile='mutant/latency_census.rs')))
            run(label, cargo+[test_filter, '--', '--nocapture'], expected_failure=expected)
        source.write_text(original)
        measured = root/'provider-mod/hooks/latency.ts'
        original_ts = measured.read_text()
        marker = '        if (exporting || closed) { failedExports += 1; return }'
        assert original_ts.count(marker) == 1
        changed_ts = original_ts.replace(marker, "        for (const [id, status] of statuses ?? []) { if (status === 'COMMITTED' || status === 'ALREADY_COMMITTED' || status === 'LOCAL_SPOOLED') captures.delete(id) }\n"+marker, 1)
        measured.write_text(changed_ts)
        label = 'mutation-delete-accepted-ledger-ids'
        (output/f'{label}.patch').write_text(''.join(difflib.unified_diff(original_ts.splitlines(keepends=True), changed_ts.splitlines(keepends=True), fromfile='repaired/latency.ts', tofile='mutant/latency.ts')))
        run(label, [args.node, '--test', '--test-reporter=tap', '--test-name-pattern', 'accepted, spooled, rejected', 'tests/latency.node.mjs'], root/'provider-mod', 'all captured UUIDs must be retained')
        measured.write_text(original_ts)
        guard = "          if (typeof cancel !== 'function') throw new Error('unavailable end-drain watchdog')"
        assert original_ts.count(guard) == 1
        changed_ts = original_ts.replace(guard, '', 1)
        measured.write_text(changed_ts)
        label = 'mutation-trust-missing-watchdog'
        (output/f'{label}.patch').write_text(''.join(difflib.unified_diff(original_ts.splitlines(keepends=True), changed_ts.splitlines(keepends=True), fromfile='repaired/latency.ts', tofile='mutant/latency.ts')))
        run(label, [args.node, '--test', '--test-reporter=tap', '--test-name-pattern', 'unverifiable watchdog handles', 'tests/latency.node.mjs'], root/'provider-mod', 'unverifiable watchdog must never start census')
        measured.write_text(original_ts)
        assert (root.stat().st_dev, root.stat().st_ino) == identity, 'disposable ownership changed'

    assert all(sha((repo/name).read_bytes()) == expected for name, expected in source_hashes.items()), 'shared source changed during test run'
    summary = {
        'status': 'PASS_PORTABLE_OBSERVER_CENSUS_CONTROLS', 'nativeExecution': False,
        'rustToolchain': subprocess.check_output(['cargo', '+1.99.0', '--version'], text=True).strip(),
        'nodeVersion': subprocess.check_output([args.node, '--version'], text=True).strip(),
        'positiveRustTests': 16, 'positiveNodeTests': 20, 'targetedMutationsKilled': 4,
        'sourceHashes': source_hashes, 'sharedSourceUnchanged': True, 'runs': records,
        'conventionalHookCensus': 'INCOMPLETE: no independent host invocation witness covering zero-output delivery-plus-telemetry failures',
        'nativeClockQualification': 'INCOMPLETE: no platform/runtime rate or precision qualification executed',
        'nativeObserverClose': 'NOT_EXECUTED: source close and positive challenge confirmation require native host qualification',
    }
    (output/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
    print(json.dumps({key: value for key, value in summary.items() if key not in ('sourceHashes', 'runs')}, indent=2))


if __name__ == '__main__':
    main()
