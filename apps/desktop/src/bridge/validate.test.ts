// Frontend layer: wire validation only. Native Channel behavior is proven in
// the real Tauri application, not here.
import { describe, expect, it } from "vitest";

import { ValidationError, compareCursors, isCursor, parseFrame, parsePage, parseSnapshot } from "./validate";

const header = {
  protocolVersion: 1,
  storeGeneration: "s",
  coreGeneration: "c",
  subscriptionId: "sub",
  viewEpoch: "epoch",
  streamSeq: 1,
  cursor: "7",
};

describe("cursors", () => {
  it("accepts canonical cursors and compares numerically", () => {
    expect(isCursor("0")).toBe(true);
    expect(isCursor("9223372036854775807")).toBe(true);
    expect(isCursor("9223372036854775808")).toBe(false);
    expect(isCursor("07")).toBe(false);
    expect(isCursor(7)).toBe(false);
    expect(compareCursors("10", "9")).toBe(1);
  });
});

describe("parseFrame", () => {
  it("accepts every frame kind", () => {
    expect(parseFrame({ header, body: { kind: "SnapshotBegin", viewRevision: "7", chunkCount: 1, totalBytes: 10 } }).body.kind).toBe("SnapshotBegin");
    expect(parseFrame({ header, body: { kind: "BridgeHeartbeat" } }).body.kind).toBe("BridgeHeartbeat");
  });

  it("rejects unknown kinds, bad sequence numbers and noncanonical cursors", () => {
    expect(() => parseFrame({ header, body: { kind: "RunScript" } })).toThrow(ValidationError);
    expect(() => parseFrame({ header: { ...header, streamSeq: 0 }, body: { kind: "BridgeHeartbeat" } })).toThrow(ValidationError);
    expect(() => parseFrame({ header: { ...header, cursor: "-1" }, body: { kind: "BridgeHeartbeat" } })).toThrow(ValidationError);
    expect(() => parseFrame({ header: { ...header, protocolVersion: 2 }, body: { kind: "BridgeHeartbeat" } })).toThrow(ValidationError);
  });
});

describe("parseSnapshot", () => {
  it("rejects unknown states", () => {
    const snapshot = {
      viewRevision: "1",
      sessions: [],
      attention: [
        {
          attentionId: "a",
          sessionId: "s",
          turnId: null,
          category: "MADE_UP",
          priority: 40,
          summary: null,
          createdAtMs: 1,
          acknowledgedAtMs: null,
          resolvedAtMs: null,
          notificationState: "NOT_REQUESTED",
          revision: "1",
        },
      ],
      counts: { needsAttention: 1, awaitingAction: 0 },
    };
    expect(() => parseSnapshot(snapshot)).toThrow(ValidationError);
  });
});

describe("provider sessions", () => {
  const session = {
    sessionId: "s",
    provider: "claude",
    nativeSessionId: "11111111-2222-4333-8444-555555555555",
    displayName: "worker",
    activation: "1",
    turnState: "UNKNOWN",
    executionPresence: "LIVE",
    observation: "CURRENT",
    process: { pid: 501, bootId: "b", startSeconds: "1000", startMicroseconds: 7, executableIdentity: "/x#1:2" },
    binding: {
      bindingId: "b1",
      surfaceKind: "terminal.app",
      proof: "NATIVE_INVENTORY",
      revision: "42",
      locator: "/dev/ttys005",
      deviceNumber: 268435461,
      pid: 501,
    },
    liveBindings: 1,
    lastInvalidation: null,
    providerStatus: "idle",
    providerWaitingFor: null,
    lastRoute: {
      requestId: "r",
      surfaceResult: "EXACT_NATIVE_SURFACE",
      sessionVerification: "CURRENT_NATIVE_REVALIDATED",
      inputReadiness: "FOREGROUND_COMPATIBLE",
      reasonCode: "OK",
      focusPerformed: true,
      latencyMs: 512,
      recordedAtMs: 1,
    },
    fixture: false,
    revision: "42",
  };
  const snapshot = (sessions: unknown[]) => ({
    viewRevision: "42",
    sessions,
    attention: [],
    counts: { needsAttention: 0, awaitingAction: 0 },
    complete: true,
    totalSessions: sessions.length,
    totalAttention: 0,
    sessionsAfter: null,
    attentionAfter: null,
  });

  it("accepts bindings and the three route axes", () => {
    const parsed = parseSnapshot(snapshot([session]));
    expect(parsed.sessions[0]?.binding?.locator).toBe("/dev/ttys005");
    expect(parsed.sessions[0]?.lastRoute?.sessionVerification).toBe("CURRENT_NATIVE_REVALIDATED");
  });

  it("rejects an unknown route axis value", () => {
    const bad = { ...session, lastRoute: { ...session.lastRoute, surfaceResult: "CONNECTED" } };
    expect(() => parseSnapshot(snapshot([bad]))).toThrow(ValidationError);
  });
});

describe("bounded views and pages", () => {
  const base = { viewRevision: "9", sessions: [], attention: [], counts: { needsAttention: 3, awaitingAction: 0 } };

  it("rejects a complete snapshot whose rows disagree with its totals", () => {
    expect(() =>
      parseSnapshot({ ...base, complete: true, totalSessions: 2, totalAttention: 0, sessionsAfter: null, attentionAfter: null }),
    ).toThrow(ValidationError);
  });

  it("accepts a bounded snapshot that keeps exact totals and a continuation", () => {
    const parsed = parseSnapshot({
      ...base,
      complete: false,
      totalSessions: 5000,
      totalAttention: 3,
      sessionsAfter: "00000000-0000-4000-8000-000000000000",
      attentionAfter: null,
    });
    expect(parsed.complete).toBe(false);
    expect(parsed.totalSessions).toBe(5000);
    expect(parsed.counts.needsAttention).toBe(3);
  });

  it("validates page replies by kind", () => {
    const page = parsePage({ kind: "FleetPage", viewRevision: "10", rows: [], nextAfter: null, total: 0 });
    expect(page.kind).toBe("FleetPage");
    expect(() => parsePage({ kind: "Diagnostics" })).toThrow(ValidationError);
  });
});
