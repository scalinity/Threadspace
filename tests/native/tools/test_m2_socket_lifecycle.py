"""Portable checks of the actual timing runner's ownership and deadline path."""
import copy
import io
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

    def test_identity_write_failure_reaps_provider_before_any_initialize(self):
        freeze = fixture()['freeze']
        policies = {role: freeze['identities'][role+'ImagePolicy'] for role in ('provider','responder')}
        children = {role: SimpleNamespace(pid=pid, returncode=None, stdin=io.BytesIO())
                    for role, pid in (('responder',444),('provider',445))}
        kernels = {child.pid: {'pid':child.pid, 'uid':os.getuid(), 'parentPid':os.getpid(),
            'startUsec':child.pid, 'imagePath':policies[role]['path']} for role,child in children.items()}
        stopped = set(); logs = [io.BytesIO(), io.BytesIO()]; original_save = runner.save
        initialized = []
        def failed_save(path, value):
            if Path(path).name == 'process-identities.json':
                initialized.append(children['provider'].stdin.getvalue())
                raise OSError('controlled identity-write failure')
            original_save(path, value)
        def spawn(argv, **kwargs):
            return children['provider' if argv[0] == policies['provider']['path'] else 'responder']
        def start(root, executable, frozen, records):
            socket = root/'s.sock'; socket.touch(mode=0o600)
            return runner.launch_owned([str(executable)], executable, records, 'responder', policies['responder'])
        def provider(root, config, prepared, records, policy):
            child, identity = runner.launch_owned([policy['path']], Path(policy['path']), records, 'provider', policy)
            return child, identity, *logs
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(runner, 'new_freeze', return_value=freeze), \
             patch.object(runner, 'approved_image', return_value=policies['provider']), \
             patch.object(runner, 'image_file', side_effect=lambda path: next(p for p in policies.values() if p['path']==str(path))), \
             patch.object(runner, 'sysctl', return_value=freeze['identities']['bootId']), \
             patch.object(runner, 'start_responder', side_effect=start), \
             patch.object(runner, 'start_provider', side_effect=provider) as start_mock, \
             patch.object(runner, 'save', side_effect=failed_save), \
             patch.object(process.subprocess, 'Popen', side_effect=spawn) as spawn_mock, \
             patch.object(process, 'kernel_snapshot', side_effect=lambda pid: kernels[pid]), \
             patch.object(process.os, 'waitpid', side_effect=lambda pid, flags: (pid,0) if pid in stopped else (0,0)), \
             patch.object(process.os, 'kill', side_effect=lambda pid, signal: stopped.add(pid)):
            try:
                report = runner.run(Path(directory).resolve()/'attempt', Path(policies['responder']['path']), freeze)
                self.assertFalse(report['complete'])
                self.assertEqual(report['controllerError'], 'controlled identity-write failure')
                self.assertEqual(initialized, [b'']); self.assertEqual(report['rows'], [])
                self.assertEqual(start_mock.call_count, 1); self.assertEqual(spawn_mock.call_count, 2)
                self.assertEqual([r['role'] for r in report['cleanup']], ['provider','responder'])
                for receipt in report['cleanup']:
                    self.assertTrue(process.verify_cleanup(receipt, receipt['identity']))
                    self.assertEqual(receipt['wait']['pid'], receipt['childPid'])
                self.assertTrue(all(stream.closed for stream in logs))
            finally:
                # Also close simulated guards when exercising the old leaking mutation.
                for child in children.values():
                    if getattr(child, '_owned_guard', None) is not None:
                        runner.cleanup(child, child._owned_receipt['identity'])
                for stream in logs: stream.close()

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
