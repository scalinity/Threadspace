import { describe, expect, it } from "vitest";

import { awaitingAction, canAcknowledge, canMarkHandled, itemStateText, needsAttention, openItems, openItemsFor, selectableItem } from "./attention";
import { attentionView } from "./testViews";

const S = "00000000-0000-4000-8000-000000000001";
const OTHER = "00000000-0000-4000-8000-000000000002";

const fresh = attentionView({ attentionId: "fresh", sessionId: S });
const acknowledged = attentionView({ attentionId: "acked", sessionId: S, acknowledgedAtMs: 10 });
const resolvedUnacknowledged = attentionView({ attentionId: "resolved-unacked", sessionId: S, resolvedAtMs: 20 });
const resolvedAcknowledged = attentionView({ attentionId: "resolved-acked", sessionId: S, acknowledgedAtMs: 10, resolvedAtMs: 20 });
const elsewhere = attentionView({ attentionId: "elsewhere", sessionId: OTHER });

describe("attention lists exclude resolved items", () => {
  const all = [resolvedUnacknowledged, fresh, resolvedAcknowledged, acknowledged, elsewhere];

  it("keeps only unresolved items, in projection order", () => {
    expect(openItems(all).map((item) => item.attentionId)).toEqual(["fresh", "acked", "elsewhere"]);
  });

  it("splits needs attention from awaiting action without resolved items", () => {
    expect(needsAttention(all).map((item) => item.attentionId)).toEqual(["fresh", "elsewhere"]);
    expect(awaitingAction(all).map((item) => item.attentionId)).toEqual(["acked"]);
  });

  it("counts a session's open items only", () => {
    expect(openItemsFor(all, S).map((item) => item.attentionId)).toEqual(["fresh", "acked"]);
    expect(openItemsFor([resolvedUnacknowledged, resolvedAcknowledged], S)).toEqual([]);
  });
});

describe("selection and controls ignore resolved items", () => {
  it("selects the first item needing attention, then one awaiting action, never a resolved one", () => {
    expect(selectableItem([resolvedUnacknowledged, acknowledged, fresh], S)?.attentionId).toBe("fresh");
    expect(selectableItem([resolvedUnacknowledged, acknowledged], S)?.attentionId).toBe("acked");
    expect(selectableItem([resolvedUnacknowledged, resolvedAcknowledged], S)).toBeNull();
    expect(selectableItem([elsewhere], S)).toBeNull();
  });

  it("allows Acknowledge only for an open unacknowledged item", () => {
    expect(canAcknowledge(fresh)).toBe(true);
    expect(canAcknowledge(acknowledged)).toBe(false);
    expect(canAcknowledge(resolvedUnacknowledged)).toBe(false);
    expect(canAcknowledge(undefined)).toBe(false);
  });

  it("allows Mark handled only for an open item", () => {
    expect(canMarkHandled(fresh)).toBe(true);
    expect(canMarkHandled(acknowledged)).toBe(true);
    expect(canMarkHandled(resolvedUnacknowledged)).toBe(false);
    expect(canMarkHandled(resolvedAcknowledged)).toBe(false);
    expect(canMarkHandled(null)).toBe(false);
  });

  it("states a resolved item as resolved whatever its acknowledgement", () => {
    expect(itemStateText(fresh)).toBe("Needs attention");
    expect(itemStateText(acknowledged)).toBe("Acknowledged, awaiting action");
    expect(itemStateText(resolvedUnacknowledged)).toBe("Resolved");
    expect(itemStateText(resolvedAcknowledged)).toBe("Resolved");
  });
});
