import { describe, expect, it } from "vitest";
import { ComposedDomStore, renderedRevisions, type ComposedContext, type DomWitness } from "./composedDuration";
import type { ViewState } from "../bridge/client";
import type { ProjectionPatch } from "../contracts/generated/ProjectionPatch";
import { attentionView, sessionView } from "../ui/testViews";

const context: ComposedContext = { runId: "run", domRuntimeId: "dom", nativeIncarnation: "native", subscriptionId: "sub", viewEpoch: "view", coreGeneration: "core", storeGeneration: "store" };
const header = (seq: number, cursor: string) => ({ protocolVersion: 1, ...context, streamSeq: seq, cursor });
const row = sessionView({ revision: "8" });
const patch = (invalidations: ProjectionPatch["pageInvalidations"] = []): ProjectionPatch => ({ fromCursor: "7", toCursor: "8", viewRevision: "8", sessionUpserts: invalidations.length ? [] : [row], attentionUpserts: [], tombstones: [], counts: { needsAttention: 0, awaitingAction: 0 }, pageInvalidations: invalidations });
const state = (partial: Partial<ViewState> = {}) => ({ phase: "live", connection: context, cursor: "8", sessions: [row], attention: [], counts: { needsAttention: 0, awaitingAction: 0 }, ...partial }) as ViewState;
const rendered = [{ entity: "SESSION" as const, id: row.sessionId, revision: "8", absent: false }];
const reply = (witness: DomWitness, ns = "1000000000") => ({ kind: "composed-dom-receipt", schemaVersion: 3, context: witness.context, witness, clock: "CLOCK_UPTIME_RAW", bootId: "boot", monotonicNs: ns });
function started() { const store = new ComposedDomStore(); store.start(context, "7", 3); store.issuedContext(context); return store; }

describe("composed DOM qualification controls (controlled renderer fixtures)", () => {
  it("requires rendered attention text and counts even when revision attributes are current", () => {
    const item = attentionView({ revision: "8" });
    const current = state({ sessions: [], attention: [item], counts: { needsAttention: 1, awaitingAction: 0 } });
    let category = "Error";
    const element = { dataset: { attentionId: item.attentionId, projectionRevision: "8" }, querySelector: (selector: string) => {
      if (selector === '.row__label') return { textContent: category };
      if (selector === '.row__meta') return { textContent: 'Needs attention · notification not requested' };
      if (selector === '[data-testid="attention-acknowledge"]') return {};
      return null;
    } };
    const panel = { dataset: { needsAttention: '1', awaitingAction: '0' }, querySelector: () => ({ textContent: '1 need attention · 0 awaiting action' }) };
    const root = { querySelectorAll: (selector: string) => selector.includes('attention-row') ? [element] : [], querySelector: () => panel } as unknown as ParentNode;
    expect(renderedRevisions(current, root)).toBeNull();
    category = "Turn complete";
    expect(renderedRevisions(current, root)).toEqual([{ entity: 'ATTENTION', id: item.attentionId, revision: '8', absent: false }]);
  });
  it("requires actual operational readback rather than a diagnostic cursor", () => {
    const store = started(); store.frame(header(4, "8"), patch());
    expect(store.witness(state(), null, true)).toBeNull();
    expect(store.witness(state(), [{ ...rendered[0], revision: "7" }], true)).toBeNull();
    expect(store.witness(state(), rendered, false)).toBeNull();
    const cursorOnly = { querySelectorAll: () => [], querySelector: () => ({ textContent: "8", dataset: {} }) } as unknown as ParentNode;
    expect(renderedRevisions(state(), cursorOnly)).toBeNull();
  });
  it("retains unresolved invalidation until affected paging completed at its revision", () => {
    const store = started(); store.frame(header(4, "8"), patch(["SESSION"]));
    expect(store.witness(state(), rendered, true)).toBeNull();
    store.hydrated("SESSION", "7");
    expect(store.witness(state(), rendered, true)).toBeNull();
    store.hydrated("SESSION", "8");
    expect(store.witness(state(), rendered, true)).not.toBeNull();
  });
  it("freezes DOM-time context and revisions through a delayed native acknowledgement", () => {
    const store = started(); store.frame(header(4, "8"), patch());
    const witness = store.witness(state(), rendered, true)!;
    expect(Object.isFrozen(witness)).toBe(true);
    expect(Object.isFrozen(witness.context)).toBe(true);
    expect(Object.isFrozen(witness.revisions[0])).toBe(true);
    context.nativeIncarnation = "replacement";
    expect(witness.context.nativeIncarnation).toBe("native");
    context.nativeIncarnation = "native";
    store.receipt(witness, reply(witness, "9000000000000"));
    expect(store.close().failures).toBe(0);
  });
  it("does not manufacture a witness while native start is unconfirmed", () => {
    const store = new ComposedDomStore(); store.start(context, "7", 3); store.frame(header(4, "8"), patch());
    expect(store.witness(state(), rendered, true)).toBeNull();
    store.issuedContext(context);
    expect(store.witness(state(), rendered, true)).not.toBeNull();
  });
  it("lost, malformed or context-replaced acknowledgement prevents a complete census", () => {
    for (const mode of ["lost", "context", "stamp"]) {
      const store = started(); store.frame(header(4, "8"), patch());
      const witness = store.witness(state(), rendered, true)!;
      if (mode === "lost") store.lost();
      else store.receipt(witness, mode === "context" ? { ...reply(witness), context: { ...context, subscriptionId: "later" } } : reply(witness, "bad"));
      expect(store.close().failures).toBeGreaterThan(0);
    }
  });
  it("sequence gaps and replacement subscriptions permanently refuse qualification", () => {
    for (const wrong of [header(5, "8"), { ...header(4, "8"), subscriptionId: "replacement" }]) {
      const store = started(); store.frame(wrong, patch());
      expect(store.witness(state(), rendered, true)).toBeNull();
      expect(store.close().failures).toBeGreaterThan(0);
    }
  });
  it("leaves unapplied tail and pending hydration in the denominator", () => {
    const store = started(); store.frame(header(4, "8"), patch(["ATTENTION"]));
    expect(store.close().failures).toBeGreaterThan(0);
    expect(store.status().pendingHydration).toBe(1);
  });
  it("heartbeat-only progress does not issue duplicate DOM witnesses", () => {
    const store = started(); store.frame(header(4, "8"), patch());
    const witness = store.witness(state(), rendered, true)!; store.receipt(witness, reply(witness));
    store.frame(header(5, "8"), null);
    expect(store.witness(state(), rendered, true)).toBeNull();
    expect(store.close().failures).toBe(0);
  });
});
