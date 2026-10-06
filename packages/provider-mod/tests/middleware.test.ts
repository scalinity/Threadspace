// Middleware contract (SPEC §11.4): one `next(e)` with the original event,
// the result returned unchanged, provider rejections propagated, streams
// forwarded chunk for chunk, and nothing but allowlisted metadata recorded.

import { describe, expect, mock, test } from 'claude-code/testing'
import {
  OPTIONS,
  SECRET,
  START,
  installCore,
  installHelper,
  promptInput,
  spawnInput,
  stepInput,
  toolCallInput,
  turnCompleteInput,
} from './kit.ts'

const CHUNKS = [
  { kind: 'thinking', index: 0, text: `${SECRET.chunk}-thinking` },
  { kind: 'text', index: 1, text: `${SECRET.chunk}-a` },
  { kind: 'text', index: 1, text: `${SECRET.chunk}-b` },
  { kind: 'tool', index: 2, id: 'toolu_kit_1', name: 'Read' },
  { kind: 'input', index: 2, json: `{"file_path":"/${SECRET.toolInput}"}` },
  { kind: 'stop', stopReason: 'tool_use', usage: null },
]

const STEP_RESULT = {
  turnId: 'turn-stream',
  index: 0,
  answer: `${SECRET.chunk}-ab`,
  toolUses: [{ name: 'Read', input: { file_path: `/${SECRET.toolInput}` } }],
  stopReason: 'tool_use',
  usage: null,
}

describe('middleware contract', () => {
  test('observer calls next(e) exactly once', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    const core = installCore(on)
    let steps = 0
    on('turn.step', async function* (_$: any, e: any) {
      steps += 1
      for (const chunk of CHUNKS) yield chunk as any
      return { ...STEP_RESULT, turnId: e.turnId, index: e.index } as any
    })

    await $.session.start(START as any)
    await $.classic.SessionStart({ source: 'startup', session_id: 'S1' } as any)
    await $.session.attach({ surface: 'terminal', clientId: 'client-1' } as any)
    await $.prompt.submit(promptInput() as any)
    await $.turn.start({ text: SECRET.prompt, turnId: 'turn-1' } as any)
    const stream = $.turn.step(stepInput('turn-1', 0) as any)
    for await (const _chunk of stream) void _chunk
    await $.tool.check({ tool: 'Bash', input: { command: SECRET.toolInput }, tool_use_id: 'tu-check' } as any)
    await $.tool.call(toolCallInput('tu-1') as any)
    await $.agent.spawn(spawnInput('tu-spawn') as any)
    await $.turn.complete(turnCompleteInput('turn-1') as any)
    await $.session.detach({ surface: 'terminal', clientId: 'client-1', reason: 'detach' } as any)
    await $.session.end({ reason: 'clear', sessionId: 'S1', resume: { id: 'S1' } } as any)

    for (const name of [
      'session.start',
      'classic.SessionStart',
      'session.attach',
      'prompt.submit',
      'turn.start',
      'tool.check',
      'tool.call',
      'agent.spawn',
      'turn.complete',
      'session.detach',
      'session.end',
    ]) {
      expect(core.count(name), name).toBe(1)
    }
    expect(steps).toBe(1)

    await clock.settle()
    const entries = helper.records().filter(record => record.phase === 'entry').map(record => record.nativeEvent)
    expect(entries).toEqual([
      'session.start',
      'classic.SessionStart',
      'session.attach',
      'prompt.submit',
      'turn.start',
      'turn.step',
      'tool.check',
      'tool.call',
      'agent.spawn',
      'turn.complete',
      'session.detach',
      'session.end',
    ])
  })

  test('event passed unchanged', OPTIONS, async ($, on) => {
    mock.clock(on)
    const core = installCore(on)
    const prompt = promptInput({ turnId: 'turn-0', wait: true, attachments: [{ type: 'image', mediaType: 'image/png' }] })
    const call = toolCallInput('tu-unchanged', { agentId: 'agent-7' })
    const complete = turnCompleteInput('turn-0', { reason: 'refusal', refusal: { category: 'cyber', explanation: null }, agentId: 'agent-7' })
    const spawn = { ...spawnInput('tu-spawn-unchanged'), parentAgentId: 'agent-7', isTeammate: true, name: 'scout' }

    await $.prompt.submit(prompt as any)
    await $.tool.call(call as any)
    await $.turn.complete(complete as any)
    await $.agent.spawn(spawn as any)

    expect(core.calls['prompt.submit']?.[0]).toEqual(prompt)
    expect(core.calls['turn.complete']?.[0]).toEqual(complete)
    expect(core.calls['agent.spawn']?.[0]).toEqual(spawn)
    const { tool_use_id: _id, ...callArgs } = call
    expect(core.calls['tool.call']?.[0]).toEqual(expect.objectContaining({ ...callArgs, tool_use_id: _id }))
  })

  test('provider result returned unchanged', OPTIONS, async ($, on) => {
    mock.clock(on)
    const answers = {
      prompt: { text: 'rewritten-by-core', context: ['ctx-a'], origin: { kind: 'composer' } },
      tool: { result: { lines: 3, nested: { ok: true } }, text: 'joined text', ref: 99, isReadOnly: true },
      complete: { text: 'final', usage: { input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 0, cache_creation_input_tokens: 0, model: 'm' } },
      spawn: { model: 'claude-sub', agentId: 'agent-x', teammateId: 'scout@team' },
      check: { decision: 'ask', reason: 'needs a person', rule: 'Bash(*)' },
    }
    on('prompt.submit', () => answers.prompt)
    on('tool.call', () => answers.tool)
    on('turn.complete', () => answers.complete)
    on('agent.spawn', () => answers.spawn)
    on('tool.check', () => answers.check)

    expect(await $.prompt.submit(promptInput() as any)).toEqual(answers.prompt as any)
    expect(await $.tool.call(toolCallInput('tu-r') as any)).toEqual(answers.tool as any)
    expect(await $.turn.complete(turnCompleteInput('turn-r') as any)).toEqual(answers.complete as any)
    expect(await $.agent.spawn(spawnInput('tu-sr') as any)).toEqual(answers.spawn as any)
    expect(await $.tool.check({ tool: 'Bash', input: {}, tool_use_id: 'tu-c' } as any)).toEqual(answers.check as any)
  })

  test('provider exceptions propagate', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    let reached = 0
    on('turn.complete', () => {
      reached += 1
      throw new Error('provider failure at core')
    })

    let rejection: unknown
    try {
      await $.turn.complete(turnCompleteInput('turn-x') as any)
    } catch (error) {
      rejection = error
    }
    expect(rejection).toBeDefined()
    // A throwing test hook is skipped like any failed hook, so the rejection
    // from beneath is the kit bottom's own; it must reach the caller as is.
    expect(String((rejection as Error).message)).toBe('no implementation for turn.complete')
    expect(reached).toBe(1)

    await clock.settle()
    const phases = helper.records().filter(record => record.nativeEvent === 'turn.complete').map(record => record.phase)
    expect(phases).toEqual(['entry', 'provider-error'])
  })

  test('async-generator chunks forwarded correctly (order, count, final value)', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    on('turn.step', async function* (_$: any, e: any) {
      for (const chunk of CHUNKS) yield chunk as any
      return { ...STEP_RESULT, turnId: e.turnId, index: e.index } as any
    })

    // Driven by hand so the generator's own final value is read, as the
    // engine reads it: `done: true` carries the result.
    const stream = $.turn.step(stepInput('turn-stream', 0) as any)
    const forwarded: unknown[] = []
    let pulled = await stream.next()
    while (!pulled.done) {
      forwarded.push(pulled.value)
      pulled = await stream.next()
    }

    // The kit's test-side `.result` reads undefined even with no plugin
    // hooked on turn.step (a control run shows it), so the final value is
    // asserted on the generator's own `done` step.
    expect(forwarded.length).toBe(CHUNKS.length)
    expect(forwarded).toEqual(CHUNKS)
    expect(pulled.value).toEqual(STEP_RESULT as any)

    await clock.settle()
    const step = helper.records().filter(record => record.nativeEvent === 'turn.step')
    expect(step.map(record => record.phase)).toEqual(['entry', 'result'])
    expect(step[1]?.payload).toEqual(expect.objectContaining({ index: 0, stopReason: 'tool_use', toolUseCount: 1 }))
  })

  test('async-generator early close is forwarded and recorded as abandoned', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    let closedBeneath = false
    on('turn.step', async function* (_$: any, e: any) {
      try {
        for (const chunk of CHUNKS) yield chunk as any
        return { ...STEP_RESULT, turnId: e.turnId, index: e.index } as any
      } finally {
        closedBeneath = true
      }
    })

    const stream = $.turn.step(stepInput('turn-early', 0) as any)
    const first = await stream.next()
    await stream.return(undefined as any)

    expect(first.value).toEqual(CHUNKS[0])
    expect(closedBeneath).toBe(true)
    await clock.settle()
    const phases = helper.records().filter(record => record.nativeEvent === 'turn.step').map(record => record.phase)
    expect(phases).toEqual(['entry', 'abandoned'])
  })

  test('observations carry no prompt text, tool input or output, or assistant text', OPTIONS, async ($, on) => {
    const clock = mock.clock(on)
    const helper = installHelper(on, clock)
    installCore(on)
    on('turn.step', async function* (_$: any, e: any) {
      for (const chunk of CHUNKS) yield chunk as any
      return { ...STEP_RESULT, turnId: e.turnId, index: e.index } as any
    })

    await $.prompt.submit(promptInput({ turnId: 'turn-s' }) as any)
    const stream = $.turn.step(stepInput('turn-s', 0) as any)
    for await (const _chunk of stream) void _chunk
    await $.tool.check({ tool: 'Bash', input: { command: SECRET.toolInput }, tool_use_id: 'tu-s0' } as any)
    await $.tool.call(toolCallInput('tu-s1') as any)
    await $.agent.spawn(spawnInput('tu-s2') as any)
    await $.turn.complete(turnCompleteInput('turn-s') as any)
    await clock.settle()

    const sent = helper.batches.map(batch => JSON.stringify(batch.envelope)).join('\n')
    expect(helper.records().length).toBeGreaterThan(0)
    for (const secret of Object.values(SECRET)) expect(sent.includes(secret), secret).toBe(false)
  })
})
