// Execute the actual qualification module in a synthetic host, never a provider.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const source = fs.readFileSync(process.argv[2], 'utf8');
const sha = text => crypto.createHash('sha256').update(text).digest('hex');
const config = {runId:'synthetic-readiness-run', responderId:'synthetic-readiness-responder', nonce:'synthetic-readiness-nonce',
  activationSha256:sha(source), positiveProbeSha256:sha(fs.readFileSync(require('node:path').join(require('node:path').dirname(process.argv[2]),'m2_socket_probe.ts'))), socketPath:'/synthetic-owned/s.sock',
  entryOutput:'entry', forwardOutput:'forward', resultOutput:'result', errorOutput:'error',
  acknowledgement:{kind:'ACTIVATION_ONLY_READY', nonce:'synthetic-readiness-nonce'}};
config.acknowledgement={kind:'ACTIVATION_ONLY_READY',runId:config.runId,responderId:config.responderId,nonce:config.nonce,activationSha256:config.activationSha256,
  fixtureSha256:sha(fs.readFileSync(require('node:path').join(require('node:path').dirname(process.argv[2]),'m2_activation_readiness.py'))),
  observerSha256:'d461d6d6cb8769dbe65d3e229212f052f5ba0c45a936f92ba76d18b18e88214b',bootId:'synthetic-boot'};
const copy = value => JSON.parse(JSON.stringify(value));
async function execute(mode, text=source) {
  let handler, calls=0, requests=0; const writes={};
  let event={cwd:'/synthetic/event',surface:null,isInteractive:false};
  if (mode==='missing') delete event.cwd;
  if (mode==='undefined') event.cwd=undefined;
  if (mode==='null') event.cwd=null;
  if (mode==='primitive') event.cwd=42;
  if (mode==='unsupported') event.extra='synthetic-extra';
  const result={cwd:'/synthetic/actual-result'};
  const rejection=new Error('synthetic original error');
  const promise=mode==='rejection' ? Promise.reject(rejection) : Promise.resolve(result);
  promise.catch(()=>{});
  const next = incoming => {
    calls++; assert.equal(incoming,event);
    if (mode==='synchronous-error') throw rejection;
    if (mode==='in-place') event.cwd='/synthetic/mutated-after-snapshot';
    return promise;
  };
  const engine={index:2,plugin:'engine',tier:'core',event:'session.start',outcome:'returned',received:copy(event),returned:copy(result),ms:987};
  if (mode==='changed-event') engine.received.cwd='/synthetic/foreign-event';
  if (mode==='changed-result') engine.returned.cwd='/synthetic/foreign-result';
  const observer={...engine,index:1,plugin:'threadspace-observer',tier:'user'};
  next.origin={plugin:'engine',tier:'core'}; next.event='session.start';
  Object.defineProperty(next,'trace',{get:()=> {
    if (mode==='callback-error') throw rejection;
    if (mode==='in-place') return [{...engine,received:copy(event)}];
    if (mode==='self-answer') return [observer];
    return mode==='observer' ? [observer,engine] : [engine];
  }});
  const host={fs:{write:async (path,text)=> {
    if (mode==='write-error' && path==='entry' || mode==='all-writes-error') throw rejection;
    writes[path]=text;
  }},http:{fetch:async (url,init)=> {
    requests++; assert.equal(init.socketPath,config.socketPath); assert.equal(init.method,'GET');
    assert.deepEqual(Object.keys(init).sort(),['method','socketPath']);
    if (mode==='http-error') throw rejection;
    return {ok:true,status:200,text:JSON.stringify(config.acknowledgement)};
  }}};
  const executable=text.replace("import { config } from './config.ts';\n",'')
    .replace('export const register =','globalThis.register =').replaceAll(': any','');
  const context=vm.createContext({config,Promise,Error,JSON}); vm.runInContext(executable,context);
  context.register((name,fn)=>{assert.equal(name,'session.start');assert.equal(handler,undefined);handler=fn;});
  let returned,synchronousOriginalError=false;
  try {returned=handler(host,event,next);} catch(error) {assert.equal(mode,'synchronous-error');assert.equal(error,rejection);synchronousOriginalError=true;}
  for(let i=0;i<35;i++) await Promise.resolve();
  if(mode==='rejection') await assert.rejects(returned,error=>error===rejection);
  if(returned===promise && mode!=='rejection') assert.equal(await returned,result);
  return {mode,calls,requests,returnedOriginalPromise:returned===promise,synchronousOriginalError,
    sourceSha256:sha(text),artifacts:Object.entries(writes).map(([label,raw])=>({label,raw,sha256:sha(raw)}))};
}
(async()=> {
  const results=[];
  for(const mode of ['engine-only','observer','changed-event','in-place','changed-result','self-answer','missing','undefined','null','primitive','unsupported','synchronous-error','rejection','write-error','all-writes-error','callback-error','http-error']) {
    const result=await execute(mode);assert.equal(result.calls,1);
    assert(result.returnedOriginalPromise || result.synchronousOriginalError);results.push(result);
  }
  const duplicate=await execute('engine-only',source.replace('const forwarded = next(e);','next(e); const forwarded = next(e);'));
  assert.equal(duplicate.calls,2);
  const wrapped=await execute('engine-only',source.replace('return forwarded;','return Promise.resolve().then(() => forwarded);'));
  assert.equal(wrapped.returnedOriginalPromise,false);
  process.stdout.write(JSON.stringify({kind:'SYNTHETIC_ACTUAL_MODULE_CONTROLS',sourceSha256:sha(source),config,nativeProviderExecuted:false,results,
    rejectedMutations:[{mutation:'duplicate-next',detected:duplicate.calls!==1,sourceSha256:duplicate.sourceSha256},
      {mutation:'wrapped-promise',detected:!wrapped.returnedOriginalPromise,sourceSha256:wrapped.sourceSha256}]})+'\n');
})().catch(error=>{process.stderr.write(String(error)+'\n');process.exitCode=1;});
