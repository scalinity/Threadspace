"""Portable checks of the actual timing runner's ownership and deadline path."""
import copy
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import m2_activation_process as process
import m2_socket_runner as runner
from m2_socket_duration import qualify, canonical_hash
from test_m2_socket_duration import fixture


class LifecycleControls(unittest.TestCase):
    def test_deadline_is_not_restarted_for_late_complete_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)/'output.json'
            output.write_text(json.dumps({'complete': True, 'rows': []}))
            with patch.object(runner.time, 'monotonic', return_value=21), patch.object(runner.time, 'sleep') as sleep:
                value, expired, lost = runner.await_result(SimpleNamespace(), output, deadline=20)
            self.assertTrue(value['complete']); self.assertTrue(expired); self.assertFalse(lost)
            sleep.assert_not_called()

    def test_frozen_native_image_substitution_rejected_before_spawn(self):
        policy = {'path': '/synthetic-responder', 'sha256': 'a'*64, 'st_ino': 1}
        for key, value in [('path','/foreign'), ('sha256','b'*64), ('st_ino',2)]:
            with patch.object(runner, 'image_file', return_value={**policy,key:value}), patch.object(process.subprocess, 'Popen') as spawn:
                with self.assertRaisesRegex(RuntimeError, 'frozen native executable changed'):
                    runner.launch_owned([], Path(policy['path']), [], 'responder', policy)
                spawn.assert_not_called()

    def test_post_spawn_failure_retains_baseline_and_reaches_actual_cleanup(self):
        policy = {'path': '/synthetic-responder', 'sha256': 'a'*64}
        baseline = {'pid': 444, 'uid': os.getuid(), 'parentPid': os.getpid(), 'startUsec': 123, 'imagePath': policy['path']}
        child = SimpleNamespace(pid=444, returncode=None, stdin=None)
        records=[]
        with patch.object(runner, 'image_file', side_effect=[policy, RuntimeError('controlled image read'), policy]), \
             patch.object(process.subprocess, 'Popen', return_value=child), \
             patch.object(process, 'kernel_snapshot', return_value=baseline), \
             patch.object(process.os, 'waitpid', side_effect=[(0,0),(0,0),(0,0),(444,0)]), \
             patch.object(process.os, 'kill') as signal:
            with self.assertRaisesRegex(RuntimeError, 'controlled image read'):
                runner.launch_owned([], Path(policy['path']), records, 'responder', policy)
        self.assertEqual(records[0]['baseline'], baseline)
        self.assertTrue(records[0]['wait']['reaped']); signal.assert_called_once()
        self.assertIn('executionError', records[0])

    def test_background_accounting_substitution_rejected(self):
        report=fixture(); report['background']=None
        self.assertFalse(qualify(report)['qualified'])
        for key, value in [('ticks',255),('operations',524287),('checksum',0),('complete',False)]:
            report=fixture(); report['background'][key]=value
            self.assertIn('incomplete frozen background fixture',qualify(report)['errors'])

    def test_native_evaluator_rejects_missing_or_forged_wait_authority(self):
        with tempfile.TemporaryDirectory() as directory:
            report = fixture(); report['executionKind']='NATIVE_INSTALLED_PROVIDER'
            prepared=runner.prepare_conditions(Path(directory).resolve(),report['freeze'])
            report['pluginStaging']=[{'condition':config['condition'],'manifest':inputs['plugins'],
                'manifestSha256':canonical_hash(inputs['plugins'])} for _,config,inputs in prepared]
            self.assertTrue(qualify(report)['qualified'])
            for mutation in ('missing-wait','changed-pid','forced-responder','missing-baseline','foreign-policy','reorder'):
                changed=copy.deepcopy(report); receipt=changed['cleanup'][-1]
                if mutation=='missing-wait': receipt.pop('wait')
                if mutation=='changed-pid': receipt['wait']['pid']+=1
                if mutation=='forced-responder': receipt['exitCode']=-9
                if mutation=='missing-baseline': receipt.pop('baseline')
                if mutation=='foreign-policy': receipt['approvedImage']['sha256']='b'*64
                if mutation=='reorder': receipt['observations'].reverse()
                result=qualify(changed)
                self.assertFalse(result['qualified'], mutation)
                self.assertIn('missing exact-image cleanup/wait authority',result['errors'])


if __name__=='__main__': unittest.main(verbosity=2)
