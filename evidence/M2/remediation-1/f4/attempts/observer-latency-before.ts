// Optional qualification instrumentation. It is disabled unless the owned
// capture argv contains --qualification-latency. No canonical fact or receipt
// status depends on this collector; a failed measurement stays missing.
import type { ProcessRunResult } from 'claude-code'
import type { Io, ReceiptStatus } from './delivery.ts'

export type BatchMeasurement = { beginMs: number; ids: readonly string[] }
export type DeliveryMeasurement = {
  capture: (id: string) => void
  begin: (ids: readonly string[]) => BatchMeasurement
  end: (io: Io, batch: BatchMeasurement, result: ProcessRunResult, statuses: ReadonlyMap<string, ReceiptStatus> | undefined) => Promise<void>
}

export function createLatencyMeasurement(argv: readonly string[] | undefined, runtimeId: string): DeliveryMeasurement | undefined {
  if (!argv?.includes('--qualification-latency')) return undefined
  const helper = argv.map((part, index) => index === 1 && part === 'mod-batch' ? 'latency-samples' : part)
  if (helper[1] !== 'latency-samples') return undefined
  const captures = new Map<string, number>()
  let totalCaptured = 0
  let overflow = 0
  let failedExports = 0
  let exporting = false
  const now = () => {
    const value = performance.now()
    if (!Number.isFinite(value) || value < 0) throw new Error('unavailable monotonic clock')
    return value
  }
  return {
    capture: id => {
      if (captures.has(id)) return
      totalCaptured += 1
      if (captures.size >= 4096) { overflow += 1; return }
      captures.set(id, now())
    },
    begin: ids => ({ beginMs: now(), ids: [...ids] }),
    end: async (io, batch, result, statuses) => {
      // Taken immediately on return of the real mod-batch invocation, before
      // optional evidence persistence. The native helper stamps inside this
      // bracket; arbitrary process/runtime time origins are never subtracted.
      const endMs = now()
      let nativeToken: unknown
      try { nativeToken = (JSON.parse(result.stdout) as { qualificationClock?: { token?: unknown } }).qualificationClock?.token } catch { /* invalid receipt remains invalid */ }
      const records = batch.ids.map(observationId => ({
        observationId,
        capturedMs: captures.get(observationId) ?? null,
        status: statuses?.get(observationId) ?? 'UNKNOWN',
      }))
      const body = {
        kind: 'observer-clock-bracket', schemaVersion: 1, runtimeId, clock: 'performance.now',
        beginMs: batch.beginMs, endMs, nativeToken: typeof nativeToken === 'string' ? nativeToken : null,
        totalCaptured, overflow, failedExports, records,
      }
      for (const [id, status] of statuses ?? []) {
        if (status === 'COMMITTED' || status === 'ALREADY_COMMITTED' || status === 'LOCAL_SPOOLED') captures.delete(id)
      }
      if (exporting) { failedExports += 1; return }
      exporting = true
      try {
        const saved = await io.run(helper, { stdin: JSON.stringify(body), timeoutMs: 100 })
        if (saved.exitCode !== 0 || saved.isStdoutTruncated || saved.stdout.length > 4096 || JSON.parse(saved.stdout)?.accepted !== true) failedExports += 1
      } catch { failedExports += 1 } finally { exporting = false }
    },
  }
}
