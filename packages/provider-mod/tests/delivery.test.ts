// Mod delivery (SPEC §8.3): the typed mod-batch receipt, retry with the
// original UUIDs, queue and batch bounds, one retry timer with backoff, one
// in-flight drain, no timer while idle, and the helper's budget on every
// run's argv. The test's `process.run` hook
// stands for the capture helper; a `{ deny }` answer reaches the observer as
// the rejection a timed-out `$.process.run` gives.

import { describe, expect, mock, test } from 'claude-code/testing'
import { ARGV, OPTIONS, START, commitAll, installCore, installHelper, receipt, spawnInput, toolCallInput, type Batch } from './kit.ts'

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const ids = (batch: Batch | undefined) => (batch?.records ?? []).map((record: any) => record.observationId)
const captureSequence = (record: any) => Number(record.callbackResultSequence ?? record.callbackEntrySequence)
const timeout = { reject: 'timed out after 250 ms' }

// Beneath the observer: counts the observer's `$.clock.after` dispatches and
// reports the running count through `$.store.set`, which the test captures.
const timerCounter = {
  name: 'timer-counter',
  tier: 'builtin' as const,
  register: (on: any) => {
    let count = 0
    on('clock.after', async ($: any, e: any, next: any) => {
      if (next.origin.plugin === 'threadspace-observer') {
        count += 1
        await $.store.set('observer-clock-after', count)
      }
      return next(e)
    })
  },
}

const countTimers = (on: any) => {
  const seen = { count: 0 }
  on('store.set', (_$: any, e: any) => {
    if (e.key === 'observer-clock-after') seen.count = Number(e.value)
    return { value: undefined }
  })
  return seen
}

const epochs: string[] = []

describe('mod delivery', () => {
  test('mod batch receipts: a valid receipt removes the records', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const statuses = ['COMMITTED', 'ALREADY_COMMITTED', 'LOCAL_SPOOLED']
    const helper = installHelper(on, clock, batch => ({ stdout: receipt(batch, (_record, index) => statuses[index % 3] as string) }))
    installCore(on)

    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    await $.tool.call(toolCallInput('tu-1') as any)
    await clock.settle()
    await clock.advance(60_000)

    expect(helper.batches.length).toBe(1)
    const [batch] = helper.batches
    expect(batch?.argv).toEqual([...ARGV, '--budget-ms', '230'])
    expect(batch?.timeoutMs).toBe(250)
    expect(batch?.envelope).toEqual(
      expect.objectContaining({ receiptVersion: 1, kind: 'mod-batch', sourceEpoch: expect.stringMatching(UUID), droppedRecords: 0 }),
    )
    expect(batch?.records.length).toBe(4)
    for (const record of batch?.records ?? []) expect(record.observationId).toMatch(UUID)
    expect(new Set(ids(batch)).size).toBe(4)
  })

  test('partial commit followed by timeout retries with the original UUIDs', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const journal = new Map<string, string>()
    const helper = installHelper(on, clock, (batch, call) => {
      if (call === 1) {
        // The helper commits the first half, then the call times out before
        // any receipt reaches the observer.
        for (const record of batch.records.slice(0, 2)) journal.set(record.observationId, JSON.stringify(record))
        return timeout
      }
      const statusOf = (record: any) => (journal.has(record.observationId) ? 'ALREADY_COMMITTED' : 'COMMITTED')
      const answer = { stdout: receipt(batch, statusOf) }
      for (const record of batch.records) if (!journal.has(record.observationId)) journal.set(record.observationId, JSON.stringify(record))
      return answer
    })
    installCore(on)

    await $.tool.call(toolCallInput('tu-a') as any)
    await $.tool.call(toolCallInput('tu-b') as any)
    await clock.settle()
    expect(helper.batches.length).toBe(1)
    await clock.advance(249)
    expect(helper.batches.length).toBe(1)
    await clock.advance(1)
    expect(helper.batches.length).toBe(2)
    await clock.advance(60_000)

    const [first, second] = helper.batches
    expect(helper.batches.length).toBe(2)
    expect(second?.at).toBe(250)
    expect(ids(second)).toEqual(ids(first))
    expect(second?.records).toEqual(first?.records)
    expect(journal.size).toBe(4)
  })

  test('malformed receipt is not acceptance', { ...OPTIONS, timeoutMs: 20_000 }, async ($, on) => {
    const clock = mock.clock(on)
    const cases: Array<[string, (batch: Batch) => { exitCode?: number; stdout?: string; isStdoutTruncated?: boolean }]> = [
      ['exit 0 with no receipt', () => ({ stdout: '' })],
      ['not JSON', () => ({ stdout: 'ok' })],
      ['JSON that is not an object', () => ({ stdout: '[]' })],
      ['another receipt version', batch => ({ stdout: receipt(batch).replace('"receiptVersion":1', '"receiptVersion":2') })],
      ['a record without a result', batch => ({ stdout: JSON.stringify({ receiptVersion: 1, results: JSON.parse(receipt(batch)).results.slice(1) }) })],
      [
        'an unknown UUID',
        batch => ({
          stdout: JSON.stringify({
            receiptVersion: 1,
            results: [...JSON.parse(receipt(batch)).results.slice(1), { observationId: '00000000-0000-4000-8000-000000000000', status: 'COMMITTED' }],
          }),
        }),
      ],
      [
        'a duplicate UUID',
        batch => {
          const results = JSON.parse(receipt(batch)).results
          return { stdout: JSON.stringify({ receiptVersion: 1, results: [...results.slice(0, -1), results[0]] }) }
        },
      ],
      ['an unknown status', batch => ({ stdout: receipt(batch, () => 'MAYBE') })],
      ['a receipt over 16 KiB', batch => ({ stdout: receipt(batch).replace(/}$/, `,"pad":"${'x'.repeat(16 * 1024)}"}`) })],
      ['a non-zero exit carrying a valid receipt', batch => ({ exitCode: 1, stdout: receipt(batch) })],
      ['truncated output carrying a valid receipt', batch => ({ stdout: receipt(batch), isStdoutTruncated: true })],
    ]
    let index = 0
    const helper = installHelper(on, clock, batch => {
      const entry = cases[index]
      index += 1
      return entry ? entry[1](batch) : commitAll(batch, 0)
    })
    installCore(on)

    await $.tool.call(toolCallInput('tu-m') as any)
    await clock.settle()
    for (let step = 0; step < cases.length; step += 1) await clock.advance(5_000)
    await clock.advance(60_000)

    expect(helper.batches.length).toBe(cases.length + 1)
    for (const batch of helper.batches) expect(ids(batch)).toEqual(ids(helper.batches[0]))
  })

  test('stable UUID replay of unaccepted records', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock, (batch, call) =>
      call === 1 ? { stdout: receipt(batch, (_record, index) => (index % 2 === 0 ? 'NOT_ACCEPTED' : 'LOCAL_SPOOLED')) } : commitAll(batch, call),
    )
    installCore(on)

    await $.tool.call(toolCallInput('tu-1') as any)
    await $.tool.call(toolCallInput('tu-2') as any)
    await clock.settle()
    await clock.advance(60_000)

    const [first, second] = helper.batches
    expect(helper.batches.length).toBe(2)
    expect(second?.at).toBe(250)
    const unaccepted = (first?.records ?? []).filter((_record: any, index: number) => index % 2 === 0)
    expect(second?.records).toEqual(unaccepted)
  })

  test('queue bounds (2,048 records) and batch bounds (128 records, 64 KiB)', { ...OPTIONS, timeoutMs: 120_000 }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock, () => timeout)
    installCore(on)

    const calls = 1100
    for (let call = 0; call < calls; call += 1) await $.tool.call(toolCallInput(`tu-${call}`) as any)
    helper.setMode(commitAll)
    await clock.settle()
    for (let step = 0; step < 40; step += 1) await clock.advance(250)

    const produced = calls * 2
    const delivered = helper.batches.flatMap(batch => batch.records)
    expect(new Set(delivered.map((record: any) => record.observationId)).size).toBe(2048)
    expect(delivered.length).toBe(2048)
    expect(helper.batches[helper.batches.length - 1]?.envelope.droppedRecords).toBe(produced - 2048)
    const sequences = delivered.map(captureSequence)
    expect(sequences.every((value, index) => index === 0 || value > (sequences[index - 1] as number))).toBe(true)
    // The oldest records were the ones evicted.
    expect(Math.min(...sequences)).toBe(produced - 2048 + 1)
    for (const batch of helper.batches) {
      expect(batch.records.length).toBeLessThanOrEqual(128)
      expect(new TextEncoder().encode(JSON.stringify(batch.envelope)).length).toBeLessThanOrEqual(64 * 1024)
    }
  })

  test('batches are as full as both bounds allow (128 records, 64 KiB)', { ...OPTIONS, timeoutMs: 60_000 }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)

    // session.attach records carry no session, turn or actor IDs: small
    // records, near the point where the record and byte bounds meet.
    for (let call = 0; call < 300; call += 1) await $.session.attach({ surface: 'terminal', clientId: `c${call}` } as any)
    await clock.settle()
    for (let step = 0; step < 40; step += 1) await clock.advance(250)

    const encoder = new TextEncoder()
    const bytesOf = (value: unknown) => encoder.encode(JSON.stringify(value)).length
    const records = helper.records()
    expect(records.length).toBe(600)
    let delivered = 0
    for (const [index, batch] of helper.batches.entries()) {
      expect(batch.records.length).toBeLessThanOrEqual(128)
      expect(bytesOf(batch.envelope)).toBeLessThanOrEqual(64 * 1024)
      delivered += batch.records.length
      // Each batch but the last ended at a bound: 128 records, or the next
      // record would not have fit in 64 KiB.
      if (index < helper.batches.length - 1 && batch.records.length < 128) {
        expect(bytesOf(batch.envelope) + bytesOf(records[delivered]) + 1).toBeGreaterThan(64 * 1024)
      }
    }
  })

  test('queue byte bound (8 MiB) with long escaped identifiers', { ...OPTIONS, timeoutMs: 120_000 }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)

    // Identifiers are capped at 256 characters, but JSON escapes a control
    // character as six bytes. A spawn's records carry three or four such
    // fields (~6 KiB each), so 2,000 records overshoot 8 MiB and the byte
    // bound binds well before the 2,048-record bound.
    const wide = (seed: string) => `${seed}${'\u0001'.repeat(256)}`.slice(0, 256)
    const calls = 1000
    for (let call = 0; call < calls; call += 1) {
      await $.agent.spawn({
        ...spawnInput(wide(`tu-${call}-`)),
        subagentType: wide(`type-${call}-`),
        parentAgentId: wide(`parent-${call}-`),
      } as any)
    }
    await clock.settle()
    let seen = -1
    for (let step = 0; step < 2_000 && helper.batches.length !== seen; step += 1) {
      seen = helper.batches.length
      await clock.advance(250)
    }

    const encoder = new TextEncoder()
    const delivered = helper.records()
    const bytes = delivered.reduce((sum: number, record: any) => sum + encoder.encode(JSON.stringify(record)).length, 0)
    expect(delivered.length).toBeLessThan(1800)
    expect(bytes).toBeLessThanOrEqual(8 * 1024 * 1024)
    expect(bytes).toBeGreaterThan(8 * 1024 * 1024 - 16 * 1024)
    expect(helper.batches[helper.batches.length - 1]?.envelope.droppedRecords).toBe(calls * 2 - delivered.length)
    for (const batch of helper.batches) expect(encoder.encode(JSON.stringify(batch.envelope)).length).toBeLessThanOrEqual(64 * 1024)
  })

  test('retry backoff runs 250 ms to 5 s and resets on success', { ...OPTIONS, timeoutMs: 20_000 }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock, () => timeout)
    installCore(on)

    await $.tool.call(toolCallInput('tu-1') as any)
    await clock.settle()
    for (let step = 0; step < 100; step += 1) await clock.advance(250)
    const failing = helper.batches.map(batch => batch.at)
    expect(failing.slice(0, 9)).toEqual([0, 250, 750, 1750, 3750, 7750, 12750, 17750, 22750])

    helper.setMode(commitAll)
    for (let step = 0; step < 20; step += 1) await clock.advance(250)
    const deliveredAt = helper.batches[helper.batches.length - 1]?.at as number
    expect(deliveredAt - (failing[failing.length - 1] as number)).toBe(5000)

    helper.setMode(() => timeout)
    await $.tool.call(toolCallInput('tu-2') as any)
    const restart = clock.now()
    await clock.settle()
    for (let step = 0; step < 12; step += 1) await clock.advance(250)
    const after = helper.batches.filter(batch => batch.at >= restart).map(batch => batch.at - restart)
    expect(after.slice(0, 3)).toEqual([0, 250, 750])
  })

  test('one pending retry timer during backoff', { ...OPTIONS, plugins: [timerCounter] }, async ($, on) => {
    const clock = mock.clock(on)
    const timers = countTimers(on)
    const helper = installHelper(on, clock, () => timeout)
    installCore(on)

    await $.tool.call(toolCallInput('tu-0') as any)
    await clock.settle()
    expect(helper.batches.length).toBe(1)
    const scheduled = timers.count
    await clock.advance(100)
    for (let call = 1; call <= 30; call += 1) await $.tool.call(toolCallInput(`tu-${call}`) as any)
    expect(timers.count).toBe(scheduled)
    await clock.advance(149)
    expect(helper.batches.length).toBe(1)
    await clock.advance(1)
    expect(helper.batches.length).toBe(2)
    expect(helper.batches[1]?.at).toBe(250)
  })

  test('no timer while idle', { ...OPTIONS, plugins: [timerCounter] }, async ($, on) => {
    const clock = mock.clock(on)
    const timers = countTimers(on)
    const helper = installHelper(on, clock)
    installCore(on)

    await $.session.start(START as any)
    await $.tool.call(toolCallInput('tu-1') as any)
    await clock.settle()
    await clock.advance(1_000)
    const batches = helper.batches.length
    const scheduled = timers.count
    expect(batches).toBeGreaterThan(0)
    await clock.advance(10 * 60_000)
    expect(helper.batches.length).toBe(batches)
    expect(timers.count).toBe(scheduled)
  })

  test('one in-flight drain per module', { ...OPTIONS, timeoutMs: 20_000 }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock, batch => ({ delayMs: 100, stdout: receipt(batch) }))
    installCore(on)

    await $.tool.call(toolCallInput('tu-0') as any)
    await clock.settle()
    for (let call = 1; call <= 20; call += 1) {
      await $.tool.call(toolCallInput(`tu-${call}`) as any)
      await clock.advance(30)
    }
    for (let step = 0; step < 20; step += 1) await clock.advance(100)

    expect(helper.maxActive()).toBe(1)
    expect(helper.records().length).toBe(42)
    const starts = helper.batches.map(batch => batch.at)
    for (let index = 1; index < starts.length; index += 1) expect((starts[index] as number) - (starts[index - 1] as number)).toBeGreaterThanOrEqual(100)
  })

  test('module load runs register afresh with a new source epoch', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)
    await $.tool.call(toolCallInput('tu-epoch-1') as any)
    await clock.settle()
    epochs.push(helper.batches[0]?.envelope.sourceEpoch)
    expect(epochs[0]).toMatch(UUID)
  })

  test('module load in a second test has another source epoch and sequence restarts', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)
    await $.tool.call(toolCallInput('tu-epoch-2') as any)
    await clock.settle()
    epochs.push(helper.batches[0]?.envelope.sourceEpoch)
    expect(epochs.length).toBe(2)
    expect(epochs[1]).toMatch(UUID)
    expect(epochs[1]).not.toBe(epochs[0])
    expect(helper.batches[0]?.records[0].callbackEntrySequence).toBe('1')
  })

  test('module retirement/reload: the kit cannot hot-reload a module (config.set)', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)
    on('config.set', (_$: any, e: any) => ({ value: e.value }))

    await $.tool.call(toolCallInput('tu-before') as any)
    await clock.settle()
    await $.config.set({ key: 'threadspace-observer.captureArgv', value: ['/opt/other', 'mod-batch'] } as any)
    await $.tool.call(toolCallInput('tu-after') as any)
    await clock.settle()

    // Same epoch and same argv after the change: the kit applied no reload,
    // so reload and timer cancellation are qualified in a native session.
    const epochsSeen = new Set(helper.batches.map(batch => batch.envelope.sourceEpoch))
    expect(epochsSeen.size).toBe(1)
    expect(helper.batches.every(batch => batch.argv.join(' ') === [...ARGV, '--budget-ms', '230'].join(' '))).toBe(true)
  })

  test('budget argv is appended', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock, (batch, call) => (call === 1 ? timeout : commitAll(batch, call)))
    installCore(on)

    // A prompt drain, its retry after a timeout, then the end-of-session drain.
    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    await clock.settle()
    await clock.advance(250)
    await $.session.end({ reason: 'prompt_input_exit', sessionId: 'S1', resume: { id: 'S1' } } as any)

    expect(helper.batches.map(batch => batch.timeoutMs)).toEqual([250, 250, 100])
    const [first, retry, end] = helper.batches
    for (const batch of [first, retry]) {
      expect(batch?.argv).toEqual([...ARGV, '--budget-ms', '230'])
      expect(batch?.budgetMs).toBe(230)
    }
    expect(end?.argv).toEqual([...ARGV, '--budget-ms', '80'])
    expect(end?.budgetMs).toBe(80)
    expect(end?.records.some((record: any) => record.nativeEvent === 'session.end')).toBe(true)
  })
})
