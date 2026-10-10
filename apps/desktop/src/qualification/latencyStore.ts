// Qualification metadata only. A store update is not a DOM application.
// All times use one performance.now runtime; native calibration is external.
export interface LatencyContext {
  phase: string;
  coreGeneration: string | null;
  storeGeneration: string | null;
  viewEpoch: string | null;
}

export interface LatencyMark {
  cursor: string;
  appliedMonotonicMs: number;
  domMonotonicMs: number | null;
  domCursor: string | null;
  hydratedVisible: boolean;
}

const cursorValue = (value: string): bigint | null => /^\d+$/.test(value) ? BigInt(value) : null;
const same = (a: LatencyContext, b: LatencyContext): boolean =>
  a.phase === "live" && b.phase === "live" && a.coreGeneration !== null && a.storeGeneration !== null && a.viewEpoch !== null
  && a.coreGeneration === b.coreGeneration && a.storeGeneration === b.storeGeneration && a.viewEpoch === b.viewEpoch;

export class LatencyStore {
  private marks: LatencyMark[] = [];
  private context: LatencyContext | null = null;
  private lastCursor: string | null = null;
  private startedMs: number | null = null;
  private overflow = 0;
  private invalid = 0;
  private contextChanged = false;
  private totalApplied = 0;
  private stopped = false;

  constructor(readonly runtimeId: string, private readonly now: () => number, private readonly capacity = 4096) {}

  start(context: LatencyContext, cursor: string, visible: boolean): void {
    if (!same(context, context) || !visible || cursorValue(cursor) === null) throw new Error("latency requires a hydrated visible view and valid cursor");
    this.context = { ...context };
    this.lastCursor = cursor;
    this.startedMs = this.now();
    this.marks = [];
    this.overflow = 0;
    this.invalid = 0;
    this.contextChanged = false;
    this.totalApplied = 0;
    this.stopped = false;
  }

  apply(cursor: string, context: LatencyContext): void {
    if (this.context === null || this.stopped) return;
    if (!same(this.context, context)) this.contextChanged = true;
    if (cursor === this.lastCursor) return;
    const value = cursorValue(cursor);
    if (value === null || this.lastCursor === null || value <= (cursorValue(this.lastCursor) ?? -1n)) {
      this.invalid += 1;
      return;
    }
    this.lastCursor = cursor;
    this.totalApplied += 1;
    if (this.marks.length >= this.capacity) { this.overflow += 1; return; }
    this.marks.push({ cursor, appliedMonotonicMs: this.now(), domMonotonicMs: null, domCursor: null, hydratedVisible: false });
  }

  rendered(cursor: string, context: LatencyContext, visible: boolean): void {
    if (this.context === null || this.stopped) return;
    if (!same(this.context, context)) this.contextChanged = true;
    const shown = cursorValue(cursor);
    const latest = this.lastCursor === null ? null : cursorValue(this.lastCursor);
    if (shown === null || latest === null || shown > latest || this.contextChanged || !visible) return;
    const at = this.now();
    for (const mark of this.marks) {
      if (mark.domMonotonicMs === null && (cursorValue(mark.cursor) ?? shown + 1n) <= shown) {
        mark.domMonotonicMs = at;
        mark.domCursor = cursor;
        mark.hydratedVisible = true;
      }
    }
  }

  clock() {
    return { schemaVersion: 2, runtimeId: this.runtimeId, clock: "performance.now", units: "milliseconds", monotonicMs: this.now() };
  }

  stop() {
    this.stopped = true;
    return this.page();
  }

  page(offset = 0, limit = 100) {
    if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(limit) || limit < 1 || limit > 100) throw new Error("invalid latency page");
    const end = Math.min(offset + limit, this.marks.length);
    return {
      ...this.clock(), context: this.context, startedMs: this.startedMs,
      totalApplied: this.totalApplied, retainedMarks: this.marks.length,
      stopped: this.stopped,
      overflow: this.overflow, invalid: this.invalid, contextChanged: this.contextChanged,
      offset, nextOffset: end < this.marks.length ? end : null,
      marks: this.marks.slice(offset, end).map(mark => ({ ...mark })),
    };
  }
}
