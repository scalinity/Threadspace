// Qualification builds only: M2 commands that drive the real fleet,
// inspector and attention controls through the DOM (`click()` on the rendered
// buttons) and read back what is rendered, so the native harness exercises
// the controls the owner presses rather than calling the client directly.
// Registered through `installQualificationCommands`' extra handlers, which a
// release view never installs.

import type { BridgeClient, ViewState } from "../bridge/client";
import type { ExtraHandler } from "./commands";
import { LatencyStore, type LatencyContext } from "./latencyStore";

/** Longer than any Return's two-second budget plus the companion round trip. */
const RESULT_TIMEOUT_MS = 10_000;
/** How long the resolving projection patch may trail the command receipt. */
const PATCH_TIMEOUT_MS = 2_000;

/** Lets React commit the latest store update before the DOM is read. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function waitFor(client: BridgeClient, predicate: (state: ViewState) => boolean, timeoutMs: number): Promise<boolean> {
  return new Promise((resolve) => {
    if (predicate(client.getSnapshot())) {
      resolve(true);
      return;
    }
    let unsubscribe = () => {};
    const timer = setTimeout(() => {
      unsubscribe();
      resolve(false);
    }, timeoutMs);
    unsubscribe = client.subscribe(() => {
      if (!predicate(client.getSnapshot())) return;
      clearTimeout(timer);
      unsubscribe();
      resolve(true);
    });
  });
}

function requireString(value: unknown, name: string): string {
  if (typeof value !== "string" || value.length === 0) throw new Error(`${name} is required`);
  return value;
}

function byTestId(root: ParentNode, id: string): HTMLElement | null {
  return root.querySelector<HTMLElement>(`[data-testid="${id}"]`);
}

function textOf(root: ParentNode, id: string): string | null {
  return byTestId(root, id)?.textContent?.trim() ?? null;
}

function control(root: ParentNode, id: string): { present: boolean; enabled: boolean; label: string | null; title: string | null } {
  const button = byTestId(root, id) as HTMLButtonElement | null;
  return { present: button !== null, enabled: button !== null && !button.disabled, label: button?.textContent?.trim() ?? null, title: button?.title || null };
}

function fleetRow(sessionId: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-testid="fleet-row"][data-session-id="${CSS.escape(sessionId)}"]`);
}

function attentionRow(attentionId: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-testid="attention-row"][data-attention-id="${CSS.escape(attentionId)}"]`);
}

function readReturnResult(root: ParentNode) {
  const group = byTestId(root, "return-result");
  if (!group) return null;
  return {
    heading: group.querySelector(".return-result__heading")?.textContent?.trim() ?? null,
    lines: [...group.querySelectorAll<HTMLElement>("[data-line]")].map((line) => ({
      line: line.dataset.line ?? null,
      value: line.dataset.value ?? null,
      text: line.textContent?.trim() ?? "",
    })),
  };
}

function readFleetRow(row: HTMLElement) {
  const count = byTestId(row, "fleet-attention-count");
  const returnControl = control(row, "fleet-return");
  return {
    sessionId: row.dataset.sessionId ?? null,
    nativeId: textOf(row, "fleet-native-id"),
    workerState: byTestId(row, "fleet-worker-state")?.dataset.state ?? null,
    workerStateText: textOf(row, "fleet-worker-state"),
    coverage: textOf(row, "fleet-coverage"),
    inventory: textOf(row, "fleet-inventory"),
    linkConflict: textOf(row, "fleet-link-conflict"),
    turn: textOf(row, "fleet-turn"),
    presence: textOf(row, "fleet-presence"),
    attentionCount: count?.dataset.count === undefined ? null : Number(count.dataset.count),
    attentionText: count?.textContent?.trim() ?? null,
    returnEnabled: returnControl.enabled,
    returnUnavailableReason: returnControl.title,
    returnResult: readReturnResult(row),
    selected: row.classList.contains("row--selected"),
  };
}

function readInspector() {
  const panel = byTestId(document, "inspector");
  if (!panel) return null;
  const fields: Record<string, string | null> = {};
  for (const field of panel.querySelectorAll<HTMLElement>("[data-field]")) {
    if (field.dataset.field) fields[field.dataset.field] = field.querySelector("dd")?.textContent?.trim() ?? null;
  }
  return {
    sessionId: panel.dataset.sessionId ?? null,
    fields,
    controls: {
      return: control(panel, "inspector-return"),
      acknowledge: control(panel, "inspector-acknowledge"),
      markHandled: control(panel, "inspector-mark-handled"),
    },
    returnUnavailable: textOf(panel, "inspector-return-gate"),
    resolution: textOf(panel, "inspector-resolution"),
    returnResult: readReturnResult(panel),
  };
}

/**
 * The rendered fleet. With `sessionId`, only that session's rows: a report
 * is bounded at 64 KiB, which a whole long-lived fleet exceeds.
 */
async function fleet(client: BridgeClient, sessionId: string | null) {
  await settle();
  const state = client.getSnapshot();
  const all = [...document.querySelectorAll<HTMLElement>('[data-testid="fleet-row"]')];
  const rows = all.filter((row) => sessionId === null || row.dataset.sessionId === sessionId).map(readFleetRow);
  return { phase: state.phase, cursor: state.cursor, sessionsInView: state.sessions.length, rowsInView: all.length, rows };
}

async function select(client: BridgeClient, sessionId: string) {
  await settle();
  const row = fleetRow(sessionId);
  if (!row) throw new Error(`no fleet row for session ${sessionId}`);
  byTestId(row, "fleet-select")?.click();
  await settle();
  return { sessionId, selected: client.getSnapshot().inspector?.sessionId === sessionId, routing: client.getSnapshot().routing, inspector: readInspector() };
}

async function pressReturn(client: BridgeClient, sessionId: string) {
  await settle();
  const row = fleetRow(sessionId);
  if (!row) throw new Error(`no fleet row for session ${sessionId}`);
  const button = byTestId(row, "fleet-return") as HTMLButtonElement | null;
  if (!button) throw new Error(`no Return button for session ${sessionId}`);
  if (button.disabled) {
    return { sessionId, clicked: false, returnEnabled: false, reason: button.title || null, row: readFleetRow(row) };
  }
  const startedAt = performance.now();
  button.click();
  const started = client.getSnapshot().routing === sessionId;
  const finished = started ? await waitFor(client, (state) => state.routing === null, RESULT_TIMEOUT_MS) : false;
  await settle();
  const state = client.getSnapshot();
  const failure = state.routeFailures[sessionId] ?? null;
  const after = fleetRow(sessionId);
  return {
    sessionId,
    clicked: true,
    started,
    finished,
    elapsedMs: Math.round(performance.now() - startedAt),
    lines: after ? readReturnResult(after) : null,
    // A failed request leaves the previous route in place; it is not this press's result.
    route: failure === null ? (state.routes[sessionId] ?? null) : null,
    failure,
  };
}

async function pressMarkHandled(client: BridgeClient, attentionId: string) {
  await settle();
  const listed = attentionRow(attentionId);
  // A resolved item is no longer listed; it can still be opened from the projection.
  if (listed) byTestId(listed, "attention-open")?.click();
  else client.openAttention(attentionId);
  await settle();
  const selectedVia = listed ? "attention-list" : "projection";
  if (client.getSnapshot().inspector?.attentionId !== attentionId) {
    return { attentionId, selected: false, selectedVia, reason: "item not in the projection" };
  }
  const button = byTestId(document, "inspector-mark-handled") as HTMLButtonElement | null;
  if (!button || button.disabled) {
    return { attentionId, selected: true, selectedVia, clicked: false, markHandledEnabled: false, inspector: readInspector() };
  }
  const before = client.getSnapshot().resolutions[attentionId];
  const startedAt = performance.now();
  button.click();
  const started = client.getSnapshot().resolving === attentionId;
  const finished = started ? await waitFor(client, (state) => state.resolving === null, RESULT_TIMEOUT_MS) : false;
  const outcome = client.getSnapshot().resolutions[attentionId];
  // Only an outcome recorded by this press is its result.
  const fresh = outcome !== undefined && outcome !== before ? outcome : null;
  // The receipt can arrive before the patch that resolves the item in this view.
  const resolvedInView = fresh?.ok
    ? await waitFor(client, (state) => (state.attention.find((item) => item.attentionId === attentionId)?.resolvedAtMs ?? null) !== null, PATCH_TIMEOUT_MS)
    : false;
  await settle();
  return {
    attentionId,
    selected: true,
    selectedVia,
    clicked: true,
    started,
    finished,
    elapsedMs: Math.round(performance.now() - startedAt),
    receipt: fresh?.ok ? fresh.receipt : null,
    error: fresh && !fresh.ok ? fresh.error : null,
    resolvedInView,
    listedAfter: attentionRow(attentionId) !== null,
    inspector: readInspector(),
  };
}

function latencyContext(client: BridgeClient): LatencyContext {
  const state = client.getSnapshot();
  return { phase: state.phase, coreGeneration: state.connection?.coreGeneration ?? null,
    storeGeneration: state.connection?.storeGeneration ?? null, viewEpoch: state.connection?.viewEpoch ?? null };
}

/** MutationObserver runs after the actual render. A store notification alone
 * cannot mark visible completion. Runtime/epoch changes and overflow are
 * retained and make a qualification population incomplete. */
function latencyMarks(client: BridgeClient): LatencyStore {
  const marks = new LatencyStore(crypto.randomUUID(), () => performance.now());
  client.subscribe(() => marks.apply(client.getSnapshot().cursor, latencyContext(client)));
  const rendered = () => {
    const shown = document.querySelector('[data-testid="diagnostics-cursor"]')?.textContent?.trim();
    if (shown !== undefined) marks.rendered(shown, latencyContext(client), document.visibilityState === "visible");
  };
  const observer = new MutationObserver(rendered);
  observer.observe(document.documentElement, { subtree: true, childList: true, characterData: true });
  document.addEventListener("visibilitychange", rendered);
  return marks;
}

/** The M2 extra handler: `m2-fleet`, `m2-select`, `m2-press-return`, `m2-press-mark-handled`, `m2-latency`. */
export function m2Commands(client: BridgeClient): ExtraHandler {
  const marks = latencyMarks(client);
  return (command, args) => {
    const input = (args ?? {}) as Record<string, unknown>;
    switch (command) {
      case "m2-latency": {
        return Promise.resolve(marks.page(Number(input.offset ?? 0), Number(input.limit ?? 100)));
      }
      case "m2-latency-start":
        marks.start(latencyContext(client), client.getSnapshot().cursor, document.visibilityState === "visible");
        return Promise.resolve(marks.page());
      case "m2-latency-clock":
        return Promise.resolve(marks.clock());
      case "m2-latency-stop":
        return Promise.resolve(marks.stop());
      case "m2-fleet":
        return fleet(client, input.sessionId === undefined ? null : requireString(input.sessionId, "sessionId"));
      case "m2-select":
        return select(client, requireString(input.sessionId, "sessionId"));
      case "m2-press-return":
        return pressReturn(client, requireString(input.sessionId, "sessionId"));
      case "m2-press-mark-handled":
        return pressMarkHandled(client, requireString(input.attentionId, "attentionId"));
      default:
        return undefined;
    }
  };
}
