"""Immutable historical refusal and revised source-admission checks."""
import subprocess
import types
import unittest
import m2_activation_native as native
import m2_activation_verdict as verdict


class HistoricalAndCurrentAdmission(unittest.TestCase):
    def test_original_fail_closed_verifier_remains_demonstrably_refusing(self):
        base = '263e644accde1f789e99717fc8b5f4ba8dcc047e'
        prefix = 'tests/native/tools/'
        code = subprocess.check_output(['git', 'show', base + ':' + prefix + 'm2_activation_verdict.py'])
        entry = subprocess.check_output(['git', 'show', base + ':' + prefix + 'm2_activation_probe.ts'])
        module = types.ModuleType('historical_verdict')
        module.__file__ = str(native.HERE / 'm2_activation_verdict.py')
        exec(compile(code, 'immutable-historical-verdict', 'exec'), module.__dict__)
        result = module.authority_preflight(entry)
        self.assertEqual(result['verdict'], 'INCOMPLETE')
        self.assertFalse(result['nativeExecutionAdmitted'])
        self.assertEqual(verdict.digest(entry), verdict.ACTIVATION_SHA)
        self.assertEqual(len(result['missingAuthority']), 3)

    def test_current_admission_retains_exact_immutable_review_and_original_activation(self):
        binding = native.source_binding()
        result = verdict.admit_source(binding, verdict.read_review())
        self.assertEqual(result['reviewRecordSha256'], verdict.REVIEW_RECORD_SHA)
        self.assertEqual(binding['originalActivationSha256'], verdict.ACTIVATION_SHA)
        self.assertNotEqual(binding['activationSha256'], verdict.ACTIVATION_SHA)
        self.assertEqual(result['nativeActivation'], 'NOT_RUN')

    def test_retired_conformance_stays_zero_of_fourteen(self):
        import json
        original = json.loads((verdict.REPO / 'evidence/M2/replacement-duration-instrument-v2/conformance.json').read_bytes())
        self.assertFalse(original['complete'])
        self.assertEqual(original['rows'], [])
        self.assertFalse(any(row.get('event') == 'received' for row in original['nativeTranscript']))


if __name__ == '__main__': unittest.main(verbosity=2)
