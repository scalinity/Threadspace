// The explicit Return control: when it may be pressed, and how its result
// reads. Surface result, session verification and input readiness stay
// separate lines (MILESTONES M2), and a refusal is spelled out by its code.

import type { InputReadiness } from "../contracts/generated/InputReadiness";
import type { SessionVerification } from "../contracts/generated/SessionVerification";
import type { SessionView } from "../contracts/generated/SessionView";
import type { SurfaceResult } from "../contracts/generated/SurfaceResult";
import { refusalText } from "./refusal";

export interface ReturnGate {
  enabled: boolean;
  /** Why the control is disabled; null when enabled. */
  reason: string | null;
}

/**
 * One rule for every Return button. A fixture has no native surface, and a
 * session without a valid binding on a live execution has nothing to return
 * to; several live bindings stay enabled so the companion can offer a choice.
 */
export function returnGate(session: Pick<SessionView, "fixture" | "liveBindings">, live: boolean, routing: string | null): ReturnGate {
  if (session.fixture) return { enabled: false, reason: "Fixture: no native surface" };
  if (session.liveBindings === 0) return { enabled: false, reason: "No live binding to return to" };
  if (!live) return { enabled: false, reason: "Companion not connected" };
  if (routing !== null) return { enabled: false, reason: "Another Return is in progress" };
  return { enabled: true, reason: null };
}

const SURFACE_TEXT: Record<SurfaceResult, string> = {
  EXACT_NATIVE_SURFACE: "Exact native surface focused",
  EXACT_WINDOW: "Containing window only",
  APP_ONLY: "Application only",
  URL_DISPATCHED: "URL dispatched; the visible tab is unproven",
  PROJECT_ONLY: "Project only",
  INSPECTOR_ONLY: "Nothing focused; inspector only",
  AMBIGUOUS: "Ambiguous; nothing focused",
  UNAVAILABLE: "Unavailable",
};

const VERIFICATION_TEXT: Record<SessionVerification, string> = {
  CURRENT_NATIVE_REVALIDATED: "Current session revalidated",
  NATIVE_BOUND_LAST_KNOWN: "Last known binding, not revalidated",
  USER_ATTESTED: "Owner attested",
  UNBOUND: "Unbound",
  CONFLICT: "Conflicting evidence",
};

const READINESS_TEXT: Record<InputReadiness, string> = {
  FOREGROUND_COMPATIBLE: "Ready for input",
  BACKGROUND_JOB: "Background job; not ready for input",
  UNKNOWN: "Input readiness unknown",
};

/** The axes a route result and its persisted summary share. */
export interface ReturnAxes {
  surfaceResult: SurfaceResult;
  sessionVerification: SessionVerification;
  inputReadiness: InputReadiness;
  reasonCode: string;
}

export interface ReturnLine {
  key: "surface" | "session" | "input" | "refusal";
  label: string;
  text: string;
  /** The raw contract value the text describes. */
  value: string;
  /** Only the strongest value on an axis reads as settled. */
  settled: boolean;
}

export function returnLines(result: ReturnAxes): ReturnLine[] {
  const lines: ReturnLine[] = [
    {
      key: "surface",
      label: "Surface",
      text: SURFACE_TEXT[result.surfaceResult],
      value: result.surfaceResult,
      settled: result.surfaceResult === "EXACT_NATIVE_SURFACE",
    },
    {
      key: "session",
      label: "Session",
      text: VERIFICATION_TEXT[result.sessionVerification],
      value: result.sessionVerification,
      settled: result.sessionVerification === "CURRENT_NATIVE_REVALIDATED",
    },
    {
      key: "input",
      label: "Input",
      text: READINESS_TEXT[result.inputReadiness],
      value: result.inputReadiness,
      settled: result.inputReadiness === "FOREGROUND_COMPATIBLE",
    },
  ];
  if (result.reasonCode !== "OK") {
    lines.push({ key: "refusal", label: "Refused", text: refusalText(result.reasonCode), value: result.reasonCode, settled: false });
  }
  return lines;
}
