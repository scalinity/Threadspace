// Bounded observation queue and mod-batch delivery (SPEC §8.3).
//
// Records leave the queue only on a valid typed receipt. Exit code 0, a
// missing or malformed receipt and a timeout are never acceptance: the
// records stay, with their original UUIDs, and one retry timer backs off
// from 250 ms to 5 s. Every run appends `--budget-ms` to the configured
// argv, 20 ms under the run's `timeoutMs` (10 ms at least), so the helper
// answers before the host kills it. The host reaches this file only
// through the `Io` closures a hook binds; a hot reload drops the
// environment and with it any pending timer (`$.clock`: "a hot reload of
// the plugin cancels its pending waits with the old environment").

import type { ProcessRunInit, ProcessRunResult, Timer } from 'claude-code'

export const LIMITS = {
  queueRecords: 2048,
  queueBytes: 8 * 1024 * 1024,
  batchRecords: 128,
  batchBytes: 64 * 1024,
  receiptBytes: 16 * 1024,
  retryMinMs: 250,
  retryMaxMs: 5000,
  drainTimeoutMs: 250,
  endDrainTimeoutMs: 100,
} as const

export const RECEIPT_VERSION = 1

export type ReceiptStatus = 'COMMITTED' | 'ALREADY_COMMITTED' | 'LOCAL_SPOOLED' | 'NOT_ACCEPTED'

const STATUSES: readonly string[] = ['COMMITTED', 'ALREADY_COMMITTED', 'LOCAL_SPOOLED', 'NOT_ACCEPTED']
const ACCEPTED: readonly string[] = ['COMMITTED', 'ALREADY_COMMITTED', 'LOCAL_SPOOLED']

export type Io = {
  run: (argv: readonly string[], init: ProcessRunInit) => Promise<ProcessRunResult>
  after: (ms: number, fn: () => void) => Timer
}

type Queued = {
  id: string
  json: string
  bytes: number
  detail: boolean
  inFlight: boolean
}

export type Delivery = {
  bind: (io: Io) => void
  enqueue: (observationId: string, record: unknown, detail: boolean) => void
  drainOnEnd: () => Promise<void>
}

const encoder = new TextEncoder()
const utf8Length = (text: string): number => encoder.encode(text).length

/**
 * Checks a mod-batch receipt against the batch it answers. Answers the
 * status per submitted observation, or undefined when the receipt is not
 * valid: non-zero exit, truncated or oversized output, unparsable JSON,
 * another version, a missing, extra, duplicate or unknown UUID, or a status
 * outside the four.
 */
export function readReceipt(result: ProcessRunResult, submitted: readonly string[]): Map<string, ReceiptStatus> | undefined {
  if (result.exitCode !== 0 || result.isStdoutTruncated) return undefined
  if (utf8Length(result.stdout) > LIMITS.receiptBytes) return undefined
  let parsed: unknown
  try {
    parsed = JSON.parse(result.stdout)
  } catch {
    return undefined
  }
  if (typeof parsed !== 'object' || parsed === null) return undefined
  const receipt = parsed as { receiptVersion?: unknown; results?: unknown }
  if (receipt.receiptVersion !== RECEIPT_VERSION || !Array.isArray(receipt.results)) return undefined
  if (receipt.results.length !== submitted.length) return undefined
  const wanted = new Set(submitted)
  const statuses = new Map<string, ReceiptStatus>()
  for (const entry of receipt.results as unknown[]) {
    if (typeof entry !== 'object' || entry === null) return undefined
    const { observationId, status } = entry as { observationId?: unknown; status?: unknown }
    if (typeof observationId !== 'string' || !wanted.has(observationId) || statuses.has(observationId)) return undefined
    if (typeof status !== 'string' || !STATUSES.includes(status)) return undefined
    statuses.set(observationId, status as ReceiptStatus)
  }
  return statuses
}

export function createDelivery(argv: readonly string[] | undefined, sourceEpoch: string): Delivery {
  const queue: Queued[] = []
  let queueBytes = 0
  let droppedRecords = 0
  let io: Io | undefined
  let timer: Timer | undefined
  let inFlight = false
  let retryMs: number = LIMITS.retryMinMs

  const remove = (index: number): void => {
    const [gone] = queue.splice(index, 1)
    if (gone) queueBytes -= gone.bytes
  }

  // Evicts the oldest high-volume detail record first, then the oldest
  // record; never one a drain has in flight.
  const makeRoom = (bytes: number): boolean => {
    while (queue.length + 1 > LIMITS.queueRecords || queueBytes + bytes > LIMITS.queueBytes) {
      let index = queue.findIndex(item => item.detail && !item.inFlight)
      if (index < 0) index = queue.findIndex(item => !item.inFlight)
      if (index < 0) return false
      remove(index)
      droppedRecords += 1
    }
    return true
  }

  // At most one pending timer: the prompt drain after an enqueue and the
  // backoff retry share it, so an enqueue during backoff waits for the retry.
  const schedule = (ms: number): void => {
    if (!io || !argv || timer || inFlight || queue.length === 0) return
    timer = io.after(ms, () => {
      timer = undefined
      void drain(LIMITS.drainTimeoutMs)
    })
  }

  const envelopePrefix = (): string =>
    `{"receiptVersion":${RECEIPT_VERSION},"kind":"mod-batch","sourceEpoch":${JSON.stringify(sourceEpoch)},"droppedRecords":${droppedRecords},"records":[`

  // Takes records from the head in queue order, so a split batch keeps the
  // capture sequence.
  const takeBatch = (prefix: string): Queued[] => {
    const batch: Queued[] = []
    let bytes = utf8Length(prefix) + 2
    for (const item of queue) {
      if (batch.length === LIMITS.batchRecords) break
      const added = item.bytes + (batch.length > 0 ? 1 : 0)
      if (bytes + added > LIMITS.batchBytes) break
      batch.push(item)
      bytes += added
    }
    return batch
  }

  const drain = async (timeoutMs: number): Promise<void> => {
    if (!io || !argv || inFlight || queue.length === 0) return
    inFlight = true
    let isDelivered = false
    let batch: Queued[] = []
    try {
      const prefix = envelopePrefix()
      batch = takeBatch(prefix)
      for (const item of batch) item.inFlight = true
      const stdin = `${prefix}${batch.map(item => item.json).join(',')}]}`
      const budgetMs = Math.max(10, timeoutMs - 20)
      const result = await io.run([...argv, '--budget-ms', String(budgetMs)], { stdin, timeoutMs })
      const statuses = readReceipt(result, batch.map(item => item.id))
      if (statuses) {
        const accepted = new Set([...statuses].filter(([, status]) => ACCEPTED.includes(status)).map(([id]) => id))
        for (let index = queue.length - 1; index >= 0; index -= 1) {
          const item = queue[index]
          if (item && accepted.has(item.id)) remove(index)
        }
        isDelivered = accepted.size === batch.length
      }
    } catch {
      // A rejected run (timeout, no such executable) is not acceptance.
    } finally {
      for (const item of batch) item.inFlight = false
      inFlight = false
    }
    if (isDelivered) {
      retryMs = LIMITS.retryMinMs
      schedule(0)
    } else {
      const delay = retryMs
      retryMs = Math.min(retryMs * 2, LIMITS.retryMaxMs)
      schedule(delay)
    }
  }

  return {
    bind: next => {
      io = next
    },
    enqueue: (observationId, record, detail) => {
      if (!argv) return
      const json = JSON.stringify(record)
      const bytes = utf8Length(json)
      if (bytes + 64 > LIMITS.batchBytes || !makeRoom(bytes)) {
        droppedRecords += 1
        return
      }
      queue.push({ id: observationId, json, bytes, detail, inFlight: false })
      queueBytes += bytes
      schedule(0)
    },
    drainOnEnd: async () => {
      try {
        if (timer) {
          timer.cancel()
          timer = undefined
        }
        await drain(LIMITS.endDrainTimeoutMs)
      } catch {
        // The bounded end drain narrows the loss window; it never fails the end.
      }
    },
  }
}
