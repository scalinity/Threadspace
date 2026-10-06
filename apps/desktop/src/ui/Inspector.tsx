import type { BridgeClient, ViewState } from "../bridge/client";

function Field({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div className="field">
      <dt>{label}</dt>
      <dd className="mono">{value ?? "—"}</dd>
    </div>
  );
}

export function Inspector({ state, client }: { state: ViewState; client: BridgeClient }) {
  const inspector = state.inspector;
  if (!inspector) {
    return (
      <section className="panel" aria-labelledby="inspector-heading">
        <h2 id="inspector-heading" className="panel__title">Inspector</h2>
        <p className="empty">Select a worker or an attention item.</p>
      </section>
    );
  }
  const session = state.sessions.find((entry) => entry.sessionId === inspector.sessionId);
  const item = inspector.attentionId ? state.attention.find((entry) => entry.attentionId === inspector.attentionId) : undefined;
  return (
    <section className="panel" aria-labelledby="inspector-heading" data-inspector-source={inspector.source}>
      <h2 id="inspector-heading" className="panel__title">Inspector</h2>
      {inspector.source === "NOTIFICATION_RESPONSE" ? (
        <p className="callout" role="status">
          Opened from a native notification{inspector.outstandingAtOpen === false ? " — this item was already handled." : "."}
        </p>
      ) : null}
      <dl className="fields">
        <Field label="Worker" value={session?.displayName} />
        <Field label="Session" value={inspector.sessionId} />
        <Field label="Native session" value={session?.nativeSessionId} />
        <Field label="Activation" value={session?.activation} />
        <Field label="Process" value={session?.process ? `pid ${session.process.pid} · born ${session.process.startSeconds}.${String(session.process.startMicroseconds).padStart(6, "0")}` : null} />
        <Field label="Binding" value={session?.binding ? `${session.binding.surfaceKind} · ${session.binding.proof} · rev ${session.binding.revision}` : null} />
        <Field label="Attention" value={inspector.attentionId} />
        <Field label="Item state" value={item ? (item.resolvedAtMs !== null ? "resolved" : item.acknowledgedAtMs !== null ? "acknowledged" : "needs attention") : inspector.attentionId ? "not in projection" : null} />
        <Field label="Intent" value={inspector.intentId} />
      </dl>
      {item && item.acknowledgedAtMs === null ? (
        <button type="button" className="button" onClick={() => void client.acknowledge(item.attentionId)} disabled={state.phase !== "live"}>
          Acknowledge
        </button>
      ) : null}
      <button type="button" className="button button--quiet" onClick={() => client.select(null)}>
        Close
      </button>
    </section>
  );
}
