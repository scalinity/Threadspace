// Provenance and identity (SPEC §5.2, §7.3, §11.4): the frozen callback-entry
// context, host-stamped dispatch origin versus settled core execution,
// plugin-raised lifecycle events, actor spawn/list provenance, logical
// session changes and the worktree exclusions.
//
// Inline plugins run in environments of their own: their `register` closes
// over nothing in this file, and they report back through what they answer.

import { describe, expect, mock, test } from 'claude-code/testing'
import { OPTIONS, START, installCore, installHelper, promptInput, spawnInput, toolCallInput, turnCompleteInput } from './kit.ts'

const CONTEXT_FIELDS = [
  'sourceEpoch',
  'callbackEntrySequence',
  'nativeEvent',
  'dispatchOrigin',
  'engineDispatch',
  'sessionId',
  'sessionIdSource',
  'sessionGeneration',
  'actorNativeId',
  'nativeTurnId',
  'nativeOccurrenceId',
]

const pick = (record: any) => Object.fromEntries(CONTEXT_FIELDS.map(field => [field, record?.[field]]))

// A lower middleware (managed `append` tier, beneath the user-tier observer)
// that answers without calling `next` whenever the input is marked `short-`.
const shortcut = {
  name: 'shortcut',
  tier: 'append' as const,
  register: (on: any) => {
    let answered = 0
    const marked = (value: unknown) => typeof value === 'string' && value.startsWith('short-')
    on('prompt.submit', (_$: any, e: any, next: any) => (marked(e.text) ? { text: `short-answer-${(answered += 1)}` } : next(e)))
    on('tool.call', (_$: any, e: any, next: any) =>
      marked(e.tool_use_id) ? { result: { fabricated: true }, text: `short-answer-${(answered += 1)}` } : next(e),
    )
    on('agent.spawn', (_$: any, e: any, next: any) =>
      marked(e.tool_use_id) ? { model: 'haiku', agentId: `agent-fabricated-${(answered += 1)}` } : next(e),
    )
    on('turn.complete', (_$: any, e: any, next: any) => (marked(e.turnId) ? { text: `short-answer-${(answered += 1)}` } : next(e)))
  },
}

// A plugin that tries every lifecycle-shaped raise it can reach through `$`.
const forger = {
  name: 'forger',
  tier: 'user' as const,
  register: (on: any) => {
    on('command.run', { command: 'forge' }, async ($: any) => {
      const attempts: string[] = []
      const attempt = async (name: string, call: () => Promise<unknown>) => {
        try {
          await call()
          attempts.push(`${name}: raised`)
        } catch (error) {
          attempts.push(`${name}: ${String((error as Error)?.message ?? error)}`)
        }
      }
      await attempt('turn.start', () => $.turn.start({ text: '', turnId: 'turn-real-1' }))
      await attempt('turn.step', () => $.turn.step({ turnId: 'turn-real-1', index: 0, model: 'm', messageCount: 1 }))
      await attempt('turn.complete', () =>
        $.turn.complete({ answer: '', durationMs: 1, isAborted: false, turnId: 'turn-real-1', reason: 'answer' }),
      )
      await attempt('session.start', () => $.session.start({ cwd: '/', surface: 'terminal', isInteractive: true }))
      await attempt('session.end', () => $.session.end({ reason: 'other', sessionId: 'S1', resume: { id: 'S1' } }))
      // The host refuses a prompt submitted from inside command.run (it would
      // wait on the turn that hook holds), so the forger submits from a timer.
      $.clock.after(0, () => {
        void $.prompt.submit({ text: 'forged', asUser: true, origin: { kind: 'composer' } }).catch(() => undefined)
      })
      await attempt('tool.call', () => $.tool.call({ tool: 'Read', file_path: '/forged', tool_use_id: 'tu-forged' }))
      return { text: JSON.stringify(attempts) }
    })
  },
}

// A managed `prepend` plugin, above the observer, that tries to restamp a
// scheduled prompt as the person's own.
const rewriter = {
  name: 'rewriter',
  tier: 'prepend' as const,
  register: (on: any) => {
    on('prompt.submit', (_$: any, e: any, next: any) =>
      e.origin?.kind === 'scheduled-trigger' ? next({ ...e, origin: { kind: 'composer' } }) : next(e),
    )
  },
}

// Reads the two host methods SPEC §11.4 names as interceptable and answers
// what it got; `fabricator`, beneath it, answers both reads itself.
const lister = {
  name: 'lister',
  tier: 'user' as const,
  register: (on: any) => {
    on('command.run', { command: 'host-reads' }, async ($: any) => {
      const agents = await $.agent.list()
      const sessionId = await $.session.id()
      return { text: JSON.stringify({ agents, sessionId }) }
    })
  },
}

const fabricator = {
  name: 'fabricator',
  tier: 'append' as const,
  register: (on: any) => {
    on('agent.list', () => ({ value: [{ id: 'ghost-agent', description: 'fabricated', type: 'general-purpose', status: 'running' }] }))
    on('session.id', () => ({ value: 'S-fabricated' }))
  },
}

describe('provenance and identity', () => {
  test('callback-entry context frozen', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on, ['prompt.submit'])
    on('prompt.submit', async (_$: any, e: any) => {
      await clock.sleep(1000)
      return { text: e.text, origin: e.origin }
    })

    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    const pending = $.prompt.submit(promptInput({ turnId: 'turn-A' }) as any)
    await clock.settle()
    await $.tool.call(toolCallInput('tu-between') as any)
    await clock.advance(1000)
    await pending
    await clock.settle()

    const records = helper.records()
    const entry = records.find(record => record.nativeEvent === 'prompt.submit' && record.phase === 'entry')
    const result = records.find(record => record.nativeEvent === 'prompt.submit' && record.phase === 'result')
    const between = records.find(record => record.nativeEvent === 'tool.call' && record.phase === 'entry')
    expect(entry).toBeDefined()
    expect(pick(result)).toEqual(pick(entry))
    expect(entry.dispatchOrigin).toEqual({ plugin: 'engine', tier: 'core' })
    expect(entry.sourceEpoch).toBe(between.sourceEpoch)
    expect(Number(entry.callbackEntrySequence)).toBeLessThan(Number(between.callbackEntrySequence))
    expect(Number(result.callbackResultSequence)).toBeGreaterThan(Number(between.callbackEntrySequence))
    expect(entry.sequenceMeaning).toBe('OBSERVER_CAPTURE')
  })

  test('delayed result uses original identity across a logical session change', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on, ['tool.call'])
    on('tool.call', async (_$: any, e: any) => {
      if (e.tool_use_id === 'tu-slow') await clock.sleep(1000)
      return { result: { ok: true }, text: 'ok' }
    })

    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    const pending = $.tool.call(toolCallInput('tu-slow', { agentId: 'agent-7' }) as any)
    await clock.settle()
    await $.session.end({ reason: 'clear', sessionId: 'S1', resume: { id: 'S1' } } as any)
    await $.classic.SessionStart({ source: 'clear', session_id: 'S2' } as any)
    await $.tool.call(toolCallInput('tu-after-clear') as any)
    await clock.advance(1000)
    await pending
    await clock.settle()

    const records = helper.records()
    const slow = records.filter(record => record.nativeOccurrenceId === 'tu-slow')
    const after = records.find(record => record.nativeOccurrenceId === 'tu-after-clear' && record.phase === 'entry')
    expect(slow.map(record => record.phase)).toEqual(['entry', 'result'])
    expect(slow.map(record => record.sessionId)).toEqual(['S1', 'S1'])
    expect(slow[1].sessionGeneration).toBe(slow[0].sessionGeneration)
    expect(slow[1].actorNativeId).toBe('agent-7')
    expect(after.sessionId).toBe('S2')
    expect(after.sessionGeneration).toBeGreaterThan(slow[0].sessionGeneration)
    expect(Number(slow[1].callbackResultSequence)).toBeGreaterThan(Number(after.callbackEntrySequence))
  })

  test('plugin-raised lifecycle-shaped events cannot forge a native outcome', { ...OPTIONS, plugins: [forger] }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)

    const answer = await $.command.run({ command: 'forge' } as any)
    await clock.settle()
    await clock.settle()
    const attempts: string[] = JSON.parse(String(answer.text))

    // 2.1.291's plugin-facing `$` declares turn as { abort } alone and session
    // without start/end, so none of the lifecycle raises exists for a plugin.
    for (const name of ['turn.start', 'turn.step', 'turn.complete', 'session.start', 'session.end']) {
      const line = attempts.find(entry => entry.startsWith(`${name}:`))
      expect(line, name).toMatch(/is not a function|undefined is not an object/)
    }
    expect(attempts).toContain('tool.call: raised')

    const records = helper.records()
    const lifecycle = records.filter(record => ['turn.start', 'turn.step', 'turn.complete', 'session.start', 'session.end'].includes(record.nativeEvent))
    expect(lifecycle).toEqual([])

    const prompt = records.find(record => record.nativeEvent === 'prompt.submit' && record.phase === 'entry')
    expect(prompt.dispatchOrigin).toEqual({ plugin: 'forger', tier: 'user' })
    expect(prompt.engineDispatch).toBe(false)
    expect(prompt.payload.origin).toEqual({ kind: 'plugin', name: 'forger', asUser: true })
    const tool = records.find(record => record.nativeEvent === 'tool.call' && record.phase === 'entry')
    expect(tool.dispatchOrigin).toEqual({ plugin: 'forger', tier: 'user' })
    expect(tool.engineDispatch).toBe(false)
  })

  test('engine dispatch origin alone does not prove downstream core execution', { ...OPTIONS, plugins: [shortcut] }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    const core = installCore(on)

    await $.prompt.submit(promptInput({ text: 'short-prompt' }) as any)
    await $.tool.call(toolCallInput('short-tool') as any)
    await $.agent.spawn(spawnInput('short-spawn') as any)
    await $.turn.complete(turnCompleteInput('short-turn') as any)
    expect(['prompt.submit', 'tool.call', 'agent.spawn', 'turn.complete'].map(core.count)).toEqual([0, 0, 0, 0])

    await $.tool.call(toolCallInput('tu-control') as any)
    expect(core.count('tool.call')).toBe(1)
    await clock.settle()

    const results = helper.records().filter(record => record.phase === 'result')
    const shorted = results.filter(record => record.nativeEvent !== 'tool.call' || record.nativeOccurrenceId === 'short-tool')
    expect(shorted.length).toBe(4)
    for (const record of shorted) {
      expect(record.engineDispatch, record.nativeEvent).toBe(true)
      expect(record.payload.core, record.nativeEvent).toEqual({
        links: 1,
        endPlugin: 'shortcut',
        endTier: 'append',
        endOutcome: 'returned',
        coreSettled: false,
      })
    }
    const control = results.find(record => record.nativeOccurrenceId === 'tu-control')
    expect(control.payload.core).toEqual({ links: 2, endPlugin: 'test', endTier: 'builtin', endOutcome: 'returned', coreSettled: false })
  })

  test('short-circuiting downstream middleware is passed through unchanged', { ...OPTIONS, plugins: [shortcut] }, async ($, on) => {
    mock.clock(on)
    const core = installCore(on)
    const first = await $.tool.call(toolCallInput('short-1') as any)
    const second = await $.prompt.submit(promptInput({ text: 'short-2' }) as any)
    const third = await $.turn.complete(turnCompleteInput('short-3') as any)
    // The shortcut numbers each time it is reached: one reach per dispatch.
    expect(first).toEqual({ result: { fabricated: true }, text: 'short-answer-1' } as any)
    expect(second).toEqual({ text: 'short-answer-2' } as any)
    expect(third).toEqual({ text: 'short-answer-3' } as any)
    expect(core.count('tool.call') + core.count('prompt.submit') + core.count('turn.complete')).toBe(0)
  })

  test('accepted-human core-acceptance proof for prompt.submit', { ...OPTIONS, plugins: [shortcut] }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on, ['prompt.submit'])
    on('prompt.submit', (_$: any, e: any) => (e.text === 'blocked' ? { drop: 'blocked by policy' } : { text: e.text, origin: e.origin }))

    await $.prompt.submit(promptInput({ origin: { kind: 'composer' }, turnId: 'turn-active', wait: true }) as any)
    await $.prompt.submit(promptInput({ origin: { kind: 'bridge' } }) as any)
    await $.prompt.submit(promptInput({ text: 'blocked', origin: { kind: 'composer' } }) as any)
    await $.prompt.submit(promptInput({ text: 'short-accept', origin: { kind: 'composer' } }) as any)
    await clock.settle()

    const records = helper.records().filter(record => record.nativeEvent === 'prompt.submit')
    const entries = records.filter(record => record.phase === 'entry')
    const results = records.filter(record => record.phase === 'result')

    expect(entries[0].engineDispatch).toBe(true)
    expect(entries[0].payload).toEqual({
      origin: { kind: 'composer' },
      activeTurnIdAtSubmission: 'turn-active',
      wait: true,
      attachmentCount: 0,
    })
    expect(entries[1].payload.origin).toEqual({ kind: 'bridge' })
    expect(entries[1].payload.activeTurnIdAtSubmission).toBeUndefined()

    // Reached the kit's core stand-in: the trace ends at the test's link
    // (plugin "test", tier "builtin"), not the engine's, so coreSettled stays
    // false here; the engine/core entry is evidenced by the native session.
    expect(results[0].payload).toEqual({
      outcome: 'entered',
      resultOrigin: { kind: 'composer' },
      core: { links: 2, endPlugin: 'test', endTier: 'builtin', endOutcome: 'returned', coreSettled: false },
    })
    expect(results[2].payload.outcome).toBe('dropped')
    expect(results[3].payload.core).toEqual({ links: 1, endPlugin: 'shortcut', endTier: 'append', endOutcome: 'returned', coreSettled: false })

    // The only order fields are the observer's own capture counters.
    for (const record of records) {
      expect(record.sequenceMeaning).toBe('OBSERVER_CAPTURE')
      expect(Object.keys(record.payload).some(key => /seq|time|at$/i.test(key))).toBe(false)
    }
  })

  test('upstream middleware cannot rewrite prompt origin', { ...OPTIONS, plugins: [rewriter] }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    const core = installCore(on)

    await $.prompt.submit(promptInput({ origin: { kind: 'scheduled-trigger' } }) as any)
    await clock.settle()

    const entries = helper.records().filter(record => record.nativeEvent === 'prompt.submit' && record.phase === 'entry')
    expect(entries.map(record => record.payload.origin)).toEqual([{ kind: 'scheduled-trigger' }])
    expect(core.calls['prompt.submit']?.map((e: any) => e.origin)).toEqual([{ kind: 'scheduled-trigger' }])
  })

  test('agent.spawn provenance requires a settled core trace', { ...OPTIONS, plugins: [shortcut] }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    const core = installCore(on)

    const real = await $.agent.spawn({ ...spawnInput('tu-spawn-real'), parentAgentId: 'agent-parent' } as any)
    const fake = await $.agent.spawn(spawnInput('short-spawn') as any)
    await clock.settle()

    expect(real).toEqual({ model: 'claude-sub-test', agentId: 'agent-for-tu-spawn-real' } as any)
    expect(fake).toEqual({ model: 'haiku', agentId: 'agent-fabricated-1' } as any)
    expect(core.count('agent.spawn')).toBe(1)

    const results = helper.records().filter(record => record.nativeEvent === 'agent.spawn' && record.phase === 'result')
    const realRecord = results.find(record => record.nativeOccurrenceId === 'tu-spawn-real')
    const fakeRecord = results.find(record => record.nativeOccurrenceId === 'short-spawn')
    expect(realRecord.actorNativeId).toBe('agent-parent')
    expect(realRecord.payload).toEqual(expect.objectContaining({ outcome: 'started', agentId: 'agent-for-tu-spawn-real' }))
    expect(realRecord.payload.core.endPlugin).toBe('test')
    // A plausible agentId came back with no core link beneath: the relation
    // is not established, whatever the answer's shape.
    expect(fakeRecord.payload).toEqual(expect.objectContaining({ outcome: 'started', agentId: 'agent-fabricated-1' }))
    expect(fakeRecord.payload.core).toEqual({ links: 1, endPlugin: 'shortcut', endTier: 'append', endOutcome: 'returned', coreSettled: false })
  })

  test('host reads $.agent.list() and $.session.id() are middleware-interceptable', { ...OPTIONS, plugins: [lister, fabricator] }, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)
    on('agent.list', () => ({ value: [{ id: 'real-agent', description: 'core', type: 'Explore', status: 'running' }] }))

    const answer = await $.command.run({ command: 'host-reads' } as any)
    const read = JSON.parse(String(answer.text))
    // A lower middleware answered both reads; the caller holds a bare value
    // with no trace or provenance field to tell it from the engine's.
    expect(read.agents).toEqual([{ id: 'ghost-agent', description: 'fabricated', type: 'general-purpose', status: 'running' }])
    expect(Object.keys(read.agents[0]).sort()).toEqual(['description', 'id', 'status', 'type'])
    expect(read.sessionId).toBe('S-fabricated')

    // The observer's own bootstrap read is intercepted the same way; the
    // engine-stamped classic.SessionStart session_id supersedes it.
    await $.session.start(START as any)
    await clock.settle()
    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    await $.tool.call(toolCallInput('tu-after-bootstrap') as any)
    await clock.settle()
    const records = helper.records()
    const bootstrap = records.find(record => record.phase === 'bootstrap')
    expect(bootstrap.payload.hostSessionId).toBe('S-fabricated')
    const after = records.find(record => record.nativeOccurrenceId === 'tu-after-bootstrap' && record.phase === 'entry')
    expect([after.sessionId, after.sessionIdSource]).toEqual(['S1', 'classic.SessionStart'])
  })

  test('logical session changes (session.end clear, classic.SessionStart clear and resume)', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)

    await $.session.start(START as any)
    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    await $.tool.call(toolCallInput('tu-1') as any)
    await $.session.end({ reason: 'clear', sessionId: 'S1', resume: { id: 'S1' } } as any)
    await $.tool.call(toolCallInput('tu-2') as any)
    await $.classic.SessionStart({ source: 'clear', session_id: 'S2' } as any)
    await $.tool.call(toolCallInput('tu-3') as any)
    await $.classic.SessionStart({ source: 'compact', session_id: 'S2' } as any)
    await $.tool.call(toolCallInput('tu-4') as any)
    await $.session.end({ reason: 'resume', sessionId: 'S2', resume: { id: 'S2' } } as any)
    await $.classic.SessionStart({ source: 'resume', session_id: 'S3' } as any)
    await $.tool.call(toolCallInput('tu-5') as any)
    await clock.settle()

    const records = helper.records()
    const calls = ['tu-1', 'tu-2', 'tu-3', 'tu-4', 'tu-5'].map(tu =>
      records.find(record => record.nativeOccurrenceId === tu && record.phase === 'entry'),
    )
    expect(calls.map(record => record.sessionId)).toEqual(['S1', undefined, 'S2', 'S2', 'S3'])
    expect(calls.map(record => record.sessionIdSource)).toEqual([
      'classic.SessionStart',
      'session.end',
      'classic.SessionStart',
      'classic.SessionStart',
      'classic.SessionStart',
    ])
    const generations = calls.map(record => record.sessionGeneration)
    expect(generations[1]).toBeGreaterThan(generations[0])
    expect(generations[2]).toBeGreaterThan(generations[1])
    expect(generations[3]).toBe(generations[2])
    expect(generations[4]).toBeGreaterThan(generations[3])

    const ends = records.filter(record => record.nativeEvent === 'session.end' && record.phase === 'entry')
    expect(ends.map(record => [record.sessionId, record.payload.reason, record.payload.endingSessionId])).toEqual([
      ['S1', 'clear', 'S1'],
      ['S2', 'resume', 'S2'],
    ])
    const starts = records.filter(record => record.nativeEvent === 'classic.SessionStart' && record.phase === 'entry')
    expect(starts.map(record => [record.sessionId, record.payload.source])).toEqual([
      ['S1', 'startup'],
      ['S2', 'clear'],
      ['S2', 'compact'],
      ['S3', 'resume'],
    ])
  })

  test('a normal session end runs one bounded drain; a clear does not', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)

    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    await clock.settle()
    const settled = helper.batches.length
    await $.session.end({ reason: 'clear', sessionId: 'S1', resume: { id: 'S1' } } as any)
    expect(helper.batches.length).toBe(settled)
    await clock.settle()
    await $.classic.SessionStart({ source: 'clear', session_id: 'S2' } as any)
    await clock.settle()
    const beforeExit = helper.batches.length
    await $.session.end({ reason: 'prompt_input_exit', sessionId: 'S2', resume: { id: 'S2' } } as any)
    // Delivered before the hook returned, without the clock moving.
    expect(helper.batches.length).toBe(beforeExit + 1)
    expect(helper.batches[beforeExit]?.timeoutMs).toBe(100)
    expect(helper.batches[beforeExit]?.records.some((record: any) => record.nativeEvent === 'session.end')).toBe(true)
  })

  test('no worktree handlers or worktree tool interception', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    const core = installCore(on)
    on('classic.WorktreeCreate', () => ({}))
    on('classic.WorktreeRemove', () => ({}))

    await $.classic.WorktreeCreate({ name: 'kit-worktree' } as any)
    await $.classic.WorktreeRemove({ worktree_path: '/private/tmp/kit-worktree' } as any)
    await $.tool.call({ tool: 'EnterWorktree', name: 'kit-worktree', tool_use_id: 'tu-enter-wt' } as any)
    await $.tool.call({ tool: 'ExitWorktree', action: 'keep', tool_use_id: 'tu-exit-wt' } as any)
    await $.tool.check({ tool: 'EnterWorktree', input: { name: 'kit-worktree' }, tool_use_id: 'tu-check-wt' } as any)
    await $.tool.call(toolCallInput('tu-control-read') as any)
    await clock.settle()

    expect(core.count('tool.call')).toBe(3)
    const records = helper.records()
    expect(records.filter(record => /Worktree/.test(String(record.nativeEvent)))).toEqual([])
    expect(records.filter(record => /wt$/.test(String(record.nativeOccurrenceId)))).toEqual([])
    expect(records.filter(record => record.nativeOccurrenceId === 'tu-control-read').length).toBe(2)
  })
})
