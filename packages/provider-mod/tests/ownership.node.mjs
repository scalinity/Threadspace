// Portable tests of the actual TypeScript observer, with an in-memory host.
// Engine origins/traces are fixtures here; native Claude stamping and kernel
// corroboration require their separate host/native qualification campaigns.
// Usage: node tests/ownership.node.mjs [--module <register.ts>]
//        [--case F1A-1] [--output <JSON>]
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { createOwnershipLedger } from '../hooks/ownership.ts'

const argument = name => {
  const index = process.argv.indexOf(name)
  return index < 0 ? undefined : process.argv[index + 1]
}
const modulePath = argument('--module') ?? fileURLToPath(new URL('../hooks/register.ts', import.meta.url))
const { register } = await import(pathToFileURL(modulePath))
const selected = argument('--case')
const results = []
const CORE = [{ plugin: 'engine', tier: 'core', outcome: 'returned' }]
const SHORTCUT = [{ plugin: 'shortcut', tier: 'append', outcome: 'returned' }]
const ENGINE = { plugin: 'engine', tier: 'core' }
const PLUGIN = { plugin: 'untrusted', tier: 'user' }

function fixture({ hostSessionId = 'Session-A', proof, captureFailure = false } = {}) {
  const handlers = new Map()
  const timers = []
  const batches = []
  const proofRequests = []
  const calls = []
  let priorEpoch = 'prior-observer-epoch'
  const host = {
    process: { run: async (argv, init) => {
      if (captureFailure) throw new Error('fixture helper unavailable')
      const body = JSON.parse(init.stdin)
      if (argv.includes('observer-proof')) {
        proofRequests.push({ argv, init, body })
        return proof ? await proof(body) : { exitCode: 1, stdout: '', isStdoutTruncated: false }
      }
      assert(argv.includes('mod-batch'))
      assert(Array.isArray(body.records))
      batches.push(body)
      return { exitCode: 0, isStdoutTruncated: false, stdout: JSON.stringify({
        receiptVersion: 1,
        results: body.records.map(record => ({ observationId: record.observationId, status: 'COMMITTED' })),
      }) }
    } },
    clock: { after: (_ms, fn) => {
      const timer = { fn, cancelled: false, cancel() { this.cancelled = true } }
      timers.push(timer)
      return timer
    } },
    state: {
      get: async () => ({ value: priorEpoch }),
      set: async (_key, value) => { priorEpoch = value },
    },
    session: {
      id: async () => hostSessionId,
      version: async () => ({ version: '2.1.295', base: '2.1.295', builtAt: 'fixture' }),
    },
  }
  register((event, ...args) => handlers.set(event, args.at(-1)), { captureArgv: ['fixture-helper', 'mod-batch'] })
  const nextFor = (event, input, run, origin = ENGINE, trace = CORE) => {
    let count = 0
    const next = received => {
      count += 1
      assert.equal(received, input, `${event}: original event reference`)
      return run(received)
    }
    next.origin = origin
    Object.defineProperty(next, 'trace', { get: () => trace })
    calls.push(() => assert.equal(count, 1, `${event}: next exactly once`))
    return next
  }
  return {
    proofRequests,
    fire: async (event, input, run = async value => value, origin = ENGINE, trace = CORE) => {
      const next = nextFor(event, input, run, origin, trace)
      return await handlers.get(event)(host, input, next)
    },
    stream: (input, run, origin = ENGINE, trace = CORE) =>
      handlers.get('turn.step')(host, input, nextFor('turn.step', input, run, origin, trace)),
    flush: async () => {
      await tick()
      for (let n = 0; timers.length && n < 64; n += 1) {
        const timer = timers.shift()
        if (!timer.cancelled) timer.fn()
        await tick()
      }
      if (!captureFailure) assert.equal(timers.length, 0, 'bounded test drains complete')
      for (const check of calls) check()
      return batches.flatMap(batch => batch.records)
    },
  }
}

const tick = () => new Promise(resolve => setImmediate(resolve))
const deferred = () => {
  let resolve
  let reject
  const promise = new Promise((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const begin = (f, sessionId, source = 'startup', actor) =>
  f.fire('classic.SessionStart', { session_id: sessionId, source, agent_id: actor })
const start = (f, turnId, trace = CORE, origin = ENGINE) =>
  f.fire('turn.start', { turnId }, async value => value, origin, trace)
const change = async (f, from, to, actor) => {
  await f.fire('session.end', { reason: 'clear', sessionId: from, resume: { id: from } })
  await begin(f, to, 'clear', actor)
}
const complete = (f, turnId, agentId, run) => f.fire('turn.complete', {
  turnId, agentId, reason: 'answer', isAborted: false, durationMs: 1, answer: 'SECRET-answer',
}, run)
const step = async (f, turnId, agentId) => {
  const input = { turnId, agentId, index: 0, model: 'fixture-model', messageCount: 1 }
  const result = { turnId, index: 0, toolUses: [], stopReason: 'end_turn' }
  const iterator = f.stream(input, async function* () { return result })
  assert.deepEqual(await iterator.next(), { done: true, value: result })
}
const spawn = (f, occurrence, child, parent, run) => f.fire('agent.spawn', {
  tool_use_id: occurrence, parentAgentId: parent, subagentType: 'fixture',
  provider: ENGINE, background: true, fork: false,
}, run ?? (async () => ({ agentId: child, model: 'fixture-model' })))
const proofReply = (request, overrides = {}) => ({
  exitCode: 0,
  isStdoutTruncated: false,
  stdout: JSON.stringify({ ...request, proofToken: '11111111-1111-4111-8111-111111111111', status: 'COMMITTED', ...overrides }),
})
const outcomes = (records, turn) => records.filter(record => record.nativeEvent === 'turn.complete' &&
  record.phase === 'result' && (turn === undefined || record.nativeTurnId === turn))
const assertOwner = (record, session, generation) => {
  assert.equal(record.sessionId, session, 'original Session identity')
  assert.equal(record.ownershipGeneration, generation, 'original Session generation')
  assert.equal(record.sessionGeneration, generation, 'adapter generation agrees')
  assert.equal(record.ownershipEpoch, record.sourceEpoch, 'one observer namespace epoch')
}
const recordCase = async (id, description, fn) => {
  if (selected && selected !== id) return
  const evidence = { id, description, records: [] }
  try {
    await fn(evidence)
    results.push({ ...evidence, result: 'PASS' })
  } catch (error) {
    results.push({ ...evidence, result: 'FAIL', error: String(error?.stack ?? error) })
  }
}

await recordCase('F1A-1', 'Delayed completion ENTRY after A→B resolves the retained A Turn', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await start(f, 'Turn-A')
  await change(f, 'Session-A', 'Session-B')
  await complete(f, 'Turn-A')
  evidence.records = await f.flush()
  const [outcome] = outcomes(evidence.records)
  assert.notEqual(outcome.sessionId, 'Session-B', 'late A completion must never acquire B')
  assertOwner(outcome, 'Session-A', 1)
  assert.equal(outcome.currentSessionId, 'Session-B')
  assert.equal(outcome.currentSessionGeneration, 3)
})

await recordCase('F1A-2', 'Delayed completion RETURN preserves the original immutable entry scope', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await start(f, 'Turn-A')
  const held = deferred()
  const result = { original: 'provider-result' }
  const pending = complete(f, 'Turn-A', undefined, () => held.promise)
  await change(f, 'Session-A', 'Session-B')
  held.resolve(result)
  assert.equal(await pending, result, 'exact original result object')
  evidence.records = await f.flush()
  for (const record of evidence.records.filter(record => record.nativeEvent === 'turn.complete')) {
    assertOwner(record, 'Session-A', 1)
    assert.equal(record.currentSessionId, 'Session-A', 'entry snapshot is not resampled on return')
  }
})

await recordCase('F1A-3', 'A→B→A preserves original generations and actor-scoped repeated Turn IDs', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A', 'startup', 'parent-A')
  await start(f, 'Turn-A-old')
  await spawn(f, 'spawn-A', 'child-A', 'parent-A')
  await step(f, 'Shared-child-Turn', 'child-A')
  await change(f, 'Session-A', 'Session-B', 'parent-B')
  await start(f, 'Turn-B')
  await spawn(f, 'spawn-B', 'child-B', 'parent-B')
  await step(f, 'Shared-child-Turn', 'child-B')
  await change(f, 'Session-B', 'Session-A', 'parent-A-new')
  await start(f, 'Turn-A-new')
  await complete(f, 'Turn-A-old')
  await complete(f, 'Turn-B')
  await complete(f, 'Shared-child-Turn', 'child-A')
  await complete(f, 'Shared-child-Turn', 'child-B')
  await complete(f, 'Turn-A-new')
  evidence.records = await f.flush()
  const expected = [
    ['Turn-A-old', undefined, 'Session-A', 1], ['Turn-B', undefined, 'Session-B', 3],
    ['Shared-child-Turn', 'child-A', 'Session-A', 1], ['Shared-child-Turn', 'child-B', 'Session-B', 3],
    ['Turn-A-new', undefined, 'Session-A', 5],
  ]
  for (const [turn, actor, session, generation] of expected) {
    const record = outcomes(evidence.records, turn).find(record => record.actorNativeId === actor)
    assertOwner(record, session, generation)
    assert.equal(record.currentSessionId, 'Session-A')
    assert.equal(record.currentSessionGeneration, 5)
  }
})

await recordCase('F1A-4', 'Explicit native Turn-ID reuse across Sessions stays ambiguous', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await start(f, 'Reused-Turn')
  await change(f, 'Session-A', 'Session-B')
  await start(f, 'Reused-Turn')
  await complete(f, 'Reused-Turn')
  await change(f, 'Session-B', 'Session-A')
  await start(f, 'Reused-Turn')
  await complete(f, 'Reused-Turn')
  evidence.records = await f.flush()
  for (const record of outcomes(evidence.records)) {
    assert.equal(record.sessionId, undefined)
    assert.equal(record.sessionGeneration, 0, 'legacy generation zero denotes no owned interval')
    assert.equal(record.ownershipGeneration, undefined)
    assert.equal(record.ownershipStatus, 'AMBIGUOUS')
  }
  assert.equal(evidence.records.filter(record => record.nativeEvent === 'turn.start' && record.phase === 'result').length, 3)
})

await recordCase('F1A-5', 'A child spawned under an original tool occurrence stays with A after parent/current change', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await start(f, 'Parent-Turn')
  const held = deferred()
  const childResult = { agentId: 'child-delayed', model: 'fixture' }
  const tool = { tool: 'Agent', tool_use_id: 'original-spawn-occurrence' }
  let entered
  const enteredPromise = new Promise(resolve => { entered = resolve })
  const pending = f.fire('tool.call', tool, async () => {
    const child = spawn(f, tool.tool_use_id, undefined, undefined, () => { entered(); return held.promise })
    await child
    return { result: 'original-tool-result' }
  })
  await enteredPromise
  await change(f, 'Session-A', 'Session-B')
  await complete(f, 'Parent-Turn')
  held.resolve(childResult)
  await pending
  await step(f, 'Child-Turn', 'child-delayed')
  await complete(f, 'Child-Turn', 'child-delayed')
  evidence.records = await f.flush()
  const spawnResult = evidence.records.find(record => record.nativeEvent === 'agent.spawn' && record.phase === 'result')
  assertOwner(spawnResult, 'Session-A', 1)
  assertOwner(outcomes(evidence.records, 'Child-Turn')[0], 'Session-A', 1)
  assert.equal(outcomes(evidence.records, 'Child-Turn')[0].currentSessionId, 'Session-B')
})

await recordCase('F1A-6', 'First-seen late outcome is retained without Session and cannot be rebound later', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await change(f, 'Session-A', 'Session-B')
  await complete(f, 'Unknown-old-Turn')
  await start(f, 'Unknown-old-Turn')
  await complete(f, 'Unknown-old-Turn')
  await spawn(f, 'unknown-root-spawn', 'unknown-child')
  await step(f, 'Unknown-child-Turn', 'unknown-child')
  await complete(f, 'Unknown-child-Turn', 'unknown-child')
  evidence.records = await f.flush()
  for (const record of outcomes(evidence.records)) assert.equal(record.sessionId, undefined)
  assert.equal(outcomes(evidence.records)[0].ownershipStatus, 'UNKNOWN')
  assert.equal(outcomes(evidence.records)[1].ownershipStatus, 'AMBIGUOUS')
  const unownedSpawn = evidence.records.find(record => record.nativeEvent === 'agent.spawn' && record.phase === 'result')
  assert.equal(unownedSpawn.sessionId, undefined, 'new root occurrence after a transition has no invented parent')
})

await recordCase('F1A-7', 'Reused actual actor ID becomes ambiguous even when its old Turn remains retained', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A', 'startup', 'parent-A')
  await spawn(f, 'spawn-A', 'reused-child', 'parent-A')
  await step(f, 'Child-A', 'reused-child')
  await change(f, 'Session-A', 'Session-B', 'parent-B')
  await spawn(f, 'spawn-B', 'reused-child', 'parent-B')
  await step(f, 'Child-B', 'reused-child')
  await complete(f, 'Child-A', 'reused-child')
  await complete(f, 'Child-B', 'reused-child')
  evidence.records = await f.flush()
  for (const record of outcomes(evidence.records)) {
    assert.equal(record.sessionId, undefined)
    assert.equal(record.ownershipStatus, 'AMBIGUOUS')
  }
})

await recordCase('F1A-8', 'Plugin dispatch and a shortcut result cannot establish native ownership', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await start(f, 'Shortcut-Turn', SHORTCUT)
  await start(f, 'Plugin-Turn', CORE, PLUGIN)
  await f.fire('classic.SessionStart', { session_id: 'Forged-Session', source: 'startup' }, async e => e, PLUGIN)
  await spawn(f, 'shortcut-spawn', 'fabricated-child', undefined,
    async () => ({ agentId: 'fabricated-child', model: 'fixture' }))
  // The explicit shortcut spawn below carries a plausible actor but no core.
  await f.fire('agent.spawn', { tool_use_id: 'forged-spawn', provider: ENGINE },
    async () => ({ agentId: 'shortcut-child' }), ENGINE, SHORTCUT)
  await step(f, 'Shortcut-child-Turn', 'shortcut-child')
  await complete(f, 'Shortcut-Turn')
  await complete(f, 'Plugin-Turn')
  await complete(f, 'Shortcut-child-Turn', 'shortcut-child')
  await start(f, 'Real-Turn')
  await complete(f, 'Real-Turn')
  evidence.records = await f.flush()
  for (const turn of ['Shortcut-Turn', 'Plugin-Turn', 'Shortcut-child-Turn']) {
    assert.equal(outcomes(evidence.records, turn)[0].sessionId, undefined)
  }
  assertOwner(outcomes(evidence.records, 'Real-Turn')[0], 'Session-A', 1)
})

await recordCase('F1A-9', 'Bounded retained maps never evict uncertainty or collide truncated identities', async evidence => {
  const ledger = createOwnershipLedger('epoch-one', { turns: 2, actors: 1, occurrences: 1 })
  ledger.changeSession('Session-A', 'classic.SessionStart')
  const original = ledger.getCurrentScope()
  ledger.rememberTurn('T1', undefined, original)
  ledger.rememberTurn('T2', undefined, original)
  ledger.changeSession('Session-B', 'classic.SessionStart')
  const changed = ledger.getCurrentScope()
  assert.equal(ledger.rememberTurn('T1', undefined, changed).status, 'AMBIGUOUS')
  assert.equal(ledger.rememberTurn('T3', undefined, changed).status, 'SATURATED')
  assert.equal(ledger.turn('T2').scope, original)
  assert.equal(ledger.rememberTurn('T3', undefined, original).status, 'SATURATED')
  assert.equal(ledger.turn('x'.repeat(256) + 'a').status, 'UNKNOWN')
  assert.equal(ledger.rememberTurn('x'.repeat(256) + 'b', undefined, original).status, 'UNKNOWN')
  assert.equal(createOwnershipLedger('epoch-two').turn('T2').status, 'UNKNOWN')
  assert.deepEqual(ledger.sizes(), { turns: 2, actors: 0, occurrences: 0 })
  assert.equal(ledger.scopeUnchanged(original), false)
  assert.equal(ledger.sealCurrentScope(original, 'old-token'), false)
  evidence.boundary = { limit: 2, sizes: ledger.sizes(), retainedT2: ledger.turn('T2'), reusedT1: ledger.turn('T1') }
})

await recordCase('F1A-10', 'Provider values, exceptions, generator chunks and cancellation remain unchanged', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  await start(f, 'Stream-Turn')
  const chunks = [{ kind: 'text', text: 'SECRET-chunk' }, { kind: 'tool', id: 'tool-id', name: 'Read' }]
  const result = { index: 0, toolUses: [], stopReason: 'end_turn', secret: 'SECRET-result' }
  const iterator = f.stream({ turnId: 'Stream-Turn', index: 0 }, async function* () {
    yield chunks[0]
    yield chunks[1]
    return result
  })
  assert.equal((await iterator.next()).value, chunks[0])
  assert.equal((await iterator.next()).value, chunks[1])
  assert.deepEqual(await iterator.next(), { done: true, value: result })
  const error = new Error('original-provider-error')
  await assert.rejects(complete(f, 'Stream-Turn', undefined, async () => { throw error }), received => received === error)
  let cancelled = 0
  const cancelling = f.stream({ turnId: 'Stream-Turn', index: 1 }, async function* () {
    try { yield chunks[0]; yield chunks[1] } finally { cancelled += 1 }
    return result
  })
  await cancelling.next()
  const cancellation = { original: 'return-value' }
  assert.deepEqual(await cancelling.return(cancellation), { done: true, value: cancellation })
  assert.equal(cancelled, 1)
  // Metadata getter failure is inside the observer's capture path only.
  const event = { turnId: 'Stream-Turn', index: 2, get model() { throw new Error('capture-only') } }
  const failingCapture = f.stream(event, async function* () { return result })
  assert.equal((await failingCapture.next()).value, result)
  evidence.records = await f.flush()
  assert(evidence.records.some(record => record.phase === 'provider-error'))
  assert(evidence.records.some(record => record.phase === 'abandoned'))
  assert(!JSON.stringify(evidence.records).includes('SECRET-'), 'provider contents stay out of capture')
  const unavailable = fixture({ captureFailure: true })
  await begin(unavailable, 'Session-A')
  const originalResult = { preserved: true }
  assert.equal(await unavailable.fire('turn.start', { turnId: 'T' }, async () => originalResult), originalResult)
  await unavailable.flush()
})

await recordCase('F1A-11', 'Overlong native IDs cannot acquire a Session through capture truncation', async evidence => {
  const f = fixture()
  await begin(f, 'Session-A')
  const prefix = 'x'.repeat(256)
  await start(f, `${prefix}A`)
  await start(f, `${prefix}B`)
  await complete(f, `${prefix}A`)
  evidence.records = await f.flush()
  for (const record of evidence.records.filter(record => record.nativeTurnId === prefix)) {
    assert.equal(record.sessionId, undefined)
    assert.equal(record.ownershipStatus, 'UNKNOWN')
  }
})

await recordCase('F1B-bridge-1', 'Detached proof seals unchanged scope; only future captured Turn starts retain its token', async evidence => {
  const held = deferred()
  const f = fixture({ proof: () => held.promise })
  const originalResult = { original: 'session-result' }
  assert.equal(await f.fire('session.start', { isInteractive: true }, async () => originalResult), originalResult)
  await tick()
  assert.equal(f.proofRequests.length, 1)
  const request = f.proofRequests[0]
  assert.equal(request.init.timeoutMs, 2000)
  await start(f, 'Pre-seal-Turn')
  held.resolve(proofReply(request.body))
  await tick()
  await start(f, 'Post-seal-Turn')
  await complete(f, 'Pre-seal-Turn')
  await complete(f, 'Post-seal-Turn')
  evidence.records = await f.flush()
  evidence.proofRequests = f.proofRequests
  const seals = evidence.records.filter(record => record.nativeEvent === 'ownership.seal' && record.phase === 'result')
  assert.equal(seals.length, 1)
  assert.equal(seals[0].engineDispatch, false, 'observer seal does not impersonate engine dispatch')
  assert.equal(outcomes(evidence.records, 'Pre-seal-Turn')[0].ownershipProofToken, undefined)
  assert.equal(outcomes(evidence.records, 'Post-seal-Turn')[0].ownershipProofToken, '11111111-1111-4111-8111-111111111111')
  assert.equal(outcomes(evidence.records, 'Post-seal-Turn')[0].ownershipStatus, 'HOST_READ', 'receipt alone does not produce native authority')
})

await recordCase('F1B-bridge-2', 'A→B→A while native probe waits refuses a stale same-string seal', async evidence => {
  const held = deferred()
  const f = fixture({ proof: () => held.promise })
  await f.fire('session.start', { isInteractive: true })
  await tick()
  const request = f.proofRequests[0]
  await change(f, 'Session-A', 'Session-B')
  await change(f, 'Session-B', 'Session-A')
  held.resolve(proofReply(request.body))
  await tick()
  await start(f, 'Current-A-Turn')
  await complete(f, 'Current-A-Turn')
  evidence.records = await f.flush()
  evidence.proofRequests = f.proofRequests
  assert.equal(evidence.records.filter(record => record.nativeEvent === 'ownership.seal').length, 0)
  assert.equal(outcomes(evidence.records)[0].ownershipProofToken, undefined)
  assertOwner(outcomes(evidence.records)[0], 'Session-A', 5)
})

await recordCase('F1B-bridge-3', 'Malformed, mismatched and unsuccessful proof receipts never seal ownership', async evidence => {
  const replies = [
    request => proofReply(request, { sourceEpoch: 'wrong-epoch' }),
    request => proofReply(request, { sessionGeneration: request.sessionGeneration + 1 }),
    request => proofReply(request, { sessionId: 'wrong-session' }),
    request => proofReply(request, { status: 'NOT_ACCEPTED' }),
    request => ({ ...proofReply(request), exitCode: 1 }),
    () => ({ exitCode: 0, stdout: '{bad', isStdoutTruncated: false }),
    () => { throw new Error('helper-timeout') },
  ]
  for (const proof of replies) {
    const f = fixture({ proof })
    await f.fire('session.start', { isInteractive: true })
    await tick()
    await start(f, 'Unsealed-Turn')
    await complete(f, 'Unsealed-Turn')
    const records = await f.flush()
    evidence.records.push(...records)
    assert.equal(records.filter(record => record.nativeEvent === 'ownership.seal').length, 0)
    assert.equal(outcomes(records)[0].ownershipProofToken, undefined)
  }
})

const hashes = {}
for (const name of ['register.ts', 'ownership.ts', 'delivery.ts', 'latency.ts']) {
  const path = join(dirname(modulePath), name)
  try {
    hashes[name] = createHash('sha256').update(await readFile(path)).digest('hex')
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error
    hashes[name] = null
  }
}
hashes['ownership.node.mjs'] = createHash('sha256').update(await readFile(fileURLToPath(import.meta.url))).digest('hex')
const report = {
  schemaVersion: 1,
  purpose: 'F1A actual TypeScript original-ownership tests and F1B observer bridge controls',
  platform: process.platform,
  runtime: process.version,
  source: { registerModule: modulePath, hashes },
  attribution: 'Executed with in-memory host origins/traces, helper receipts and timers; not native Claude or macOS execution',
  passed: results.filter(result => result.result === 'PASS').length,
  failed: results.filter(result => result.result === 'FAIL').length,
  results,
}
if (argument('--output')) await writeFile(argument('--output'), `${JSON.stringify(report, null, 2)}\n`)
for (const result of results) {
  process.stdout.write(`${result.result} ${result.id}: ${result.description}\n`)
  if (result.error) process.stdout.write(`${result.error}\n`)
}
process.stdout.write(`${report.passed} passed; ${report.failed} failed (${process.version}, ${process.platform})\n`)
if (report.failed) process.exitCode = 1
