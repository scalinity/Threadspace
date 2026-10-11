import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import m2_activation_native as native
import m2_activation_verdict as verdict


class SourcePinControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = verdict.read_review()
        cls.binding = native.source_binding()

    def test_exact_accepted_git_source(self):
        result = verdict.admit_source(self.binding, self.raw)
        self.assertEqual(result['reviewedGitCommit'], verdict.REVIEWED_COMMIT)
        self.assertEqual(result['sourceAdmission'], 'PASS')
        self.assertEqual(result['nativeActivation'], 'NOT_RUN')

    def test_old_source_and_matching_relabel_rejected(self):
        old = (verdict.REPO / 'evidence/M2/replacement-activation-repair/source-identities.json').read_bytes()
        with self.assertRaisesRegex(RuntimeError, 'unaccepted Git source'):
            verdict.admit_source(self.binding, old, '5b8ac24c3bcdc2d7e17f8e8777094c93a1ca5e8c')
        changed = json.loads(self.raw)
        changed['sourceBinding']['conformanceSourceIdentities']['probeSha256'] = 'a' * 64
        binding = copy.deepcopy(self.binding); binding['positiveProbeSha256'] = 'a' * 64
        with self.assertRaisesRegex(RuntimeError, 'unaccepted Git source'):
            verdict.admit_source(binding, json.dumps(changed).encode())

    def test_all_component_substitutions_rejected(self):
        for key in ('providerPath', 'providerSha256', 'providerVersion', 'positiveProbeSha256',
                    'activationSha256', 'observerSha256', 'backgroundSha256', 'admittedStagerSha256',
                    'fixtureSha256', 'reviewedControllerSha256', 'controllerSha256', 'verdictSha256',
                    'verdictTestSha256', 'projectionControlSha256'):
            with self.subTest(key=key):
                changed = copy.deepcopy(self.binding); changed[key] = 'a' * 64
                with self.assertRaises(RuntimeError): verdict.admit_source(changed, self.raw)
        for key in ('observerFiles', 'conformanceSourceIdentities', 'reviewedDependencyHashes'):
            with self.subTest(key=key):
                changed = copy.deepcopy(self.binding); changed[key] = {}
                with self.assertRaises(RuntimeError): verdict.admit_source(changed, self.raw)

    def test_changed_composition_and_config_rejected(self):
        for key in ('name', 'modules', 'files'):
            changed = copy.deepcopy(self.binding)
            target = changed['conformanceSourceIdentities']['pluginComposition']['observer']
            target[key] = {'foreign': 'capture-enabled'} if key == 'files' else 'foreign'
            with self.assertRaisesRegex(RuntimeError, 'composition'): verdict.admit_source(changed, self.raw)
        with tempfile.TemporaryDirectory() as d:
            observer = native.stage(Path(d) / 'observer', 'observer', {})
            activation = native.stage(Path(d) / 'activation', 'activation', {'kind': 'SYNTHETIC_ONLY'})
            native.verify_staging([observer, activation])
            (Path(activation['root']) / 'hooks/config.ts').write_text('export const config = {foreign: true};')
            with self.assertRaisesRegex(RuntimeError, 'changed staged source'): native.verify_staging([observer, activation])
            activation['files']['hooks/config.ts'] = native.file_hash(Path(activation['root']) / 'hooks/config.ts')
            with self.assertRaisesRegex(RuntimeError, 'configuration'):
                native.verify_staging([observer, activation], {'kind': 'SYNTHETIC_ONLY'})

    def test_retired_activation_freeze_and_identities_rejected(self):
        old = json.loads((verdict.REPO / 'evidence/M2/replacement-activation-native/frozen-activation.json').read_text())
        with self.assertRaisesRegex(RuntimeError, 'stale activation'): verdict.reject_retired(old)
        fresh = {'reviewedGitCommit': verdict.REVIEWED_COMMIT,
                 'ids': {k: 'synthetic-unused-' + k for k in verdict.RETIRED}}
        verdict.reject_retired(fresh)
        for key, value in verdict.RETIRED.items():
            changed = copy.deepcopy(fresh); changed['ids'][key] = value
            with self.assertRaisesRegex(RuntimeError, 'retired'): verdict.reject_retired(changed)

    def test_prepare_and_direct_smoke_stop_before_native_or_freeze(self):
        with tempfile.TemporaryDirectory() as d, patch.object(native, 'read_review', return_value=self.raw), \
             patch('subprocess.Popen', side_effect=AssertionError('native process forbidden')) as process, \
             patch('subprocess.check_output', side_effect=AssertionError('native query forbidden')) as query:
            with self.assertRaisesRegex(RuntimeError, 'NATIVE_EVIDENCE_AUTHORITY_INCOMPLETE'):
                native.prepare(Path(d))
            freeze = {'reviewedGitCommit': verdict.REVIEWED_COMMIT,
                      'ids': {k: 'synthetic-unused-' + k for k in verdict.RETIRED}}
            with self.assertRaisesRegex(RuntimeError, 'NATIVE_EVIDENCE_AUTHORITY_INCOMPLETE'):
                native.smoke(Path(d), freeze, {})
            self.assertEqual(list(Path(d).iterdir()), [])
            process.assert_not_called(); query.assert_not_called()


class EvidenceVerdictControls(unittest.TestCase):
    def setUp(self):
        self.config = {'runId': 'synthetic-only-run', 'nonce': 'synthetic-only-nonce',
                       'activationSha256': verdict.ACTIVATION_SHA, 'positiveProbeSha256': 'synthetic-only-positive',
                       'acknowledgement': {'kind': 'ACTIVATION_ONLY_READY', 'nonce': 'synthetic-only-nonce',
                                           'responderId': 'synthetic-only-responder'}}
        identity = {'kind': 'ACTIVATION_ONLY', **{k: self.config[k] for k in
                    ('runId', 'nonce', 'activationSha256', 'positiveProbeSha256')}}
        self.values = [{'label': 'entry', 'value': {**identity, 'stage': 'CALLBACK_ENTERED'}},
                       {'label': 'forward', 'value': {**identity, 'stage': 'DOWNSTREAM_SETTLED',
                        'origin': {'plugin': 'engine', 'tier': 'core'}, 'event': 'session.start', 'trace': [
                            {'index': 1, 'plugin': 'threadspace-observer', 'tier': 'user', 'event': 'session.start', 'outcome': 'returned'},
                            {'index': 2, 'plugin': 'engine', 'tier': 'core', 'event': 'session.start', 'outcome': 'returned'}]}},
                       {'label': 'result', 'value': {**identity, 'stage': 'READINESS_READ_BACK', 'acknowledgement': self.config['acknowledgement']}}]
        self.transcript = [{'event': 'start', 'identity': self.config['acknowledgement']},
                           {'event': 'received', 'attempt': 1, 'status': 200, 'nonceMatched': True},
                           {'event': 'generated', 'attempt': 1, 'status': 200, 'body': self.config['acknowledgement']},
                           {'event': 'answered', 'attempt': 1, 'status': 200},
                           {'event': 'stop', 'attempts': 1, 'generated': 1, 'answered': 1, 'accepted': 1}]

    def artifacts(self):
        result = []
        for item in self.values:
            raw = json.dumps(item['value'], sort_keys=True).encode()
            result.append({'label': item['label'], 'raw': raw, 'sha256': verdict.digest(raw)})
        return result

    def assess(self):
        return verdict.assess_partial_outputs(self.config, self.artifacts(), self.transcript)

    def test_even_consistent_complete_synthetic_files_cannot_pass(self):
        result = self.assess()
        self.assertTrue(result['structuralReadbacksConsistent'])
        self.assertEqual(result['verdict'], 'INCOMPLETE')
        self.assertFalse(result['nativeExecutionOrCleanupCertified'])
        self.assertEqual(len(result['missingAuthority']), 3)
        # classify()'s positive-looking label is descriptive only.
        outputs = {item['label']: item['value'] for item in self.values}
        self.assertEqual(native.classify(outputs, ''), 'ACTIVATION_OUTPUT_AND_HTTP_RETURNED')

    def test_foreign_stale_source_run_nonce_and_host_origin(self):
        for key in ('activationSha256', 'positiveProbeSha256', 'runId', 'nonce'):
            with self.subTest(key=key):
                old = self.values[2]['value'][key]; self.values[2]['value'][key] = 'forged'
                self.assertIn('foreign or stale source/run/nonce', self.assess()['failures'])
                self.values[2]['value'][key] = old
        self.values[1]['value']['origin']['plugin'] = 'foreign'
        self.assertIn('foreign dispatch origin/event', self.assess()['failures'])

    def test_missing_duplicate_reordered_or_hash_inconsistent_outputs(self):
        artifacts = self.artifacts()
        for changed in (artifacts[1:], artifacts[:-1], artifacts[::-1], artifacts + artifacts[:1]):
            result = verdict.assess_partial_outputs(self.config, changed, self.transcript)
            self.assertIn('missing, duplicate or reordered output readbacks', result['failures'])
        changed = copy.deepcopy(artifacts); changed[0]['raw'] = b'forged'
        self.assertIn('output bytes/hash mismatch', verdict.assess_partial_outputs(self.config, changed, self.transcript)['failures'])
        changed[0]['sha256'] = verdict.digest(b'forged')
        self.assertIn('malformed output bytes', verdict.assess_partial_outputs(self.config, changed, self.transcript)['failures'])

    def test_engine_only_skipped_reordered_and_foreign_trace(self):
        trace = self.values[1]['value']['trace']
        self.values[1]['value']['trace'] = trace[1:]
        self.assertIn('original observer attribution UNVERIFIED', self.assess()['failures'])
        self.values[1]['value']['trace'] = trace[::-1]
        self.assertIn('duplicate or reordered trace', self.assess()['failures'])
        self.values[1]['value']['trace'] = trace
        trace[0]['outcome'] = 'skipped'
        self.assertIn('unsettled or bypassed dispatch', self.assess()['failures'])
        trace[0]['outcome'] = 'returned'; trace[0]['plugin'] = 'foreign'
        self.assertIn('unexpected trace contributor', self.assess()['failures'])

    def test_wrong_responder_ack_replay_and_incomplete_transcript(self):
        original = copy.deepcopy(self.transcript)
        for changed in (original[1:], original[:-1], original[::-1], original + original[1:4]):
            self.transcript = changed
            self.assertIn('incomplete or reordered native transcript', self.assess()['failures'])
        self.transcript = copy.deepcopy(original)
        self.transcript[2]['body'] = {'kind': 'ACTIVATION_ONLY_READY', 'responderId': 'foreign'}
        self.assertIn('inconsistent, replayed or foreign readiness exchange', self.assess()['failures'])
        self.transcript = copy.deepcopy(original); self.transcript[-1]['accepted'] = 2
        self.assertIn('inconsistent, replayed or foreign readiness exchange', self.assess()['failures'])
        self.values[2]['value']['acknowledgement'] = {'kind': 'ACTIVATION_ONLY_READY', 'nonce': 'foreign'}
        self.assertIn('foreign readiness acknowledgement', self.assess()['failures'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
