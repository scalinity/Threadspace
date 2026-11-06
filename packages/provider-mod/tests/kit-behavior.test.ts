// 2.1.318 test-kit controls: tool.call performs a permission preflight even
// above a mocked tool bottom. These distinguish nested dispatch from a
// duplicate observer load and pin why the delivery/provenance fixtures
// count or select tool.call and tool.check separately.
import { expect, mock, test } from 'claude-code/testing'
import { OPTIONS, installHelper, toolCallInput } from './kit.ts'

test('tool call preflight is one nested check in one observer epoch', OPTIONS, async ($, on) => {
  const clock = mock.clock(on)
  const helper = installHelper(on, clock)
  const seen: string[] = []
  on('tool.check', (_$: any, e: any) => {
    seen.push(`check:${e.tool_use_id}`)
    return { decision: 'allow' }
  })
  const result = { result: { fixture: true }, text: 'fixture', ref: 71 }
  on('tool.call', (_$: any, e: any) => {
    seen.push(`call:${e.tool_use_id}`)
    return result
  })
  expect(await $.tool.call(toolCallInput('tu-probe') as any)).toEqual(result as any)
  await clock.settle()
  expect(seen).toEqual(['check:tu-probe', 'call:tu-probe'])
  const records = helper.records().filter(r => r.nativeOccurrenceId === 'tu-probe')
  expect(records.map(r => [r.nativeEvent, r.phase])).toEqual([
    ['tool.call', 'entry'], ['tool.check', 'entry'], ['tool.check', 'result'], ['tool.call', 'result'],
  ])
  expect(new Set(records.map(r => r.sourceEpoch)).size).toBe(1)
})

test('ask preflight prevents the mocked call, allow reaches it', OPTIONS, async ($, on) => {
  mock.clock(on)
  let decision = 'ask'
  let calls = 0
  on('tool.check', () => ({ decision, reason: 'fixture ask' }))
  const result = { result: { fixture: true }, text: 'fixture', ref: 71 }
  on('tool.call', () => { calls += 1; return result })
  const denied = await $.tool.call(toolCallInput('tu-deny') as any)
  expect(calls).toBe(0)
  expect(denied.isError).toBe(true)
  expect(denied.text).toContain('Permission to use Read has been denied')
  decision = 'allow'
  expect(await $.tool.call(toolCallInput('tu-allow') as any)).toEqual(result as any)
  expect(calls).toBe(1)
})

// Capture the kit's trace directly in a separate plugin beneath the
// observer: a mock tool bottom now belongs to the engine's call pipeline;
// a generic mocked prompt still belongs to test/builtin. The observer must
// preserve both and must never upgrade a shortcut to engine/core.
const traceProbe = {
  name: 'kit-trace-probe',
  tier: 'append' as const,
  register: (on: any) => {
    on('tool.call', async ($: any, e: any, next: any) => {
      const result = await next(e)
      await $.store.set('tool.call', next.trace)
      return result
    })
    on('prompt.submit', async ($: any, e: any, next: any) => {
      const result = await next(e)
      await $.store.set('prompt.submit', next.trace)
      return result
    })
  },
}

test('mocked tool bottom has a core trace; generic mock has a test trace', { ...OPTIONS, plugins: [traceProbe] }, async ($, on) => {
  mock.clock(on)
  const traces: Record<string, unknown> = {}
  on('store.set', (_$: any, e: any) => {
    traces[e.key] = e.value.map(({ plugin, tier, outcome }: any) => ({ plugin, tier, outcome }))
    return { value: undefined }
  })
  on('tool.check', () => ({ decision: 'allow' }))
  on('tool.call', () => ({ result: 'fixture', text: 'fixture' }))
  on('prompt.submit', (_$: any, e: any) => ({ text: e.text, origin: e.origin }))
  await $.tool.call(toolCallInput('tu-trace') as any)
  await $.prompt.submit({ text: 'fixture', origin: { kind: 'composer' } } as any)
  expect(traces['tool.call']).toEqual([{ plugin: 'engine', tier: 'core', outcome: 'returned' }])
  expect(traces['prompt.submit']).toEqual([{ plugin: 'test', tier: 'builtin', outcome: 'returned' }])
})
