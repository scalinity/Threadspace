// Execute the unchanged activation entry under synthetic, non-measuring APIs.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const raw = fs.readFileSync(process.argv[2]);
const sha = crypto.createHash('sha256').update(raw).digest('hex');
assert.equal(sha, 'ea54a1cbe9253388f6e4a0e2575ee31470746e0c236e2d99ad4e5fc56c273136');
const code = raw.toString().replace("import { config } from './config.ts';\n", '')
  .replace('export const register =', 'globalThis.register =').replaceAll(': any', '');
const config = { runId: 'synthetic-only-run', nonce: 'synthetic-only-nonce',
  activationSha256: sha, positiveProbeSha256: '0e5f94b669ec2936323a9056a545886ee60333d110db2a63cbacf68772d41cfa',
  responderId: 'synthetic-only-responder', socketPath: '/synthetic-only-socket',
  entryOutput: 'entry', forwardOutput: 'forward', resultOutput: 'result', errorOutput: 'error',
  acknowledgement: { kind: 'ACTIVATION_ONLY_READY', nonce: 'synthetic-only-nonce' } };
async function execute(event, result, fullTrace) {
  let handler; let nextCalls = 0; const writes = {};
  const promise = Promise.resolve(result);
  const next = received => { nextCalls++; assert.equal(received, event); return promise; };
  next.origin = { plugin: 'engine', tier: 'core' }; next.event = 'session.start'; next.trace = fullTrace;
  const host = { fs: { write: async (path, bytes) => { writes[path] = bytes; } },
    http: { fetch: async () => ({ ok: true, status: 200, text: JSON.stringify(config.acknowledgement) }) } };
  const ctx = vm.createContext({ config, Promise, Error, JSON });
  vm.runInContext(code, ctx);
  ctx.register((name, callback) => { assert.equal(name, 'session.start'); assert.equal(handler, undefined); handler = callback; });
  const actual = handler(host, event, next);
  assert.equal(actual, promise); assert.equal(nextCalls, 1);
  for (let i = 0; i < 24; i++) await Promise.resolve();
  assert.equal(JSON.parse(writes.result).stage, 'READINESS_READ_BACK');
  return { writes, nextCalls, returnedOriginalPromise: actual === promise };
}
(async () => {
  const e1 = { cwd: '/synthetic-one', surface: null, isInteractive: false };
  const e2 = { ...e1, cwd: '/synthetic-two' };
  const r1 = { cwd: '/synthetic-one' }; const r2 = { cwd: '/synthetic-two' };
  const trace = (e, r) => [{ index: 1, plugin: 'threadspace-observer', tier: 'user', event: 'session.start',
    outcome: 'returned', received: e, returned: r }, { index: 2, plugin: 'engine', tier: 'core',
    event: 'session.start', outcome: 'returned', received: e, returned: r }];
  const first = await execute(e1, r1, trace(e1, r1));
  const changed = await execute(e2, r2, trace(e2, r2));
  assert.notDeepEqual(e1, e2); assert.notDeepEqual(r1, r2);
  assert.deepEqual(first.writes, changed.writes);
  // Below-only trace cannot expose whether a link above ran or was skipped.
  const engineOnly = await execute(e1, r1, trace(e1, r1).slice(1));
  assert.equal(JSON.parse(engineOnly.writes.forward).trace.length, 1);
  assert(!engineOnly.writes.forward.includes('threadspace-observer'));
  process.stdout.write(JSON.stringify({ kind: 'ACTUAL_ENTRY_PROJECTION_AUTHORITY_CONTROL', activationSha256: sha,
    nativeProviderExecuted: false, nativeHttpExecuted: false, nativeClockRead: false,
    distinctEventAndResultValuesProduceIdenticalOutputBytes: true,
    engineOnlyTraceHasNoObserverWitness: true,
    syntheticExactlyOnceAndOriginalPromiseControls: [first, changed, engineOnly].map(({ nextCalls, returnedOriginalPromise }) => ({ nextCalls, returnedOriginalPromise })),
    verdict: 'NATIVE_EVIDENCE_AUTHORITY_INCOMPLETE' }) + '\n');
})().catch(error => { process.stderr.write(String(error) + '\n'); process.exitCode = 1; });
