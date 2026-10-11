// Separate schema 3. No WebKit clock conversion and no flow ACKs.
import type { FrameHeader } from "../contracts/generated/FrameHeader";
import type { ProjectionPatch } from "../contracts/generated/ProjectionPatch";
import type { ViewState } from "../bridge/client";
import { presenceText, turnText } from "../ui/coverage";
import { WORKER_STATE_TEXT, workerState } from "../ui/worker";
import { categoryText, openCountText } from "../ui/attention";

export interface ComposedContext {
  runId: string; domRuntimeId: string; nativeIncarnation: string;
  subscriptionId: string; viewEpoch: string; coreGeneration: string; storeGeneration: string;
}
export interface CoveredRevision { entity: "SESSION" | "ATTENTION"; id: string; revision: string; absent: boolean }
export interface DomWitness {
  schemaVersion: 3; witnessId: string; context: ComposedContext;
  fromCursor: string; cursor: string; fromStreamSeq: number; streamSeq: number;
  revisions: CoveredRevision[]; pendingHydration: number; visible: boolean;
}
const freezeContext = (context: ComposedContext): ComposedContext => Object.freeze({ ...context });
const sameJson = (a: unknown, b: unknown): boolean => {
  const ordered = (value: unknown): unknown => Array.isArray(value) ? value.map(ordered)
    : value !== null && typeof value === "object" ? Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => [k, ordered(v)])) : value;
  return JSON.stringify(ordered(a)) === JSON.stringify(ordered(b));
};

export class ComposedDomStore {
  private context: ComposedContext | null = null;
  private baselineCursor = "0";
  private baselineSeq = 0;
  private cursor = "0";
  private seq = 0;
  private lastWitnessSeq = 0;
  private lastWitnessCursor = "0";
  private required = new Map<string, CoveredRevision>();
  private pending = new Map<string, string>();
  private issued = 0;
  private acknowledged = 0;
  private inFlight = 0;
  private failures = 0;
  private stopped = false;
  private armed = false;

  start(context: ComposedContext, cursor: string, seq: number): void {
    if (this.context) throw new Error("composed run already started");
    this.context = freezeContext(context);
    this.cursor = this.baselineCursor = cursor;
    this.lastWitnessCursor = cursor;
    this.seq = this.baselineSeq = this.lastWitnessSeq = seq;
  }
  issuedContext(context: ComposedContext): void {
    if (!this.context || Object.keys(this.context).some(key => key !== "nativeIncarnation" && context[key as keyof ComposedContext] !== this.context![key as keyof ComposedContext])) {
      this.fail(); return;
    }
    this.context = freezeContext(context);
    this.armed = true;
  }
  fail(): void { this.failures += 1; }
  frame(header: FrameHeader, patch: ProjectionPatch | null): void {
    if (!this.context || this.stopped || this.failures) return;
    if (header.subscriptionId !== this.context.subscriptionId || header.viewEpoch !== this.context.viewEpoch
      || header.coreGeneration !== this.context.coreGeneration || header.storeGeneration !== this.context.storeGeneration
      || header.streamSeq !== this.seq + 1) { this.fail(); return; }
    this.seq = header.streamSeq;
    if (patch) {
      if (patch.fromCursor !== this.cursor || patch.toCursor !== header.cursor) { this.fail(); return; }
      for (const row of patch.sessionUpserts) this.required.set(`SESSION:${row.sessionId}`, { entity: "SESSION", id: row.sessionId, revision: row.revision, absent: false });
      for (const row of patch.attentionUpserts) this.required.set(`ATTENTION:${row.attentionId}`, { entity: "ATTENTION", id: row.attentionId, revision: row.revision, absent: row.resolvedAtMs !== null });
      for (const row of patch.tombstones) this.required.set(`${row.entity}:${row.id}`, { entity: row.entity, id: row.id, revision: patch.toCursor, absent: true });
      for (const kind of patch.pageInvalidations) this.pending.set(kind, patch.toCursor);
      if (this.required.size > 1024) { this.fail(); this.required.clear(); return; }
      this.cursor = patch.toCursor;
    } else if (header.cursor !== this.cursor) this.fail();
  }
  hydrated(kind: string, through: string): void {
    const required = this.pending.get(kind);
    if (required !== undefined && BigInt(through) >= BigInt(required)) this.pending.delete(kind);
  }
  witness(state: ViewState, rendered: CoveredRevision[] | null, visible: boolean): DomWitness | null {
    if (!this.context || !this.armed || this.stopped || this.failures || this.pending.size || !visible || !rendered || this.inFlight
      || this.seq <= this.lastWitnessSeq || this.cursor === this.lastWitnessCursor) return null;
    const c = state.connection;
    if (state.phase !== "live" || !c || c.subscriptionId !== this.context.subscriptionId || c.viewEpoch !== this.context.viewEpoch
      || c.coreGeneration !== this.context.coreGeneration || c.storeGeneration !== this.context.storeGeneration || state.cursor !== this.cursor) { this.fail(); return null; }
    const rows = new Map(rendered.map(r => [`${r.entity}:${r.id}`, r]));
    for (const [key, required] of this.required) {
      const actual = rows.get(key);
      if (required.absent && !actual) { rows.set(key, { ...required }); continue; }
      if (!actual || BigInt(actual.revision) < BigInt(required.revision)
        || (actual.revision === required.revision && actual.absent !== required.absent)) return null;
    }
    if (rows.size > 1024 || this.issued >= 4096) { this.fail(); return null; }
    const witness: DomWitness = { schemaVersion: 3, witnessId: crypto.randomUUID(), context: freezeContext(this.context),
      fromCursor: this.baselineCursor, cursor: this.cursor, fromStreamSeq: this.baselineSeq, streamSeq: this.seq,
      revisions: [...rows.values()].map(row => Object.freeze({ ...row })), pendingHydration: 0, visible: true };
    Object.freeze(witness.revisions);
    Object.freeze(witness);
    this.lastWitnessSeq = this.seq;
    this.lastWitnessCursor = this.cursor;
    this.issued += 1;
    this.inFlight += 1;
    return witness;
  }
  receipt(witness: DomWitness, reply: unknown): void {
    this.inFlight = Math.max(0, this.inFlight - 1);
    const value = reply as { kind?: string; schemaVersion?: number; context?: ComposedContext; witness?: DomWitness; clock?: string; monotonicNs?: string; bootId?: string } | null;
    if (!value || value.kind !== "composed-dom-receipt" || value.schemaVersion !== 3 || value.clock !== "CLOCK_UPTIME_RAW"
      || !value.bootId || !/^\d+$/.test(value.monotonicNs ?? "") || !sameJson(value.context, witness.context)
      || !sameJson(value.witness, witness)) this.fail();
    else this.acknowledged += 1;
  }
  lost(): void { this.inFlight = Math.max(0, this.inFlight - 1); this.fail(); }
  close() {
    this.stopped = true;
    if (!this.context || this.pending.size || this.inFlight || this.acknowledged !== this.issued || this.cursor !== this.lastWitnessCursor) this.fail();
    return { op: "Close", context: this.context, totalWitnesses: this.issued, failures: this.failures };
  }
  status() { return { schemaVersion: 3, context: this.context, baselineCursor: this.baselineCursor, cursor: this.cursor,
    baselineSeq: this.baselineSeq, streamSeq: this.seq, issued: this.issued, acknowledged: this.acknowledged,
    pendingHydration: this.pending.size, inFlight: this.inFlight, failures: this.failures, stopped: this.stopped }; }
}

/** Called only after a mutation notification. Read actual operational rows,
 * revisions, counts and membership; a diagnostic cursor is never sufficient. */
export function renderedRevisions(state: ViewState, root: ParentNode): CoveredRevision[] | null {
  const fleet = [...root.querySelectorAll<HTMLElement>('[data-testid="fleet-row"]')];
  const attention = [...root.querySelectorAll<HTMLElement>('[data-testid="attention-row"]')];
  const open = state.attention.filter(row => row.resolvedAtMs === null);
  const panel = root.querySelector<HTMLElement>('[data-testid="attention-panel"]');
  if (fleet.length !== state.sessions.length || attention.length !== open.length || !panel
    || panel.dataset.needsAttention !== String(state.counts.needsAttention) || panel.dataset.awaitingAction !== String(state.counts.awaitingAction)
    || panel.querySelector('.panel__subtitle')?.textContent !== `${state.counts.needsAttention} need attention · ${state.counts.awaitingAction} awaiting action`) return null;
  const revisions: CoveredRevision[] = [];
  for (const row of state.sessions) {
    const element = fleet.find(e => e.dataset.sessionId === row.sessionId);
    if (!element || element.dataset.projectionRevision !== row.revision
      || element.querySelector('[data-testid="fleet-turn"]')?.textContent !== turnText(row.turnState)
      || element.querySelector('[data-testid="fleet-presence"]')?.textContent !== presenceText(row.executionPresence)
      || element.querySelector<HTMLElement>('[data-testid="fleet-worker-state"]')?.dataset.state !== workerState(row, state.attention)
      || element.querySelector('[data-testid="fleet-worker-state"]')?.textContent !== WORKER_STATE_TEXT[workerState(row, state.attention)]
      || element.querySelector<HTMLElement>('[data-testid="fleet-attention-count"]')?.dataset.count !== String(open.filter(a => a.sessionId === row.sessionId).length)
      || element.querySelector('[data-testid="fleet-attention-count"]')?.textContent !== openCountText(open.filter(a => a.sessionId === row.sessionId).length)) return null;
    revisions.push({ entity: "SESSION", id: row.sessionId, revision: row.revision, absent: false });
  }
  for (const row of state.attention) {
    const element = attention.find(e => e.dataset.attentionId === row.attentionId);
    if (row.resolvedAtMs !== null ? element !== undefined : element?.dataset.projectionRevision !== row.revision) return null;
    if (element && (element.querySelector('.row__label')?.textContent !== categoryText(row.category)
      || element.querySelector('.row__meta')?.textContent !== `${row.acknowledgedAtMs === null ? "Needs attention" : "Acknowledged"} · notification ${row.notificationState.toLowerCase().replaceAll("_", " ")}`
      || (element.querySelector('.row__summary')?.textContent ?? null) !== (row.summary || null)
      || (element.querySelector('[data-testid="attention-acknowledge"]') !== null) !== (row.acknowledgedAtMs === null))) return null;
    revisions.push({ entity: "ATTENTION", id: row.attentionId, revision: row.revision, absent: row.resolvedAtMs !== null });
  }
  return revisions;
}
