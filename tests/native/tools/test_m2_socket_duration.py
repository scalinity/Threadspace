import copy
import http.client
import json
import os
import socket
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from m2_socket_duration import *
from m2_socket_runner import new_freeze, start_responder, cleanup, image_identity, await_result, save, start_provider, launch_owned, image_file

def record(name, result):
    if 'M2_CONTROL_OUTPUT' not in os.environ: return
    path=Path(os.environ['M2_CONTROL_OUTPUT'])
    with path.open('a') as output:
        output.write(json.dumps({'control':name,'result':result},sort_keys=True,allow_nan=False)+'\n')
        output.flush(); os.fsync(output.fileno())

def fixture():
    runtimes=[{'condition':c,'runtimeId':('11111111-1111-4111-8111-111111111111' if i==0 else f'00000000-0000-4000-8000-{i+1:012d}'),
        'sourceEpoch':('11111111-1111-4111-8111-111111111111' if i==0 else f'00000000-0000-4000-8000-{i+1:012d}'),
        'process':{'pid':100+i,'startUsec':100+i,'kernelVerified':True,'sha256':PROVIDER_SHA,'version':PROVIDER_VERSION}} for i,c in enumerate(POLICY['conditions'])]
    ids={**source_identities(),'responderExecutableSha256':'a'*64,'bootId':'qualification-boot','osBuild':'test-build',
        'nativeClock':'CLOCK_UPTIME_RAW','observerClock':'performance.now','clockScope':POLICY['clockScope'],
        'runId':'test-run','responderId':'test-responder','runtimes':runtimes,
        'providerImagePolicy':{'path':'/synthetic-provider','sha256':PROVIDER_SHA},
        'responderImagePolicy':{'path':'/synthetic-responder','sha256':'a'*64},
        'responderProcess':{'pid':99,'startUsec':99,'kernelVerified':True,'sha256':'a'*64}}
    for proc in [r['process'] for r in runtimes]+[ids['responderProcess']]:
        proc.update(uid=os.getuid(),parentPid=os.getpid(),imagePath='/synthetic-provider' if proc['pid']!=99 else '/synthetic-responder')
    from test_m2_activation_process import synthetic_receipt
    receipts=[{**synthetic_receipt(r['process'],ids['providerImagePolicy']),'role':'provider'} for r in runtimes]
    receipts.append({**synthetic_receipt(ids['responderProcess'],ids['responderImagePolicy']),'role':'responder'})
    plan=[{'sequence':i,'runtimeId':runtimes[i//28]['runtimeId'],'sourceEpoch':runtimes[i//28]['sourceEpoch'],'challenge':f'{i:064x}'} for i in range(56)]
    freeze={'identities':{k:v for k,v in ids.items() if k not in ('runtimes','responderProcess')},'policy':POLICY,'plan':plan,'runtimePlan':[{k:v for k,v in r.items() if k!='process'} for r in runtimes]}
    frozen=canonical_hash(freeze)
    transcript=[{'event':'start',**{k:ids[k] for k in ('runId','bootId','responderId')},'pid':99,'uid':os.getuid(),'manifestSha256':frozen}]
    rows=[]
    for index in range(14):
        runtime=runtimes[index//7]; start=1000+index*1000; delay=POLICY['requestedDelaysMs'][index%7]
        c=start+1; r=c+delay+1
        row={k:runtime[k] for k in ('condition','runtimeId','sourceEpoch')}
        row.update(requestedDelayMs=delay,readings=[start+i*.001 for i in range(256)]+[c,r])
        ns=[round((v)*1e6)+1000000000000 for v in (c-.0005,c+.0005,r-.0005,r+.0005)]
        for m,name in enumerate(MARKERS):
            seq=index*4+m
            response={**plan[seq],**{k:ids[k] for k in ('runId','bootId','responderId')},'manifestSha256':frozen,'clock':'CLOCK_UPTIME_RAW','nativeNs':str(ns[m])}
            row[name]=response
            transcript.extend([{'event':'received','attempt':seq+1,'sequence':seq,'accepted':True},
                {'event':'generated','attempt':seq+1,'response':response}, {'event':'answered','attempt':seq+1,'sequence':seq}])
        rows.append(row)
    transcript.append({'event':'stop','attempts':56,'generated':56,'answered':56})
    runtime_results=[{'complete':True,'error':None,'wrapper':'() => guarded(helpers.now)()',
        'responses':[rows[index][name] for index in range(i*7,(i+1)*7) for name in MARKERS]} for i in range(2)]
    return {'instrument':POLICY['instrument'],'policy':POLICY,'identities':ids,'freeze':freeze,'freezeSha256':frozen,
        'nativeTranscript':transcript,'transcriptSha256':canonical_hash(transcript),'rows':rows,'complete':True,
        'positiveAttempt':1,'bootBefore':'qualification-boot','bootAfter':'qualification-boot','powerDiscontinuity':False,
        'deadlineExceeded':False,'providerLost':False,'responderLost':False,
        'background':{'ticks':256,'operations':524288,'checksum':1722810369,'complete':True},'executionKind':'SYNTHETIC_CONTROL',
        'socketRemoved':True,'socketOwnership':{'directoryMode':0o700,'socketMode':0o600,'uid':os.getuid()},
        'cleanup':receipts,
        'runtimeResults':runtime_results}

class EvaluatorControls(unittest.TestCase):
    def test_positive_and_offset_invariance(self):
        original=fixture(); shifted=copy.deepcopy(original)
        for row in shifted['rows']:
            row['readings']=[r+1234567 for r in row['readings']]
        a,b=qualify(original),qualify(shifted)
        self.assertTrue(a['qualified'],a['errors']); self.assertTrue(b['qualified'],b['errors'])
        self.assertEqual(a['checks'],b['checks'])
        record('positive-constant-offset-invariance',{'baseline':a,'offset':b})

    def test_clocks_discriminate(self):
        for mode in ('slow','pause','reset','nan','negative','units','coarse','step','sleep'):
            report=fixture()
            if mode=='slow':
                for row in report['rows']: row['readings']=[v*.9 for v in row['readings']]
            if mode=='pause':
                for row in report['rows']: row['readings'][-1]=row['readings'][-2]
            if mode=='reset': report['rows'][0]['readings'][-1]=0
            if mode=='nan': report['rows'][0]['readings'][0]='NaN'
            if mode=='negative': report['rows'][0]['readings'][0]=-1
            if mode=='units':
                for row in report['rows']: row['readings']=[v*1000 for v in row['readings']]
            if mode=='coarse': report['rows'][0]['readings'][:256]=[1000]*256
            if mode=='step': report['rows'][0]['readings']=[1000+i*2 for i in range(256)]+[1600,1601]
            if mode=='sleep': report['powerDiscontinuity']=True
            result=qualify(report)
            self.assertFalse(result['qualified'],mode); self.assertEqual(result['assessedRows'],14)
            record('clock-'+mode,result)

    def test_transcript_and_coverage_discriminate(self):
        for mode in ('first','final','reorder','duplicate','challenge','run','boot','responder','incomplete','lost-provider','lost-responder','deadline','wide','near-zero','boundary'):
            report=fixture()
            if mode=='first': del report['nativeTranscript'][1:4]
            if mode=='final': del report['nativeTranscript'][-4:-1]
            if mode=='reorder': report['nativeTranscript'][2],report['nativeTranscript'][5]=report['nativeTranscript'][5],report['nativeTranscript'][2]
            if mode=='duplicate': report['nativeTranscript'][4]['sequence']=0
            if mode in ('challenge','run','boot','responder'):
                key={'challenge':'challenge','run':'runId','boot':'bootId','responder':'responderId'}[mode]
                report['nativeTranscript'][2]['response'][key]='foreign'
            if mode=='incomplete': report['complete']=False
            if mode=='lost-provider': report['providerLost']=True
            if mode=='lost-responder': report['responderLost']=True
            if mode=='deadline': report['deadlineExceeded']=True
            if mode in ('wide','near-zero','boundary'):
                row=report['rows'][0 if mode!='boundary' else 6]
                row['endUpper']['nativeNs']=str(int(row['endUpper']['nativeNs'])+(20_000_000 if mode!='boundary' else 50_000_000))
            report['transcriptSha256']=canonical_hash(report['nativeTranscript'])
            result=qualify(report)
            self.assertFalse(result['qualified'],mode); self.assertEqual(result['assessedRows'],14)
            record('transcript-coverage-'+mode,result)
        self.assertGreater(qualify(report)['maximumResidualMs'],10)

    def test_every_identity_join(self):
        report=fixture(); profile=make_profile(report); artifact=artifact_identity(report)
        self.assertEqual(validate_binding(report,profile,artifact)[1],[])
        for key in IDENTITY_KEYS:
            p=copy.deepcopy(profile); a=copy.deepcopy(artifact)
            p[key]=a[key]='substituted-together'
            self.assertTrue(validate_binding(report,p,a)[1],key)
            record('identity-'+key,{'errors':validate_binding(report,p,a)[1]})
        for key in source_identities():
            changed=copy.deepcopy(report); changed['identities'][key]='substituted'
            self.assertFalse(qualify(changed)['qualified'],key)
        old=fixture(); old['instrument']='installed-duration-v1'; old['policy']={'maximumEnclosureMs':60}
        self.assertTrue(validate_binding(old,profile,artifact)[1])

class NativeProtocolControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.executable=Path(os.environ['M2_RESPONDER']) if 'M2_RESPONDER' in os.environ else None
        if cls.executable is None: raise unittest.SkipTest('explicit disposable responder build required')

    def exchange(self,root,freeze,sequence=0,**changes):
        ids=freeze['identities']; entry=freeze['plan'][sequence]
        values={**ids,**entry,**changes}
        path='/'+'/'.join(str(values[k]) for k in ('runId','bootId','responderId','runtimeId','sourceEpoch','sequence','challenge'))
        conn=http.client.HTTPConnection('localhost',timeout=2)
        conn.sock=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); conn.sock.settimeout(2); conn.sock.connect(str(root/'s.sock'))
        conn.request('GET',path); response=conn.getresponse(); body=response.read(); conn.close()
        return response.status,json.loads(body)

    def test_real_protocol_rejects_then_accounts_complete(self):
        with tempfile.TemporaryDirectory(prefix='ts-socket-control-') as temp:
            root=Path(temp).resolve(); os.chmod(root,0o700); freeze=new_freeze(self.executable)
            child,identity=start_responder(root,self.executable,freeze)
            try:
                for changes in ({'challenge':'forged'},{'sequence':1},{'runId':'foreign'},{'bootId':'foreign'},
                    {'responderId':'foreign'},{'runtimeId':'foreign'},{'sourceEpoch':'foreign'},{'challenge':'invalid?'}):
                    self.assertEqual(self.exchange(root,freeze,**changes)[0],409,changes)
                for sequence in range(56):
                    self.assertEqual(self.exchange(root,freeze,sequence)[0],200)
                    if sequence==0: self.assertEqual(self.exchange(root,freeze,0)[0],409)
            finally: proof=cleanup(child,identity)
            self.assertTrue(proof['reaped']); self.assertFalse((root/'s.sock').exists())
            log=[json.loads(v) for v in (root/'native.jsonl').read_text().splitlines()]
            self.assertEqual(sum(v['event']=='generated' for v in log),56)
            self.assertEqual(sum(v['event']=='rejected' for v in log),9)
            self.assertEqual(log[-1]['answered'],56)
            record('native-protocol',{'freeze':freeze,'transcript':log,'cleanup':proof,'socketRemoved':not (root/'s.sock').exists()})

    def test_delayed_response_remains_in_native_enclosure(self):
        with tempfile.TemporaryDirectory(prefix='ts-delay-control-') as temp:
            root=Path(temp).resolve(); os.chmod(root,0o700); freeze=new_freeze(self.executable)
            child,identity=start_responder(root,self.executable,freeze,delay=(0,30))
            try: responses=[self.exchange(root,freeze,i)[1] for i in range(4)]
            finally: proof=cleanup(child,identity)
            report=fixture(); row=report['rows'][0]
            # Actual four native timestamps enter the actual evaluator. Delay is never subtracted.
            for name,response in zip(MARKERS,responses): row[name]['nativeNs']=response['nativeNs']
            report['transcriptSha256']=canonical_hash(report['nativeTranscript'])
            result=qualify(report)
            self.assertFalse(result['qualified']); self.assertGreater(result['checks'][0]['enclosureMs'],10)
            record('native-delayed-response',{'responses':responses,'assessment':result,'cleanup':proof})

    def test_actual_external_deadline_and_owned_termination(self):
        with tempfile.TemporaryDirectory(prefix='ts-deadline-control-') as temp:
            root=Path(temp).resolve(); os.chmod(root,0o700); freeze=new_freeze(self.executable)
            child,identity=start_responder(root,self.executable,freeze)
            try:
                result,expired,lost=await_result(child,root/'never-produced.json',seconds=.05)
                self.assertTrue(expired); self.assertFalse(lost)
                forged=copy.deepcopy(identity); forged['startUsec']+=1
                with self.assertRaisesRegex(RuntimeError,'changed process'): cleanup(child,forged)
                self.assertIsNone(child.poll())
            finally: proof=cleanup(child,identity)
            self.assertTrue(proof['reaped'])
            result,expired,lost=await_result(child,root/'never-produced.json',seconds=.05)
            self.assertTrue(lost); self.assertFalse(expired)
            record('native-external-deadline-responder-loss',{'deadlineObserved':True,'lostProcessObserved':lost,'cleanup':proof})

    def test_actual_target_provider_loss_is_observed_and_owned(self):
        with tempfile.TemporaryDirectory(prefix='ts-provider-loss-') as temp:
            root=Path(temp).resolve(); os.chmod(root,0o700)
            # No initialize request or timing markers: a process-loss control only.
            child,identity,out,err=start_provider(root,{})
            try:
                self.assertEqual(identity['sha256'],PROVIDER_SHA)
                proof=cleanup(child,identity)
                self.assertTrue(proof['reaped'])
                _,expired,lost=await_result(child,root/'never-produced.json',seconds=.05)
                self.assertTrue(lost); self.assertFalse(expired)
                record('native-target-provider-loss',{'lostProcessObserved':lost,'cleanup':proof})
            finally:
                if child.poll() is None: cleanup(child,identity)
                out.close(); err.close()

    def test_hung_http_exchange_is_bounded_and_retained(self):
        with tempfile.TemporaryDirectory(prefix='ts-hung-http-') as temp:
            root=Path(temp).resolve(); os.chmod(root,0o700); freeze=new_freeze(self.executable)
            server,server_id=start_responder(root,self.executable,freeze,delay=(0,100))
            entry={**freeze['identities'],**freeze['plan'][0]}
            route='/'.join(str(entry[k]) for k in ('runId','bootId','responderId','runtimeId','sourceEpoch','sequence','challenge'))
            client_receipts=[]
            child,pid=launch_owned(['/usr/bin/curl','--silent','--max-time','2','--unix-socket',str(root/'s.sock'),'http://localhost/'+route],
                Path('/usr/bin/curl'),client_receipts,'client',image_file('/usr/bin/curl'),
                cwd=root,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            try:
                deadline=time.monotonic()+2
                while '"event":"generated"' not in (root/'native.jsonl').read_text() and time.monotonic()<deadline: time.sleep(.001)
                self.assertIn('"event":"generated"',(root/'native.jsonl').read_text())
                _,expired,lost=await_result(child,root/'missing-body.json',seconds=.02)
                self.assertTrue(expired); self.assertFalse(lost)
            finally:
                client_cleanup=cleanup(child,pid); server_cleanup=cleanup(server,server_id)
            log=[json.loads(v) for v in (root/'native.jsonl').read_text().splitlines()]
            self.assertEqual(sum(v['event']=='generated' for v in log),1)
            self.assertLess(log[-1]['answered'],56)
            record('native-hung-http-exchange',{'transcript':log,'deadlineObserved':expired,'clientCleanup':client_cleanup,'responderCleanup':server_cleanup})

if __name__=='__main__': unittest.main(verbosity=2)
