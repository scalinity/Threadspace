// Textual equivalents of a session's coverage, turn and presence (SPEC §15.5).
// Each line says what the evidence supports and no more: inventory status is
// what the provider reported, not a turn outcome, and a lower-tier observer
// never reads as native.

import type { ExecutionPresence } from "../contracts/generated/ExecutionPresence";
import type { ObservationState } from "../contracts/generated/ObservationState";
import type { ObserverTier } from "../contracts/generated/ObserverTier";
import type { SessionView } from "../contracts/generated/SessionView";
import type { TurnState } from "../contracts/generated/TurnState";

export function observerText(tier: ObserverTier | null, version: string | null): string {
  switch (tier) {
    case "NATIVE":
      return version ? `Native observer ${version}` : "Native observer";
    case "RESTORED":
      return "Observer restored after reload";
    case "LOWER_TIER":
      return version ? `Limited: unqualified Claude ${version}` : "Lower tier: identity unverified";
    case null:
      return "No observer";
  }
}

export function observationText(observation: ObservationState): string {
  return observation.toLowerCase();
}

/** Observer tier and observation state, the fleet row's coverage line. */
export function coverageLine(session: SessionView): string {
  if (session.fixture) return "Fixture: not observed";
  return `${observerText(session.observerTier, session.observerVersion)} · observation ${observationText(session.observation)}`;
}

/** Native inventory status, labelled as the provider's own report. */
export function inventoryText(session: SessionView): string {
  if (!session.providerStatus) return "No inventory report";
  const waiting = session.providerWaitingFor ? ` (${session.providerWaitingFor})` : "";
  return `Provider-reported: ${session.providerStatus}${waiting}`;
}

export const LINK_CONFLICT_TEXT = "Observer reports disagree: coverage is uncertain";

const TURN_TEXT: Record<TurnState, string> = {
  UNKNOWN: "No turn observed",
  QUEUED: "Turn queued",
  WORKING: "Turn working",
  WAITING: "Turn waiting",
  COMPLETED: "Turn completed",
  INTERRUPTED: "Turn interrupted",
  FAILED: "Turn failed",
  REFUSED: "Turn refused",
};

export function turnText(turn: TurnState): string {
  return TURN_TEXT[turn];
}

const PRESENCE_TEXT: Record<ExecutionPresence, string> = {
  LIVE: "Live",
  DETACHED: "Detached",
  PARKED: "Parked",
  ENDED: "Ended",
  UNKNOWN: "Presence unknown",
};

export function presenceText(presence: ExecutionPresence): string {
  return PRESENCE_TEXT[presence];
}
