import { describe, expect, it } from "vitest";
import { LatencyStore, type LatencyContext } from "./latencyStore";

const context: LatencyContext = { phase: "live", coreGeneration: "core", storeGeneration: "store", viewEpoch: "view" };

describe("F4 actual DOM latency marks", () => {
  it("never promotes a received/store-applied patch to a DOM mark", () => {
    let clock = 1;
    const store = new LatencyStore("runtime", () => clock);
    store.start(context, "10", true);
    clock = 2; store.apply("11", context);
    expect(store.page().marks[0]?.domMonotonicMs).toBeNull();
    clock = 7; store.rendered("11", context, true);
    expect(store.page().marks[0]).toMatchObject({ appliedMonotonicMs: 2, domMonotonicMs: 7, hydratedVisible: true });
  });
  it("retains all coalesced cursors and their actual common DOM boundary", () => {
    const store = new LatencyStore("runtime", () => 20);
    store.start(context, "9007199254740992", true);
    store.apply("9007199254740993", context);
    store.apply("9007199254740994", context);
    store.rendered("9007199254740994", context, true);
    expect(store.page().marks).toHaveLength(2);
    expect(store.page().marks.every(mark => mark.domCursor === "9007199254740994")).toBe(true);
  });
  it("refuses a hidden or unhydrated start and leaves hidden DOM unmatched", () => {
    const store = new LatencyStore("runtime", () => 1);
    expect(() => store.start(context, "0", false)).toThrow();
    expect(() => store.start({ ...context, phase: "hydrating" }, "0", true)).toThrow();
    store.start(context, "0", true);
    store.apply("1", context);
    store.rendered("1", context, false);
    expect(store.page().marks[0]?.domMonotonicMs).toBeNull();
  });
  it("does not join observations across a changed view or store epoch", () => {
    const store = new LatencyStore("runtime", () => 1);
    store.start(context, "0", true);
    store.apply("1", context);
    store.apply("2", { ...context, storeGeneration: "other" });
    store.rendered("2", context, true);
    expect(store.page().contextChanged).toBe(true);
    expect(store.page().marks.every(mark => mark.domMonotonicMs === null)).toBe(true);
  });
  it("counts overflow instead of evicting slow or unmatched samples", () => {
    const store = new LatencyStore("runtime", () => 1, 1);
    store.start(context, "0", true);
    store.apply("1", context); store.apply("2", context);
    expect(store.page()).toMatchObject({ totalApplied: 2, retainedMarks: 1, overflow: 1 });
    expect(store.page().marks[0]?.cursor).toBe("1");
  });
  it("makes invalid/regressing cursors and page boundaries explicit", () => {
    const store = new LatencyStore("runtime", () => 1);
    store.start(context, "10", true);
    store.apply("9", context); store.apply("invalid", context);
    store.apply("11", context); store.apply("12", context);
    expect(store.page(0, 1)).toMatchObject({ invalid: 2, nextOffset: 1, retainedMarks: 2 });
    expect(store.page(1, 1).marks[0]?.cursor).toBe("12");
    expect(() => store.page(0, 101)).toThrow();
  });
  it("freezes the raw population for pagination without completing pending DOM work", () => {
    const store = new LatencyStore("runtime", () => 1);
    store.start(context, "0", true);
    store.apply("1", context);
    store.stop();
    store.apply("2", context);
    store.rendered("1", context, true);
    expect(store.page()).toMatchObject({ totalApplied: 1, stopped: true });
    expect(store.page().marks[0]?.domMonotonicMs).toBeNull();
  });
});
