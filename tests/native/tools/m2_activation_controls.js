// Portable execution of the actual activation callback with synthetic host APIs.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync(process.argv[2], 'utf8');
const config = { runId: 'synthetic-run', nonce: 'synthetic-nonce', activationSha256: 'a'.repeat(64),
  positiveProbeSha256: 'p'.repeat(64), responderId: 'synthetic-responder', socketPath: '/synthetic-socket',
  entryOutput: 'entry', forwardOutput: 'forward', resultOutput: 'result', errorOutput: 'error',
  acknowledgement: { kind: 'ACTIVATION_ONLY_READY', nonce: 'synthetic-nonce' } };
assert(source.startsWith("import { config } from './config.ts';\n"));
// Only the one fixture import, export keyword and explicit any annotations differ.
const executable = source.replace("import { config } from './config.ts';\n", '')
  .replace('export const register =', 'globalThis.register =').replaceAll(': any', '');
async function control(mode) {
  let callback; let calls = 0; let httpCalls = 0; const writes = {};
  const event = Object.freeze({ synthetic: true });
  const forwarded = mode === 'downstream' ? Promise.reject(new Error('synthetic downstream failure')) : Promise.resolve(Object.freeze({ synthetic: 'result' }));
  forwarded.catch(() => {});
  const next = e => { calls++; assert.equal(e, event); return forwarded; };
  next.trace = mode === 'callback' ? undefined : [{ index: 1, plugin: 'threadspace-observer', tier: 'wrap', event: 'session.start', outcome: 'returned', ms: 999, received: { private: 'synthetic' } }, { index: 2, plugin: 'engine', tier: 'core', event: 'session.start', outcome: 'returned' }];
  next.origin = { plugin: 'engine', tier: 'core' }; next.event = 'session.start';
  const host = { fs: { write: async (path, text) => {
    if ((mode === 'write' && path === 'entry') || mode === 'all-writes') throw new Error('synthetic write failure');
    writes[path] = JSON.parse(text);
  } }, http: { fetch: async (url, init) => {
    httpCalls++; assert.equal(init.socketPath, config.socketPath); assert.equal(init.method, 'GET');
    assert(url.includes('/ready/synthetic-run/synthetic-responder/synthetic-nonce/'));
    if (mode === 'http') throw new Error('synthetic HTTP failure');
    return { ok: true, status: 200, text: JSON.stringify(mode === 'foreign-ack' ? { kind: 'ACTIVATION_ONLY_READY', nonce: 'foreign' } : config.acknowledgement) };
  } } };
  const context = vm.createContext({ config, Promise, Error, JSON });
  vm.runInContext(executable, context);
  context.register((name, handler) => { assert.equal(name, 'session.start'); assert.equal(callback, undefined); callback = handler; });
  const returned = callback(host, event, next);
  assert.equal(returned, forwarded); assert.equal(calls, 1);
  for (let i = 0; i < 20; i++) await Promise.resolve();
  if (mode === 'positive') {
    assert.equal(writes.result.stage, 'READINESS_READ_BACK'); assert.equal(writes.entry.stage, 'CALLBACK_ENTERED');
    assert.equal(writes.forward.trace.length, 2); assert.equal(httpCalls, 1);
    assert(!JSON.stringify(writes).includes('private')); assert(!JSON.stringify(writes).includes('999'));
  } else if (mode === 'all-writes') { assert.equal(httpCalls, 0); assert.equal(Object.keys(writes).length, 0); }
  else {
    const expected = { downstream: 'DOWNSTREAM_FAILURE', callback: 'CALLBACK_FAILURE', write: 'OUTPUT_WRITE_FAILURE', http: 'HTTP_READINESS_FAILURE', 'foreign-ack': 'HTTP_READINESS_FAILURE' }[mode];
    assert.equal(writes.error.stage, expected); assert.equal(writes.result, undefined);
  }
  return { control: mode, nextCalls: calls, returnedOriginalPromise: returned === forwarded, passed: true };
}
(async () => {
  const results = [];
  for (const mode of ['positive', 'downstream', 'callback', 'write', 'http', 'foreign-ack', 'all-writes']) results.push(await control(mode));
  process.stdout.write(JSON.stringify({ kind: 'PORTABLE_ACTUAL_CALLBACK_CONTROLS', nativeProviderExecuted: false, results }) + '\n');
})().catch(error => { process.stderr.write(String(error) + '\n'); process.exitCode = 1; });
