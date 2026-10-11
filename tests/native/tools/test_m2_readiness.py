"""Source-bound synthetic readiness controls. No provider/native fixture launch."""
import copy
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import m2_activation_native as native
import m2_activation_verdict as verdict
from m2_activation_readiness import acknowledgement, reply


class ReadinessControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        output = subprocess.run([shutil.which('node'), str(native.HERE / 'm2_readiness_controls.js'), str(native.ACTIVATION)],
                                capture_output=True, timeout=10, check=True)
        cls.module = json.loads(output.stdout)
        cls.cases = {r['mode']: r for r in cls.module['results']}
        cls.raw_review = verdict.read_review()
        root = os.environ.get('THREADSPACE_READINESS_CONTROL_ROOT')
        if root:
            path = Path(root) / 'module-controls.json'
            if path.exists():
                if path.read_bytes() != output.stdout: raise RuntimeError('changed retained module control output')
            else: native.write_private(path, output.stdout)
        print(json.dumps({'actualModuleSha256': cls.module['sourceSha256'], 'cases': len(cls.cases),
                          'mutationControls': cls.module['rejectedMutations'], 'nativeProviderExecuted': False}))

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); os.chmod(self.root, 0o700)
        self.ids = {**native.source_binding(), 'runId': self.module['config']['runId'],
                    'responderId': self.module['config']['responderId'], 'nonce': self.module['config']['nonce'], 'bootId': 'synthetic-boot'}
        self.config = {**self.ids, 'acknowledgement': acknowledgement(self.ids), 'socketPath': str(self.root / 'readiness/s.sock')}
        self.config.update({k + 'Output': str(self.root / (k + '.json')) for k in ('entry', 'forward', 'result', 'error')})
        self.entries = [native.stage(self.root / 'observer-plugin', 'observer', self.config),
                        native.stage(self.root / 'activation-plugin', 'activation', self.config)]
        self.env = native.environment(self.root)
        argv = native.provider_argv(self.root, {'plugins': self.entries}) + ['--debug-file', str(self.root / 'provider-debug.log')]
        self.freeze = {'kind': verdict.CONTRACT, 'reviewedGitCommit': verdict.REVIEWED_COMMIT,
                       'reviewRecordSha256': verdict.REVIEW_RECORD_SHA, 'deadlineSeconds': 20, 'initializeRequestId': 'synthetic-initialize',
                       'ids': self.ids, 'config': self.config, 'smokeEntries': self.entries, 'argv': argv}
        self.record = {'domain': 'SYNTHETIC', 'freezeSha256': native.canonical_hash(self.freeze), 'attempts': 1,
                       'attemptRunId': self.ids['runId'], 'controllerUid': os.getuid(), 'controllerPid': os.getpid(),
                       'deadlineExceeded': False, 'timingRows': 0, 'positiveTimingModuleExecuted': False,
                       'socketRemoved': True, 'processes': {}, 'cleanup': [], 'childConfiguration': {'CLAUDE_CONFIG_DIR': self.env['CLAUDE_CONFIG_DIR']},
                       'initializer': {'type': 'control_response', 'response': {'subtype': 'success', 'request_id': self.freeze['initializeRequestId']}},
                       'scanner': {'activationSha256': self.ids['activationSha256'], 'providerSha256': self.ids['providerSha256'],
                                   'success': True, 'exitCode': 0, 'reaped': True}}
        self.record['composition'] = native.verify_isolation(self.root, self.entries, self.config, argv, self.env)
        self.record['composition']['runtimeLoadedNames'] = ['threadspace-observer', 'threadspace-socket-duration']
        self.record['composition']['loadedModuleCounts'] = {'threadspace-observer': 1, 'threadspace-socket-duration': 1}
        for n, role in enumerate(('provider', 'responder'), 100):
            identity = {'pid': n, 'uid': os.getuid(), 'parentPid': os.getpid(), 'startUsec': 1000 + n,
                        'kernelVerified': True, 'imagePath': self.ids['providerPath'] if role == 'provider' else self.ids['pythonExecutable'],
                        'sha256': self.ids['providerSha256'] if role == 'provider' else self.ids['pythonExecutableSha256']}
            self.record['processes'][role] = identity
            self.record['cleanup'].append({'role': role, 'identity': copy.deepcopy(identity), 'reaped': True})
        self.record['transcript'] = [
            {'event': 'start', 'pid': 101, 'uid': os.getuid(), 'identity': acknowledgement(self.ids)},
            {'event': 'received', 'attempt': 1, 'peerUid': os.getuid(), 'status': 200, 'nonceMatched': True},
            {'event': 'generated', 'attempt': 1, 'status': 200, 'body': acknowledgement(self.ids)},
            {'event': 'answered', 'attempt': 1, 'status': 200},
            {'event': 'stop', 'attempts': 1, 'generated': 1, 'answered': 1, 'accepted': 1}]
        self.artifacts = self.for_mode('engine-only')

    def for_mode(self, mode):
        return [{**a, 'raw': a['raw'].encode()} for a in self.cases[mode]['artifacts']]

    def assess(self):
        return verdict.adjudicate(self.freeze, self.artifacts, self.record, synthetic=True)

    def alter(self, label, change):
        for a in self.artifacts:
            if a['label'] == label:
                value = json.loads(a['raw']); change(value)
                a['raw'] = json.dumps(value).encode(); a['sha256'] = verdict.digest(a['raw'])

    def test_reachable_engine_only_and_observer_baselines(self):
        result = self.assess(); self.assertEqual(result['verdict'], 'PASS', result)
        self.assertEqual(result['domain'], 'SYNTHETIC')
        self.assertEqual(result['canonicalObserver'], 'CANONICAL_OBSERVER_PARTICIPATION_UNVERIFIED')
        self.artifacts = self.for_mode('observer'); result = self.assess()
        self.assertEqual(result['verdict'], 'PASS', result); self.assertEqual(result['canonicalObserver'], 'OBSERVED DOWNSTREAM')
        self.assertFalse(result['timingQualified'])

    def test_actual_operands_mutation_and_missing_controls(self):
        for mode in ('changed-event', 'in-place', 'changed-result', 'self-answer', 'missing', 'undefined', 'null', 'primitive', 'unsupported'):
            with self.subTest(mode=mode):
                self.artifacts = self.for_mode(mode); result = self.assess()
                self.assertNotEqual(result['verdict'], 'PASS', result)
        self.assertEqual(self.cases['in-place']['calls'], 1)
        self.artifacts = self.for_mode('in-place')
        self.assertEqual(self.assess()['comparisons']['originalToReceived']['mismatchFields'], ['cwd'])

    def test_actual_error_and_telemetry_forwarding(self):
        for mode in ('synchronous-error', 'rejection', 'write-error', 'all-writes-error', 'callback-error', 'http-error'):
            case = self.cases[mode]
            self.assertEqual(case['calls'], 1)
            self.assertTrue(case['returnedOriginalPromise'] or case['synchronousOriginalError'])
            self.artifacts = self.for_mode(mode); self.assertNotEqual(self.assess()['verdict'], 'PASS')
        self.assertTrue(all(m['detected'] for m in self.module['rejectedMutations']))
        self.assertEqual(len(self.module['rejectedMutations']), 2)

    def test_source_review_and_identity_substitutions(self):
        verdict.admit_source(self.ids, self.raw_review)
        with self.assertRaises(RuntimeError): verdict.admit_source(self.ids, self.raw_review, '5b8ac24c3bcdc2d7e17f8e8777094c93a1ca5e8c')
        for key in ('providerPath', 'providerSha256', 'providerVersion', 'originalActivationSha256', 'activationSha256',
                    'positiveProbeSha256', 'observerSha256', 'backgroundSha256', 'fixtureSha256', 'admittedStagerSha256',
                    'controllerSha256', 'verdictSha256', 'readinessControlsSha256', 'readinessTestSha256', 'reviewedControllerSha256'):
            changed = copy.deepcopy(self.ids); changed[key] = 'foreign'
            with self.subTest(key=key), self.assertRaises(RuntimeError): verdict.admit_source(changed, self.raw_review)
        for key in ('reviewedDependencyHashes', 'conformanceSourceIdentities', 'observerFiles'):
            changed = copy.deepcopy(self.ids); changed[key] = {}
            with self.subTest(key=key), self.assertRaises(RuntimeError): verdict.admit_source(changed, self.raw_review)
        changed = json.loads(self.raw_review); changed['sourceBinding']['providerSha256'] = 'foreign'
        with self.assertRaises(RuntimeError): verdict.admit_source(self.ids, json.dumps(changed).encode())
        for key, value in verdict.RETIRED.items():
            freeze = copy.deepcopy(self.freeze); freeze['ids'][key] = value
            with self.subTest(key=key), self.assertRaises(RuntimeError): verdict.reject_retired(freeze)

    def test_missing_duplicate_reordered_malformed_readback(self):
        baseline = copy.deepcopy(self.artifacts)
        for altered in (baseline[1:], baseline[:-1], baseline + baseline[:1], baseline[::-1]):
            self.artifacts = altered; self.assertNotEqual(self.assess()['verdict'], 'PASS')
        for raw in (b'{broken', b'[]'):
            self.artifacts = copy.deepcopy(baseline); self.artifacts[0]['raw'] = raw; self.artifacts[0]['sha256'] = verdict.digest(raw)
            self.assertNotEqual(self.assess()['verdict'], 'PASS')
        self.artifacts = baseline; self.artifacts[0]['sha256'] = 'foreign'
        self.assertNotEqual(self.assess()['verdict'], 'PASS')

    def test_output_identity_dispatch_and_sequence_substitutions(self):
        baseline = copy.deepcopy(self.artifacts)
        for key, value in [('runId', 'foreign'), ('nonce', 'foreign'), ('activationSha256', 'foreign'), ('positiveProbeSha256', 'foreign'),
                           ('origin', {'plugin': 'synthetic', 'tier': 'user'}), ('event', 'other'), ('sequence', 4)]:
            self.artifacts = copy.deepcopy(baseline); self.alter('forward', lambda v: v.update({key: value}))
            self.assertNotEqual(self.assess()['verdict'], 'PASS')

    def test_trace_boundary_and_order_not_matching_search(self):
        baseline = copy.deepcopy(self.artifacts)
        for trace in ([], [{'index': 1, 'plugin': 'observer'}]):
            self.artifacts = copy.deepcopy(baseline); self.alter('forward', lambda v: v.update(trace=trace))
            self.assertNotEqual(self.assess()['verdict'], 'PASS')
        for key, value in [('outcome', 'rejected'), ('outcome', 'kept'), ('tier', 'user'), ('event', 'other'), ('reason', 'bypassed'), ('returned', None)]:
            self.artifacts = copy.deepcopy(baseline); self.alter('forward', lambda v: v['trace'][0].update({key: value}))
            self.assertNotEqual(self.assess()['verdict'], 'PASS')
        self.artifacts = self.for_mode('observer'); self.alter('forward', lambda v: v['trace'][0]['received']['fields']['cwd'].update(value='/synthetic/foreign-nearest'))
        # The farther engine still matches. Selecting it to obtain equality is forbidden.
        self.assertEqual(self.assess()['verdict'], 'FAIL')
        self.artifacts = self.for_mode('observer'); self.alter('forward', lambda v: v['trace'].reverse())
        self.assertNotEqual(self.assess()['verdict'], 'PASS')

    def test_http_actual_protocol_and_incomplete_transcript(self):
        config = self.ids; route = 'GET /' + '/'.join(('ready', config['runId'], config['responderId'], config['nonce'], config['activationSha256'])) + ' HTTP/1.1'
        self.assertEqual(reply(route, config, False), (200, acknowledgement(config), True))
        self.assertEqual(reply(route, config, True)[0], 409)
        for k in ('runId', 'nonce', 'responderId', 'activationSha256'):
            self.assertEqual(reply(route.replace(config[k], 'foreign'), config, False)[0], 403)
        baseline = copy.deepcopy(self.record['transcript'])
        for changed in (baseline[:-1], baseline[1:], baseline[::-1], baseline + baseline[1:2]):
            self.record['transcript'] = changed; self.assertNotEqual(self.assess()['verdict'], 'PASS')
        self.record['transcript'] = baseline; self.record['transcript'][1]['status'] = 409
        self.assertNotEqual(self.assess()['verdict'], 'PASS')
        self.record['transcript'] = copy.deepcopy(baseline); self.record['transcript'][0]['identity']['responderId'] = 'foreign'
        self.assertNotEqual(self.assess()['verdict'], 'PASS')

    def test_process_initializer_scanner_deadline_cleanup_failures(self):
        baseline = copy.deepcopy(self.record)
        for change in (lambda r: r['processes']['provider'].update(startUsec=0),
                       lambda r: r['processes']['responder'].update(imagePath='/synthetic/foreign'),
                       lambda r: r['cleanup'][0].update(reaped=False), lambda r: r['cleanup'].pop(),
                       lambda r: r.update(socketRemoved=False), lambda r: r.update(deadlineExceeded=True),
                       lambda r: r['initializer']['response'].update(request_id='foreign'),
                       lambda r: r['scanner'].update(activationSha256='foreign'), lambda r: r.update(attempts=2),
                       lambda r: r['composition'].update(unexpectedContributors=['foreign'])):
            self.record = copy.deepcopy(baseline); change(self.record); self.assertNotEqual(self.assess()['verdict'], 'PASS')

    def test_staged_and_configuration_joint_relabel_rejected(self):
        native.verify_isolation(self.root, self.entries, self.config, self.freeze['argv'], self.env)
        path = Path(self.entries[1]['root']) / 'hooks/activation.ts'; path.write_bytes(b'foreign')
        self.entries[1]['files']['hooks/activation.ts'] = verdict.digest(b'foreign')
        with self.assertRaises(RuntimeError): native.verify_staging(self.entries, self.config)
        self.assertNotEqual(self.assess()['verdict'], 'PASS')

    def test_source_has_no_timing_or_async_callback(self):
        source = native.ACTIVATION.read_text()
        self.assertEqual(source.count('const forwarded = next(e);'), 1)
        self.assertEqual(source.count('return forwarded;'), 1)
        self.assertLess(source.index('original = snapshot(e, false)'), source.index('const forwarded = next(e);'))
        self.assertEqual(source.count("on('session.start',"), 1)
        for token in ('performance.', '$.clock', '$.process', '$.prompt', 'startBackground', 'config.delays', 'config.plan', 'link.ms'):
            self.assertNotIn(token, source)
        self.assertNotIn("on('session.start', async", source)

    def test_no_native_spawn_by_verifier_and_stale_freeze(self):
        with patch('subprocess.Popen', side_effect=AssertionError('provider/native process forbidden')):
            # Immutable Git read is already cached explicitly here; no child needed.
            with patch.object(verdict, 'read_review', return_value=self.raw_review):
                self.assertEqual(self.assess()['verdict'], 'PASS')
                stale = copy.deepcopy(self.freeze); stale['kind'] = 'ACTIVATION_ONLY_ONE_POSITIVE'
                self.assertNotEqual(verdict.adjudicate(stale, self.artifacts, self.record, synthetic=True)['verdict'], 'PASS')


class CleanupControls(unittest.TestCase):
    def test_identity_acquisition_failure_after_spawn_reaches_guard(self):
        from unittest.mock import MagicMock
        child = MagicMock(); child.pid = 99; child.poll.return_value = None
        records = []
        with patch.object(native.subprocess, 'Popen', return_value=child), \
             patch.object(native, 'image_identity', side_effect=RuntimeError('controlled identity failure')), \
             patch.object(native, 'reap_owned', return_value={'identity': None, 'reaped': True}) as cleanup:
            with self.assertRaisesRegex(RuntimeError, 'controlled identity failure'):
                with native.owned_process('provider', [], native.PROVIDER, records): pass
            cleanup.assert_called_once_with(child, None, native.PROVIDER)
            self.assertTrue(records[0]['reaped'])

    def test_fallback_revalidates_before_signals_and_refuses_foreign(self):
        from unittest.mock import MagicMock
        child = MagicMock(); child.stdin = None; child.returncode = -15
        child.poll.return_value = -15
        identity = {'pid': 99, 'uid': os.getuid(), 'parentPid': os.getpid(), 'startUsec': 999, 'imagePath': str(native.PROVIDER)}
        with patch.object(native, 'kernel_child', side_effect=[identity, copy.deepcopy(identity)]):
            result = native.reap_owned(child, None, native.PROVIDER)
        self.assertTrue(result['reaped']); child.send_signal.assert_called_once()
        child = MagicMock()
        with patch.object(native, 'kernel_child', side_effect=[identity, {**identity, 'startUsec': 1000}]):
            with self.assertRaises(RuntimeError): native.reap_owned(child, None, native.PROVIDER)
        child.send_signal.assert_not_called()


if __name__ == '__main__': unittest.main(verbosity=2)
