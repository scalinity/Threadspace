import type { BridgeClient, ViewState } from "../bridge/client";
import type { RouteResult } from "../contracts/generated/RouteResult";
import { RouteAxes } from "./RouteAxes";

function Field({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div className="field">
      <dt>{label}</dt>
      <dd className="mono">{value ?? "—"}</dd>
    </div>
  );
}

function sampleText(sample: RouteResult["evidence"]["preLookupSample"]): string | null {
  if (!sample) return null;
  if (sample.outcome === "Failed") return `${sample.code}`;
  const s = sample.sample;
  return `pid ${s.pid} · born ${s.startSeconds}.${String(s.startMicroseconds).padStart(6, "0")} · tdev ${s.controllingDevice ?? "none"} · pgid ${s.pgid}/tpgid ${s.tpgid}`;
}

/** The reconstructible proof chain of the last Return (MILESTONES M0B). */
function RouteProof({ result }: { result: RouteResult }) {
  const e = result.evidence;
  const lookup = e.lookup;
  const match = e.terminal?.matches[0];
  return (
    <dl className="fields">
      <Field label="Request" value={result.requestId} />
      <Field label="Native session" value={e.nativeSessionId} />
      <Field label="Binding" value={e.bindingId ? `${e.bindingId.slice(0, 8)} · rev ${e.bindingRevisionLoaded} → ${e.bindingRevisionBeforeFocus ?? "—"} → ${e.bindingRevisionAfterFocus ?? "—"}` : null} />
      <Field label="ProcessKey" value={e.processKey ? `pid ${e.processKey.pid} · born ${e.processKey.startSeconds}.${String(e.processKey.startMicroseconds).padStart(6, "0")}` : null} />
      <Field label="Before lookup" value={sampleText(e.preLookupSample)} />
      <Field
        label="Provider lookup"
        value={lookup ? (lookup.error ?? `${lookup.pidRows.length} row(s) for pid · ${lookup.requestEndedMs - lookup.requestStartedMs} ms`) : null}
      />
      <Field label="After lookup" value={sampleText(e.postLookupSample)} />
      <Field label="Terminal tab" value={match ? `${match.tty} · rdev ${match.rdev} · window ${match.windowId} tab ${match.tabIndex}` : e.terminal ? `${e.terminal.matches.length} matching tabs` : null} />
      <Field label="Readback" value={e.focus ? `${e.focus.outcome} · ${e.focus.readbackTty ?? "—"} · rdev ${e.focus.readbackRdev ?? "—"}` : null} />
      <Field label="Frontmost" value={e.focus?.frontmostApplication ? `${e.focus.frontmostApplication.bundleIdentifier ?? "?"} (${e.focus.frontmostApplication.pid})` : null} />
      <Field label="After focus" value={sampleText(e.postFocusSample)} />
      <Field label="Latency" value={`${result.latencyMs} ms · focus ${result.focusPerformed ? "performed" : "not performed"}`} />
    </dl>
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
  const route = state.routes[inspector.sessionId];
  return (
    <section className="panel" aria-labelledby="inspector-heading" data-inspector-source={inspector.source}>
      <h2 id="inspector-heading" className="panel__title">Inspector</h2>
      {inspector.source === "NOTIFICATION_RESPONSE" ? (
        <p className="callout" role="status">
          Opened from a native notification{inspector.outstandingAtOpen === false ? " — this item was already handled." : "."}
          {inspector.observationEnabled === false ? " Observation is stopped or under maintenance, so nothing was routed." : ""}
          {inspector.route ? ` Return: ${inspector.route.surfaceResult} · ${inspector.route.sessionVerification} · ${inspector.route.reasonCode}.` : ""}
        </p>
      ) : null}
      <dl className="fields">
        <Field label="Worker" value={session?.displayName} />
        <Field label="Session" value={inspector.sessionId} />
        <Field label="Native session" value={session?.nativeSessionId} />
        <Field label="Observation" value={session ? `${session.observation.toLowerCase()} · ${session.executionPresence.toLowerCase()}` : null} />
        <Field label="Activation" value={session?.activation} />
        <Field label="Process" value={session?.process ? `pid ${session.process.pid} · born ${session.process.startSeconds}.${String(session.process.startMicroseconds).padStart(6, "0")}` : null} />
        <Field label="Executable" value={session?.process?.executableIdentity} />
        <Field
          label="Binding"
          value={
            session?.binding
              ? `${session.binding.surfaceKind} ${session.binding.locator} · tdev ${session.binding.deviceNumber ?? "—"} · ${session.binding.proof} · rev ${session.binding.revision}`
              : session?.lastInvalidation
                ? `none — last invalidated: ${session.lastInvalidation}`
                : null
          }
        />
        <Field label="Reported" value={session?.providerStatus ? `${session.providerStatus}${session.providerWaitingFor ? ` (${session.providerWaitingFor})` : ""}` : null} />
        <Field label="Attention" value={inspector.attentionId} />
        <Field label="Item state" value={item ? (item.resolvedAtMs !== null ? "resolved" : item.acknowledgedAtMs !== null ? "acknowledged" : "needs attention") : inspector.attentionId ? "not in projection" : null} />
      </dl>
      {route ? (
        <>
          <h3 className="panel__subtitle">Last return</h3>
          <RouteAxes surface={route.surfaceResult} verification={route.sessionVerification} readiness={route.inputReadiness} reason={route.reasonCode} />
          {route.reasonCode === "MULTIPLE_ATTACHMENTS" ? (
            <div className="chooser" role="group" aria-label="Choose an attachment">
              <p className="row__meta">This session has several live attachments. Choose one; the newest is never picked for you.</p>
              {route.choices.map((choice) => (
                <button key={choice.bindingId} type="button" className="button button--quiet" onClick={() => void client.returnTo(inspector.sessionId, choice.bindingId)}>
                  pid {choice.pid} · {choice.tty}
                </button>
              ))}
            </div>
          ) : null}
          <RouteProof result={route} />
        </>
      ) : null}
      <div className="actions">
        {session && !session.fixture ? (
          <button type="button" className="button" onClick={() => void client.returnTo(session.sessionId)} disabled={state.phase !== "live" || state.routing !== null}>
            Return
          </button>
        ) : null}
        {item && item.acknowledgedAtMs === null ? (
          <button type="button" className="button" onClick={() => void client.acknowledge(item.attentionId)} disabled={state.phase !== "live"}>
            Acknowledge
          </button>
        ) : null}
        {item && item.resolvedAtMs === null ? (
          <button type="button" className="button button--quiet" onClick={() => void client.resolve(item.attentionId, "Marked handled in the inspector")} disabled={state.phase !== "live"}>
            Mark handled
          </button>
        ) : null}
        <button type="button" className="button button--quiet" onClick={() => client.select(null)}>
          Close
        </button>
      </div>
    </section>
  );
}
