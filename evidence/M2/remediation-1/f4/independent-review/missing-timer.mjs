import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { register } from '../../Threadspace-review/packages/provider-mod/hooks/register.ts';
const root = '/workspace/scratch/c8272833f682/Threadspace-review/';
const report = { execution: 'Actual TypeScript, injected missing timer and deterministic helper receipts; NOT native execution', sourceHashes: {}, cases: [] };
for (const name of ['register.ts','delivery.ts','latency.ts','ownership.ts']) report.sourceHashes[name]=createHash('sha256').update(await readFile(root+'packages/provider-mod/hooks/'+name)).digest('hex');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const ok = body => ({exitCode:0,stdout:JSON.stringify(body),stderr:'',isStdoutTruncated:false,isStderrTruncated:false});
const token='44444444-4444-4444-8444-444444444444';
for (const delayed of [false,true]) {
  const handlers = new Map();
  register((name,handler)=>handlers.set(name,handler),{captureArgv:['/owned/helper','mod-batch','--qualification-latency']});
  let nextCalls=0, timerCalls=0, close, settled=false;
  const calls=[];
  const host={clock:{after(){timerCalls++;return undefined;}},process:{run:async(argv,init)=>{
    const body=JSON.parse(init.stdin);calls.push({kind:body.kind??argv[1],timeoutMs:init.timeoutMs});
    if(argv[1]==='mod-batch') { if(delayed) await wait(40);return ok({receiptVersion:1,results:body.records.map(record=>({observationId:record.observationId,status:'COMMITTED'})),qualificationClock:{token}}); }
    if(argv[1]==='latency-samples')return ok({accepted:true});
    const base={kind:'observer-census-receipt',schemaVersion:1,accepted:true,runtimeId:body.runtimeId,censusId:body.censusId};
    if(body.kind==='observer-census-page'){if(delayed)await wait(90);return ok({...base,status:'PAGE_RECORDED',pageIndex:body.pageIndex});}
    if(body.kind==='observer-census-close'){close=body;return ok({...base,status:'CLOSED',token,totalCaptured:body.totalCaptured,observationIdsSha256:body.observationIdsSha256});}
    if(body.kind==='observer-census-confirm')return ok({...base,status:'CONFIRMED',token,totalCaptured:close.totalCaptured,observationIdsSha256:close.observationIdsSha256});
    throw new Error('unexpected fixture call');
  }}};
  const event={sessionId:'native-session',reason:'exit'};const original={originalResult:true};
  const next=Object.assign(async e=>{nextCalls++;assert.equal(e,event);return original;},{origin:{plugin:'engine',tier:'core'},trace:[{plugin:'engine',tier:'core',outcome:'returned'}]});
  const began=performance.now();
  const pending=handlers.get('session.end')(host,event,next).then(value=>{settled=true;assert.equal(value,original);return value;});
  let stillPendingPastOriginalDeadline;
  if(delayed){await wait(110);stillPendingPastOriginalDeadline=!settled;}
  await pending;
  const elapsedMs=performance.now()-began;
  report.cases.push({delayed,timerCalls,returnedTimer:null,nextCalls,originalResultPreserved:true,elapsedMs,stillPendingPastOriginalDeadline,confirmationsSent:calls.filter(call=>call.kind==='observer-census-confirm').length,calls});
}
await writeFile(new URL('./result.json',import.meta.url),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report,null,2));
assert.equal(report.cases[0].confirmationsSent,0,'missing timer must leave census incomplete');
assert.equal(report.cases[1].stillPendingPastOriginalDeadline,false,'optional census must not hold original provider result past one100ms ceiling');
