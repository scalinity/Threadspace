import { describe, expect, it } from "vitest";

import { refusalText } from "./refusal";
import { returnGate, returnLines } from "./returnControl";
import { sessionView } from "./testViews";

describe("Return gating", () => {
  it("is disabled for a fixture, whatever its bindings", () => {
    expect(returnGate(sessionView({ fixture: true, liveBindings: 0 }), true, null)).toEqual({ enabled: false, reason: "Fixture: no native surface" });
    expect(returnGate(sessionView({ fixture: true, liveBindings: 1 }), true, null).enabled).toBe(false);
  });

  it("is disabled without a live binding", () => {
    expect(returnGate(sessionView({ liveBindings: 0 }), true, null)).toEqual({ enabled: false, reason: "No live binding to return to" });
  });

  it("is enabled for one or several live bindings", () => {
    expect(returnGate(sessionView({ liveBindings: 1 }), true, null)).toEqual({ enabled: true, reason: null });
    expect(returnGate(sessionView({ liveBindings: 3 }), true, null).enabled).toBe(true);
  });

  it("is disabled while disconnected or while another Return runs", () => {
    expect(returnGate(sessionView(), false, null)).toEqual({ enabled: false, reason: "Companion not connected" });
    expect(returnGate(sessionView(), true, "00000000-0000-4000-8000-0000000000ff")).toEqual({ enabled: false, reason: "Another Return is in progress" });
  });
});

describe("Return result lines", () => {
  it("keeps surface, session verification and input readiness as separate lines", () => {
    const lines = returnLines({
      surfaceResult: "EXACT_NATIVE_SURFACE",
      sessionVerification: "CURRENT_NATIVE_REVALIDATED",
      inputReadiness: "FOREGROUND_COMPATIBLE",
      reasonCode: "OK",
    });
    expect(lines.map((line) => line.key)).toEqual(["surface", "session", "input"]);
    expect(lines.every((line) => line.settled)).toBe(true);
    expect(lines.map((line) => line.value)).toEqual(["EXACT_NATIVE_SURFACE", "CURRENT_NATIVE_REVALIDATED", "FOREGROUND_COMPATIBLE"]);
  });

  it("adds a readable refusal line with its raw code when the Return did not complete", () => {
    const lines = returnLines({ surfaceResult: "UNAVAILABLE", sessionVerification: "CONFLICT", inputReadiness: "UNKNOWN", reasonCode: "NO_MATCHING_TAB" });
    expect(lines.map((line) => line.key)).toEqual(["surface", "session", "input", "refusal"]);
    expect(lines.some((line) => line.settled)).toBe(false);
    expect(lines[3]).toMatchObject({ label: "Refused", value: "NO_MATCHING_TAB", text: refusalText("NO_MATCHING_TAB") });
  });

  it("keeps an exact surface with an unverified session visibly uncertain on that axis", () => {
    const lines = returnLines({
      surfaceResult: "EXACT_NATIVE_SURFACE",
      sessionVerification: "NATIVE_BOUND_LAST_KNOWN",
      inputReadiness: "BACKGROUND_JOB",
      reasonCode: "POST_FOCUS_LOOKUP_FAILED",
    });
    expect(lines.map((line) => line.settled)).toEqual([true, false, false, false]);
  });
});
