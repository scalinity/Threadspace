// Threadspace passive observer mod (SPEC §11.4, §8.3).
//
// Every hook calls `next(e)` exactly once with the event it received,
// returns that result unchanged and lets a rejection from beneath propagate.
// Only the observer's own work is guarded. At callback entry the hook
// freezes its context (source epoch, entry sequence, host-stamped
// `next.origin`, retained original work ownership and native IDs) before
// awaiting anything; the result record reuses that context. Current Session
// metadata never supplies ownership for a later Turn/actor callback. Records carry
// allowlisted metadata only: no prompt text, tool input or output, or
// assistant text. No handler exists for WorktreeCreate or WorktreeRemove,
// and the tool hooks are not seated for the worktree tools.

import type { Origin, PluginOptions, ProcessRunInit, ProcessRunResult, PromptOrigin, Register, TraceEntry } from 'claude-code'
import { createDelivery } from './delivery.ts'
import { createLatencyMeasurement } from './latency.ts'
import { createOwnershipLedger, nativeId } from './ownership.ts'
import type { Ownership, OwnershipScope, OwnershipStatus } from './ownership.ts'

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
  ownership: OwnershipScope | undefined
  ownershipStatus: OwnershipStatus
  currentSessionId: string | undefined
  currentSessionIdSource: string | undefined
  currentSessionGeneration: number
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

const coreSettled = (trace: readonly TraceEntry[]): boolean => {
  const last = trace[trace.length - 1]
  return last !== undefined && last.plugin === 'engine' && last.tier === 'core' && (last.outcome === 'returned' || last.outcome === 'passed')
}

const readArgv = (value: PluginOptions[string] | undefined): readonly string[] | undefined => {
  if (!Array.isArray(value) || value.length === 0) return undefined
  return value.every(part => typeof part === 'string' && part.length > 0) ? [...value] : undefined
}

export const register: Register = (on, options) => {
  const sourceEpoch = crypto.randomUUID()
  const captureArgv = readArgv(options.captureArgv)
  const measurement = createLatencyMeasurement(captureArgv, sourceEpoch)
  const delivery = createDelivery(captureArgv, sourceEpoch, measurement)
  const ownership = createOwnershipLedger(sourceEpoch)
  let sequence = 0
  let proofInFlight = false
  const unknown: Ownership = Object.freeze({ status: 'UNKNOWN' })

  const original = (nativeEvent: string, ids: Ids): Ownership => {
    if (Object.values(ids).some(value => value !== undefined && nativeId(value) === undefined)) return unknown
    if ((nativeEvent === 'turn.step' || nativeEvent === 'turn.complete') && ids.actor !== undefined) {
      const actor = ownership.actor(ids.actor)
      if (actor.status === 'AMBIGUOUS' || actor.status === 'SATURATED') return actor
    }
    if (nativeEvent === 'turn.complete') return ownership.turn(ids.turn, ids.actor)
    if (nativeEvent === 'turn.step') {
      const turn = ownership.turn(ids.turn, ids.actor)
      // Child loops have no turn.start. A core-settled spawn establishes the
      // actor first; its first step may then establish that actor's Turn.
      if (turn.status !== 'UNKNOWN') return turn
      return ids.actor === undefined ? unknown : ownership.actor(ids.actor)
    }
    if (nativeEvent === 'tool.call' || nativeEvent === 'tool.check' || nativeEvent === 'agent.spawn') {
      const occurrence = ownership.occurrence(ids.occurrence, ids.actor)
      if (occurrence.status !== 'UNKNOWN') return occurrence
      return ids.actor === undefined ? ownership.initialRoot() : ownership.actor(ids.actor)
    }
    if (nativeEvent === 'prompt.submit' && ids.turn !== undefined) return ownership.turn(ids.turn, ids.actor)
    // Only start/Session/input entry contexts may use the observed current
    // interval. Later work callbacks above must resolve retained ownership.
    return ownership.current()
  }

  const emit = (ctx: EntryContext, phase: string, resultSequence: string | undefined, payload: () => Payload): void => {
    try {
      const observationId = crypto.randomUUID()
      try { measurement?.capture(observationId) } catch { /* Optional qualification telemetry is fail-open. */ }
      // The callback's own time: a retried record carries it unchanged, so
      // the helper rebuilds the same envelope and the journal recognizes it.
      const capturedAtMs = Date.now()
      delivery.enqueue(
        observationId,
        {
          schemaVersion: 1,
          observationId,
          capturedAtMs,
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
          ownershipEpoch: ctx.ownership?.epoch ?? ctx.sourceEpoch,
          ownershipGeneration: ctx.ownership?.generation,
          ownershipStatus: ctx.ownershipStatus,
          ownershipProofToken: ctx.ownership?.proofToken,
          currentSessionId: ctx.currentSessionId,
          currentSessionIdSource: ctx.currentSessionIdSource,
          currentSessionGeneration: ctx.currentSessionGeneration,
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

  const enter = (
    nativeEvent: string,
    origin: Origin,
    ids: Ids,
    payload: () => Payload,
    owner?: Ownership,
  ): EntryContext | undefined => {
    let ctx: EntryContext
    try {
      const resolved = owner ?? original(nativeEvent, ids)
      const current = ownership.getCurrentScope()
      ctx = Object.freeze({
        sourceEpoch,
        callbackEntrySequence: String((sequence += 1)),
        nativeEvent,
        dispatchOrigin: Object.freeze({ plugin: id(origin.plugin) ?? '', tier: String(origin.tier) }),
        engineDispatch: origin.plugin === 'engine' && origin.tier === 'core',
        sessionId: resolved.scope?.sessionId,
        sessionIdSource: resolved.scope?.sessionIdSource,
        // Legacy schema requires a number; zero means no owned generation.
        // ownershipGeneration stays absent rather than naming the current one.
        sessionGeneration: resolved.scope?.generation ?? 0,
        ownership: resolved.scope,
        ownershipStatus: resolved.status,
        currentSessionId: current.sessionId,
        currentSessionIdSource: current.sessionIdSource,
        currentSessionGeneration: current.generation,
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

  const remember = (
    ctx: EntryContext | undefined,
    trace: () => readonly TraceEntry[],
    action: (scope: OwnershipScope | undefined) => void,
  ): void => {
    try {
      if (ctx?.engineDispatch && coreSettled(trace())) action(ctx.ownership)
    } catch {
      // Losing optional ownership evidence degrades later capture to UNKNOWN.
    }
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

  // A host-read Session ID is not independent identity. Ask the native
  // helper to persist its own process/inventory proof, then seal the reply
  // only if this exact observer ownership interval survived the probe. The
  // token alone is untrusted: the reducer requires that separate native
  // fact and this seal. No Turn captured before the seal is rewritten.
  const probeOwnership = (
    scope: OwnershipScope,
    run: (argv: readonly string[], init: ProcessRunInit) => Promise<ProcessRunResult>,
  ): void => {
    try {
      if (proofInFlight || scope.status !== 'HOST_READ' || scope.sessionId === undefined || !captureArgv) return
      const command = captureArgv.indexOf('mod-batch')
      if (command < 0) return
      const argv = [...captureArgv]
      argv[command] = 'observer-proof'
      proofInFlight = true
      void (async () => {
        try {
          const result = await run(argv, {
            stdin: JSON.stringify({
              protocolVersion: 1,
              sourceEpoch: scope.epoch,
              sessionGeneration: scope.generation,
              sessionId: scope.sessionId,
            }),
            timeoutMs: 2000,
          })
          if (result.exitCode !== 0 || result.isStdoutTruncated || result.stdout.length > 16 * 1024) return
          const receipt: unknown = JSON.parse(result.stdout)
          if (receipt === null || typeof receipt !== 'object') return
          const proof = receipt as Record<string, unknown>
          if (
            proof.protocolVersion !== 1 ||
            proof.sourceEpoch !== scope.epoch ||
            proof.sessionGeneration !== scope.generation ||
            proof.sessionId !== scope.sessionId ||
            (proof.status !== 'COMMITTED' && proof.status !== 'LOCAL_SPOOLED') ||
            nativeId(proof.proofToken) === undefined ||
            !ownership.scopeUnchanged(scope)
          ) return
          if (!ownership.sealCurrentScope(scope, proof.proofToken)) return
          const sealed = ownership.current()
          const ctx = enter('ownership.seal', { plugin: ADAPTER_ID, tier: 'user' }, {}, () => ({}), sealed)
          settle(ctx, 'result', () => ({ proofToken: nativeId(proof.proofToken) }))
        } catch {
          // A missing helper, malformed/intercepted reply or timeout leaves
          // host-read outcomes pending. It never fails provider execution.
        } finally {
          proofInFlight = false
        }
      })()
    } catch {
      proofInFlight = false
    }
  }

  // Calls the rest of the chain once and hands back exactly what it settled
  // to: the result object, or the rejection, rethrown untouched.
  const pass = async <R>(
    ctx: EntryContext | undefined,
    run: () => Promise<R>,
    trace: () => readonly TraceEntry[],
    payload: (result: R) => Payload,
    remembered?: (result: R, scope: OwnershipScope | undefined) => void,
  ): Promise<R> => {
    let result: R
    try {
      result = await run()
    } catch (error) {
      settle(ctx, 'provider-error', () => ({ core: traceProof(trace()) }))
      throw error
    }
    if (remembered) remember(ctx, trace, scope => remembered(result, scope))
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
        if (ownership.getCurrentScope().sessionId === undefined) ownership.changeSession(hostSessionId, 'session.id')
        if (ctx) {
          settle(ctx, 'bootstrap', () => ({
            hostSessionId: id(hostSessionId),
            predecessorEpoch,
            version: { version: id(version.version), base: id(version.base), builtAt: id(version.builtAt) },
          }))
        }
        probeOwnership(ownership.getCurrentScope(), (argv, init) => $.process.run(argv, init))
      } catch {
        // Bootstrap metadata is optional.
      }
    })()
    return settled
  })

  on('classic.SessionStart', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    try {
      if (next.origin.plugin === 'engine' && next.origin.tier === 'core') ownership.changeSession(e.session_id, 'classic.SessionStart')
    } catch {
      // Keep the previous logical session.
    }
    const ctx = enter('classic.SessionStart', next.origin, { actor: e.agent_id }, () => ({
      source: e.source,
      agentType: id(e.agent_type),
    }))
    return pass(ctx, () => next(e), () => next.trace, () => ({}), (_result, scope) => {
      if (e.agent_id !== undefined) ownership.rememberActor(e.agent_id, scope)
    })
  })

  on('session.end', async ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ending = nativeId(e.sessionId) === ownership.getCurrentScope().sessionId ? ownership.current() : unknown
    const ctx = enter('session.end', next.origin, {}, () => ({
      reason: e.reason,
      endingSessionId: id(e.sessionId),
      resumeId: id(e.resume?.id),
    }), ending)
    try {
      if (ctx?.engineDispatch && ctx.ownership && ownership.scopeUnchanged(ctx.ownership)) ownership.changeSession(undefined, 'session.end')
    } catch {
      // Keep the previous logical session.
    }
    const result = await pass(ctx, () => next(e), () => next.trace, () => ({}))
    // Qualification only: the census uses the unused part of the existing
    // 100 ms Session-end drain ceiling. Normal callback timing is unchanged.
    // The original result and any exception from next(e) remain untouched.
    let finish: (() => Promise<void>) | undefined
    try {
      if (measurement && e.reason !== 'clear' && ctx?.engineDispatch && coreSettled(next.trace)) {
        finish = () => measurement.finishSession({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) }, () => delivery.drainOnEnd())
      }
    } catch { /* fail open */ }
    if (e.reason !== 'clear') {
      if (finish) { try { await finish() } catch { /* optional census cannot change the provider result */ } }
      else await delivery.drainOnEnd()
    }
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
    }), original('prompt.submit', { turn: e.turnId }))
    return pass(ctx, () => next(e), () => next.trace, result => ({
      outcome: result.drop === undefined ? 'entered' : 'dropped',
      resultOrigin: result.drop === undefined ? promptOrigin(result.origin) : undefined,
    }))
  })

  on('turn.start', ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('turn.start', next.origin, { turn: e.turnId }, () => ({}))
    return pass(ctx, () => next(e), () => next.trace, result => ({ echoedTurnId: id(result.turnId) }), (result, scope) => {
      if (nativeId(result.turnId) === nativeId(e.turnId)) ownership.rememberTurn(e.turnId, undefined, scope)
    })
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
      remember(ctx, () => next.trace, scope => {
        ownership.rememberTurn(e.turnId, e.agentId, scope)
      })
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
    try {
      if (ctx?.engineDispatch && !ctx.ownership) ownership.rememberTurn(e.turnId, e.agentId, undefined)
    } catch {
      // Uncertainty capture is optional; it never controls provider execution.
    }
    // The result carries the reason too: only it shows core settlement, and
    // a capture adapter reads each record on its own.
    return pass(ctx, () => next(e), () => next.trace, () => ({ reason: e.reason, isAborted: e.isAborted }))
  })

  on('tool.call', { tool: /^(?!(?:EnterWorktree|ExitWorktree)$)/ }, ($, e, next) => {
    delivery.bind({ run: (argv, init) => $.process.run(argv, init), after: (ms, fn) => $.clock.after(ms, fn) })
    const ctx = enter('tool.call', next.origin, { actor: e.agentId, occurrence: e.tool_use_id }, () => ({
      tool: id(String(e.tool)),
    }))
    try {
      // Freeze the original engine dispatch's occurrence before next can
      // invoke nested tool.check/agent.spawn or outlive a Session change.
      if (ctx?.engineDispatch && ctx.ownership) ownership.rememberOccurrence(e.tool_use_id, e.agentId, ctx.ownership)
    } catch {
      // Optional ownership capture must not change execution.
    }
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
    }), (result, scope) => {
      if (result.deny === undefined) ownership.rememberActor(result.agentId, scope?.status === 'KNOWN' ? scope : undefined)
    })
  })
}
