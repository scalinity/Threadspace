import type { BridgeClient, ViewState } from "../bridge/client";

const CATEGORY_LABEL: Record<string, string> = {
  TURN_COMPLETE: "Turn complete",
  INPUT_REQUIRED: "Input required",
  APPROVAL_REQUIRED: "Approval required",
  ERROR: "Error",
  BLOCKED: "Blocked",
  HANDOFF_READY: "Handoff ready",
  OWNER_DECISION_REQUIRED: "Decision required",
};

export function AttentionPanel({ state, client }: { state: ViewState; client: BridgeClient }) {
  const needs = state.attention.filter((item) => item.acknowledgedAtMs === null);
  const awaiting = state.attention.filter((item) => item.acknowledgedAtMs !== null);
  return (
    <section className="panel" aria-labelledby="attention-heading">
      <h2 id="attention-heading" className="panel__title">Attention</h2>
      <p className="panel__subtitle">
        {state.counts.needsAttention} need attention · {state.counts.awaitingAction} awaiting action
      </p>
      <ul className="list">
        {[...needs, ...awaiting].map((item) => (
          <li key={item.attentionId} className={`row ${state.inspector?.attentionId === item.attentionId ? "row--selected" : ""}`}>
            <button type="button" className="row__main" onClick={() => client.openAttention(item.attentionId)}>
              <span className="row__label">{CATEGORY_LABEL[item.category] ?? item.category}</span>
              <span className="row__meta">
                {item.acknowledgedAtMs === null ? "Needs attention" : "Acknowledged"} · notification {item.notificationState.toLowerCase().replaceAll("_", " ")}
              </span>
              {item.summary ? <span className="row__summary">{item.summary}</span> : null}
            </button>
            {item.acknowledgedAtMs === null ? (
              <button type="button" className="button button--quiet" onClick={() => void client.acknowledge(item.attentionId)} disabled={state.phase !== "live"}>
                Acknowledge
              </button>
            ) : null}
          </li>
        ))}
        {state.attention.length === 0 ? <li className="empty">No outstanding owner actions.</li> : null}
      </ul>
    </section>
  );
}
