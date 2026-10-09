// Shared fixtures for the observer's engine-test-kit suites.
//
// The test's `on` hooks sit beneath the plugin and stand for the engine. A
// `process.run` hook stands for the capture helper's `mod-batch` command,
// which does not exist before M1/M2: it records each batch the observer
// sends and answers with the receipt a test chooses.

export const ARGV = ['/opt/threadspace/bin/threadspace-hook', 'mod-batch']
export const OPTIONS = { options: { captureArgv: ARGV } }

// Sentinels that must never appear in any batch the observer sends.
export const SECRET = {
  prompt: 'SECRET-PROMPT-4f1c',
  answer: 'SECRET-ANSWER-9a2e',
  toolInput: 'SECRET-TOOL-INPUT-77b0',
  toolOutput: 'SECRET-TOOL-OUTPUT-c3d9',
  task: 'SECRET-SPAWN-TASK-51aa',
  chunk: 'SECRET-CHUNK-TEXT-0e6b',
}

export const START = { cwd: '/private/tmp/threadspace-kit', surface: null, isInteractive: false }

export const promptInput = (over: Record<string, unknown> = {}) => ({
  text: SECRET.prompt,
  wait: false,
  origin: { kind: 'composer' },
  ...over,
})

export const toolCallInput = (toolUseId: string, over: Record<string, unknown> = {}) => ({
  tool: 'Read',
  file_path: `/private/tmp/${SECRET.toolInput}`,
  tool_use_id: toolUseId,
  ...over,
})

export const turnCompleteInput = (turnId: string, over: Record<string, unknown> = {}) => ({
  answer: SECRET.answer,
  durationMs: 12,
  isAborted: false,
  turnId,
  reason: 'answer',
  ...over,
})

export const stepInput = (turnId: string, index: number) => ({ turnId, index, model: 'claude-haiku-test', messageCount: 3 })

export const spawnInput = (toolUseId: string) => ({
  tool_use_id: toolUseId,
  prompt: SECRET.task,
  description: 'kit spawn',
  subagentType: 'general-purpose',
  provider: { plugin: 'engine', tier: 'core' },
  parentModel: 'claude-parent-test',
  background: false,
  fork: false,
})

export type Batch = {
  at: number
  argv: string[]
  budgetMs: number | undefined
  timeoutMs: number | undefined
  envelope: any
  records: any[]
}

export type Answer = { exitCode?: number; stdout?: string; reject?: string; delayMs?: number; isStdoutTruncated?: boolean }
export type Mode = (batch: Batch, call: number) => Answer

export const receipt = (batch: Batch, status: (record: any, index: number) => string = () => 'COMMITTED') =>
  JSON.stringify({
    receiptVersion: 1,
    results: batch.records.map((record, index) => ({ observationId: record.observationId, status: status(record, index) })),
  })

export const commitAll: Mode = batch => ({ stdout: receipt(batch) })

// The helper takes its budget as the argv's last two parts,
// `--budget-ms <ms>`; without them it answers a usage error and no receipt.
const budgetOf = (argv: readonly string[]): number | undefined => {
  const [flag, value] = argv.slice(-2)
  return flag === '--budget-ms' && /^[0-9]+$/.test(value ?? '') ? Number(value) : undefined
}

export function installHelper(on: any, clock: any, mode: Mode = commitAll) {
  const batches: Batch[] = []
  let active = 0
  let maxActive = 0
  let current = mode
  on('process.run', async (_$: any, e: any) => {
    active += 1
    maxActive = Math.max(maxActive, active)
    try {
      const envelope = JSON.parse(e.init?.stdin ?? 'null')
      const batch: Batch = {
        at: clock.now(),
        argv: [...e.argv],
        budgetMs: budgetOf(e.argv),
        timeoutMs: e.init?.timeoutMs,
        envelope,
        records: envelope?.records ?? [],
      }
      batches.push(batch)
      const answer: Answer = batch.budgetMs === undefined ? { exitCode: 2 } : current(batch, batches.length)
      if (answer.delayMs) await clock.sleep(answer.delayMs)
      if (answer.reject) return { deny: answer.reject }
      return {
        value: {
          exitCode: answer.exitCode ?? 0,
          stdout: answer.stdout ?? '',
          stderr: '',
          isStdoutTruncated: answer.isStdoutTruncated ?? false,
          isStderrTruncated: false,
        },
      }
    } finally {
      active -= 1
    }
  })
  return {
    batches,
    records: () => batches.flatMap(batch => batch.records),
    maxActive: () => maxActive,
    setMode: (next: Mode) => {
      current = next
    },
  }
}

// Default bottoms for the observed events: each answers as core would, and
// counts how often the chain reached it.
export function installCore(on: any, skip: readonly string[] = []) {
  const calls: Record<string, any[]> = {}
  const seen = (name: string, e: any) => (calls[name] ??= []).push(JSON.parse(JSON.stringify(e)))
  const add = (name: string, hook: any) => {
    if (!skip.includes(name)) on(name, hook)
  }
  add('session.start', (_$: any, e: any) => (seen('session.start', e), { cwd: e.cwd }))
  add('classic.SessionStart', (_$: any, e: any) => (seen('classic.SessionStart', e), {}))
  add('session.end', (_$: any, e: any) => (seen('session.end', e), { sessionId: e.sessionId }))
  add('session.attach', (_$: any, e: any) => (seen('session.attach', e), { clientId: e.clientId }))
  add('session.detach', (_$: any, e: any) => (seen('session.detach', e), { clientId: e.clientId }))
  add('prompt.submit', (_$: any, e: any) => (seen('prompt.submit', e), { text: e.text, origin: e.origin }))
  add('turn.start', (_$: any, e: any) => (seen('turn.start', e), { turnId: e.turnId }))
  add('turn.complete', (_$: any, e: any) => (seen('turn.complete', e), { text: e.answer }))
  add('tool.call', (_$: any, e: any) => (seen('tool.call', e), { result: { content: SECRET.toolOutput }, text: SECRET.toolOutput, ref: 7 }))
  add('tool.check', (_$: any, e: any) => (seen('tool.check', e), { decision: 'allow' }))
  add('agent.spawn', (_$: any, e: any) => (seen('agent.spawn', e), { model: 'claude-sub-test', agentId: `agent-for-${e.tool_use_id}` }))
  add('session.id', () => ({ value: 'S-host' }))
  add('session.version', () => ({ value: { version: '2.1.291', base: '2.1.291', builtAt: '2026-10-05T00:00:00Z' } }))
  return { calls, count: (name: string) => calls[name]?.length ?? 0 }
}
