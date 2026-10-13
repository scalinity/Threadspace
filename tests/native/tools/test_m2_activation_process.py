"""Discriminating process-state controls; no native processes are created."""
import copy
import os
from pathlib import Path
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import m2_activation_process as process


def synthetic_receipt(identity, policy):
    baseline = {k: identity[k] for k in ('pid', 'uid', 'parentPid', 'startUsec', 'imagePath')}
    wait = {'pid': identity['pid'], 'waitStatus': 0, 'exitCode': 0, 'reaped': True}
    return {'role': 'synthetic', 'approvedImage': policy, 'identity': identity, 'baseline': baseline,
        'reaped': True, 'exitCode': 0, 'wait': wait, 'signals': [], 'observations': [
            {'phase': 'file-before-spawn', 'file': policy, 'admitted': True},
            {'phase': 'launch-baseline', 'kernel': baseline, 'admitted': True},
            {'phase': 'full-image', 'file': policy, 'admitted': True}, {'phase': 'terminal-wait', **wait}]}


class OwnershipControls(unittest.TestCase):
    def setUp(self):
        self.policy = {'path': str(process.RESPONDER), 'sha256': process.RESPONDER_SHA, 'st_ino': 123}
        self.baseline = {'pid': 444, 'uid': os.getuid(), 'parentPid': os.getpid(), 'startUsec': 9876, 'imagePath': str(process.RESPONDER)}
        self.child = SimpleNamespace(pid=444, returncode=None, stdin=None)
        self.receipt = {'observations': [], 'signals': []}

    def test_all_identity_mutations_refuse_before_signal_and_retain_snapshot(self):
        for key, value in [('pid', 445), ('uid', os.getuid()+1), ('parentPid', os.getpid()+1), ('startUsec', 0), ('startUsec', -1), ('startUsec', None), ('startUsec', 9999), ('imagePath', '/foreign')]:
            changed = {**self.baseline, key: value}
            with self.subTest(key=key,value=value), patch.object(process, 'kernel_snapshot', return_value=changed), \
                    patch.object(process.os, 'waitpid', return_value=(0,0)), patch.object(process.os, 'kill') as signal:
                with self.assertRaises(RuntimeError): process.terminate_owned(self.child, self.baseline, self.policy, self.receipt)
                signal.assert_not_called(); self.assertEqual(self.receipt['observations'][-1]['kernel'], changed)

    def test_no_baseline_never_adopts_cleanup_snapshot(self):
        with patch.object(process.os, 'waitpid', return_value=(0,0)), patch.object(process.os, 'kill') as signal:
            with self.assertRaisesRegex(RuntimeError, 'no authoritative'): process.terminate_owned(self.child,None,self.policy,self.receipt)
            signal.assert_not_called()

    def test_wait_echild_cannot_manufacture_reaping(self):
        with patch.object(process.os, 'waitpid', side_effect=ChildProcessError()):
            with self.assertRaisesRegex(RuntimeError,'ECHILD'): process.wait_exact(self.child,self.receipt)
        self.assertNotIn('wait',self.receipt)

    def test_exact_wait_retained_and_reused_without_new_wait(self):
        with patch.object(process.os, 'waitpid', return_value=(444,0)) as wait:
            first=process.wait_exact(self.child,self.receipt);second=process.wait_exact(self.child,self.receipt)
            self.assertEqual(first,second);wait.assert_called_once();self.assertTrue(first['reaped'])

    def test_exit_before_or_between_checks_reaped_without_signal(self):
        for schedule in [[(444,0)],[(0,0),(444,0)],[(0,0),(0,0),(444,0)]]:
            with patch.object(process.os,'waitpid',side_effect=schedule), patch.object(process,'kernel_snapshot',return_value=self.baseline), \
                 patch.object(process,'approved_image',return_value=self.policy),patch.object(process.os,'kill') as signal:
                receipt={'observations':[],'signals':[]};process.terminate_owned(self.child,self.baseline,self.policy,receipt)
                signal.assert_not_called();self.assertTrue(receipt['wait']['reaped'])

    def test_unreadable_kernel_live_refuses_signal(self):
        with patch.object(process.os,'waitpid',return_value=(0,0)), patch.object(process,'kernel_snapshot',side_effect=RuntimeError('unreadable')),patch.object(process.os,'kill') as signal:
            with self.assertRaises(RuntimeError):process.terminate_owned(self.child,self.baseline,self.policy,self.receipt)
            signal.assert_not_called();self.assertIn('error',self.receipt['observations'][0])

    def test_substituted_file_and_jointly_relabeled_policy_rejected(self):
        foreign={**self.policy,'sha256':'foreign'}
        with patch.object(process,'image_file',return_value=foreign):
            with self.assertRaisesRegex(RuntimeError,'unapproved'):process.check_file(foreign,self.receipt,'test')
        with patch.object(process,'approved_image',return_value={**self.policy,'st_ino':124}):
            with self.assertRaisesRegex(RuntimeError,'substituted'):process.check_file(self.policy,self.receipt,'test')

    def test_initial_full_image_failure_uses_retained_baseline_and_actual_cleanup(self):
        records=[];files=[self.policy,RuntimeError('injected hash failure'),self.policy]
        with patch.object(process.subprocess,'Popen',return_value=self.child),patch.object(process,'kernel_snapshot',return_value=self.baseline), \
             patch.object(process,'approved_image',side_effect=files),patch.object(process.os,'waitpid',side_effect=[(0,0),(0,0),(0,0),(444,0)]),patch.object(process.os,'kill') as signal:
            with self.assertRaisesRegex(RuntimeError,'injected hash failure'):
                with process.owned_process('responder',[],process.RESPONDER,records,image_policy=self.policy):pass
        signal.assert_called_once_with(444,process.signal.SIGTERM)
        self.assertTrue(records[0]['reaped']);self.assertEqual(records[0]['baseline'],self.baseline)
        self.assertIn('executionError',records[0]);self.assertFalse(process.verify_cleanup(records[0],None,True))

    def test_no_launch_baseline_leaves_explicit_cleanup_failure(self):
        records=[]
        with patch.object(process.subprocess,'Popen',return_value=self.child),patch.object(process,'kernel_snapshot',side_effect=RuntimeError('no baseline')), \
             patch.object(process,'approved_image',return_value=self.policy),patch.object(process.os,'waitpid',return_value=(0,0)),patch.object(process.os,'kill') as signal:
            with self.assertRaisesRegex(RuntimeError,'cleanup incomplete'):
                with process.owned_process('responder',[],process.RESPONDER,records,image_policy=self.policy):pass
        signal.assert_not_called();self.assertFalse(records[0]['reaped']);self.assertIn('error',records[0])

    def test_escalation_image_failure_cannot_send_sigkill(self):
        count=0
        def snapshot(pid):
            nonlocal count
            count+=1
            if count==3:raise RuntimeError('escalation identity unavailable')
            return self.baseline
        with patch.object(process,'wait_exact',return_value=None),patch.object(process,'kernel_snapshot',side_effect=snapshot), \
             patch.object(process,'approved_image',return_value=self.policy),patch.object(process.os,'kill') as signal:
            with self.assertRaisesRegex(RuntimeError,'escalation'):process.terminate_owned(self.child,self.baseline,self.policy,self.receipt)
        signal.assert_called_once_with(444,process.signal.SIGTERM)

    def test_orderly_verifier_rejects_missing_reordered_recovery_and_forced_exit(self):
        identity={**self.baseline,'sha256':self.policy['sha256'],'kernelVerified':True}
        baseline=synthetic_receipt(identity,self.policy)
        self.assertTrue(process.verify_cleanup(baseline,identity,True))
        mutations=[lambda r:r['observations'].pop(1),lambda r:r['observations'].reverse(),lambda r:r['observations'].append(r['observations'][-1]),
                   lambda r:r.update(signals=['SIGKILL']),lambda r:r['wait'].update(exitCode=-9),lambda r:r.update(reaped=False),lambda r:r.pop('wait'),
                   lambda r:r.update(recovery={'pidAbsent':True}),lambda r:r['observations'][1]['kernel'].update(startUsec=0)]
        for mutate in mutations:
            value=copy.deepcopy(baseline);mutate(value)
            if 'recovery' in value: value.pop('wait');value['observations'].pop()
            self.assertFalse(process.verify_cleanup(value,identity,True))


if __name__=='__main__':unittest.main(verbosity=2)
