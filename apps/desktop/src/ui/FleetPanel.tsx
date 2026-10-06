import type { BridgeClient, ViewState } from "../bridge/client";
import type { SessionView } from "../contracts/generated/SessionView";
import { RouteAxes } from "./RouteAxes";

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
          const route = state.routes[session.sessionId];
          const last = session.lastRoute;
          return (
            <li key={session.sessionId} className={`row row--worker ${state.inspector?.sessionId === session.sessionId ? "row--selected" : ""}`}>
              <button type="button" className="row__main" onClick={() => client.select(session.sessionId)}>
                <span className="row__label">
                  {session.displayName}
                  {session.fixture ? <span className="tag">fixture</span> : <span className="tag">{session.provider}</span>}
                  <span className="mono row__id">{session.nativeSessionId.slice(0, 8)}</span>
                </span>
                <span className="row__meta">
                  observation {session.observation.toLowerCase()} · {session.executionPresence.toLowerCase()}
                  {session.process ? ` · pid ${session.process.pid}` : ""}
                  {session.providerStatus ? ` · reported ${session.providerStatus}` : ""}
                  {session.providerWaitingFor ? ` (${session.providerWaitingFor})` : ""}
                </span>
                <span className="row__meta">{surfaceLine(session)}</span>
                {route ? (
                  <RouteAxes
                    surface={route.surfaceResult}
                    verification={route.sessionVerification}
                    readiness={route.inputReadiness}
                    reason={route.reasonCode}
                  />
                ) : last ? (
                  <RouteAxes surface={last.surfaceResult} verification={last.sessionVerification} readiness={last.inputReadiness} reason={last.reasonCode} />
                ) : null}
              </button>
              <button
                type="button"
                className="button row__action"
                onClick={() => void client.returnTo(session.sessionId)}
                disabled={!live || state.routing !== null}
                aria-label={`Return to ${session.displayName}`}
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
