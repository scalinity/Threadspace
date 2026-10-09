import type { BridgeClient, CommandOutcome, ViewState } from "../bridge/client";
import type { RouteResult } from "../contracts/generated/RouteResult";
import { canAcknowledge, canMarkHandled, categoryText, itemStateText, openCountText, openItemsFor } from "./attention";
import { LINK_CONFLICT_TEXT, inventoryText, observationText, observerText, presenceText, turnText } from "./coverage";
import { ReturnResult } from "./ReturnResult";
import { returnGate } from "./returnControl";
import { WORKER_STATE_TEXT, workerState } from "./worker";

const MARK_HANDLED_REASON = "Marked handled in the inspector";

function Field({ id, label, value, warning }: { id?: string; label: string; value: string | null | undefined; warning?: boolean }) {
  return (
    <div className="field" data-field={id}>
      <dt>{label}</dt>
      <dd className={warning ? "mono warning" : "mono"}>{value ?? "—"}</dd>
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

function resolutionText(outcome: CommandOutcome): string {
  return outcome.ok ? `Marked handled: ${outcome.receipt.status} at cursor ${outcome.receipt.cursor}` : `Mark handled failed: ${outcome.error}`;
}

export function Inspector({ state, client }: { state: ViewState; client: BridgeClient }) {
  const inspector = state.inspector;
  if (!inspector) {
    return (
      <section className="panel" aria-labelledby="inspector-heading" data-testid="inspector">
        <h2 id="inspector-heading" className="panel__title">Inspector</h2>
        <p className="empty">Select a worker or an attention item.</p>
      </section>
    );
  }
  const live = state.phase === "live";
  const session = state.sessions.find((entry) => entry.sessionId === inspector.sessionId);
  const item = inspector.attentionId ? state.attention.find((entry) => entry.attentionId === inspector.attentionId) : undefined;
  const route = state.routes[inspector.sessionId] ?? null;
  const failure = state.routeFailures[inspector.sessionId] ?? null;
  const gate = session ? returnGate(session, live, state.routing) : null;
  const resolution = item ? state.resolutions[item.attentionId] : undefined;
  return (
    <section className="panel" aria-labelledby="inspector-heading" data-inspector-source={inspector.source} data-testid="inspector" data-session-id={inspector.sessionId}>
      <h2 id="inspector-heading" className="panel__title">Inspector</h2>
      {inspector.source === "NOTIFICATION_RESPONSE" ? (
        <p className="callout" role="status">
          Opened from a native notification{inspector.outstandingAtOpen === false ? " — this item was already handled." : "."}
          {inspector.observationEnabled === false ? " Observation is stopped, unavailable or under maintenance, so nothing was routed." : ""}
          {inspector.route ? ` Return: ${inspector.route.surfaceResult} · ${inspector.route.sessionVerification} · ${inspector.route.reasonCode}.` : ""}
        </p>
      ) : null}
      <dl className="fields">
        <Field id="worker" label="Worker" value={session?.displayName} />
        <Field id="workerState" label="State" value={session ? WORKER_STATE_TEXT[workerState(session, state.attention)] : null} />
        <Field id="session" label="Session" value={inspector.sessionId} />
        <Field id="nativeSession" label="Native session" value={session?.nativeSessionId} />
        <Field id="turn" label="Turn" value={session ? turnText(session.turnState) : null} />
        <Field id="presence" label="Presence" value={session ? presenceText(session.executionPresence) : null} />
        <Field id="observer" label="Observer" value={session ? (session.fixture ? "Fixture: not observed" : observerText(session.observerTier, session.observerVersion)) : null} />
        <Field id="observation" label="Observation" value={session ? observationText(session.observation) : null} />
        <Field
          id="link"
          label="Observer link"
          value={session ? (session.linkConflict ? LINK_CONFLICT_TEXT : "Reports agree") : null}
          warning={session?.linkConflict === true}
        />
        <Field id="inventory" label="Inventory" value={session ? inventoryText(session) : null} />
        <Field id="activation" label="Activation" value={session?.activation} />
        <Field id="process" label="Process" value={session?.process ? `pid ${session.process.pid} · born ${session.process.startSeconds}.${String(session.process.startMicroseconds).padStart(6, "0")}` : null} />
        <Field id="executable" label="Executable" value={session?.process?.executableIdentity} />
        <Field
          id="binding"
          label="Binding"
          value={
            session?.binding
              ? `${session.binding.surfaceKind} ${session.binding.locator} · tdev ${session.binding.deviceNumber ?? "—"} · ${session.binding.proof} · rev ${session.binding.revision}`
              : session?.lastInvalidation
                ? `none — last invalidated: ${session.lastInvalidation}`
                : null
          }
        />
        <Field id="openAttention" label="Open attention" value={openCountText(openItemsFor(state.attention, inspector.sessionId).length)} />
        <Field id="attention" label="Attention" value={inspector.attentionId} />
        <Field id="itemCategory" label="Item" value={item ? categoryText(item.category) : null} />
        <Field id="itemState" label="Item state" value={item ? itemStateText(item) : inspector.attentionId ? "not in projection" : null} />
      </dl>
      <ReturnResult result={route ?? session?.lastRoute ?? null} failure={failure} heading={route || failure ? "Return" : "Last recorded return"} />
      {route?.reasonCode === "MULTIPLE_ATTACHMENTS" ? (
        <div className="chooser" role="group" aria-label="Choose an attachment">
          <p className="row__meta">This session has several live attachments. Choose one; the newest is never picked for you.</p>
          {route.choices.map((choice) => (
            <button
              key={choice.bindingId}
              type="button"
              className="button button--quiet"
              onClick={() => void client.returnTo(inspector.sessionId, choice.bindingId)}
              disabled={!live || state.routing !== null}
            >
              pid {choice.pid} · {choice.tty}
            </button>
          ))}
        </div>
      ) : null}
      {route ? <RouteProof result={route} /> : null}
      <div className="actions">
        {session && gate ? (
          <button
            type="button"
            className="button"
            onClick={() => void client.returnTo(session.sessionId)}
            disabled={!gate.enabled}
            title={gate.reason ?? undefined}
            data-testid="inspector-return"
          >
            {state.routing === session.sessionId ? "Returning…" : "Return"}
          </button>
        ) : null}
        {item && canAcknowledge(item) ? (
          <button type="button" className="button" onClick={() => void client.acknowledge(item.attentionId)} disabled={!live} data-testid="inspector-acknowledge">
            Acknowledge
          </button>
        ) : null}
        {item ? (
          <button
            type="button"
            className="button button--quiet"
            onClick={() => void client.resolve(item.attentionId, MARK_HANDLED_REASON)}
            disabled={!live || !canMarkHandled(item) || state.resolving !== null}
            data-testid="inspector-mark-handled"
          >
            {state.resolving === item.attentionId ? "Marking handled…" : "Mark handled"}
          </button>
        ) : null}
        <button type="button" className="button button--quiet" onClick={() => client.select(null)}>
          Close
        </button>
      </div>
      {gate && !gate.enabled && gate.reason ? (
        <p className="row__meta" data-testid="inspector-return-gate">
          Return unavailable: {gate.reason}
        </p>
      ) : null}
      {resolution ? (
        <p className={resolution.ok ? "row__meta" : "row__meta warning"} role="status" data-testid="inspector-resolution">
          {resolutionText(resolution)}
        </p>
      ) : null}
    </section>
  );
}
