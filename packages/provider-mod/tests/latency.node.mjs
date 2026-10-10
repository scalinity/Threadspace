// Pure execution of the actual qualification collector and delivery adapter.
// The subprocess responder below is a fault fixture, not native evidence.
import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createLatencyMeasurement, MEASUREMENT_LIMITS } from '../hooks/latency.ts';
import { createDelivery } from '../hooks/delivery.ts';
import { register } from '../hooks/register.ts';

const runtime = '11111111-1111-4111-8111-111111111111';
const token = '44444444-4444-4444-8444-444444444444';
const argv = ['/owned/threadspace-hook', 'mod-batch', '--agent', 'ai.scalinity.threadspace.dev.agent', '--qualification-latency'];
const id = index => `22222222-2222-4222-8222-${index.toString(16).padStart(12, '0')}`;
const result = body => ({ exitCode: 0, stdout: JSON.stringify(body), stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const batchResult = result({ qualificationClock: { token } });
const ticks = async () => { for (let n = 0; n < 5; n += 1) await new Promise(resolve => setImmediate(resolve)); };
const retain = (name, value) => {
  if (process.env.THREADSPACE_CENSUS_EVIDENCE_DIR) writeFileSync(join(process.env.THREADSPACE_CENSUS_EVIDENCE_DIR, name), JSON.stringify({ executionKind: 'NODE_FAULT_FIXTURE', nativeExecution: false, ...value }, null, 2) + '\n', { flag: 'wx' });
};

function backend(change = async () => undefined) {
  const calls = [];
  const saved = [];
  const pages = new Map();
  const closes = new Map();
  const confirmations = [];
  const timers = [];
  const io = {
    after: (_ms, callback) => { const timer = { ms: _ms, callback, cancelled: false, cancel() { this.cancelled = true; } }; timers.push(timer); return timer; },
    run: async (actualArgv, init) => {
      const body = JSON.parse(init.stdin);
      const call = { argv: actualArgv, init, body };
      calls.push(call);
      const overridden = await change(call, { calls, saved, pages, closes, confirmations });
      if (overridden !== undefined) return overridden;
      if (actualArgv[1] === 'mod-batch') return result({ receiptVersion: 1, results: body.records.map(record => ({ observationId: record.observationId, status: 'COMMITTED' })), qualificationClock: { token } });
      if (body.kind === 'observer-clock-bracket') { saved.push(body); return result({ accepted: true }); }
      const response = { kind: 'observer-census-receipt', schemaVersion: 1, accepted: true, runtimeId: body.runtimeId, censusId: body.censusId };
      if (body.kind === 'observer-census-page') {
        pages.set(`${body.censusId}:${body.pageIndex}`, body);
        saved.push(body);
        return result({ ...response, status: 'PAGE_RECORDED', pageIndex: body.pageIndex });
      }
      if (body.kind === 'observer-census-close') {
        const captures = Array.from({ length: body.pageCount }, (_, index) => pages.get(`${body.censusId}:${index}`)?.records).flat();
        const ids = captures.map(record => record?.observationId);
        const digest = createHash('sha256').update(ids.join('\n')).digest('hex');
        if (body.overflow !== 0 || captures.some(record => record === undefined) || ids.length !== body.totalCaptured || digest !== body.observationIdsSha256) return result({ accepted: false });
        closes.set(body.censusId, { body, token });
        saved.push(body);
        return result({ ...response, status: 'CLOSED', token, totalCaptured: body.totalCaptured, observationIdsSha256: digest });
      }
      if (body.kind === 'observer-census-confirm') {
        const close = closes.get(body.censusId);
        if (!close || close.token !== body.token || close.body.observationIdsSha256 !== body.observationIdsSha256) return result({ accepted: false });
        confirmations.push(body);
        saved.push(body);
        return result({ ...response, status: 'CONFIRMED', token: body.token, totalCaptured: close.body.totalCaptured, observationIdsSha256: body.observationIdsSha256 });
      }
      throw new Error('unexpected fixture command');
    },
  };
  return { io, calls, saved, pages, closes, confirmations, timers };
}

function collector() { return createLatencyMeasurement(argv, runtime); }
async function sample(measurement, fixture, ids, status = 'COMMITTED') {
  await measurement.end(fixture.io, measurement.begin(ids), batchResult, new Map(ids.map(value => [value, status])));
}

// All globals are restored before each synchronous test finishes. Node's
// top-level tests run serially; none use concurrency:true.
test('qualification remains opt-in and validates the configured runtime/command', () => {
  assert.equal(createLatencyMeasurement(undefined, runtime), undefined);
  assert.equal(createLatencyMeasurement(argv.filter(part => part !== '--qualification-latency'), runtime), undefined);
  assert.equal(createLatencyMeasurement([argv[0], 'hook', '--qualification-latency'], runtime), undefined);
  assert.equal(createLatencyMeasurement(argv, 'not-a-runtime'), undefined);
});

test('accepted, spooled, rejected, duplicate and never-drained captures all remain in final ledger', async () => {
  const measurement = collector();
  const fixture = backend();
  for (const value of [id(0), id(1), id(2), id(3), id(0)]) measurement.capture(value);
  await sample(measurement, fixture, [id(0)], 'COMMITTED');
  await sample(measurement, fixture, [id(1)], 'LOCAL_SPOOLED');
  await sample(measurement, fixture, [id(2)], 'NOT_ACCEPTED');
  await measurement.finalize(fixture.io);
  const close = fixture.saved.find(body => body.kind === 'observer-census-close');
  assert.ok(close, 'all captured UUIDs must be retained before a native close can be accepted');
  assert.equal(close.totalCaptured, 4);
  assert.equal(close.failedExports, 0);
  assert.deepEqual([...fixture.pages.values()].flatMap(page => page.records.map(record => record.observationId)), [id(0), id(1), id(2), id(3)]);
  assert.equal(fixture.confirmations.length, 1);
  await measurement.finalize(fixture.io);
  assert.equal(fixture.confirmations.length, 1, 'one immutable final export only');
});

test('stable UUID retries do not duplicate the closed capture population', async () => {
  const measurement = collector();
  const fixture = backend();
  measurement.capture(id(0));
  await sample(measurement, fixture, [id(0)], 'NOT_ACCEPTED');
  await sample(measurement, fixture, [id(0)], 'ALREADY_COMMITTED');
  await measurement.finalize(fixture.io);
  assert.equal(fixture.saved.filter(body => body.kind === 'observer-clock-bracket').length, 2);
  assert.equal(fixture.saved.find(body => body.kind === 'observer-census-close').totalCaptured, 1);
});

test('lost last sample cannot reuse an earlier zero-failure prefix as the final census', async () => {
  let lose = false;
  const fixture = backend(async call => { if (lose && call.body.kind === 'observer-clock-bracket') throw new Error('last sample lost'); });
  const measurement = collector();
  measurement.capture(id(0));
  await sample(measurement, fixture, [id(0)]);
  lose = true;
  measurement.capture(id(1));
  await sample(measurement, fixture, [id(1)], 'NOT_ACCEPTED');
  await measurement.finalize(fixture.io);
  const samples = fixture.saved.filter(body => body.kind === 'observer-clock-bracket');
  const close = fixture.saved.find(body => body.kind === 'observer-census-close');
  assert.equal(samples.length, 1);
  assert.equal(samples[0].totalCaptured, 1);
  assert.equal(samples[0].failedExports, 0);
  assert.equal(close.totalCaptured, 2);
  assert.equal(close.failedExports, 1);
  assert.equal([...fixture.pages.values()].flatMap(page => page.records).length, 2);
  retain('node-lost-final-sample.json', { actualCaptureCalls: 2, persistedSamples: samples, finalCensus: close,
    censusPages: [...fixture.pages.values()], finalConfirmations: fixture.confirmations });
});

test('failed final page, close or confirmation leaves no final native confirmation', async () => {
  for (const kind of ['observer-census-page', 'observer-census-close', 'observer-census-confirm']) {
    const fixture = backend(async call => { if (call.body.kind === kind) throw new Error(`lost ${kind}`); });
    const measurement = collector();
    measurement.capture(id(0));
    await sample(measurement, fixture, [id(0)]);
    await assert.doesNotReject(measurement.finalize(fixture.io));
    assert.equal(fixture.confirmations.length, 0, kind);
  }
});

test('malformed, wrong-runtime, wrong-digest, truncated or nonzero close receipts cannot trigger confirmation', async () => {
  for (const wrong of ['runtime', 'digest', 'kind', 'truncated', 'exit', 'json']) {
    const fixture = backend(async call => {
      if (call.body.kind !== 'observer-census-close') return;
      const fake = result({ kind: 'observer-census-receipt', schemaVersion: 1, accepted: true, status: 'CLOSED',
        runtimeId: runtime, censusId: call.body.censusId, token, totalCaptured: call.body.totalCaptured, observationIdsSha256: call.body.observationIdsSha256 });
      const parsed = JSON.parse(fake.stdout);
      if (wrong === 'runtime') parsed.runtimeId = id(99);
      if (wrong === 'digest') parsed.observationIdsSha256 = '0'.repeat(64);
      if (wrong === 'kind') parsed.kind = 'capture-receipt';
      fake.stdout = wrong === 'json' ? '{' : JSON.stringify(parsed);
      if (wrong === 'truncated') fake.isStdoutTruncated = true;
      if (wrong === 'exit') fake.exitCode = 1;
      return fake;
    });
    const measurement = collector();
    measurement.capture(id(0));
    await sample(measurement, fixture, [id(0)]);
    await measurement.finalize(fixture.io);
    assert.equal(fixture.calls.filter(call => call.body.kind === 'observer-census-confirm').length, 0, wrong);
  }
});

test('fabricated positive close reply without native close state fails its challenge confirmation', async () => {
  const fixture = backend(async call => call.body.kind === 'observer-census-close' ? result({
    kind: 'observer-census-receipt', schemaVersion: 1, accepted: true, status: 'CLOSED', runtimeId: runtime,
    censusId: call.body.censusId, token, totalCaptured: call.body.totalCaptured, observationIdsSha256: call.body.observationIdsSha256,
  }) : undefined);
  const measurement = collector();
  measurement.capture(id(0));
  await sample(measurement, fixture, [id(0)]);
  await measurement.finalize(fixture.io);
  assert.equal(fixture.calls.filter(call => call.body.kind === 'observer-census-confirm').length, 1);
  assert.equal(fixture.confirmations.length, 0, 'an echoed fabricated token was never persisted by native helper');
});

test('overlapping telemetry exports stay single-flight and retain all omitted-count failures', async () => {
  let release;
  const blocked = new Promise(resolve => { release = resolve; });
  const fixture = backend(async call => { if (call.body.kind === 'observer-clock-bracket') await blocked; });
  const measurement = collector();
  measurement.capture(id(0));
  const first = sample(measurement, fixture, [id(0)]);
  measurement.capture(id(1));
  await sample(measurement, fixture, [id(1)]);
  measurement.capture(id(2));
  await sample(measurement, fixture, [id(2)]);
  assert.equal(fixture.calls.length, 1, 'a skipped export cannot clear another export in flight');
  release();
  await first;
  await measurement.finalize(fixture.io);
  const close = fixture.saved.find(body => body.kind === 'observer-census-close');
  assert.equal(close.totalCaptured, 3);
  assert.equal(close.failedExports, 2);
});

test('an in-flight batch or a callback racing census pages invalidates final closure', async () => {
  const measurement = collector();
  const fixture = backend();
  measurement.capture(id(0));
  measurement.begin([id(0)]);
  await measurement.finalize(fixture.io);
  assert.equal(fixture.calls.length, 0, 'pending real delivery cannot be sealed');
  const racing = collector();
  const race = backend(async call => { if (call.body.kind === 'observer-census-page') racing.capture(id(9)); });
  racing.capture(id(0));
  await sample(racing, race, [id(0)]);
  await racing.finalize(race.io);
  assert.equal(race.calls.filter(call => call.body.kind === 'observer-census-close').length, 0);
  assert.equal(race.confirmations.length, 0);
});

test('ledger and page bounds are explicit and overflow cannot seal as a smaller population', async () => {
  const fixture = backend();
  const measurement = collector();
  for (let index = 0; index <= MEASUREMENT_LIMITS.captures; index += 1) measurement.capture(id(index));
  await sample(measurement, fixture, [id(0)]);
  await measurement.finalize(fixture.io);
  const close = fixture.calls.find(call => call.body.kind === 'observer-census-close').body;
  assert.equal(fixture.pages.size, 32);
  assert.equal([...fixture.pages.values()].flatMap(page => page.records).length, 4096);
  assert.equal(close.totalCaptured, 4097);
  assert.equal(close.overflow, 1);
  assert.equal(fixture.confirmations.length, 0);
  assert.ok(fixture.calls.every(call => call.init.timeoutMs === 100));
  assert.ok([...fixture.pages.values()].every(page => page.records.length <= 128));
});

test('the bounded close deadline aborts a slow export without provider rejection', async () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'performance');
  let clock = 1;
  Object.defineProperty(globalThis, 'performance', { configurable: true, value: { now: () => clock } });
  try {
    const measurement = collector();
    const fixture = backend(async call => { if (call.body.kind === 'observer-census-page') clock += 1001; });
    measurement.capture(id(0));
    await sample(measurement, fixture, [id(0)]);
    await assert.doesNotReject(measurement.finalize(fixture.io));
    assert.equal(fixture.calls.filter(call => call.body.kind === 'observer-census-close').length, 0);
  } finally { Object.defineProperty(globalThis, 'performance', descriptor); }
});

test('real delivery timeout retains the same UUID and measurement cannot manufacture acceptance', async () => {
  let tries = 0;
  const fixture = backend(async call => { if (call.argv[1] === 'mod-batch' && tries++ === 0) throw new Error('actual helper timed out'); });
  const measurement = collector();
  const delivery = createDelivery(argv, runtime, measurement);
  delivery.bind(fixture.io);
  measurement.capture(id(0));
  delivery.enqueue(id(0), { observationId: id(0) }, false);
  fixture.timers.shift().callback();
  await ticks();
  fixture.timers.shift().callback();
  await ticks();
  const batches = fixture.calls.filter(call => call.argv[1] === 'mod-batch');
  assert.equal(batches.length, 2);
  assert.equal(batches[0].body.records[0].observationId, batches[1].body.records[0].observationId);
  assert.equal(fixture.saved.filter(body => body.kind === 'observer-clock-bracket')[0].records[0].status, 'UNKNOWN');
  await delivery.drainOnEnd();
  assert.equal(fixture.calls.filter(call => call.argv[1] === 'mod-batch').length, 2, 'only actual COMMITTED removed the queued record');
  await measurement.finalize(fixture.io);
  assert.equal(fixture.saved.find(body => body.kind === 'observer-census-close').totalCaptured, 1);
});

test('optional measurement throwing leaves successful canonical delivery behavior unchanged', async () => {
  const fixture = backend();
  const measurement = { capture() { throw new Error('capture fault'); }, begin() { throw new Error('clock fault'); }, end() { throw new Error('export fault'); }, finalize() { throw new Error('close fault'); } };
  const delivery = createDelivery(argv, runtime, measurement);
  delivery.bind(fixture.io);
  delivery.enqueue(id(0), { observationId: id(0) }, false);
  await delivery.drainOnEnd();
  await delivery.drainOnEnd();
  assert.equal(fixture.calls.filter(call => call.argv[1] === 'mod-batch').length, 1);
});

function sessionEnd(fixture, origin = { plugin: 'engine', tier: 'core' }, reason = 'exit') {
  const handlers = new Map();
  register((name, handler) => handlers.set(name, handler), { captureArgv: argv });
  const event = { sessionId: 'native-session', reason };
  const original = { originalResult: true };
  let calls = 0;
  const next = Object.assign(async actual => { calls += 1; assert.equal(actual, event); return original; }, {
    origin, trace: [{ plugin: 'engine', tier: 'core', outcome: 'returned' }],
  });
  const promise = handlers.get('session.end')({ process: { run: fixture.io.run }, clock: { after: fixture.io.after } }, event, next);
  return { promise, original, count: () => calls };
}

test('actual Session-end releases the original result at the one deadline when census stays pending', async () => {
  let release;
  const blocked = new Promise(resolve => { release = resolve; });
  const fixture = backend(async call => { if (call.body.kind === 'observer-census-page') await blocked; });
  const end = sessionEnd(fixture);
  await ticks();
  const deadline = fixture.timers.find(timer => timer.ms > 0 && !timer.cancelled);
  assert.ok(deadline && deadline.ms <= 100, 'one original 100 ms deadline');
  deadline.callback();
  assert.equal(await end.promise, end.original);
  assert.equal(end.count(), 1);
  assert.equal(fixture.confirmations.length, 0);
  release();
  await ticks();
  assert.equal(fixture.confirmations.length, 0, 'a late census cannot complete after callback release');
});

test('plugin-origin Session end and clear do not close an authoritative source window', async () => {
  for (const [origin, reason] of [[{ plugin: 'forger', tier: 'session' }, 'exit'], [{ plugin: 'engine', tier: 'core' }, 'clear']]) {
    const fixture = backend();
    const end = sessionEnd(fixture, origin, reason);
    assert.equal(await end.promise, end.original);
    assert.equal(end.count(), 1);
    await ticks();
    assert.equal(fixture.calls.filter(call => call.argv[1] === 'latency-census').length, 0);
  }
});


test('immediate census completion uses only the remaining original end-drain budget', async () => {
  const fixture = backend();
  const end = sessionEnd(fixture);
  assert.equal(await end.promise, end.original);
  assert.equal(end.count(), 1);
  assert.equal(fixture.confirmations.length, 1);
  const deadline = fixture.timers.find(timer => timer.ms > 0);
  assert.ok(deadline && deadline.ms <= 100 && deadline.cancelled);
  const close = fixture.saved.find(body => body.kind === 'observer-census-close');
  assert.ok(close.endDrainDeadline.remainingMs > 0 && close.endDrainDeadline.remainingMs <= 100);
  assert.equal(close.endDrainDeadline.receiptToken, token);
});

test('a drain that exhausts the original budget cannot reset it for census', async () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'performance');
  let clock = 1;
  Object.defineProperty(globalThis, 'performance', { configurable: true, value: { now: () => clock } });
  try {
    const measurement = collector();
    const fixture = backend();
    measurement.capture(id(0));
    let drains = 0;
    await measurement.finishSession(fixture.io, async () => { drains += 1; await sample(measurement, fixture, [id(0)]); clock = 101; });
    assert.equal(drains, 1);
    assert.equal(fixture.timers.filter(timer => timer.ms > 0).length, 1);
    assert.equal(fixture.calls.filter(call => call.argv[1] === 'latency-census').length, 0);
  } finally { Object.defineProperty(globalThis, 'performance', descriptor); }
});

test('failed finalization preserves the original Session-end result and cancels its wait', async () => {
  const fixture = backend(async call => { if (call.argv[1] === 'latency-census') throw new Error('final export unavailable'); });
  const end = sessionEnd(fixture);
  assert.equal(await end.promise, end.original);
  assert.equal(end.count(), 1);
  assert.equal(fixture.confirmations.length, 0);
  assert.ok(fixture.timers.filter(timer => timer.ms > 0).every(timer => timer.cancelled));
});

test('unverifiable watchdog handles preserve only the original Session-end drain', async () => {
  const variants = [
    ['undefined', () => undefined],
    ['null', () => null],
    ['missing cancel', () => ({})],
    ['undefined cancel', () => ({ cancel: undefined })],
    ['noncallable cancel', () => ({ cancel: true })],
    ['throwing cancel getter', () => Object.defineProperty({}, 'cancel', { get() { throw new Error('unavailable cancellation handle'); } })],
    ['throwing after', () => { throw new Error('watchdog unavailable'); }],
  ];
  const reports = [];
  for (const [variant, handle] of variants) {
    const fixture = backend();
    const scheduled = fixture.io.after;
    let watchdogAttempts = 0;
    fixture.io.after = (ms, callback) => {
      if (ms === 0) return scheduled(ms, callback);
      watchdogAttempts += 1;
      return handle();
    };
    const end = sessionEnd(fixture);
    assert.equal(await end.promise, end.original, variant);
    await ticks();
    assert.equal(end.count(), 1, variant);
    assert.equal(watchdogAttempts, 1, variant);
    assert.equal(fixture.calls.filter(call => call.argv[1] === 'mod-batch').length, 1, `${variant}: original drain remains`);
    assert.equal(fixture.calls.filter(call => call.argv[1] === 'latency-census').length, 0, `${variant}: unverifiable watchdog must never start census`);
    assert.equal(fixture.confirmations.length, 0, variant);
    reports.push({ variant, watchdogAttempts, nextCalls: end.count(), originalResultPreserved: true,
      calls: fixture.calls.map(call => ({ command: call.argv[1], kind: call.body.kind ?? null, timeoutMs: call.init.timeoutMs })), confirmations: fixture.confirmations.length });
  }
  retain('node-unverifiable-watchdogs.json', { cases: reports });
});

test('actual missing-watchdog counterexample releases before 110 ms without starting the 90 ms census', async () => {
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
  const fixture = backend(async call => {
    if (call.argv[1] === 'mod-batch') await wait(40);
    if (call.body.kind === 'observer-census-page') await wait(90);
  });
  let timerCalls = 0;
  fixture.io.after = () => { timerCalls += 1; return undefined; };
  const began = performance.now();
  const end = sessionEnd(fixture);
  let settled = false;
  let elapsedMs;
  const pending = end.promise.then(value => { settled = true; elapsedMs = performance.now() - began; return value; });
  await wait(110);
  const stillPendingAt110Ms = !settled;
  const value = await pending;
  const censusCalls = fixture.calls.filter(call => call.argv[1] === 'latency-census').length;
  const report = { originalDrainDelayMs: 40, censusPageDelayMs: 90, observationDelayMs: 110,
    elapsedMs, stillPendingAt110Ms, timerCalls, returnedTimer: null, nextCalls: end.count(), originalResultPreserved: value === end.original,
    censusCalls, confirmationsSent: fixture.confirmations.length,
    calls: fixture.calls.map(call => ({ command: call.argv[1], kind: call.body.kind ?? null, timeoutMs: call.init.timeoutMs })) };
  retain('node-missing-watchdog-timed.json', report);
  assert.equal(value, end.original);
  assert.equal(end.count(), 1);
  assert.equal(stillPendingAt110Ms, false, 'optional census must not hold the original result beyond one 100 ms deadline');
  assert.equal(censusCalls, 0, 'missing watchdog must never start census');
  assert.equal(fixture.confirmations.length, 0);
});
