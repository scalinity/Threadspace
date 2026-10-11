"""Portable source/staging controls; every process launch is forbidden here."""
from __future__ import annotations
import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import m2_socket_plugins as plugins
import m2_socket_runner as runner
import m2_socket_duration as evaluator
import m2_activation_native as activation
import m2_positive_scanner as scanner

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
EVIDENCE = REPO / 'evidence/M2/replacement-positive-scanner-repair'
OLD_SHA = 'bfe2cbba9b1bdfa3c307f3044b262aee76aa155cee54f584c91ba3ae4af0b73c'
REFERENCE_SHA = '9379a83f4f6f009d989856697379f541596c8dfa451cf4885327f5310b9feb58'
ACTIVATION_SHA = 'ea54a1cbe9253388f6e4a0e2575ee31470746e0c236e2d99ad4e5fc56c273136'


class PortableScannerRepairControls(unittest.TestCase):
    def setUp(self):
        self.blockers = [patch.object(runner.subprocess, name, side_effect=AssertionError('portable process launch forbidden'))
            for name in ('Popen', 'run', 'check_output')]
        self.mocks = [p.start() for p in self.blockers]
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name); self.root.chmod(0o700)
        self.config = {'kind': 'SYNTHETIC_STAGING_ONLY', 'runtimeId': 'synthetic-runtime',
            'sourceEpoch': 'synthetic-epoch', 'plan': [], 'delays': []}
        self.composition = plugins.composition_identity()

    def tearDown(self):
        self.temp.cleanup()
        for mock in self.mocks: self.assertEqual(mock.call_count, 0)
        for blocker in reversed(self.blockers): blocker.stop()

    def test_original_and_callback_bytes(self):
        original = (EVIDENCE / 'original-positive-probe.ts').read_text()
        corrected = (HERE / 'm2_socket_probe.ts').read_text()
        self.assertEqual(plugins.digest(original.encode()), OLD_SHA)
        self.assertIn("import { startBackground } from './background.ts';", original)
        self.assertNotIn("from './background.ts'", corrected)
        self.assertEqual(original[original.index('export const register'):], corrected[corrected.index('export const register'):])
        self.assertEqual(corrected.count("on('session.start'"), 1)
        self.assertEqual(corrected.count('next(e)'), 1)
        self.assertIn('return forwarded;', corrected)
        local = corrected[corrected.index('const startBackground ='):corrected.index('\n\nexport const register')]
        reference = (HERE / 'm2_socket_background.ts').read_text()
        self.assertEqual(local, reference[reference.index('export const startBackground ='):].removeprefix('export ').rstrip())
        self.assertEqual(evaluator.file_hash(HERE / 'm2_socket_background.ts'), REFERENCE_SHA)
        self.assertEqual(evaluator.file_hash(HERE / 'm2_activation_probe.ts'), ACTIVATION_SHA)

    def test_distinct_plugins_and_unchanged_observer(self):
        prepared = runner.prepare_provider(self.root, self.config, self.composition)
        observer, duration = prepared['plugins']['plugins']
        self.assertEqual([p['name'] for p in (observer, duration)], ['threadspace-observer', 'threadspace-socket-duration'])
        self.assertNotEqual(observer['root'], duration['root'])
        for name in plugins.OBSERVER_FILES:
            self.assertEqual((Path(observer['root']) / name).read_bytes(), (plugins.OBSERVER / name).read_bytes())
        self.assertEqual(self.composition['observer']['modules'], ['./register.ts'])
        self.assertEqual(self.composition['qualification']['modules'], ['./register.ts'])
        manifest = json.loads((Path(observer['root']) / '.claude-plugin/plugin.json').read_text())
        self.assertEqual(manifest['userConfig']['captureArgv']['default'], [])
        argv = prepared['argv']
        self.assertEqual([argv[i+1] for i, value in enumerate(argv) if value == '--plugin-dir'], [observer['root'], duration['root']])
        self.assertFalse(any(k.startswith('CLAUDE_CODE_PLUGIN_') for k in prepared['env']))
        self.assertEqual(argv[argv.index('--setting-sources')+1], '')
        self.assertEqual(argv[argv.index('--mcp-config')+1], '{"mcpServers":{}}')

    def test_source_and_configuration_substitutions_rejected(self):
        prepared = runner.prepare_provider(self.root, self.config, self.composition)
        entry = prepared['plugins']['plugins'][1]
        path = Path(entry['root']) / 'hooks/register.ts'
        path.write_text(path.read_text() + '\n// substituted source\n')
        with self.assertRaisesRegex(RuntimeError, 'changed staged plugin source'):
            runner.verify_provider(self.root, self.config, self.composition, prepared)
        old = copy.deepcopy(self.composition)
        old['qualification']['files']['hooks/register.ts'] = OLD_SHA
        with self.assertRaisesRegex(RuntimeError, 'changed canonical plugin sources'):
            plugins.expected_manifest(self.root, self.config, old)
        changed = copy.deepcopy(self.config); changed['sourceEpoch'] = 'foreign'
        with self.assertRaises(RuntimeError): plugins.validate_manifest(prepared['plugins'], self.root, changed, self.composition)

    def test_current_identity_joins_include_effective_background(self):
        ids = evaluator.source_identities()
        self.assertEqual(ids['adapterSha256'], evaluator.ADAPTER_SHA)
        self.assertEqual(ids['probeSha256'], evaluator.file_hash(HERE / 'm2_socket_probe.ts'))
        self.assertNotEqual(ids['probeSha256'], OLD_SHA)
        self.assertEqual(ids['backgroundSha256'], REFERENCE_SHA)
        self.assertEqual(ids['pluginComposition'], self.composition)
        self.assertEqual(self.composition['qualification']['files']['hooks/register.ts'], ids['probeSha256'])
        self.assertEqual(activation.source_binding()['positiveProbeSha256'], ids['probeSha256'])
        self.assertEqual(activation.source_binding()['activationSha256'], ACTIVATION_SHA)
        self.assertEqual(runner.POLICY['externalDeadlineSeconds'], 20)
        self.assertEqual(runner.POLICY['maximumEnclosureMs'], 10)
        self.assertEqual(runner.POLICY['observerAdditiveMs'], 10)

    def test_consumed_conformance_and_matching_relabel_rejected(self):
        old = json.loads((REPO / 'evidence/M2/replacement-duration-instrument-v2/conformance.json').read_text())
        self.assertEqual(len(old['rows']), 0)
        result = evaluator.qualify(old)
        self.assertFalse(result['qualified'])
        self.assertIn('missing conformance rows', result['errors'])
        relabeled = copy.deepcopy(old)
        relabeled['identities'].update(evaluator.source_identities())
        relabeled['freeze']['identities'].update(evaluator.source_identities())
        relabeled['freezeSha256'] = evaluator.canonical_hash(relabeled['freeze'])
        profile = evaluator.make_profile(relabeled)
        artifact = evaluator.artifact_identity(relabeled)
        checked, errors = evaluator.validate_binding(relabeled, profile, artifact)
        self.assertFalse(checked['qualified']); self.assertTrue(errors)

    def test_actual_scanner_freeze_and_attempt_guards_before_process_creation(self):
        freeze = scanner.prepare(self.root)
        freeze = json.loads((self.root / 'scanner-input-freeze.json').read_text())
        scanner.verify(self.root, freeze)
        with self.assertRaises(FileNotFoundError): scanner.validate(self.root, freeze, 'activation')
        (self.root / 'positive-scan-started.json').write_text('{}')
        with self.assertRaises(FileExistsError): scanner.validate(self.root, freeze, 'positive')
        path = Path(freeze['entries']['positive']['root']) / 'hooks/register.ts'
        path.write_text(path.read_text() + '\n// substituted scanner input\n')
        with self.assertRaisesRegex(RuntimeError, 'changed staged plugin source'):
            scanner.validate(self.root, freeze, 'positive')


if __name__ == '__main__': unittest.main()
