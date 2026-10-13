"""One predeclared signed-helper comparison. Never install or prewarm an arm."""
from __future__ import annotations
import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from m2_f4_startup import native_ns, batch
from m2_f4_phases import account
from m2_socket_runner import save
from m2_activation_process import image_file, wait_exact, verify_cleanup, observe, check_file, terminate_owned
from m2_socket_duration import file_hash, canonical_hash

HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
EPOCH='6d1c7f0e-1111-4aaa-8bbb-000000000001'
OBSERVATION='00000000-0000-4000-8000-000000000001'


def signature(path):
    result=subprocess.run(['/usr/bin/codesign','-d','--verbose=4',str(path)],capture_output=True,text=True,timeout=20)
    if result.returncode: raise RuntimeError('signature inspection failed')
    fields={}
    for line in result.stderr.splitlines():
        if line.startswith(('Identifier=','CodeDirectory ','TeamIdentifier=','CDHash=')):
            key,_,value=line.partition('='); fields[key]=value
    subprocess.run(['/usr/bin/codesign','--verify','--strict',str(path)],check=True,capture_output=True,timeout=20)
    return fields


def acquire(root):
    plan=json.loads((root/'plan.json').read_bytes())
    listing=subprocess.check_output(['/usr/bin/security','find-identity','-v','-p','codesigning'],text=True)
    identity=next((line.split()[1] for line in listing.splitlines() if '"Apple Development' in line),None)
    if identity is None: raise RuntimeError('required signing identity missing')
    (root/'private-signing-identities.txt').write_text(listing)
    request=json.loads(batch(EPOCH,record=True)); request['records'][0]['observationId']=OBSERVATION
    request=json.dumps(request,separators=(',',':')).encode()
    (root/'request.json').write_bytes(request); (root/'publication.key').write_bytes(os.urandom(32))
    artifacts=[]
    for variant in plan['comparison']['variants']:
        for arm,copy in plan['comparison']['order']:
            directory=root/'acquired'/variant/(arm+'-'+str(copy)); directory.mkdir(parents=True,mode=0o700)
            helper=directory/'threadspace-hook'
            source=root/('build-'+variant)/'aarch64-apple-darwin'/arm/'threadspace-hook'
            shutil.copy2(source,helper)
            subprocess.run(['/usr/bin/codesign','--force','--sign',identity,'--options','runtime','--timestamp=none',str(helper)],
                           check=True,capture_output=True,timeout=20)
            artifacts.append({'variant':variant,'arm':arm,'copy':copy,'file':image_file(helper),'signature':signature(helper)})
    if len({(a['file']['st_dev'],a['file']['st_ino']) for a in artifacts})!=8: raise RuntimeError('reused acquired inode')
    for variant in plan['comparison']['variants']:
        selected=[a for a in artifacts if a['variant']==variant]
        if len({a['signature'].get('Identifier') for a in selected})!=1 or len({a['signature'].get('TeamIdentifier') for a in selected})!=1:
            raise RuntimeError('incomparable signing identity')
        if not all(any('runtime' in value for value in a['signature'].values()) for a in selected): raise RuntimeError('missing hardened runtime')
    sources={str(p.relative_to(REPO)):file_hash(p) for p in [Path(__file__),HERE/'m2_f4_startup.py',HERE/'m2_f4_phases.py',HERE/'m2_socket_runner.py',HERE/'m2_activation_process.py',REPO/'Cargo.toml',REPO/'Cargo.lock',REPO/'crates/relay/tests/mod_batch.rs']}
    freeze={'plan':plan,'artifacts':artifacts,'sources':sources,'requestSha256':file_hash(root/'request.json'),
            'bootId':subprocess.check_output(['/usr/sbin/sysctl','-n','kern.bootsessionuuid'],text=True).strip(),
            'osBuild':subprocess.check_output(['/usr/bin/sw_vers','-buildVersion'],text=True).strip(),
            'signingIdentityFingerprint':identity,'acquisitionOrder':list(range(8))}
    save(root/'comparison-freeze.json',freeze)
    return freeze


def measured(root,artifact,call,freeze):
    helper=Path(artifact['file']['path'])
    if image_file(helper)!=artifact['file']: raise RuntimeError('changed acquired helper')
    name=artifact['variant']+'-'+artifact['arm']+'-'+str(artifact['copy'])+'-'+str(call)
    store=root/'stores'/name; store.mkdir(parents=True,mode=0o700)
    request=(root/'request.json').read_bytes()
    env={k:v for k,v in os.environ.items() if not k.startswith('THREADSPACE_')}
    if artifact['variant']=='qualification-startup': env['THREADSPACE_QUALIFY_STARTUP']='1'
    receipt_record={'role':'helper','spawned':False,'reaped':False,'approvedImage':artifact['file'],'observations':[],'signals':[]}
    records=[receipt_record]; child=None; baseline=None
    def pinned_image(path):
        actual=image_file(path)
        if actual!=artifact['file']: raise RuntimeError('changed frozen helper image')
        return actual
    check_file(artifact['file'],receipt_record,'file-before-spawn',pinned_image)
    row={'variant':artifact['variant'],'arm':artifact['arm'],'copy':artifact['copy'],'call':call,
         'firstExecutionOfAcquiredInode':call==0,'file':artifact['file'],'cleanup':records}
    requested=native_ns()
    try:
        argv=[str(helper),'mod-batch','--store-dir',str(store),'--budget-ms','80']
        child=subprocess.Popen(argv,cwd=root,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        spawned=native_ns();receipt_record.update(spawned=True,childPid=child.pid)
        baseline=observe(child,artifact['file'],receipt_record,'launch-baseline');receipt_record['baseline']=baseline
        check_file(artifact['file'],receipt_record,'full-image',pinned_image)
        identity={**baseline,'sha256':artifact['file']['sha256'],'kernelVerified':True};receipt_record['identity']=identity
        child.stdin.write(request);child.stdin.close();child.stdin=None
        terminal=wait_exact(child,records[0],4)
        if terminal is None: raise RuntimeError('external helper deadline')
        stdout=child.stdout.read(65537);stderr=child.stderr.read(65537)
        reaped=native_ns()
        if len(stdout)>65536 or len(stderr)>65536: raise RuntimeError('unbounded helper output')
        row.update(parent={'parentLaunchRequestNs':requested,'successfulSpawnReturnNs':spawned,
                           'parentReapNs':reaped,'clock':'CLOCK_UPTIME_RAW','bootId':freeze['bootId']},
                   ownershipChecksCountedInWallInterval=True,launchToReapMs=(reaped-requested)/1e6,
                   exitCode=terminal['exitCode'],stdout=stdout.decode(),stderr=stderr.decode(),process=identity)
    except Exception as error: row['error']=str(error)
    finally:
        if child is not None:
            try:
                terminate_owned(child,baseline,artifact['file'],receipt_record,pinned_image)
                receipt_record.update(reaped=True,exitCode=receipt_record['wait']['exitCode'])
            except Exception as error: row['cleanupError']=str(error)
            child.stdout.close();child.stderr.close()
        save(root/(name+'.raw.json'),row)
    if row.get('error') or row.get('cleanupError'): return {**row,'semanticPass':False}
    receipt=json.loads(row['stdout']); ready=list((store/'capture-spool/ready').glob('*.json'))
    spool=json.loads(ready[0].read_bytes()) if len(ready)==1 else {}
    proof={'readyCount':len(ready),'observationId':spool.get('observationId'),'sourceEpoch':spool.get('sourceEpoch'),
           'privateSpoolHmacSha256':hmac.new((root/'publication.key').read_bytes(),ready[0].read_bytes(),hashlib.sha256).hexdigest() if len(ready)==1 else None}
    row['spoolProof']=proof; row['receipt']=receipt
    row['semanticPass']=(row['exitCode']==0 and receipt.get('receiptVersion')==1 and len(receipt.get('results',[]))==1
        and receipt['results'][0].get('observationId')==OBSERVATION and receipt['results'][0].get('status')=='LOCAL_SPOOLED'
        and proof['readyCount']==1 and proof['observationId']==OBSERVATION and proof['sourceEpoch']==EPOCH
        and verify_cleanup(records[0],row['process'],orderly=True) and image_file(helper)==artifact['file'])
    if artifact['variant']=='qualification-startup': row.update(account(row['parent'],row['stderr'],freeze['bootId']))
    save(root/(name+'.result.json'),row)
    return row


def numerical_verdict(rows,plan):
    if any(not r.get('semanticPass') for r in rows): return 'NO_GO'
    if any(r['arm']=='capture-compact' and r['launchToReapMs']>=plan['comparison']['wallLimitMs'] for r in rows): return 'NO_GO'
    for variant in plan['comparison']['variants']:
        current=[r for r in rows if r['variant']==variant]
        if len(current)!=8: return 'INCOMPLETE'
        for pair in range(2):
            first={r['arm']:r for r in current if r['copy']==pair and r['call']==0}
            difference=first['release']['launchToReapMs']-first['capture-compact']['launchToReapMs']
            if difference<=plan['comparison']['discriminatingFirstImprovementMs']: return 'INCONCLUSIVE_BENEFIT'
            if variant=='qualification-startup' and first['release']['phaseMs']['launchToMain']<=first['capture-compact']['phaseMs']['launchToMain']:
                return 'INCONCLUSIVE_BENEFIT'
    return 'NUMERICAL_GO_SEMANTIC_CONTROLS_PENDING'


def run(root):
    freeze=json.loads((root/'comparison-freeze.json').read_bytes());plan=freeze['plan']
    if any(file_hash(REPO/name)!=digest for name,digest in freeze['sources'].items()): raise RuntimeError('changed frozen source')
    if file_hash(root/'request.json')!=freeze['requestSha256']: raise RuntimeError('changed frozen input')
    with (root/'comparison-started.json').open('x') as f:
        json.dump({'freezeSha256':canonical_hash(freeze),'attempt':1},f);f.flush();os.fsync(f.fileno())
    rows=[];verdict='INCOMPLETE'
    try:
        for artifact in freeze['artifacts']:
            for call in range(2):
                try: row=measured(root,artifact,call,freeze)
                except Exception as error:
                    row={**{k:artifact[k] for k in ('variant','arm','copy')},'call':call,'semanticPass':False,'error':str(error)}
                rows.append(row)
                verdict=numerical_verdict(rows,plan)
                save(root/'comparison.json',{'freezeSha256':canonical_hash(freeze),'rows':rows,'verdict':verdict})
                if verdict=='NO_GO': return verdict
            # Complete the paired first/repeat grid before an inconclusive stop.
            if len(rows)==8 and verdict=='INCONCLUSIVE_BENEFIT': return verdict
    finally:
        after=subprocess.check_output(['/usr/sbin/sysctl','-n','kern.bootsessionuuid'],text=True).strip()
        if after!=freeze['bootId']: verdict='NO_GO'
        save(root/'comparison.json',{'freezeSha256':canonical_hash(freeze),'rows':rows,'verdict':verdict,'bootAfter':after})
    return verdict


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('mode',choices=['acquire','run']);parser.add_argument('root',type=Path)
    args=parser.parse_args();root=args.root.resolve(strict=True)
    if root.stat().st_uid!=os.getuid() or root.stat().st_mode&0o777!=0o700: raise SystemExit('unowned comparison root')
    print(json.dumps(acquire(root)) if args.mode=='acquire' else run(root))
