// Execute the byte-identical retained pre-correction observer collector.
// This is a synthetic failure injection, never native execution evidence.
import { createLatencyMeasurement } from './observer-latency-before.ts';
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';

const runtime = '11111111-1111-4111-8111-111111111111';
const first = '22222222-2222-4222-8222-222222222222';
const lost = '33333333-3333-4333-8333-333333333333';
const persisted = [];
let fail = false;
const io = {
  run: async (_argv, init) => {
    if (fail) throw new Error('final exporter unavailable');
    persisted.push(JSON.parse(init.stdin));
    return { exitCode: 0, stdout: '{"accepted":true}', stderr: '', isStdoutTruncated: false, isStderrTruncated: false };
  },
  after: () => { throw new Error('not used'); },
};
const measurement = createLatencyMeasurement(['/owned/threadspace-hook', 'mod-batch', '--qualification-latency'], runtime);
const response = { exitCode: 0, stdout: '{"qualificationClock":{"token":"44444444-4444-4444-8444-444444444444"}}', stderr: '', isStdoutTruncated: false, isStderrTruncated: false };
measurement.capture(first);
await measurement.end(io, measurement.begin([first]), response, new Map([[first, 'COMMITTED']]));
fail = true;
measurement.capture(lost);
await measurement.end(io, measurement.begin([lost]), response, new Map([[lost, 'NOT_ACCEPTED']]));
const result = {
  syntheticActualModuleExecution: true,
  sourceSha256: createHash('sha256').update(readFileSync(new URL('./observer-latency-before.ts', import.meta.url))).digest('hex'),
  nodeVersion: process.version,
  actualCaptureCalls: 2,
  persistedReports: persisted.length,
  lastPersistedTotalCaptured: persisted.at(-1).totalCaptured,
  lastPersistedFailedExports: persisted.at(-1).failedExports,
  lastPersistedRecordIds: persisted.at(-1).records.map(record => record.observationId),
  finalReportFailed: true,
  finalSealAvailable: typeof measurement.finalize === 'function',
};
if (result.lastPersistedTotalCaptured !== 1 || result.lastPersistedFailedExports !== 0) {
  throw new Error('retained pre-correction witness no longer reproduces');
}
writeFileSync(new URL('./observer-final-export-before.json', import.meta.url), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
