// Projection rows for the UI unit tests. Only test files import this module.

import type { AttentionView } from "../contracts/generated/AttentionView";
import type { SessionView } from "../contracts/generated/SessionView";

export function sessionView(overrides: Partial<SessionView> = {}): SessionView {
  return {
    sessionId: "00000000-0000-4000-8000-000000000001",
    provider: "claude",
    nativeSessionId: "11111111-2222-4333-8444-555555555555",
    displayName: "worker",
    activation: "1",
    turnState: "COMPLETED",
    executionPresence: "LIVE",
    observation: "CURRENT",
    observerTier: "NATIVE",
    observerVersion: "2.1.295",
    linkConflict: false,
    process: null,
    binding: null,
    liveBindings: 1,
    lastInvalidation: null,
    providerStatus: "idle",
    providerWaitingFor: null,
    lastRoute: null,
    fixture: false,
    revision: "1",
    ...overrides,
  };
}

export function attentionView(overrides: Partial<AttentionView> = {}): AttentionView {
  return {
    attentionId: "a-1",
    sessionId: "00000000-0000-4000-8000-000000000001",
    turnId: null,
    category: "TURN_COMPLETE",
    priority: 40,
    summary: null,
    createdAtMs: 1,
    acknowledgedAtMs: null,
    resolvedAtMs: null,
    notificationState: "NOT_REQUESTED",
    revision: "1",
    ...overrides,
  };
}
