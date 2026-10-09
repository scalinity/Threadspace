// Threadspace passive observer mod (SPEC §11.4, §8.3).
//
// Every hook calls `next(e)` exactly once with the event it received,
// returns that result unchanged and lets a rejection from beneath propagate.
// Only the observer's own work is guarded. At callback entry the hook
// freezes its context (source epoch, entry sequence, host-stamped
// `next.origin`, the logical session and the IDs the event supplies) before
// awaiting anything; the result record reuses that context. Records carry
// allowlisted metadata only: no prompt text, tool input or output, or
// assistant text. No handler exists for WorktreeCreate or WorktreeRemove,
// and the tool hooks are not seated for the worktree tools.

import type { Origin, PluginOptions, PromptOrigin, Register, TraceEntry } from 'claude-code'
import { createDelivery } from './delivery.ts'

type Scalar = string | number | boolean | null | undefined
type Payload = { readonly [key: string]: Scalar | { readonly [key: string]: Scalar } }

type EntryContext = Readonly<{
  sourceEpoch: string
  callbackEntrySequence: string
  nativeEvent: string
  dispatchOrigin: Readonly<{ plugin: string; tier: string }>
  engineDispatch: boolean
  sessionId: string | undefined
  sessionIdSource: string | undefined
  sessionGeneration: number
  actorNativeId: string | undefined
  nativeTurnId: string | undefined
  nativeOccurrenceId: string | undefined
}>

type Ids = { turn?: string; actor?: string; occurrence?: string }

const ADAPTER_ID = 'threadspace-observer'
const ADAPTER_VERSION = '0.1.0'
const MAX_ID_CHARS = 256
const DETAIL_EVENTS: readonly string[] = ['turn.step', 'tool.call', 'tool.check']

// The session value holding the source epoch of the load that last
// bootstrapped (types/index.d.ts). It survives a hot reload; module
// variables do not.
const EPOCH = { plugin: 'threadspace-observer', key: 'epoch' } as const

const id = (value: unknown): string | undefined => (typeof value === 'string' ? value.slice(0, MAX_ID_CHARS) : undefined)

const promptOrigin = (origin: PromptOrigin | undefined): Payload[string] => {
  if (!origin) return undefined
  return {
    kind: id(origin.kind),
    name: origin.kind === 'plugin' ? id(origin.name) : undefined,
    asUser: origin.kind === 'plugin' ? origin.asUser === true : undefined,
    server: origin.kind === 'channel' ? id(origin.server) : undefined,
  }
}

// The allowlisted proof of what settled beneath a hook: the chain's last
// link and whether it is the engine's own core. A lower middleware that
// answered without `next` leaves the trace ending at itself.
const traceProof = (trace: readonly TraceEntry[]): Payload[string] => {
  const last = trace[trace.length - 1]
  return {
    links: trace.length,
    endPlugin: last ? id(last.plugin) : null,
    endTier: last ? last.tier : null,
    endOutcome: last ? last.outcome : null,
    coreSettled:
      last !== undefined && last.plugin === 'engine' && last.tier === 'core' && (last.outcome === 'returned' || last.outcome === 'passed'),
  }
}

const readArgv = (value: PluginOptions[string] | undefined): readonly string[] | undefined => {
  if (!Array.isArray(value) || value.length === 0) return undefined
  return value.every(part => typeof part === 'string' && part.length > 0) ? [...value] : undefined
}

export const register: Register = (on, options) => {
  const sourceEpoch = crypto.randomUUID()
  const delivery = createDelivery(readArgv(options.captureArgv), sourceEpoch)
  let sequence = 0
  let session: { id: string | undefined; source: string | undefined; generation: number } = {
    id: undefined,
    source: undefined,
    generation: 0,
  }

  const changeSession = (nextId: string | undefined, source: string): void => {
    if (nextId !== undefined && nextId === session.id) return
    session = { id: nextId, source, generation: session.generation + 1 }
  }

  const emit = (ctx: EntryContext, phase: string, resultSequence: string | undefined, payload: () => Payload): void => {
    try {
      const observationId = crypto.randomUUID()
      delivery.enqueue(
        observationId,
        {
          schemaVersion: 1,
          observationId,
          adapterId: ADAPTER_ID,
          adapterVersion: ADAPTER_VERSION,
          sourceEpoch: ctx.sourceEpoch,
          sequenceMeaning: 'OBSERVER_CAPTURE',
          callbackEntrySequence: ctx.callbackEntrySequence,
          callbackResultSequence: resultSequence,
          phase,
          nativeEvent: ctx.nativeEvent,
          dispatchOrigin: ctx.dispatchOrigin,
          engineDispatch: ctx.engineDispatch,
          sessionId: ctx.sessionId,
          sessionIdSource: ctx.sessionIdSource,
          sessionGeneration: ctx.sessionGeneration,
          actorNativeId: ctx.actorNativeId,
          nativeTurnId: ctx.nativeTurnId,
          nativeOccurrenceId: ctx.nativeOccurrenceId,
          payload: payload(),
        },
        DETAIL_EVENTS.includes(ctx.nativeEvent),
      )
    } catch {
      // An observer failure never reaches the provider path.
    }
  }

  const enter = (nativeEvent: string, origin: Origin, ids: Ids, payload: () => Payload): EntryContext | undefined => {
    let ctx: EntryContext
    try {
      ctx = Object.freeze({
        sourceEpoch,
        callbackEntrySequence: String((sequence += 1)),
        nativeEvent,
        dispatchOrigin: Object.freeze({ plugin: id(origin.plugin) ?? '', tier: String(origin.tier) }),
        engineDispatch: origin.plugin === 'engine' && origin.tier === 'core',
        sessionId: session.id,
        sessionIdSource: session.source,
        sessionGeneration: session.generation,
        actorNativeId: id(ids.actor),
        nativeTurnId: id(ids.turn),
        nativeOccurrenceId: id(ids.occurrence),
      })
    } catch {
      return undefined
    }
    emit(ctx, 'entry', undefined, payload)
    return ctx
  }

  const settle = (ctx: EntryContext | undefined, phase: string, payload: () => Payload): void => {
    if (!ctx) return
    let resultSequence: string
    try {
      resultSequence = String((sequence += 1))
    } catch {
      return
    }
    emit(ctx, phase, resultSequence, payload)
  }

  // Calls the rest of the chain once and hands back exactly what it settled
  // to: the result object, or the rejection, rethrown untouched.
  const pass = async <R>(
    ctx: EntryContext | undefined,
    run: () => Promise<R>,
    trace: () => readonly TraceEntry[],
    payload: (result: R) => Payload,
  ): Promise<R> => {
    let result: R
    try {
      result = await run()
    } catch (error) {
      settle(ctx, 'provider-error', () => ({ core: traceProof(trace()) }))
      throw error
    }
    settle(ctx, 'result', () => ({ ...payload(result), core: traceProof(trace()) }))
    return result
  }

  on('session.start', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('session.start', next.origin, {}, () => ({ isInteractive: e.isInteractive, surface: e.surface }))
    const settled = pass(ctx, () => next(e), () => next.trace, () => ({}))
    // Bootstrap metadata only: every read here is middleware-interceptable, so
    // the session ID they give stays at a lower tier than classic.SessionStart.
    // The previous load's epoch is read before this load's is written; a
    // failed read or write leaves it null and never reaches the provider path.
    void (async () => {
      let predecessorEpoch: string | null = null
      try {
        const held = await $.state.get(EPOCH)
        await $.state.set(EPOCH, sourceEpoch)
        predecessorEpoch = id(held.value) ?? null
      } catch {
        // The predecessor stays unknown.
      }
      try {
        const [hostSessionId, version] = await Promise.all([$.session.id(), $.session.version()])
        if (session.id === undefined) changeSession(id(hostSessionId), 'session.id')
        if (ctx) {
          settle(ctx, 'bootstrap', () => ({
            hostSessionId: id(hostSessionId),
            predecessorEpoch,
            version: { version: id(version.version), base: id(version.base), builtAt: id(version.builtAt) },
          }))
        }
      } catch {
        // Bootstrap metadata is optional.
      }
    })()
    return settled
  })

  on('classic.SessionStart', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    try {
      changeSession(id(e.session_id), 'classic.SessionStart')
    } catch {
      // Keep the previous logical session.
    }
    const ctx = enter('classic.SessionStart', next.origin, { actor: e.agent_id }, () => ({
      source: e.source,
      agentType: id(e.agent_type),
    }))
    return pass(ctx, () => next(e), () => next.trace, () => ({}))
  })

  on('session.end', async ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('session.end', next.origin, {}, () => ({
      reason: e.reason,
      endingSessionId: id(e.sessionId),
      resumeId: id(e.resume?.id),
    }))
    try {
      changeSession(undefined, 'session.end')
    } catch {
      // Keep the previous logical session.
    }
    const result = await pass(ctx, () => next(e), () => next.trace, () => ({}))
    if (e.reason !== 'clear') await delivery.drainOnEnd()
    return result
  })

  on('session.attach', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('session.attach', next.origin, {}, () => ({ surface: e.surface, clientId: id(e.clientId) }))
    return pass(ctx, () => next(e), () => next.trace, () => ({}))
  })

  on('session.detach', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('session.detach', next.origin, {}, () => ({
      surface: e.surface,
      clientId: id(e.clientId),
      reason: e.reason,
    }))
    return pass(ctx, () => next(e), () => next.trace, () => ({}))
  })

  on('prompt.submit', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('prompt.submit', next.origin, {}, () => ({
      origin: promptOrigin(e.origin),
      activeTurnIdAtSubmission: id(e.turnId),
      wait: e.wait,
      attachmentCount: e.attachments?.length ?? 0,
    }))
    return pass(ctx, () => next(e), () => next.trace, result => ({
      outcome: result.drop === undefined ? 'entered' : 'dropped',
      resultOrigin: result.drop === undefined ? promptOrigin(result.origin) : undefined,
    }))
  })

  on('turn.start', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('turn.start', next.origin, { turn: e.turnId }, () => ({}))
    return pass(ctx, () => next(e), () => next.trace, result => ({ echoedTurnId: id(result.turnId) }))
  })

  on('turn.step', async function* ($, e, next) {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('turn.step', next.origin, { turn: e.turnId, actor: e.agentId }, () => ({
      index: e.index,
      messageCount: e.messageCount,
      model: id(e.model),
    }))
    let isSettled = false
    try {
      const result = yield* next(e)
      isSettled = true
      settle(ctx, 'result', () => ({
        index: result.index,
        stopReason: result.stopReason,
        toolUseCount: result.toolUses.length,
        serverToolUseCount: result.serverToolUses?.length ?? 0,
        chunksBeneath: next.trace[0]?.chunks ?? null,
        core: traceProof(next.trace),
      }))
      return result
    } catch (error) {
      isSettled = true
      settle(ctx, 'provider-error', () => ({ core: traceProof(next.trace) }))
      throw error
    } finally {
      if (!isSettled) settle(ctx, 'abandoned', () => ({ core: traceProof(next.trace) }))
    }
  })

  on('turn.complete', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('turn.complete', next.origin, { turn: e.turnId, actor: e.agentId }, () => ({
      reason: e.reason,
      isAborted: e.isAborted,
      durationMs: e.durationMs,
      refusalCategory: e.reason === 'refusal' ? id(e.refusal.category) : undefined,
    }))
    // The result carries the reason too: only it shows core settlement, and
    // a capture adapter reads each record on its own.
    return pass(ctx, () => next(e), () => next.trace, () => ({ reason: e.reason, isAborted: e.isAborted }))
  })

  on('tool.call', { tool: /^(?!(?:EnterWorktree|ExitWorktree)$)/ }, ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('tool.call', next.origin, { actor: e.agentId, occurrence: e.tool_use_id }, () => ({
      tool: id(String(e.tool)),
    }))
    return pass(ctx, () => next(e), () => next.trace, result => ({
      tool: id(String(e.tool)),
      resultKind: result.deny !== undefined ? 'deny' : result.isError === true ? 'error' : 'result',
      isReadOnly: result.isReadOnly === true,
    }))
  })

  on('tool.check', { tool: /^(?!(?:EnterWorktree|ExitWorktree)$)/ }, ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('tool.check', next.origin, { actor: e.agentId, occurrence: e.tool_use_id }, () => ({
      tool: id(String(e.tool)),
      ceiling: e.ceiling,
    }))
    return pass(ctx, () => next(e), () => next.trace, result => ({ decision: result.decision }))
  })

  on('agent.spawn', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('agent.spawn', next.origin, { actor: e.parentAgentId, occurrence: e.tool_use_id }, () => ({
      subagentType: id(e.subagentType),
      provider: { plugin: id(e.provider.plugin), tier: e.provider.tier },
      background: e.background,
      fork: e.fork,
      isTeammate: e.isTeammate === true,
    }))
    return pass(ctx, () => next(e), () => next.trace, result => ({
      outcome: result.deny === undefined ? 'started' : 'denied',
      agentId: id(result.agentId),
      teammateId: id(result.teammateId),
    }))
  })
}
