import copy
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import m2_activation_native as native
from m2_activation_readiness import reply, acknowledgement


class PortableActivationControls(unittest.TestCase):
    def test_actual_callback_return_forwarding_and_failures(self):
        node = shutil.which('node')
        self.assertIsNotNone(node, 'portable Node runtime required; never substitute provider')
        result = subprocess.run([node, str(native.HERE / 'm2_readiness_controls.js'), str(native.ACTIVATION)], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        controls = json.loads(result.stdout)
        self.assertEqual(len(controls['results']), 17)
        self.assertTrue(all(c['calls'] == 1 and (c['returnedOriginalPromise'] or c['synchronousOriginalError']) for c in controls['results']))
        print(json.dumps({'kind': controls['kind'], 'cases': len(controls['results']), 'sourceSha256': controls['sourceSha256'], 'nativeProviderExecuted': False}, sort_keys=True))

    def test_no_measuring_or_provider_input_apis(self):
        source = native.ACTIVATION.read_text()
        for forbidden in ('performance.', '$.clock', 'startBackground', 'createLatencyMeasurement', 'nativeNs', '$.prompt', '$.process', 'config.delays', 'config.plan'):
            self.assertNotIn(forbidden, source)
        self.assertEqual(source.count("on('session.start',"), 1)
        self.assertEqual(source.count('next(e)'), 1)
        self.assertNotIn('link.ms', source)
        self.assertIn('snapshot(link.received, false)', source)
        self.assertIn('snapshot(link.returned, true)', source)

    def test_actual_readiness_protocol_forgery_and_replay(self):
        config = {key: 'synthetic-' + key for key in ('runId', 'responderId', 'nonce', 'activationSha256', 'fixtureSha256', 'observerSha256', 'bootId')}
        valid = 'GET /' + '/'.join(('ready', config['runId'], config['responderId'], config['nonce'], config['activationSha256'])) + ' HTTP/1.1'
        self.assertEqual(reply(valid, config, False), (200, acknowledgement(config), True))
        self.assertEqual(reply(valid, config, True)[0], 409)
        for key in ('runId', 'responderId', 'nonce', 'activationSha256'):
            self.assertEqual(reply(valid.replace(config[key], 'foreign-' + key), config, False)[0], 403)
        self.assertEqual(reply(valid.replace('GET ', 'POST '), config, False)[0], 403)
        self.assertEqual(reply('malformed', config, False)[0], 403)
        self.assertNotIn('nativeNs', acknowledgement(config))

    def test_both_package_bytes_identity_and_loading(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            observer = native.stage(root / 'observer-plugin', 'observer', {})
            activation = native.stage(root / 'duration-plugin', 'activation', {})
            native.verify_staging([observer, activation])
            self.assertNotEqual(observer['name'], activation['name'])
            self.assertNotEqual(observer['root'], activation['root'])
            for name in native.OBSERVER_FILES: self.assertEqual((Path(observer['root']) / name).read_bytes(), (native.OBSERVER / name).read_bytes())
            self.assertEqual(activation['modules'], ['./activation.ts'])
            self.assertNotIn('hooks/register.ts', activation['files'])
            argv = native.provider_argv(root, {'plugins': [observer, activation]})
            self.assertEqual([argv[i+1] for i, v in enumerate(argv) if v == '--plugin-dir'], [observer['root'], activation['root']])
            for entry in (observer, activation):
                for name in entry['files']:
                    path = Path(entry['root']) / name; original = path.read_bytes()
                    path.write_bytes(b'foreign')
                    with self.assertRaises(RuntimeError): native.verify_staging([observer, activation])
                    path.write_bytes(original)
            changed = copy.deepcopy(activation); changed['name'] = observer['name']
            with self.assertRaises(RuntimeError): native.verify_staging([observer, changed])
            changed = copy.deepcopy(activation); changed['modules'] = ['./register.ts']
            with self.assertRaises(RuntimeError): native.verify_staging([changed])

    def test_disposable_configuration_and_stage_failure_labels(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.dict('os.environ', {'CLAUDE_CODE_PLUGIN_DIRS': '/synthetic-owner-plugin', 'CLAUDE_CONFIG_DIR': '/synthetic-owner-config'}):
                env = native.environment(Path(directory))
            self.assertFalse(any(k.startswith('CLAUDE_CODE_PLUGIN_') for k in env))
            self.assertEqual(env['CLAUDE_CONFIG_DIR'], str(Path(directory) / 'config'))
        self.assertEqual(native.classify({}, 'hooks module example failed to load: refused'), 'REGISTRATION_REFUSAL_REPORTED')
        self.assertEqual(native.classify({'error': {'stage': 'CALLBACK_FAILURE'}}, ''), 'CALLBACK_FAILURE')
        self.assertEqual(native.classify({'error': {'stage': 'OUTPUT_WRITE_FAILURE'}}, ''), 'OUTPUT_WRITE_FAILURE')
        self.assertEqual(native.classify({'forward': {}}, ''), 'INITIALIZATION_ONLY_OR_MISSING_CALLBACK_WITNESS')
        self.assertEqual(native.classify({'forward': {'stage': 'DOWNSTREAM_SETTLED'}}, ''), 'HTTP_READINESS_MISSING_AFTER_FORWARDING')
        self.assertEqual(native.classify({}, ''), 'INITIALIZATION_ONLY_OR_MISSING_CALLBACK_WITNESS')


if __name__ == '__main__': unittest.main(verbosity=2)
