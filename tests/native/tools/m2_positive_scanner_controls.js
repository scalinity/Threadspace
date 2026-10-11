// Portable execution of source bytes with synthetic APIs and a manual scheduler.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const [candidatePath, referencePath, originalPath] = process.argv.slice(2);
const candidate = fs.readFileSync(candidatePath, 'utf8');
const reference = fs.readFileSync(referencePath, 'utf8');
const original = fs.readFileSync(originalPath, 'utf8');
const hash = text => crypto.createHash('sha256').update(text).digest('hex');

function executable(source) {
  return source.replace(/^import .*;\n/gm, '')
    .replace('export const register =', 'globalThis.register =')
    .replace(/(?:export )?const startBackground =/, 'globalThis.startBackground =')
    .replaceAll(': any[]', '').replaceAll(': number[]', '')
    .replaceAll(': string | null', '').replaceAll(': boolean', '')
    .replaceAll(': any', '').replaceAll(': number', '')
    .replaceAll('new Promise<void>', 'new Promise').replace(')!;', ');');
}

class Scheduler {
  constructor() { this.position = 0; this.queue = []; this.events = []; this.sequence = 0; }
  after(ms, callback) {
    this.events.push({ kind: 'schedule', ms });
    this.queue.push({ due: this.position + ms, sequence: this.sequence++, callback });
  }
  async drive(done) {
    for (let turn = 0; turn < 2000; turn++) {
      for (let micro = 0; micro < 16; micro++) await Promise.resolve();
      if (done()) return;
      this.queue.sort((a, b) => a.due - b.due || a.sequence - b.sequence);
      const timer = this.queue.shift();
      if (!timer) continue; // Cross-context promise continuations may still be queued.
      this.position = timer.due;
      this.events.push({ kind: 'fire', ms: timer.due });
      timer.callback();
    }
    throw new Error('synthetic scheduler operation bound exceeded');
  }
}

async function fixture(source) {
  const scheduler = new Scheduler();
  const context = vm.createContext({ Promise });
  vm.runInContext(executable(source), context);
  let output;
  context.startBackground({ clock: { after: (ms, cb) => scheduler.after(ms, cb) } })
    .then(value => { output = JSON.parse(JSON.stringify(value)); });
  await scheduler.drive(() => output !== undefined);
  return { output, schedule: scheduler.events.filter(e => e.kind === 'schedule').map(e => e.ms),
    virtualCompletion: scheduler.position, remainingTimers: scheduler.queue.length };
}

async function callback(source, condition, foreignResponse = false) {
  const scheduler = new Scheduler();
  const config = { condition, runtimeId: 'synthetic-runtime', sourceEpoch: 'synthetic-epoch',
    runId: 'synthetic-run', bootId: 'synthetic-boot', responderId: 'synthetic-responder',
    freezeSha256: 'synthetic-source-fixture', socketPath: '/synthetic-unused-socket',
    output: 'synthetic-output', firstSequence: 0, delays: [0, 25],
    plan: Array.from({ length: 8 }, (_, sequence) => ({ sequence,
      runtimeId: 'synthetic-runtime', sourceEpoch: 'synthetic-epoch', challenge: 'synthetic-' + sequence })) };
  let handler, nextCalls = 0, readCalls = 0;
  const writes = [], requests = [];
  const event = Object.freeze({ synthetic: true });
  const downstream = Promise.resolve(Object.freeze({ synthetic: 'unchanged-result' }));
  const host = { clock: { after: (ms, cb) => scheduler.after(ms, cb) },
    fs: { write: async (path, data) => { assert.equal(path, config.output); writes.push(JSON.parse(data)); } },
    http: { fetch: async (url, init) => {
      assert.equal(init.socketPath, config.socketPath); assert.equal(init.method, 'GET');
      const entry = config.plan[requests.length];
      assert(entry, 'extra marker');
      assert.equal(url, 'http://localhost/' + [config.runId, config.bootId, config.responderId,
        config.runtimeId, config.sourceEpoch, entry.sequence, entry.challenge].join('/'));
      requests.push(entry.sequence);
      const response = { ...entry, runId: config.runId, bootId: config.bootId, responderId: config.responderId,
        manifestSha256: config.freezeSha256, clock: 'CLOCK_UPTIME_RAW', nativeNs: 'synthetic-not-a-timestamp' };
      if (foreignResponse) response.challenge = 'foreign';
      return { ok: true, status: 200, text: JSON.stringify(response) };
    } } };
  const context = vm.createContext({ config, Promise, Error, JSON,
    performance: { now: function syntheticSourceControl() {} },
    createLatencyMeasurement: () => ({ readCapture: () => {
      scheduler.events.push({ kind: 'read' }); return ++readCalls / 1000;
    } }) });
  if (source.includes("import { startBackground }")) vm.runInContext(executable(reference), context);
  vm.runInContext(executable(source), context);
  context.register((name, cb) => { assert.equal(name, 'session.start'); assert.equal(handler, undefined); handler = cb; });
  const returned = handler(host, event, e => { assert.equal(e, event); nextCalls++; return downstream; });
  assert.equal(nextCalls, 1); assert.equal(returned, downstream);
  await scheduler.drive(() => writes.some(w => w.complete || w.error));
  const result = writes.at(-1);
  if (foreignResponse) {
    assert.equal(result.complete, false); assert.match(result.error, /foreign or reordered/);
    assert.equal(requests.length, 1);
    return { rejectedForeignResponse: true, nextCalls };
  }
  assert.equal(result.complete, true); assert.equal(result.error, null);
  assert.deepEqual(result.rows.map(r => r.requestedDelayMs), config.delays);
  assert.equal(readCalls, 516); assert.deepEqual(requests, [0, 1, 2, 3, 4, 5, 6, 7]);
  for (let row = 0; row < 2; row++) {
    assert.equal(result.rows[row].readings.length, 258);
    for (const [offset, key] of ['captureLower', 'captureUpper', 'endLower', 'endUpper'].entries())
      assert.equal(result.rows[row][key].sequence, row * 4 + offset);
  }
  if (condition === 'background') {
    assert(scheduler.events.findIndex(e => e.kind === 'schedule') < scheduler.events.findIndex(e => e.kind === 'read'),
      'background must start before the first measurement read');
    assert.deepEqual(result.background, (await fixture(reference)).output);
    assert.equal(scheduler.queue.length, 0, 'final completion must await all background work');
  } else assert.equal(result.background, null);
  return { condition, nextCalls, returnedOriginalPromise: true, syntheticRows: 2,
    markerOrder: requests, readingsPerRow: 258, background: result.background };
}

(async () => {
  const expected = await fixture(reference);
  assert.equal(expected.output.ticks, 256); assert.equal(expected.output.operations, 524288);
  assert.equal(expected.output.complete, true); assert.equal(expected.virtualCompletion, 1280);
  assert.deepEqual(expected.schedule, Array(256).fill(5)); assert.equal(expected.remainingTimers, 0);
  assert.deepEqual(await fixture(candidate), expected);
  const mutations = [
    ['tick-count', 'tick < 256', 'tick < 255'],
    ['operations-per-tick', 'i < 2048', 'i < 2047'],
    ['arithmetic', '* 1664525', '* 1664526'],
    ['schedule', 'await wait(5)', 'await wait(4)'],
    ['operation-accounting', 'operations++', 'operations += 2'],
    ['completion-accounting', 'checksum: x, complete: true', 'checksum: x, complete: false'],
  ];
  const rejected = [];
  for (const [name, before, after] of mutations) {
    assert(candidate.includes(before));
    await assert.rejects(async () => assert.deepEqual(await fixture(candidate.replace(before, after)), expected));
    rejected.push(name);
  }
  for (const condition of ['quiet', 'background'])
    assert.deepEqual(await callback(candidate, condition), await callback(original, condition));
  const callbackResults = [await callback(candidate, 'quiet'), await callback(candidate, 'background'),
    await callback(candidate, 'quiet', true)];
  const start = "const background = config.condition === 'background' ? startBackground($) : null;";
  const shifted = candidate.replace(start, 'let background = null;').replace('row.captureLower = await marker();',
    "if (background === null && config.condition === 'background') background = startBackground($);\n          row.captureLower = await marker();");
  await assert.rejects(() => callback(shifted, 'background'), /background must start/);
  await assert.rejects(() => callback(candidate.replace('background ? await background : null', 'null'), 'background'));
  await assert.rejects(() => callback(candidate.replace('const forwarded = next(e);', 'const forwarded = next(e); next(e);'), 'quiet'));
  rejected.push('start-boundary', 'completion-boundary', 'duplicate-forwarding');
  process.stdout.write(JSON.stringify({ kind: 'PORTABLE_SOURCE_BOUND_BACKGROUND_EQUIVALENCE',
    nativeProviderExecuted: false, nativeResponderExecuted: false, nativeClockRead: false,
    candidateSha256: hash(candidate), referenceSha256: hash(reference), originalSha256: hash(original),
    scheduler: 'manual due-time order; deterministic microtask drains; no real timers',
    referenceResult: expected, callbackResults, discriminatingMutationsRejected: rejected }) + '\n');
})().catch(error => { process.stderr.write(String(error.stack || error) + '\n'); process.exitCode = 1; });
