import type { BridgeClient, ViewState } from "../bridge/client";
import type { SessionView } from "../contracts/generated/SessionView";
import { openCountText, openItemsFor } from "./attention";
import { LINK_CONFLICT_TEXT, coverageLine, inventoryText, presenceText, turnText } from "./coverage";
import { ReturnResult } from "./ReturnResult";
import { returnGate } from "./returnControl";
import { WORKER_STATE_TEXT, workerState } from "./worker";

function surfaceLine(session: SessionView): string {
  if (session.fixture) return "fixture — no native surface";
  const parts: string[] = [];
  if (session.binding) {
    parts.push(`Terminal ${session.binding.locator} · binding rev ${session.binding.revision}`);
  } else {
    parts.push(session.lastInvalidation ? `unbound — ${session.lastInvalidation.toLowerCase().replaceAll("_", " ")}` : "unbound");
  }
  if (session.liveBindings > 1) parts.push(`${session.liveBindings} live attachments — choose in inspector`);
  return parts.join(" · ");
}

/**
 * One row per Session, whatever its turn or attention. Selecting a row only
 * opens the inspector; the Return button is the only control that routes.
 */
export function FleetPanel({ state, client }: { state: ViewState; client: BridgeClient }) {
  const live = state.phase === "live";
  return (
    <section className="panel" aria-labelledby="fleet-heading">
      <div className="panel__head">
        <h2 id="fleet-heading" className="panel__title">Fleet</h2>
        <button type="button" className="button button--quiet" onClick={() => void client.refreshEvidence()} disabled={!live}>
          Refresh evidence
        </button>
      </div>
      <ul className="list">
        {state.sessions.map((session) => {
          const worker = workerState(session, state.attention);
          const open = openItemsFor(state.attention, session.sessionId).length;
          const gate = returnGate(session, live, state.routing);
          const route = state.routes[session.sessionId] ?? null;
          const failure = state.routeFailures[session.sessionId] ?? null;
          const selected = state.inspector?.sessionId === session.sessionId;
          return (
            <li
              key={session.sessionId}
              className={`row row--worker ${selected ? "row--selected" : ""} ${worker === "ended" ? "row--ended" : ""}`}
              data-testid="fleet-row"
              data-session-id={session.sessionId}
            >
              <button type="button" className="row__main" onClick={() => client.select(session.sessionId)} aria-current={selected ? "true" : undefined} data-testid="fleet-select">
                <span className="row__label">
                  {session.displayName}
                  {session.fixture ? <span className="tag">fixture</span> : <span className="tag">{session.provider}</span>}
                  <span className="mono row__id" data-testid="fleet-native-id">{session.nativeSessionId.slice(0, 8)}</span>
                </span>
                <span className="row__meta">
                  <span className={`worker-state worker-state--${worker}`} data-testid="fleet-worker-state" data-state={worker}>
                    {WORKER_STATE_TEXT[worker]}
                  </span>
                  {" · "}
                  <span data-testid="fleet-turn">{turnText(session.turnState)}</span>
                  {" · "}
                  <span data-testid="fleet-presence">{presenceText(session.executionPresence)}</span>
                  {" · "}
                  <span data-testid="fleet-attention-count" data-count={open}>{openCountText(open)}</span>
                </span>
                <span className="row__meta" data-testid="fleet-coverage">{coverageLine(session)}</span>
                <span className="row__meta" data-testid="fleet-inventory">{inventoryText(session)}</span>
                {session.linkConflict ? (
                  <span className="row__meta warning" data-testid="fleet-link-conflict">
                    {LINK_CONFLICT_TEXT}
                  </span>
                ) : null}
                <span className="row__meta">{surfaceLine(session)}</span>
                <ReturnResult result={route ?? session.lastRoute} failure={failure} heading={route || failure ? "Return" : "Last recorded return"} />
              </button>
              <button
                type="button"
                className="button row__action"
                onClick={() => void client.returnTo(session.sessionId)}
                disabled={!gate.enabled}
                title={gate.reason ?? undefined}
                aria-label={`Return to ${session.displayName}${gate.reason ? ` (unavailable: ${gate.reason})` : ""}`}
                data-testid="fleet-return"
              >
                {state.routing === session.sessionId ? "Returning…" : "Return"}
              </button>
            </li>
          );
        })}
        {state.sessions.length === 0 ? <li className="empty">No sessions in the projection.</li> : null}
      </ul>
    </section>
  );
}
