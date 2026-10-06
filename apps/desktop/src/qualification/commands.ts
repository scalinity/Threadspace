// Qualification builds only: commands the native harness sends to the live
// office view as `QualificationCommand` intents (via the companion's
// qualification role). Each runs inside the real packaged WebView against the
// real Tauri 3 command path and returns a result the client records natively
// as a `qualification-command` report. Release views ignore these intents.

import type { BridgeClient } from "../bridge/client";
import { BridgeError, ipc, normalizeFailure } from "../bridge/ipc";
import { runRendererQualificationCommand } from "./rendererQualification";

interface Outcome {
  ok: boolean;
  code: string;
  value: unknown;
}

interface CaseResult {
  name: string;
  expectation: string;
  outcome: string;
  pass: boolean;
}

async function outcomeOf(run: () => Promise<unknown>): Promise<Outcome> {
  try {
    return { ok: true, code: "OK", value: await run() };
  } catch (error) {
    const failure = error instanceof BridgeError ? error.failure : normalizeFailure(error);
    return { ok: false, code: failure.code, value: failure.detail };
  }
}

function percentile(sorted: number[], fraction: number): number {
  if (sorted.length === 0) return 0;
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(fraction * sorted.length) - 1))] ?? 0;
}

const receiptStatus = (o: Outcome) => (o.value as { receipt?: { status?: string } } | null)?.receipt?.status;

/** G03: ≥`rounds` typed round trips plus the negative and boundary cases. */
async function ipcSuite(client: BridgeClient, rounds: number): Promise<unknown> {
  const results: CaseResult[] = [];
  const check = async (name: string, expectation: string, run: () => Promise<unknown>, pass: (o: Outcome) => boolean) => {
    const outcome = await outcomeOf(run);
    results.push({ name, expectation, outcome: `${outcome.code}${outcome.ok ? "" : ` (${String(outcome.value).slice(0, 160)})`}`, pass: pass(outcome) });
    return outcome;
  };
  const context = client.context();
  const started = performance.now();

  // Volume: real round trips through the three paths a view uses.
  const latencies: number[] = [];
  const failures: Record<string, number> = {};
  const kinds = ["ConnectionStatus", "WindowState", "FleetPage"] as const;
  for (let index = 0; index < rounds; index += 1) {
    const kind = kinds[index % kinds.length] ?? "ConnectionStatus";
    const t0 = performance.now();
    const outcome = await outcomeOf(() =>
      kind === "ConnectionStatus"
        ? ipc.query({ query: { kind }, context: null })
        : kind === "WindowState"
          ? ipc.query({ query: { kind }, context: null })
          : ipc.query({ query: { kind, after: null, limit: 5 }, context }),
    );
    latencies.push(performance.now() - t0);
    const typed = outcome.ok && (outcome.value as { kind?: string }).kind === kind;
    if (!typed) failures[`${kind}:${outcome.code}`] = (failures[`${kind}:${outcome.code}`] ?? 0) + 1;
  }
  latencies.sort((a, b) => a - b);

  await check("typed success", "ConnectionStatus", () => ipc.query({ query: { kind: "ConnectionStatus" }, context: null }), (o) =>
    o.ok && (o.value as { kind?: string }).kind === "ConnectionStatus");
  await check("typed success with context", "Diagnostics", () => ipc.query({ query: { kind: "Diagnostics" }, context }), (o) =>
    o.ok && (o.value as { kind?: string }).kind === "Diagnostics");
  await check("typed error (query)", "SessionDetail NOT_IMPLEMENTED_FOR_MILESTONE", () => ipc.query({ query: { kind: "SessionDetail" }, context }), (o) =>
    !o.ok && o.code === "NOT_IMPLEMENTED_FOR_MILESTONE");
  await check("typed error (action)", "SnoozeAttention NOT_IMPLEMENTED_FOR_MILESTONE", () =>
    ipc.action({ action: { kind: "SnoozeAttention" }, expectedRevision: null, requestId: crypto.randomUUID(), context }), (o) =>
    !o.ok && o.code === "NOT_IMPLEMENTED_FOR_MILESTONE");
  await check("malformed: extra field", "INVALID_REQUEST", () =>
    ipc.raw("ui_query", { request: { query: { kind: "Diagnostics", sql: "select 1" }, context } }), (o) => !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: unknown action", "INVALID_REQUEST", () =>
    ipc.raw("ui_action", { request: { action: { kind: "RunShell", command: "ls" }, expectedRevision: null, requestId: crypto.randomUUID(), context } }), (o) =>
    !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: non-UUID requestId", "INVALID_REQUEST", () =>
    ipc.raw("ui_action", { request: { action: { kind: "AcknowledgeAttention", attentionId: "x" }, expectedRevision: null, requestId: "nope", context } }), (o) =>
    !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: wrong field type", "INVALID_REQUEST", () =>
    ipc.raw("ui_query", { request: { query: { kind: "FleetPage", after: null, limit: "5" }, context } }), (o) => !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: page limit 0", "INVALID_REQUEST", () =>
    ipc.query({ query: { kind: "FleetPage", after: null, limit: 0 }, context }), (o) => !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: Mark handled without reason", "INVALID_REQUEST", () =>
    ipc.action({ action: { kind: "ResolveAttention", attentionId: crypto.randomUUID(), reason: "  " }, expectedRevision: null, requestId: crypto.randomUUID(), context }), (o) =>
    !o.ok && o.code === "INVALID_REQUEST");
  await check("unsupported protocol", "UNSUPPORTED_PROTOCOL", () =>
    ipc.raw("ui_connect", { request: { protocolVersion: 99, viewEpoch: crypto.randomUUID() }, events: { toJSON: () => "__CHANNEL__:0" } }), (o) =>
    !o.ok && (o.code === "UNSUPPORTED_PROTOCOL" || o.code === "IPC_REFUSED"));
  await check("reused view epoch", "STALE_CONTEXT: a used epoch never reconnects", () =>
    ipc.raw("ui_connect", { request: { protocolVersion: 1, viewEpoch: context.viewEpoch }, events: { toJSON: () => "__CHANNEL__:0" } }), (o) =>
    !o.ok && (o.code === "STALE_CONTEXT" || o.code === "IPC_REFUSED"));
  await check("stale context: view epoch", "STALE_CONTEXT", () =>
    ipc.query({ query: { kind: "Diagnostics" }, context: { ...context, viewEpoch: crypto.randomUUID() } }), (o) => !o.ok && o.code === "STALE_CONTEXT");
  await check("stale context: core generation", "STALE_CONTEXT", () =>
    ipc.query({ query: { kind: "Diagnostics" }, context: { ...context, coreGeneration: crypto.randomUUID() } }), (o) => !o.ok && o.code === "STALE_CONTEXT");
  await check("stale context: unknown subscription", "UNKNOWN_SUBSCRIPTION", () =>
    ipc.query({ query: { kind: "Diagnostics" }, context: { ...context, subscriptionId: crypto.randomUUID() } }), (o) => !o.ok && o.code === "UNKNOWN_SUBSCRIPTION");
  await check("future ACK", "ACK_REJECTED", () =>
    ipc.ack({ subscriptionId: context.subscriptionId, viewEpoch: context.viewEpoch, highestAppliedStreamSeq: 1_000_000, appliedJournalCursor: "1" }), (o) =>
    !o.ok && o.code === "ACK_REJECTED");
  await check("stale ACK", "ACK_REJECTED: sequence 1 was acknowledged long ago", () =>
    ipc.ack({ subscriptionId: context.subscriptionId, viewEpoch: context.viewEpoch, highestAppliedStreamSeq: 1, appliedJournalCursor: "1" }), (o) =>
    !o.ok && o.code === "ACK_REJECTED");
  await check("old-epoch ACK", "STALE_CONTEXT or UNKNOWN_SUBSCRIPTION", () =>
    ipc.ack({ subscriptionId: context.subscriptionId, viewEpoch: crypto.randomUUID(), highestAppliedStreamSeq: 1, appliedJournalCursor: "1" }), (o) =>
    !o.ok && (o.code === "STALE_CONTEXT" || o.code === "UNKNOWN_SUBSCRIPTION"));
  await check("disallowed core command", "plugin:window|close refused by the capability", () => ipc.raw("plugin:window|close", {}), (o) =>
    !o.ok && o.code === "IPC_REFUSED");
  await check("unknown command", "ui_shell refused", () => ipc.raw("ui_shell", { request: {} }), (o) => !o.ok && o.code === "IPC_REFUSED");

  // Concurrency bound: four data queries in flight per view (SPEC §18.3).
  const burst = await Promise.all(
    Array.from({ length: 12 }, () => outcomeOf(() => ipc.query({ query: { kind: "FleetPage", after: null, limit: 200 }, context }))),
  );
  results.push({
    name: "query concurrency bound",
    expectation: "some of 12 concurrent pages refused TOO_MANY_IN_FLIGHT, the rest typed",
    outcome: burst.map((o) => o.code).join(","),
    pass: burst.every((o) => o.ok || o.code === "TOO_MANY_IN_FLIGHT") && burst.some((o) => o.ok),
  });

  // Cancellation: an action whose subscription was retired is refused before
  // any effect, and the target is unchanged.
  const throwawayEpoch = crypto.randomUUID();
  const { Channel } = await import("@tauri-apps/api/core");
  const throwawayChannel = new Channel<unknown>();
  // A well-behaved subscriber acknowledges what it applied; retiring a
  // stream with unacknowledged large frames would (correctly) make the
  // native side recreate this whole view (SPEC §18.5).
  let lastHeader: { streamSeq: number; cursor: string } | null = null;
  throwawayChannel.onmessage = (raw) => {
    const header = (raw as { header?: { streamSeq?: unknown; cursor?: unknown } }).header;
    if (typeof header?.streamSeq === "number" && typeof header.cursor === "string") lastHeader = { streamSeq: header.streamSeq, cursor: header.cursor };
  };
  const throwaway = await outcomeOf(() => ipc.connect({ protocolVersion: 1, viewEpoch: throwawayEpoch }, throwawayChannel));
  const victim = client.getSnapshot().attention.find((item) => item.resolvedAtMs === null);
  if (throwaway.ok && victim) {
    const reply = throwaway.value as { subscriptionId: string; viewEpoch: string; coreGeneration: string; storeGeneration: string };
    const retiredContext = { subscriptionId: reply.subscriptionId, viewEpoch: reply.viewEpoch, coreGeneration: reply.coreGeneration, storeGeneration: reply.storeGeneration };
    await new Promise((resolve) => setTimeout(resolve, 300));
    const applied = lastHeader as { streamSeq: number; cursor: string } | null;
    if (applied) {
      await ipc.ack({ subscriptionId: reply.subscriptionId, viewEpoch: reply.viewEpoch, highestAppliedStreamSeq: applied.streamSeq, appliedJournalCursor: applied.cursor }).catch(() => {});
    }
    await ipc.disconnect({ subscriptionId: reply.subscriptionId, viewEpoch: reply.viewEpoch });
    await check("cancellation: action after retirement", "UNKNOWN_SUBSCRIPTION and nothing committed", () =>
      ipc.action({ action: { kind: "ResolveAttention", attentionId: victim.attentionId, reason: "qualification cancellation probe" }, expectedRevision: null, requestId: crypto.randomUUID(), context: retiredContext }), (o) =>
      !o.ok && o.code === "UNKNOWN_SUBSCRIPTION");
    await check("cancellation: page after retirement", "UNKNOWN_SUBSCRIPTION", () =>
      ipc.query({ query: { kind: "FleetPage", after: null, limit: 5 }, context: retiredContext }), (o) => !o.ok && o.code === "UNKNOWN_SUBSCRIPTION");
    const stillOpen = client.getSnapshot().attention.find((item) => item.attentionId === victim.attentionId);
    results.push({ name: "cancellation: target unchanged", expectation: "item still unresolved", outcome: String(stillOpen?.resolvedAtMs ?? "unresolved"), pass: stillOpen !== undefined && stillOpen.resolvedAtMs === null });
  } else {
    results.push({ name: "cancellation", expectation: "throwaway subscription and an open item", outcome: throwaway.code, pass: false });
  }

  // Durable owner command: commit, idempotent retry, conflicting reuse.
  const target = client.getSnapshot().attention.find((item) => item.resolvedAtMs === null);
  if (target) {
    const requestId = crypto.randomUUID();
    const acknowledge = () => client.action({ kind: "AcknowledgeAttention", attentionId: target.attentionId }, null, requestId);
    await check("owner command", "AcknowledgeAttention COMMITTED (or ALREADY_COMMITTED on a rerun)", acknowledge, (o) => o.ok && receiptStatus(o) === "COMMITTED");
    await check("idempotent retry", "same requestId returns ALREADY_COMMITTED", acknowledge, (o) => o.ok && receiptStatus(o) === "ALREADY_COMMITTED");
    await check("conflicting reused request ID", "CONFLICT: same requestId, different payload", () =>
      client.action({ kind: "ResolveAttention", attentionId: target.attentionId, reason: "conflict probe" }, null, requestId), (o) =>
      !o.ok && o.code === "CONFLICT");
  } else {
    results.push({ name: "owner command", expectation: "an unresolved attention item", outcome: "none open", pass: false });
  }

  const passed = results.filter((result) => result.pass).length;
  return {
    rounds,
    roundTripFailures: failures,
    roundTripsOk: Object.keys(failures).length === 0,
    latencyMs: { p50: percentile(latencies, 0.5), p95: percentile(latencies, 0.95), max: latencies[latencies.length - 1] ?? 0 },
    passed,
    total: results.length,
    elapsedMs: Math.round(performance.now() - started),
    results,
  };
}

async function sha256(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/**
 * The applied projection in the canonical form the harness also computes
 * from a companion snapshot: rows sorted by id, fixed key order.
 */
export async function projectionDigest(client: BridgeClient): Promise<unknown> {
  const state = client.getSnapshot();
  const sessions = [...state.sessions]
    .sort((a, b) => (a.sessionId < b.sessionId ? -1 : a.sessionId > b.sessionId ? 1 : 0))
    .map((row) => ({ sessionId: row.sessionId, revision: row.revision, displayName: row.displayName }));
  const attention = [...state.attention]
    .filter((row) => row.resolvedAtMs === null)
    .sort((a, b) => (a.attentionId < b.attentionId ? -1 : a.attentionId > b.attentionId ? 1 : 0))
    .map((row) => ({ attentionId: row.attentionId, revision: row.revision, acknowledgedAtMs: row.acknowledgedAtMs }));
  const canonical = JSON.stringify({ sessions, attention, counts: { needsAttention: state.counts.needsAttention, awaitingAction: state.counts.awaitingAction } });
  return {
    digest: await sha256(canonical),
    cursor: state.cursor,
    viewRevision: state.viewRevision,
    sessions: sessions.length,
    attention: attention.length,
    counts: state.counts,
    paging: state.paging,
    stream: state.stream,
    connection: state.connection,
    phase: state.phase,
    faults: { ...client.faults },
    notices: state.notices,
    visibilityState: document.visibilityState,
    watchdogTicks: client.watchdogTicks,
    stallDecisions: client.stallDecisions,
    resumes: state.resumes,
  };
}

export type ExtraHandler = (command: string, args: unknown) => Promise<unknown> | undefined;

/** Installs the handler; `extra` lets other qualification modules add commands. */
export function installQualificationCommands(client: BridgeClient, extra: ExtraHandler[] = []): void {
  client.setQualificationHandler(async (command, args) => {
    const input = (args ?? {}) as Record<string, unknown>;
    switch (command) {
      case "ipc-suite":
        return ipcSuite(client, typeof input.rounds === "number" ? input.rounds : 1000);
      case "projection-digest":
        return projectionDigest(client);
      case "drop-frames":
        client.faults.dropFrames = input.mode === "all" ? "all" : input.mode === "next" ? "next" : "none";
        return { ...client.faults };
      case "ack-mode":
        client.faults.withholdAcks = input.withhold === true;
        client.faults.ackDelayMs = typeof input.delayMs === "number" ? input.delayMs : 0;
        client.faults.stallNext = input.stallNext === true;
        return { ...client.faults };
      case "reconnect":
        // A renderer-initiated resubscription, as after a detected gap.
        client.qualificationReconnect(typeof input.reason === "string" ? input.reason : "qualification reconnect");
        return { reconnecting: true };
      case "reload":
        setTimeout(() => location.reload(), 50);
        return { reloading: true };
      case "request-then-reload":
        // A slow native request is still in flight when the document is replaced.
        void client.action({ kind: "RefreshEvidence" }).catch(() => {});
        setTimeout(() => location.reload(), 20);
        return { reloading: true };
      default:
        if (command.startsWith("renderer:")) {
          return runRendererQualificationCommand(command.slice("renderer:".length), input);
        }
        for (const handler of extra) {
          const result = handler(command, args);
          if (result !== undefined) return result;
        }
        throw new Error(`unknown qualification command ${command}`);
    }
  });
}
