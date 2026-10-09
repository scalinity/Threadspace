import type { BridgeClient, ViewState } from "../bridge/client";
import { awaitingAction, categoryText, needsAttention } from "./attention";

/** Outstanding items only: a resolved item stays in the projection but leaves this list. */
export function AttentionPanel({ state, client }: { state: ViewState; client: BridgeClient }) {
  const needs = needsAttention(state.attention);
  const awaiting = awaitingAction(state.attention);
  return (
    <section className="panel" aria-labelledby="attention-heading">
      <h2 id="attention-heading" className="panel__title">Attention</h2>
      <p className="panel__subtitle">
        {state.counts.needsAttention} need attention · {state.counts.awaitingAction} awaiting action
      </p>
      <ul className="list">
        {[...needs, ...awaiting].map((item) => (
          <li
            key={item.attentionId}
            className={`row ${state.inspector?.attentionId === item.attentionId ? "row--selected" : ""}`}
            data-testid="attention-row"
            data-attention-id={item.attentionId}
          >
            <button type="button" className="row__main" onClick={() => client.openAttention(item.attentionId)} data-testid="attention-open">
              <span className="row__label">{categoryText(item.category)}</span>
              <span className="row__meta">
                {item.acknowledgedAtMs === null ? "Needs attention" : "Acknowledged"} · notification {item.notificationState.toLowerCase().replaceAll("_", " ")}
              </span>
              {item.summary ? <span className="row__summary">{item.summary}</span> : null}
            </button>
            {item.acknowledgedAtMs === null ? (
              <button
                type="button"
                className="button button--quiet"
                onClick={() => void client.acknowledge(item.attentionId)}
                disabled={state.phase !== "live"}
                data-testid="attention-acknowledge"
              >
                Acknowledge
              </button>
            ) : null}
          </li>
        ))}
        {needs.length + awaiting.length === 0 ? <li className="empty">No outstanding owner actions.</li> : null}
      </ul>
    </section>
  );
}
