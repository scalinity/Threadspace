// Optional qualification instrumentation. It is disabled unless the owned
// capture argv contains --qualification-latency. No canonical fact, receipt
// status, provider result or end-drain deadline depends on this collector.
import type { ProcessRunResult } from 'claude-code'
import type { Io, ReceiptStatus } from './delivery.ts'

export const MEASUREMENT_LIMITS = {
  captures: 4096,
  pageRecords: 128,
  exportTimeoutMs: 100,
  closeBudgetMs: 1000,
  endDrainBudgetMs: 100,
  receiptBytes: 4096,
} as const

export type BatchMeasurement = { beginMs: number; ids: readonly string[] }
export type DeliveryMeasurement = {
  capture: (id: string) => void
  begin: (ids: readonly string[]) => BatchMeasurement
  end: (io: Io, batch: BatchMeasurement, result: ProcessRunResult | undefined, statuses: ReadonlyMap<string, ReceiptStatus> | undefined) => Promise<void>
  // Standalone primitive used by focused fixtures. Production calls
  // finishSession after an original engine/core session.end result and
  // awaits census only within the original 100 ms end-drain ceiling.
  // Neither path claims the native process lifetime is over.
  finalize: (io: Io) => Promise<void>
  finishSession: (io: Io, drain: () => Promise<void>) => Promise<void>
}

type Capture = { observationId: string; capturedMs: number | null }
type Reply = {
  kind?: unknown; schemaVersion?: unknown; accepted?: unknown; status?: unknown
  runtimeId?: unknown; censusId?: unknown; pageIndex?: unknown; token?: unknown
  totalCaptured?: unknown; observationIdsSha256?: unknown
}

const canonicalUuid = (value: unknown): value is string => typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value)

export function createLatencyMeasurement(argv: readonly string[] | undefined, runtimeId: string): DeliveryMeasurement | undefined {
  if (!argv?.includes('--qualification-latency') || !canonicalUuid(runtimeId)) return undefined
  const helper = argv.map((part, index) => index === 1 && part === 'mod-batch' ? 'latency-samples' : part)
  if (helper[1] !== 'latency-samples') return undefined
  const censusHelper = helper.map((part, index) => index === 1 ? 'latency-census' : part)
  // This ledger is independent of the delivery queue: accepted, spooled,
  // rejected, evicted and never-drained UUIDs all remain in the denominator.
  // Removing accepted UUIDs here would conceal a lost final telemetry batch.
  const captures = new Map<string, Capture>()
  let totalCaptured = 0
  let overflow = 0
  let failedExports = 0
  let inFlightBatches = 0
  let exporting: Promise<void> | undefined
  let closing: Promise<void> | undefined
  let closed = false
  let expired = false
  let anchor: { token: string; endMs: number } | undefined
  let openedMs: number | undefined
  const now = () => {
    const value = performance.now()
    if (!Number.isFinite(value) || value < 0) throw new Error('unavailable monotonic clock')
    return value
  }
  const reply = (value: ProcessRunResult): Reply => {
    if (value.exitCode !== 0 || value.isStdoutTruncated || new TextEncoder().encode(value.stdout).length > MEASUREMENT_LIMITS.receiptBytes) throw new Error('missing measurement receipt')
    const parsed: unknown = JSON.parse(value.stdout)
    if (parsed === null || typeof parsed !== 'object') throw new Error('malformed measurement receipt')
    return parsed as Reply
  }
  const close = async (io: Io, endDeadline?: number): Promise<void> => {
    // Freeze synchronously before awaiting an earlier sample export. A
    // callback racing this source boundary increments failedExports below;
    // every stage checks it before publishing the final confirmation.
    closed = true
    const frozenFailures = failedExports
    const frozenTotal = totalCaptured
    const frozenOverflow = overflow
    const closedMs = now()
    const deadline = Math.min(closedMs + MEASUREMENT_LIMITS.closeBudgetMs, endDeadline ?? Number.POSITIVE_INFINITY)
    const records = [...captures.values()].sort((a, b) => a.observationId < b.observationId ? -1 : a.observationId > b.observationId ? 1 : 0)
    if (records.length === 0 || inFlightBatches !== 0 || !anchor) throw new Error('source window is not drained or lacks an independent receipt clock')
    const endDrainDeadline = { receiptToken: anchor.token, remainingMs: Math.min(MEASUREMENT_LIMITS.endDrainBudgetMs, deadline - anchor.endMs) }
    if (endDrainDeadline.remainingMs <= 0) throw new Error('source deadline exhausted')
    if (exporting) await exporting
    const check = () => {
      if (expired || now() >= deadline || failedExports !== frozenFailures || totalCaptured !== frozenTotal || inFlightBatches !== 0 || exporting) throw new Error('capture or export raced the closed source window')
    }
    check()
    const digestBytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(records.map(record => record.observationId).join('\n')))
    const observationIdsSha256 = [...new Uint8Array(digestBytes)].map(byte => byte.toString(16).padStart(2, '0')).join('')
    const censusId = crypto.randomUUID()
    const pageCount = Math.ceil(records.length / MEASUREMENT_LIMITS.pageRecords)
    const send = async (body: object, status: string): Promise<Reply> => {
      check()
      const result = reply(await io.run(censusHelper, { stdin: JSON.stringify(body), timeoutMs: MEASUREMENT_LIMITS.exportTimeoutMs }))
      check()
      if (result.kind !== 'observer-census-receipt' || result.schemaVersion !== 1 || result.accepted !== true || result.status !== status || result.runtimeId !== runtimeId || result.censusId !== censusId) throw new Error('unbound census receipt')
      return result
    }
    for (let pageIndex = 0; pageIndex < pageCount; pageIndex += 1) {
      const saved = await send({ kind: 'observer-census-page', schemaVersion: 1, runtimeId, censusId, pageIndex, pageCount,
        records: records.slice(pageIndex * MEASUREMENT_LIMITS.pageRecords, (pageIndex + 1) * MEASUREMENT_LIMITS.pageRecords) }, 'PAGE_RECORDED')
      if (saved.pageIndex !== pageIndex) throw new Error('wrong census page receipt')
    }
    const saved = await send({ kind: 'observer-census-close', schemaVersion: 1, runtimeId, censusId, pageCount,
      boundary: 'SESSION_END', clock: 'performance.now', openedMs: openedMs ?? closedMs, closedMs,
      totalCaptured: frozenTotal, overflow: frozenOverflow, failedExports: frozenFailures, observationIdsSha256, endDrainDeadline }, 'CLOSED')
    if (!canonicalUuid(saved.token) || saved.totalCaptured !== frozenTotal || saved.observationIdsSha256 !== observationIdsSha256) throw new Error('wrong census close receipt')
    // The helper independently retains the close and its fresh challenge.
    // Echoing that challenge proves this runtime received the positive close
    // receipt. A forged process.run reply alone cannot create the retained
    // native confirmation required by the exporter.
    const confirmed = await send({ kind: 'observer-census-confirm', schemaVersion: 1, runtimeId, censusId,
      token: saved.token, observationIdsSha256 }, 'CONFIRMED')
    if (confirmed.token !== saved.token || confirmed.totalCaptured !== frozenTotal || confirmed.observationIdsSha256 !== observationIdsSha256) throw new Error('wrong census confirmation receipt')
  }
  return {
    capture: id => {
      if (closed) { failedExports += 1; return }
      if (captures.has(id)) return
      totalCaptured += 1
      if (captures.size >= MEASUREMENT_LIMITS.captures) { overflow += 1; return }
      let capturedMs: number | null = null
      try { capturedMs = now(); openedMs ??= capturedMs } catch { failedExports += 1 }
      captures.set(id, { observationId: id, capturedMs })
    },
    begin: ids => {
      if (closed) { failedExports += 1; throw new Error('source measurement window closed') }
      const batch = { beginMs: now(), ids: [...ids] }
      inFlightBatches += 1
      return batch
    },
    end: async (io, batch, result, statuses) => {
      inFlightBatches = Math.max(0, inFlightBatches - 1)
      let ownExport: Promise<void> | undefined
      try {
        // Taken immediately on return of the real mod-batch invocation,
        // including timeout/rejection, before optional evidence persistence.
        const endMs = now()
        let nativeToken: unknown
        try { nativeToken = result && (JSON.parse(result.stdout) as { qualificationClock?: { token?: unknown } }).qualificationClock?.token } catch { /* invalid receipt remains invalid */ }
        if (canonicalUuid(nativeToken)) anchor = { token: nativeToken, endMs }
        const body = {
          kind: 'observer-clock-bracket', schemaVersion: 1, runtimeId, clock: 'performance.now',
          beginMs: batch.beginMs, endMs, nativeToken: canonicalUuid(nativeToken) ? nativeToken : null,
          totalCaptured, overflow, failedExports,
          records: batch.ids.map(observationId => ({ observationId,
            capturedMs: captures.get(observationId)?.capturedMs ?? null,
            status: statuses?.get(observationId) ?? 'UNKNOWN' })),
        }
        if (exporting || closed) { failedExports += 1; return }
        ownExport = (async () => {
          try {
            const saved = reply(await io.run(helper, { stdin: JSON.stringify(body), timeoutMs: MEASUREMENT_LIMITS.exportTimeoutMs }))
            if (saved.accepted !== true) failedExports += 1
          } catch { failedExports += 1 }
        })()
        exporting = ownExport
        await ownExport
      } catch { failedExports += 1 } finally { if (ownExport && exporting === ownExport) exporting = undefined }
    },
    finalize: io => {
      // At most one bounded standalone final export. Production uses the
      // stricter shared end-drain deadline in finishSession below.
      // A failed final export has no matching native confirmation. Earlier
      // successful counters can therefore never stand in for a final seal.
      closing ??= close(io).catch(() => { failedExports += 1 })
      return closing
    },
    finishSession: async (io, drain) => {
      // One deadline starts before the original drain. Normal mode does
      // not call this wrapper. Optional census uses only the time left in
      // that existing 100 ms ceiling, and never holds the provider after it.
      let deadline: number
      try { deadline = now() + MEASUREMENT_LIMITS.endDrainBudgetMs } catch { await drain(); return }
      await new Promise<void>(resolve => {
        let settled = false
        let timer: ReturnType<Io['after']> | undefined
        const release = () => {
          if (settled) return
          settled = true
          try { timer?.cancel() } catch { /* optional wait cleanup */ }
          resolve()
        }
        try {
          timer = io.after(Math.max(0, deadline - now()), () => { expired = true; failedExports += 1; release() })
        } catch {
          // A failed optional timer gives no safe additional waiting budget.
          // Preserve the original drain and leave census confirmation absent.
          expired = true
        }
        void (async () => {
          try {
            await drain()
            if (expired || now() >= deadline) { expired = true; return }
            closing ??= close(io, deadline).catch(() => { failedExports += 1 })
            await closing
          } catch { failedExports += 1 } finally { release() }
        })()
      })
    },
  }
}
