import { describe, expect, it } from "vitest";

import { KNOWN_REFUSAL_CODES, refusalText } from "./refusal";

/** Every reason code `crates/surfaces` route() can finish with. */
const ROUTE_CODES = [
  "SESSION_NOT_FOUND",
  "NO_NATIVE_SURFACE",
  "BINDING_NOT_FOUND",
  "NO_LIVE_MAPPING",
  "MULTIPLE_ATTACHMENTS",
  "BINDING_STALE",
  "TARGET_GONE",
  "PROCESS_UNREADABLE",
  "EXECUTABLE_CHANGED",
  "DEVICE_CHANGED",
  "SESSION_CHANGED",
  "PROVIDER_CONFLICT",
  "AUTOMATION_DENIED",
  "AUTOMATION_UNKNOWN",
  "TERMINAL_ENUMERATION_FAILED",
  "TERMINAL_GENERATION_CHANGED",
  "NO_MATCHING_TAB",
  "MULTIPLE_MATCHING_TABS",
  "SURFACE_CHANGED",
  "READBACK_FAILED",
  "ACTIVATION_REFUSED",
  "POST_FOCUS_LOOKUP_FAILED",
  "BINDING_CHANGED_DURING_ROUTE",
  "TIMEOUT",
];

/** `crates/provider-claude` InventoryError::code(), returned when the route's lookup fails. */
const INVENTORY_CODES = ["INVENTORY_SPAWN_FAILED", "INVENTORY_TIMEOUT", "INVENTORY_FAILED", "INVENTORY_TRUNCATED", "INVENTORY_PARSE_FAILED"];

/** Surface statuses an unbound session's route carries (`crates/journal` identity, `apps/agent-macos` discovery). */
const SURFACE_STATUS_CODES = [
  "SURFACE_UNPROVEN",
  "SURFACE_NOT_ATTEMPTED",
  "UNSUPPORTED_KIND",
  "MULTIPLE_TERMINAL_PROCESSES",
  "TERMINAL_NOT_RUNNING",
  "TERMINAL_STATE_UNKNOWN",
  "AUTOMATION_NOT_AUTHORIZED",
  "AUTOMATION_STATE_UNKNOWN",
  "PROCESS_CHANGED_DURING_SURFACE_JOIN",
];

const ALL_CODES = [...ROUTE_CODES, ...INVENTORY_CODES, ...SURFACE_STATUS_CODES];

describe("refusalText", () => {
  it.each(ALL_CODES)("has dedicated readable text for %s", (code) => {
    const text = refusalText(code);
    expect(text).not.toMatch(/unrecognized/);
    expect(text).not.toContain(code);
    expect(text.length).toBeGreaterThan(20);
  });

  it("covers exactly the known codes plus OK", () => {
    expect([...KNOWN_REFUSAL_CODES].sort()).toEqual(["OK", ...ALL_CODES].sort());
  });

  it("gives distinct text to distinct codes", () => {
    expect(new Set(ALL_CODES.map(refusalText)).size).toBe(ALL_CODES.length);
  });

  it("falls back honestly to the raw code", () => {
    expect(refusalText("SOMETHING_NEW")).toBe("Return refused with an unrecognized code: SOMETHING_NEW.");
    expect(refusalText("")).toContain("unrecognized");
  });

  it("does not treat object prototype keys as known codes", () => {
    expect(refusalText("constructor")).toBe("Return refused with an unrecognized code: constructor.");
    expect(refusalText("toString")).toContain("unrecognized");
  });
});
