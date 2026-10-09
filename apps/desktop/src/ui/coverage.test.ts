import { describe, expect, it } from "vitest";

import { coverageLine, inventoryText, observationText, observerText, presenceText, turnText } from "./coverage";
import { sessionView } from "./testViews";

describe("observer tier text", () => {
  it("names a native observer with its version", () => {
    expect(observerText("NATIVE", "2.1.295")).toBe("Native observer 2.1.295");
    expect(observerText("NATIVE", null)).toBe("Native observer");
  });

  it("says a restored observer was restored after reload", () => {
    expect(observerText("RESTORED", "2.1.295")).toBe("Observer restored after reload");
    expect(observerText("RESTORED", null)).toBe("Observer restored after reload");
  });

  it("never lets a lower tier read as native", () => {
    expect(observerText("LOWER_TIER", null)).toBe("Lower tier: identity unverified");
    expect(observerText("LOWER_TIER", "2.1.292")).toBe("Limited: unqualified Claude 2.1.292");
  });

  it("says when there is no observer", () => {
    expect(observerText(null, null)).toBe("No observer");
    expect(observerText(null, "2.1.295")).toBe("No observer");
  });
});

describe("coverage line", () => {
  it("joins observer tier and observation state", () => {
    expect(coverageLine(sessionView())).toBe("Native observer 2.1.295 · observation current");
    expect(coverageLine(sessionView({ observerTier: null, observerVersion: null, observation: "DISCONNECTED" }))).toBe("No observer · observation disconnected");
    expect(coverageLine(sessionView({ observerTier: "LOWER_TIER", observerVersion: null, observation: "STALE" }))).toBe(
      "Lower tier: identity unverified · observation stale",
    );
  });

  it("does not claim observation for a fixture", () => {
    expect(coverageLine(sessionView({ fixture: true }))).toBe("Fixture: not observed");
  });

  it("has text for every observation state", () => {
    for (const state of ["CURRENT", "STALE", "DISCONNECTED", "CONFLICT", "UNKNOWN"] as const) {
      expect(observationText(state)).toBe(state.toLowerCase());
    }
  });
});

describe("inventory status", () => {
  it("is labelled as the provider's report", () => {
    expect(inventoryText(sessionView({ providerStatus: "busy" }))).toBe("Provider-reported: busy");
    expect(inventoryText(sessionView({ providerStatus: "waiting", providerWaitingFor: "input needed" }))).toBe("Provider-reported: waiting (input needed)");
  });

  it("says when there is no report", () => {
    expect(inventoryText(sessionView({ providerStatus: null }))).toBe("No inventory report");
  });
});

describe("turn and presence", () => {
  it("has text for every turn state and presence", () => {
    const turns = ["UNKNOWN", "QUEUED", "WORKING", "WAITING", "COMPLETED", "INTERRUPTED", "FAILED", "REFUSED"] as const;
    expect(new Set(turns.map(turnText)).size).toBe(turns.length);
    expect(turnText("COMPLETED")).toBe("Turn completed");
    const presences = ["LIVE", "DETACHED", "PARKED", "ENDED", "UNKNOWN"] as const;
    expect(new Set(presences.map(presenceText)).size).toBe(presences.length);
    expect(presenceText("ENDED")).toBe("Ended");
  });
});
