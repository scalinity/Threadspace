// Qualification only (`--qualify-ipc` on a qualification build): exercises the
// real Tauri command layer from the actual office WebView after hydration and
// records the outcomes natively. Each case states its expectation; the report
// carries pass/fail per case rather than a single verdict.

import type { BridgeClient } from "../bridge/client";
import { BridgeError, ipc, normalizeFailure } from "../bridge/ipc";

interface CaseResult {
  name: string;
  expectation: string;
  outcome: string;
  pass: boolean;
}

async function outcomeOf(run: () => Promise<unknown>): Promise<{ ok: boolean; code: string; value: unknown }> {
  try {
    const value = await run();
    return { ok: true, code: "OK", value };
  } catch (error) {
    const failure = error instanceof BridgeError ? error.failure : normalizeFailure(error);
    return { ok: false, code: failure.code, value: failure.detail };
  }
}

export async function runIpcSelfTest(client: BridgeClient): Promise<void> {
  const results: CaseResult[] = [];
  const check = async (name: string, expectation: string, run: () => Promise<unknown>, pass: (o: { ok: boolean; code: string; value: unknown }) => boolean) => {
    const outcome = await outcomeOf(run);
    results.push({ name, expectation, outcome: `${outcome.code}${outcome.ok ? "" : ` (${String(outcome.value).slice(0, 160)})`}`, pass: pass(outcome) });
  };
  const context = client.context();
  const started = performance.now();

  await check("typed success", "ConnectionStatus returns a typed result", () => ipc.query({ query: { kind: "ConnectionStatus" }, context: null }), (o) =>
    o.ok && typeof o.value === "object" && o.value !== null && (o.value as { kind?: string }).kind === "ConnectionStatus");
  await check("typed success with context", "Diagnostics returns a typed result", () => ipc.query({ query: { kind: "Diagnostics" }, context }), (o) =>
    o.ok && (o.value as { kind?: string }).kind === "Diagnostics");
  await check("typed error", "FleetPage is NOT_IMPLEMENTED_FOR_MILESTONE", () => ipc.query({ query: { kind: "FleetPage" }, context }), (o) =>
    !o.ok && o.code === "NOT_IMPLEMENTED_FOR_MILESTONE");
  await check("typed error (action)", "ReturnToSession is NOT_IMPLEMENTED_FOR_MILESTONE", () =>
    ipc.action({ action: { kind: "ReturnToSession" }, expectedRevision: null, requestId: crypto.randomUUID(), context }), (o) =>
    !o.ok && o.code === "NOT_IMPLEMENTED_FOR_MILESTONE");
  await check("malformed: extra field", "INVALID_REQUEST", () =>
    ipc.raw("ui_query", { request: { query: { kind: "Diagnostics", sql: "select 1" }, context } }), (o) => !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: unknown action", "INVALID_REQUEST", () =>
    ipc.raw("ui_action", { request: { action: { kind: "RunShell", command: "ls" }, expectedRevision: null, requestId: crypto.randomUUID(), context } }), (o) =>
    !o.ok && o.code === "INVALID_REQUEST");
  await check("malformed: non-UUID requestId", "INVALID_REQUEST", () =>
    ipc.raw("ui_action", { request: { action: { kind: "AcknowledgeAttention", attentionId: "x" }, expectedRevision: null, requestId: "nope", context } }), (o) =>
    !o.ok && o.code === "INVALID_REQUEST");
  await check("unsupported protocol", "UNSUPPORTED_PROTOCOL", () =>
    ipc.raw("ui_connect", { request: { protocolVersion: 99, viewEpoch: crypto.randomUUID() }, events: { toJSON: () => "__CHANNEL__:0" } }), (o) =>
    !o.ok && (o.code === "UNSUPPORTED_PROTOCOL" || o.code === "IPC_REFUSED"));
  await check("stale context", "STALE_CONTEXT for a wrong view epoch", () =>
    ipc.query({ query: { kind: "Diagnostics" }, context: { ...context, viewEpoch: crypto.randomUUID() } }), (o) => !o.ok && o.code === "STALE_CONTEXT");
  await check("future ACK", "ACK_REJECTED", () =>
    ipc.ack({ subscriptionId: context.subscriptionId, viewEpoch: context.viewEpoch, highestAppliedStreamSeq: 1_000_000, appliedJournalCursor: "1" }), (o) =>
    !o.ok && o.code === "ACK_REJECTED");
  await check("disallowed core command", "plugin:window|close refused by the capability", () => ipc.raw("plugin:window|close", {}), (o) =>
    !o.ok && o.code === "IPC_REFUSED");
  await check("unknown command", "ui_shell refused", () => ipc.raw("ui_shell", { request: {} }), (o) => !o.ok && o.code === "IPC_REFUSED");

  const attention = client.getSnapshot().attention[0];
  if (attention) {
    const requestId = crypto.randomUUID();
    const action = () => client.action({ kind: "AcknowledgeAttention", attentionId: attention.attentionId }, null, requestId);
    await check("owner command", "AcknowledgeAttention COMMITTED", action, (o) =>
      o.ok && (o.value as { receipt?: { status?: string } }).receipt?.status === "COMMITTED");
    await check("idempotent retry", "same requestId returns ALREADY_COMMITTED", action, (o) =>
      o.ok && (o.value as { receipt?: { status?: string } }).receipt?.status === "ALREADY_COMMITTED");
  }

  const passed = results.filter((result) => result.pass).length;
  client.recordQualificationReport("ipc-selftest", {
    passed,
    total: results.length,
    elapsedMs: Math.round(performance.now() - started),
    results,
  });
  client.notice(`IPC self-test: ${passed}/${results.length} cases as expected`);
}
