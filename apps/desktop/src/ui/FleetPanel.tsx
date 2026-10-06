import type { BridgeClient, ViewState } from "../bridge/client";

export function FleetPanel({ state, client }: { state: ViewState; client: BridgeClient }) {
  return (
    <section className="panel" aria-labelledby="fleet-heading">
      <h2 id="fleet-heading" className="panel__title">Fleet</h2>
      <ul className="list">
        {state.sessions.map((session) => (
          <li key={session.sessionId} className={`row ${state.inspector?.sessionId === session.sessionId ? "row--selected" : ""}`}>
            <button type="button" className="row__main" onClick={() => client.select(session.sessionId)}>
              <span className="row__label">
                {session.displayName}
                {session.fixture ? <span className="tag">fixture</span> : null}
              </span>
              <span className="row__meta">
                {session.provider} · turn {session.turnState.toLowerCase()} · {session.executionPresence.toLowerCase()} · observation{" "}
                {session.observation.toLowerCase()}
              </span>
            </button>
          </li>
        ))}
        {state.sessions.length === 0 ? <li className="empty">No sessions in the projection.</li> : null}
      </ul>
    </section>
  );
}
