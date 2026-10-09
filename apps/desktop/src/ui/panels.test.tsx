// Static renders of the fleet and inspector (react-dom/server, no DOM): the
// `data-testid` / `data-*` contract the M2 qualification commands read back.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { BridgeClient, ViewState } from "../bridge/client";
import { FleetPanel } from "./FleetPanel";
import { Inspector } from "./Inspector";
import { attentionView, sessionView } from "./testViews";

const LIVE = "00000000-0000-4000-8000-0000000000a1";
const FIXTURE = "00000000-0000-4000-8000-0000000000f1";
const client = {} as BridgeClient;

function view(partial: Partial<ViewState>): ViewState {
  return {
    phase: "live",
    sessions: [],
    attention: [],
    counts: { needsAttention: 0, awaitingAction: 0 },
    inspector: null,
    routes: {},
    routeFailures: {},
    routing: null,
    resolving: null,
    resolutions: {},
    ...partial,
  } as ViewState;
}

function rowMarkup(html: string, sessionId: string): string {
  const start = html.indexOf(`data-session-id="${sessionId}"`);
  const end = html.indexOf("</li>", start);
  expect(start).toBeGreaterThan(-1);
  return html.slice(start, end);
}

describe("fleet rows", () => {
  const sessions = [
    sessionView({ sessionId: LIVE, turnState: "COMPLETED", liveBindings: 1, providerStatus: "idle", linkConflict: true }),
    sessionView({ sessionId: FIXTURE, fixture: true, liveBindings: 0, observerTier: null, observerVersion: null, observation: "UNKNOWN" }),
  ];
  const attention = [
    attentionView({ attentionId: "open", sessionId: LIVE }),
    attentionView({ attentionId: "done", sessionId: LIVE, resolvedAtMs: 9 }),
  ];

  it("renders every session with state, turn, presence, coverage, inventory and open count", () => {
    const html = renderToStaticMarkup(<FleetPanel state={view({ sessions, attention })} client={client} />);
    const live = rowMarkup(html, LIVE);
    expect(live).toContain('data-testid="fleet-worker-state" data-state="attention"');
    expect(live).toContain(">Turn completed<");
    expect(live).toContain(">Live<");
    expect(live).toContain('data-count="1"');
    expect(live).toContain("Native observer 2.1.295 · observation current");
    expect(live).toContain("Provider-reported: idle");
    expect(live).toContain('data-testid="fleet-link-conflict"');
    expect(rowMarkup(html, FIXTURE)).toContain("Fixture: not observed");
  });

  it("disables Return for a fixture and enables it for a live binding", () => {
    const html = renderToStaticMarkup(<FleetPanel state={view({ sessions, attention })} client={client} />);
    expect(rowMarkup(html, FIXTURE)).toMatch(/<button[^>]*disabled=""[^>]*data-testid="fleet-return"/);
    expect(rowMarkup(html, LIVE)).not.toMatch(/<button[^>]*disabled=""[^>]*data-testid="fleet-return"/);
  });

  it("shows a refused Return as separate lines with its readable refusal", () => {
    const route = {
      surfaceResult: "UNAVAILABLE",
      sessionVerification: "CONFLICT",
      inputReadiness: "UNKNOWN",
      reasonCode: "BINDING_STALE",
    } as ViewState["routes"][string];
    const html = renderToStaticMarkup(<FleetPanel state={view({ sessions, attention, routes: { [LIVE]: route } })} client={client} />);
    const live = rowMarkup(html, LIVE);
    for (const line of ["surface", "session", "input", "refusal"]) expect(live).toContain(`data-line="${line}"`);
    expect(live).toContain("The binding changed after it was loaded, so it was not used.");
  });
});

describe("inspector", () => {
  const session = sessionView({ sessionId: LIVE });
  const inspector = (attentionId: string) => ({
    attentionId,
    sessionId: LIVE,
    source: "SELECTION" as const,
    outstandingAtOpen: true,
    intentId: null,
    openedAtMs: 0,
    route: null,
    observationEnabled: null,
  });

  it("enables Mark handled for an open item", () => {
    const state = view({ sessions: [session], attention: [attentionView({ attentionId: "open", sessionId: LIVE })], inspector: inspector("open") });
    const html = renderToStaticMarkup(<Inspector state={state} client={client} />);
    expect(html).toMatch(/<button[^>]*data-testid="inspector-mark-handled"[^>]*>Mark handled</);
    expect(html).not.toMatch(/disabled=""[^>]*data-testid="inspector-mark-handled"/);
    expect(html).toContain('data-testid="inspector-acknowledge"');
  });

  it("disables Mark handled and hides Acknowledge for a resolved item", () => {
    const state = view({
      sessions: [session],
      attention: [attentionView({ attentionId: "done", sessionId: LIVE, resolvedAtMs: 9 })],
      inspector: inspector("done"),
    });
    const html = renderToStaticMarkup(<Inspector state={state} client={client} />);
    expect(html).toMatch(/disabled=""[^>]*data-testid="inspector-mark-handled"/);
    expect(html).not.toContain('data-testid="inspector-acknowledge"');
    expect(html).toContain(">Resolved<");
  });
});
