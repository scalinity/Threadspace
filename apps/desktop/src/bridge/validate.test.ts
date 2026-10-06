// Frontend layer: wire validation only. Native Channel behavior is proven in
// the real Tauri application, not here.
import { describe, expect, it } from "vitest";

import { ValidationError, compareCursors, isCursor, parseFrame, parseSnapshot } from "./validate";

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
