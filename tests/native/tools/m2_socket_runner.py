#!/usr/bin/env python3
"""One-attempt, owned-process installed-provider Unix-socket conformance."""
from __future__ import annotations
import argparse
import ctypes
import json
import os
import signal
import subprocess
import time
import uuid
from pathlib import Path
from m2_activation_process import owned_process, image_file, approved_image, wait_exact, observe
from m2_socket_duration import HERE, POLICY, PROVIDER_SHA, PROVIDER_VERSION, canonical_hash, file_hash, source_identities, qualify, make_profile
from m2_socket_plugins import composition_identity, stage_plugins, verify_plugins, json_bytes

PROVIDER = Path('/Users/danny/.local/share/claude/versions/2.1.295')

def save(path, value):
    path = Path(path)
    with path.open('w') as f:
        os.chmod(path, 0o600)
        json.dump(value,f,sort_keys=True,indent=2,allow_nan=False)
        f.write('\n'); f.flush(); os.fsync(f.fileno())

def sysctl(name):
    return subprocess.check_output(['/usr/sbin/sysctl','-n',name],text=True).strip().lower()

def image_identity(child, executable, version=None):
    """Kernel image path and PID birth time, independently of mod-returned labels."""
    lib = ctypes.CDLL('/usr/lib/libproc.dylib')
    buf = ctypes.create_string_buffer(4096)
    if child.poll() is not None or lib.proc_pidpath(child.pid,buf,len(buf)) <= 0: raise RuntimeError('owned child image unavailable')
    kernel_path = Path(os.fsdecode(buf.value)).resolve(strict=True)
    data = ctypes.create_string_buffer(136)
    if lib.proc_pidinfo(child.pid,3,0,data,136) != 136: raise RuntimeError('owned child birth identity unavailable')
    raw = data.raw
    uid = int.from_bytes(raw[20:24],'little')
    ppid = int.from_bytes(raw[16:20],'little')
    seconds = int.from_bytes(raw[120:128],'little'); usec = int.from_bytes(raw[128:136],'little')
    if uid != os.getuid() or ppid != os.getpid() or kernel_path != Path(executable).resolve(): raise RuntimeError('foreign process ownership')
    result = {'pid':child.pid,'uid':uid,'parentPid':ppid,'startUsec':seconds*1000000+usec,
        'imagePath':str(kernel_path),'sha256':file_hash(kernel_path),'kernelVerified':True}
    if version: result['version'] = version
    return result

def cleanup(child, identity):
    if hasattr(child, '_owned_guard'):
        if child._owned_receipt.get('identity') != identity:
            raise RuntimeError('cleanup refused changed process identity')
        guard = child._owned_guard
        if guard is not None:
            child._owned_guard = None
            guard.__exit__(None, None, None)
        return child._owned_receipt
    # Historical non-executing scanner compatibility; positive runs use a guard.
    events = []
    if child.poll() is None:
        current = image_identity(child,identity['imagePath'],identity.get('version'))
        if current != identity: raise RuntimeError('cleanup refused changed process identity')
        child.send_signal(signal.SIGTERM); events.append('verified-child-SIGTERM')
        try: child.wait(timeout=POLICY['cleanupGraceSeconds'])
        except subprocess.TimeoutExpired:
            if image_identity(child,identity['imagePath'],identity.get('version')) != identity: raise RuntimeError('cleanup refused reused PID')
            child.kill(); events.append('verified-child-SIGKILL'); child.wait(timeout=POLICY['cleanupGraceSeconds'])
    else: events.append('already-exited')
    if child.stdin is not None: child.stdin.close()
    return {'identity':identity,'events':events,'exitCode':child.returncode,'reaped':child.poll() is not None}

def child_exited(child):
    if hasattr(child, '_owned_receipt'): return wait_exact(child, child._owned_receipt) is not None
    return child.poll() is not None


def launch_owned(argv, executable, records, role, policy, version=None, **kwargs):
    def pinned_image(path):
        actual = image_file(path)
        if actual != policy: raise RuntimeError('frozen native executable changed')
        if role == 'provider' and actual != approved_image(PROVIDER): raise RuntimeError('unapproved provider image')
        return actual
    guard = owned_process(role, argv, executable, records, version, image_policy=policy,
                          image_checker=pinned_image, **kwargs)
    child, identity = guard.__enter__()
    child._owned_guard = guard; child._owned_receipt = records[-1]
    return child, identity


def await_result(child, output, seconds=POLICY['externalDeadlineSeconds'], deadline=None):
    if deadline is None: deadline = time.monotonic()+seconds
    while time.monotonic()<deadline:
        if output.exists():
            try:
                result=json.loads(output.read_text())
                if result.get('complete') or result.get('error'): return result,False,False
            except ValueError: pass
        if child_exited(child): return {},False,True
        time.sleep(.02)
    try: result=json.loads(output.read_text()) if output.exists() else {}
    except ValueError: result={}
    return result,True,False

def start_responder(root, executable, freeze, delay=None, records=None):
    planfile=root/'plan.tsv'
    with planfile.open('x') as f:
        os.chmod(planfile,0o600)
        for p in freeze['plan']: f.write(f"{p['sequence']} {p['runtimeId']} {p['sourceEpoch']} {p['challenge']}\n")
        f.flush(); os.fsync(f.fileno())
    ids=freeze['identities']
    argv=[str(executable),str(root),ids['runId'],ids['bootId'],ids['responderId'],canonical_hash(freeze),str(planfile)]
    if delay: argv.extend(map(str,delay))
    records = [] if records is None else records
    policy = freeze['identities']['responderImagePolicy']
    child, identity = launch_owned(argv, executable, records, 'responder', policy,
                                  cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        until=time.monotonic()+3
        while not (root/'s.sock').exists() and not child_exited(child) and time.monotonic()<until: time.sleep(.01)
        if not (root/'s.sock').exists(): raise RuntimeError('responder failed to bind')
        st = (root/'s.sock').stat()
        if st.st_mode & 0o777 != 0o600 or st.st_uid != os.getuid(): raise RuntimeError('unprotected socket')
        observe(child, policy, child._owned_receipt, 'fixture-ready', child._owned_receipt['baseline'])
        return child,identity
    except BaseException:
        cleanup(child, identity)
        raise

def provider_argv(root, plugins):
    # The exact 2.1.295 CLI declares --plugin-dir <path> repeatable.
    return [str(PROVIDER),'--print','--input-format','stream-json','--output-format','stream-json','--verbose',
        '--no-session-persistence',*[arg for p in plugins['plugins'] for arg in ('--plugin-dir',p['root'])],
        '--settings',str(root/'config/settings.json'),'--setting-sources','','--strict-mcp-config','--mcp-config','{"mcpServers":{}}']

def prepare_provider(root, config, composition=None):
    """Construct and verify disposable inputs without invoking the provider."""
    root=Path(root).resolve(strict=True)
    composition=composition_identity() if composition is None else composition
    plugins=stage_plugins(root,config,composition)
    cfg=root/'config'; cfg.mkdir(mode=0o700)
    save(cfg/'settings.json',{'hooks':{},'disableAllHooks':False})
    env={k:v for k,v in os.environ.items() if not k.startswith('CLAUDE_CODE_PLUGIN_')}
    env['CLAUDE_CONFIG_DIR']=str(cfg)
    prepared={'plugins':plugins,'argv':provider_argv(root,plugins),'env':env}
    verify_provider(root,config,composition,prepared)
    return prepared

def verify_provider(root, config, composition, prepared):
    verify_plugins(root,config,composition,prepared['plugins'])
    if prepared['argv']!=provider_argv(root,prepared['plugins']): raise RuntimeError('changed provider/plugin loading arguments')
    if prepared['env'].get('CLAUDE_CONFIG_DIR')!=str(root/'config') or any(k.startswith('CLAUDE_CODE_PLUGIN_') for k in prepared['env']):
        raise RuntimeError('unexpected provider plugin/configuration environment')
    cfg=root/'config'
    if cfg.is_symlink() or cfg.stat().st_uid!=os.getuid() or cfg.stat().st_mode&0o777!=0o700:
        raise RuntimeError('unowned or aliased disposable configuration')
    settings=root/'config/settings.json'
    if settings.is_symlink() or settings.stat().st_uid!=os.getuid() or settings.stat().st_mode&0o777!=0o600 or settings.read_bytes()!=json_bytes({'hooks':{},'disableAllHooks':False}):
        raise RuntimeError('changed disposable settings')

def start_provider(root, config, prepared=None, records=None, policy=None):
    root=Path(root).resolve(strict=True)
    prepared=prepare_provider(root,config) if prepared is None else prepared
    verify_provider(root,config,composition_identity(),prepared)
    out=(root/'provider-stdout.log').open('xb'); err=(root/'provider-stderr.log').open('xb')
    os.chmod(root/'provider-stdout.log', 0o600); os.chmod(root/'provider-stderr.log', 0o600)
    records = [] if records is None else records
    try:
        child, identity = launch_owned(prepared['argv'], PROVIDER, records, 'provider',
            policy or approved_image(PROVIDER), PROVIDER_VERSION, cwd=root,env=prepared['env'],
            stdin=subprocess.PIPE,stdout=out,stderr=err,start_new_session=True)
        return child,identity,out,err
    except BaseException:
        out.close(); err.close()
        raise

def new_freeze(executable):
    ids={**source_identities(),'responderExecutableSha256':file_hash(executable),
        'responderImagePolicy':image_file(executable),'providerImagePolicy':approved_image(PROVIDER),
        'osBuild':subprocess.check_output(['/usr/bin/sw_vers','-buildVersion'],text=True).strip(),
        'bootId':sysctl('kern.bootsessionuuid'),'nativeClock':'CLOCK_UPTIME_RAW','observerClock':'performance.now',
        'clockScope':POLICY['clockScope'],'runId':str(uuid.uuid4()),'responderId':str(uuid.uuid4())}
    runtimes=[]
    for condition in POLICY['conditions']:
        epoch=str(uuid.uuid4())
        runtimes.append({'condition':condition,'runtimeId':epoch,'sourceEpoch':epoch})
    plan=[{'sequence':index,'runtimeId':runtimes[index//28]['runtimeId'],'sourceEpoch':runtimes[index//28]['sourceEpoch'],
        'challenge':uuid.uuid4().hex+uuid.uuid4().hex} for index in range(56)]
    return {'policy':POLICY,'identities':ids,'runtimePlan':runtimes,'plan':plan}

def prepare_conditions(root, freeze):
    """All plugin staging must pass before any provider or responder launch."""
    composition=freeze['identities']['pluginComposition']
    if [r['condition'] for r in freeze['runtimePlan']]!=POLICY['conditions']: raise RuntimeError('changed runtime condition order')
    prepared=[]
    for index,runtime in enumerate(freeze['runtimePlan']):
        owned=root/runtime['condition']; owned.mkdir(mode=0o700); output=owned/'host-result.json'
        config={**runtime,**{k:freeze['identities'][k] for k in ('runId','bootId','responderId')},
            'plan':freeze['plan'],'firstSequence':index*28,'delays':POLICY['requestedDelaysMs'],
            'socketPath':str(root/'s.sock'),'freezeSha256':canonical_hash(freeze),'output':str(output)}
        prepared.append((owned,config,prepare_provider(owned,config,composition)))
    return prepared

def run(root, executable, freeze):
    if root.exists(): raise RuntimeError('attempt directory already exists: no retry')
    root.mkdir(mode=0o700)
    save(root/'frozen-manifest.json',freeze)
    if freeze['identities'] != {**new_freeze(executable)['identities'], 'runId':freeze['identities']['runId'],'responderId':freeze['identities']['responderId']}: raise RuntimeError('frozen sources/build/boot changed')
    prepared=prepare_conditions(root,freeze)
    # Version authority is the retained independent 2.1.295 executable audit;
    # exact bytes are reverified here and on every owned process admission.
    if approved_image(PROVIDER)!=freeze['identities']['providerImagePolicy'] or freeze['identities']['providerVersion']!=PROVIDER_VERSION:
        raise RuntimeError('required provider changed')
    sleep_before=sysctl('kern.sleeptime'); wake_before=sysctl('kern.waketime')
    receipts=[]; responder=None
    report={'instrument':POLICY['instrument'],'positiveAttempt':1,'policy':POLICY,'freeze':freeze,'freezeSha256':canonical_hash(freeze),
        'bootBefore':freeze['identities']['bootId'],'identities':{**freeze['identities'],'runtimes':[]},
        'rows':[],'complete':True,'deadlineExceeded':False,'providerLost':False,'responderLost':False,'cleanup':[],
        'executionKind':'NATIVE_INSTALLED_PROVIDER',
        'pluginStaging':[{'condition':config['condition'],'manifest':inputs['plugins'],
            'manifestSha256':canonical_hash(inputs['plugins']),'argv':inputs['argv'],
            'settingsSha256':file_hash(owned/'config/settings.json')}
            for owned,config,inputs in prepared]}
    try:
        responder,rpid=start_responder(root,executable,freeze,records=receipts)
        report['identities']['responderProcess']=rpid
        report['socketOwnership']={'directoryMode':root.stat().st_mode&0o777,'socketMode':(root/'s.sock').stat().st_mode&0o777,'uid':root.stat().st_uid}
        for index,runtime in enumerate(freeze['runtimePlan']):
            owned,config,inputs=prepared[index]; output=owned/'host-result.json'
            deadline=time.monotonic()+POLICY['externalDeadlineSeconds']
            child,pid,out,err=start_provider(owned,config,inputs,receipts,freeze['identities']['providerImagePolicy'])
            try:
                report['identities']['runtimes'].append({**runtime,'process':pid})
                save(root/'process-identities.json',report['identities'])
                child.stdin.write((json.dumps({'type':'control_request','request_id':str(uuid.uuid4()),'request':{'subtype':'initialize'}})+'\n').encode()); child.stdin.flush()
                result,expired,lost=await_result(child,output,deadline=deadline)
                report['rows'].extend(result.get('rows',[]))
                report['complete'] &= result.get('complete') is True
                report['deadlineExceeded'] |= expired; report['providerLost'] |= lost
                report.setdefault('runtimeResults',[]).append(result)
                if runtime['condition']=='background': report['background']=result.get('background')
            finally:
                try: cleanup(child,pid)
                finally: out.close(); err.close()
            if expired or lost or not result.get('complete'): break
    except Exception as error:
        report['complete']=False; report['controllerError']=str(error)
    finally:
        if responder is not None:
            try:
                report['responderLost']=child_exited(responder)
                cleanup(responder,rpid)
            except Exception as error:
                report['complete']=False; report['cleanupError']=str(error)
        report['cleanup']=[c for c in receipts if c['role']=='provider']+[c for c in receipts if c['role']=='responder']
        report['socketRemoved']=not (root/'s.sock').exists()
        report['bootAfter']=sysctl('kern.bootsessionuuid')
        report['powerDiscontinuity']=sysctl('kern.sleeptime')!=sleep_before or sysctl('kern.waketime')!=wake_before
        transcript=root/'native.jsonl'
        report['nativeTranscript']=[json.loads(line) for line in transcript.read_text().splitlines()] if transcript.exists() else []
        report['transcriptSha256']=canonical_hash(report['nativeTranscript'])
        report['rowAccounting']=[{'index':i,'condition':POLICY['conditions'][i//7],'requestedDelayMs':POLICY['requestedDelaysMs'][i%7],
            'disposition':'RETAINED' if i<len(report['rows']) else 'MISSING_AFTER_FAILURE'} for i in range(14)]
        save(root/'conformance.json',report)
    save(root/'assessment.json',qualify(report)); save(root/'profile.json',make_profile(report))
    return report

def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--freeze',type=Path,required=True); parser.add_argument('--responder',type=Path,required=True)
    parser.add_argument('--attempt',type=Path); args=parser.parse_args()
    if args.attempt:
        report=run(args.attempt.resolve(),args.responder.resolve(),json.loads(args.freeze.read_text()))
        print(json.dumps(qualify(report))); return 0 if qualify(report)['qualified'] else 1
    with args.freeze.open('x') as f:
        json.dump(new_freeze(args.responder.resolve()),f,sort_keys=True,indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
    os.chmod(args.freeze,0o600); print('frozen '+canonical_hash(json.loads(args.freeze.read_text())))
    return 0

if __name__=='__main__': raise SystemExit(main())
