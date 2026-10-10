#!/usr/bin/env python3
"""Compile byte-identical F2 sources without importing Darwin process APIs.

Only the crate wrapper/dependency graph is portable. The installer modules and
Scratch implementation/tests are copied without changes for the positive run.
Each negative control then changes one identified predicate in a disposable
copy. No native executable or owner configuration is invoked.
"""
import argparse
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
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument('--target-dir', type=Path, required=True)
    args = parser.parse_args()
    repo, output = args.repo.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    setup_source = repo / 'crates/provider-claude/src/setup'
    ownership_source = repo / 'tests/native/harness/src/bin/m0c/m2_ownership.rs'
    manifest = '''[package]
name = "threadspace-f2-portable"
version = "0.0.0"
edition = "2024"
rust-version = "1.99"
[workspace]
[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = { version = "=1.0.151", features = ["raw_value"] }
sha2 = "=0.11.0"
uuid = { version = "=1.27.0", features = ["v4", "serde"] }
[lints.rust]
unsafe_op_in_unsafe_fn = "deny"
unused_must_use = "deny"
rust_2018_idioms = { level = "deny", priority = -1 }
[lints.clippy]
all = { level = "deny", priority = -1 }
unwrap_used = "deny"
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
'''
    wrapper = '''#[cfg(test)]
extern crate self as threadspace_provider_claude;
pub mod setup;
#[cfg(test)]
#[path = "../../../harness/m2_ownership.rs"]
mod ownership;
'''
    source_hashes = {str(p.relative_to(repo)): sha(p.read_bytes()) for p in sorted(setup_source.glob('*.rs'))}
    source_hashes[str(ownership_source.relative_to(repo))] = sha(ownership_source.read_bytes())
    records = []
    with tempfile.TemporaryDirectory(prefix='threadspace-f2-') as disposable:
        root = Path(disposable)
        identity = (root.stat().st_dev, root.stat().st_ino)
        crate = root / 'crates/provider-claude'
        (crate/'src').mkdir(parents=True)
        shutil.copytree(setup_source, crate/'src/setup')
        (root/'harness').mkdir()
        shutil.copyfile(ownership_source, root/'harness/m2_ownership.rs')
        (root/'packages').mkdir()
        shutil.copytree(repo/'packages/provider-mod', root/'packages/provider-mod',
            ignore=shutil.ignore_patterns('node_modules', '.git'))
        provider_mod_hashes = {f'packages/provider-mod/{p.relative_to(root/"packages/provider-mod")}': sha(p.read_bytes())
            for p in sorted((root/'packages/provider-mod/hooks').glob('*')) if p.is_file()}
        for relative, expected in source_hashes.items():
            copied = root/'harness/m2_ownership.rs' if relative.endswith('m2_ownership.rs') else crate/'src/setup'/Path(relative).name
            assert sha(copied.read_bytes()) == expected, f'positive source changed while copying: {relative}'
        (crate/'Cargo.toml').write_text(manifest)
        (crate/'src/lib.rs').write_text(wrapper)
        (root/'fixtures').mkdir()
        (root/'inventories').mkdir()
        env = os.environ | {'CARGO_TARGET_DIR': str(args.target_dir.resolve()),
            'CARGO_TERM_COLOR': 'never', 'TMPDIR': str(root/'fixtures'),
            'THREADSPACE_F2_INVENTORY_DIR': str(root/'inventories')}

        def run(label, argv, expected_failure=False):
            completed = subprocess.run(argv, cwd=crate, env=env, text=True,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=600)
            text = completed.stdout.replace(str(root), '<disposable>')
            (output/f'{label}.log').write_text(text)
            result = {'label': label, 'argv': argv, 'exitCode': completed.returncode,
                'expectedFailure': expected_failure,
                'passedTests': re.findall(r'^test (\S+) \.\.\. ok$', text, re.M),
                'failedTests': re.findall(r'^test (\S+) \.\.\. FAILED$', text, re.M),
                'outputSha256': sha(text.encode())}
            records.append(result)
            print(json.dumps({k: v for k, v in result.items() if k not in ('passedTests', 'failedTests')}) , flush=True)
            if expected_failure:
                assert completed.returncode != 0 and result['failedTests'], f'{label}: mutant must fail an assertion, not merely compilation'
            else:
                assert completed.returncode == 0, f'{label}: see retained output'
            return result

        run('portable-lock', ['cargo', '+1.99.0', 'generate-lockfile', '--offline'])
        shutil.copyfile(crate/'Cargo.lock', output/'portable-Cargo.lock')
        (output/'portable-Cargo.toml').write_text(manifest)
        (output/'portable-lib.rs').write_text(wrapper)
        cargo = ['cargo', '+1.99.0', 'test', '--offline', '--locked', '--target', 'x86_64-unknown-linux-gnu']
        baseline = run('portable-positive', cargo)
        focused = [name for name in baseline['passedTests'] if re.search(r'::f2_\d\d_', name)]
        assert len(focused) == 12, f'expected all 12 F2 cases, got {focused}'
        inventories = sorted((root/'inventories').glob('*.json'))
        assert len(inventories) == 5, 'all wrong-installation fixture inventories must be retained'
        (output/'byte-inventories').mkdir(exist_ok=True)
        inventory_hashes = {}
        for inventory in inventories:
            record = json.loads(inventory.read_text())
            assert record['before'] == record['after'], 'refusal mutated a fixture'
            shutil.copyfile(inventory, output/'byte-inventories'/inventory.name)
            inventory_hashes[inventory.name] = sha(inventory.read_bytes())
        run('portable-clippy', ['cargo', '+1.99.0', 'clippy', '--offline', '--locked', '--target', 'x86_64-unknown-linux-gnu', '--all-targets', '--', '-D', 'warnings'])

        sources = {'settings': crate/'src/setup/settings.rs', 'setup': crate/'src/setup/mod.rs',
            'scratch': root/'harness/m2_ownership.rs'}
        originals = {name: path.read_text() for name, path in sources.items()}
        mutations = []
        text = originals['settings']
        start = text.index('        // The installed group has only matcher/hooks.')
        end = text.index('        let Some(list)', start)
        text = text[:start] + text[end:]
        assert 'list.retain(|hook| !unchanged_hook(hook, command));' in text
        text = text.replace('list.retain(|hook| !unchanged_hook(hook, command));',
            'list.retain(|hook| hook.get("command").and_then(Value::as_str) != Some(command));', 1)
        mutations.append(('mutation-command-marker-only', 'settings', text, 'f2_02_'))

        text = originals['setup']
        predicate = 'if !report.conflicts.is_empty() || !report.retained_references.is_empty() {'
        assert text.count(predicate) == 1
        mutations.append(('mutation-delete-conflicted-resources', 'setup', text.replace(predicate, 'if false {', 1), 'f2_05_'))

        text = originals['setup']
        start = text.index('pub fn uninstall(')
        guard = '    verify_identity(target, &layout, &record)?;'
        guard_at = text.index(guard, start)
        text = text[:guard_at] + text[guard_at:].replace(guard, '', 1)
        mutations.append(('mutation-uninstall-without-identity', 'setup', text, 'f2_09_'))

        text = originals['scratch']
        marker = '    fn drop(&mut self) {\n'
        assert text.count(marker) == 1
        text = text.replace(marker, marker + '        let _ = (self.integration)("uninstall", &self.dir.join("session-config"), "session");\n', 1)
        mutations.append(('mutation-drop-without-acquisition', 'scratch', text, 'f2_10_'))

        for label, changed, text, test_filter in mutations:
            for name, path in sources.items():
                path.write_text(text if name == changed else originals[name])
            import difflib
            patch = ''.join(difflib.unified_diff(originals[changed].splitlines(keepends=True),
                text.splitlines(keepends=True), fromfile=f'repaired/{changed}', tofile=f'mutant/{changed}'))
            (output/f'{label}.patch').write_text(patch)
            result = run(label, cargo+[test_filter], expected_failure=True)
            assert any(test_filter in name for name in result['failedTests'])
        for name, path in sources.items():
            path.write_text(originals[name])
        assert (root.stat().st_dev, root.stat().st_ino) == identity, 'disposable ownership changed'

    summary = {'status': 'PORTABLE_PASS_NATIVE_PENDING', 'runtime': 'Rust 1.99.0 / x86_64-unknown-linux-gnu',
        'sourceHashes': source_hashes, 'workspaceCargoLockSha256': sha((repo/'Cargo.lock').read_bytes()),
        'providerModRuntimeSourceHashes': provider_mod_hashes, 'byteInventoryHashes': inventory_hashes,
        'portableCargoLockSha256': sha((output/'portable-Cargo.lock').read_bytes()),
        'focusedCasesPassed': len(focused), 'allPortableTestsPassed': len(baseline['passedTests']),
        'mutationsKilledByAssertions': 4, 'executions': records,
        'limitations': ['The full workspace imports Darwin process APIs unavailable on Linux.',
            'This standalone crate compiles copied byte-identical installer and pure Scratch sources; it is not a native application build.',
            'Development-channel native integration smoke and native reversible cycles remain pending.']}
    (output/'portable-summary.json').write_text(json.dumps(summary, indent=2)+'\n')


if __name__ == '__main__':
    main()
