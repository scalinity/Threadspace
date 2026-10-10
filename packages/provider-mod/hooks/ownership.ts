// Original native-work ownership for one observer load. The provider
// namespace/process is supplied by the helper; this ledger never crosses a
// source epoch. A native ID is only a lookup key, not a globally unique ID.
//
// Slots retain their original Session AND generation. Reusing a lookup key
// for another owner makes it permanently ambiguous for this load. Maps have
// fixed bounds and never evict: forgetting a collision and then assigning a
// delayed callback to a newer owner would manufacture authority.

export type OwnershipStatus = 'KNOWN' | 'HOST_READ' | 'UNKNOWN' | 'AMBIGUOUS' | 'SATURATED'

export type OwnershipScope = Readonly<{
  epoch: string
  generation: number
  sessionId: string | undefined
  sessionIdSource: string | undefined
  status: 'KNOWN' | 'HOST_READ' | 'UNKNOWN'
  proofToken?: string
}>

export type Ownership = Readonly<{ status: OwnershipStatus; scope?: OwnershipScope }>

export const OWNERSHIP_LIMITS = { turns: 4096, actors: 4096, occurrences: 8192 } as const
const MAX_ID_CHARS = 256

// Never truncate lookup identities: two distinct long native IDs must not
// become the same retained ownership key. Capture may retain a bounded
// display of such a claim, but it cannot acquire a Session from this ledger.
export const nativeId = (value: unknown): string | undefined =>
  typeof value === 'string' && value.length > 0 && value.length <= MAX_ID_CHARS ? value : undefined

const UNKNOWN: Ownership = Object.freeze({ status: 'UNKNOWN' })
const AMBIGUOUS: Ownership = Object.freeze({ status: 'AMBIGUOUS' })
const SATURATED: Ownership = Object.freeze({ status: 'SATURATED' })

const sameOwner = (left: OwnershipScope, right: OwnershipScope): boolean =>
  left.epoch === right.epoch && left.generation === right.generation && left.sessionId === right.sessionId

const owned = (scope: OwnershipScope): Ownership =>
  scope.sessionId === undefined || scope.status === 'UNKNOWN' ? UNKNOWN : Object.freeze({ status: scope.status, scope })

const key = (value: unknown, actor: unknown): string | undefined => {
  const identity = nativeId(value)
  const actorId = actor === undefined ? undefined : nativeId(actor)
  if (identity === undefined || (actor !== undefined && actorId === undefined)) return undefined
  return JSON.stringify([identity, actorId ?? null])
}

function retained(limit: number) {
  const slots = new Map<string, Ownership>()
  return {
    find: (lookup: string | undefined): Ownership =>
      lookup === undefined ? UNKNOWN : slots.get(lookup) ?? (slots.size >= limit ? SATURATED : UNKNOWN),
    remember: (lookup: string | undefined, scope: OwnershipScope | undefined): Ownership => {
      if (lookup === undefined) return UNKNOWN
      const previous = slots.get(lookup)
      // A real native creation/claim with unknown ownership is still evidence
      // against assigning this key to a later owner. Keep a bounded tombstone.
      if (scope === undefined || scope.sessionId === undefined || scope.status === 'UNKNOWN') {
        if (previous === undefined && slots.size >= limit) return SATURATED
        slots.set(lookup, AMBIGUOUS)
        return AMBIGUOUS
      }
      if (previous !== undefined) {
        if (previous.scope !== undefined && sameOwner(previous.scope, scope)) return previous
        slots.set(lookup, AMBIGUOUS)
        return AMBIGUOUS
      }
      if (slots.size >= limit) return SATURATED
      const value = owned(scope)
      slots.set(lookup, value)
      return value
    },
    size: () => slots.size,
  }
}

export function createOwnershipLedger(
  epoch: string,
  limits: { turns: number; actors: number; occurrences: number } = OWNERSHIP_LIMITS,
) {
  const turns = retained(limits.turns)
  const actors = retained(limits.actors)
  const occurrences = retained(limits.occurrences)
  let current: OwnershipScope = Object.freeze({
    epoch,
    generation: 0,
    sessionId: undefined,
    sessionIdSource: undefined,
    status: 'UNKNOWN',
  })
  let changedSession = false

  const scopeUnchanged = (scope: OwnershipScope): boolean => sameOwner(current, scope)

  return {
    getCurrentScope: (): OwnershipScope => current,
    scopeUnchanged,
    // The token is only a claim for the native adapter to join to a separate
    // durable proof and seal. Receiving it from process.run grants no native
    // authority by itself. Previously captured scopes remain immutable.
    sealCurrentScope: (scope: OwnershipScope, token: unknown): boolean => {
      const proofToken = nativeId(token)
      if (!scopeUnchanged(scope) || scope.sessionId === undefined || proofToken === undefined) return false
      current = Object.freeze({ ...current, proofToken })
      return true
    },
    changeSession: (value: unknown, source: string): void => {
      const sessionId = nativeId(value)
      if (sessionId !== undefined && sessionId === current.sessionId) {
        // A genuine classic event can improve the identity source without
        // pretending the Session changed. Retained old scopes stay as read.
        if (source === 'classic.SessionStart' && current.sessionIdSource !== source) {
          current = Object.freeze({ ...current, sessionIdSource: source, status: 'KNOWN' })
        }
        return
      }
      if (current.sessionId !== undefined) changedSession = true
      current = Object.freeze({
        epoch,
        generation: current.generation + 1,
        sessionId,
        sessionIdSource: source,
        status: sessionId === undefined ? 'UNKNOWN' : source === 'classic.SessionStart' ? 'KNOWN' : 'HOST_READ',
      })
    },
    current: (): Ownership => owned(current),
    // Root tool/spawn callbacks have no native Turn field. Only the first,
    // uninterrupted, engine-identified interval rules out an earlier owner.
    // After a Session change they require a retained actor/occurrence key.
    // HOST_READ bootstrap alone never establishes subordinate ownership.
    initialRoot: (): Ownership => (!changedSession && current.status === 'KNOWN' ? owned(current) : UNKNOWN),
    turn: (turn: unknown, actor?: unknown): Ownership => turns.find(key(turn, actor)),
    rememberTurn: (turn: unknown, actor: unknown, scope: OwnershipScope | undefined): Ownership => turns.remember(key(turn, actor), scope),
    actor: (actor: unknown): Ownership => actors.find(key(actor, undefined)),
    rememberActor: (actor: unknown, scope: OwnershipScope | undefined): Ownership => actors.remember(key(actor, undefined), scope),
    occurrence: (occurrence: unknown, actor?: unknown): Ownership => occurrences.find(key(occurrence, actor)),
    rememberOccurrence: (occurrence: unknown, actor: unknown, scope: OwnershipScope | undefined): Ownership =>
      occurrences.remember(key(occurrence, actor), scope),
    sizes: () => ({ turns: turns.size(), actors: actors.size(), occurrences: occurrences.size() }),
  }
}
