// Qualification only: runs inside the `acl-probe` WebView, which no
// capability names. Every bridge command and a core command must be refused
// by the framework before reaching native handlers. The probe cannot use IPC
// to report, so it writes its result into the document title, which the
// native side observes.

import { invoke } from "@tauri-apps/api/core";

async function attempt(command: string, args: Record<string, unknown>): Promise<{ command: string; refused: boolean; detail: string }> {
  try {
    await invoke(command, args);
    return { command, refused: false, detail: "accepted" };
  } catch (error) {
    const detail = typeof error === "string" ? error : JSON.stringify(error);
    // A typed UiError means the native handler ran: that is not an ACL refusal.
    const typed = typeof error === "object" && error !== null && "code" in error;
    return { command, refused: !typed, detail: detail.slice(0, 200) };
  }
}

export async function runAclProbe(): Promise<void> {
  const context = { subscriptionId: crypto.randomUUID(), viewEpoch: crypto.randomUUID(), coreGeneration: "x", storeGeneration: "x" };
  const results = [
    await attempt("ui_query", { request: { query: { kind: "ConnectionStatus" }, context: null } }),
    await attempt("ui_connect", { request: { protocolVersion: 1, viewEpoch: crypto.randomUUID() }, events: { toJSON: () => "__CHANNEL__:0" } }),
    await attempt("ui_ack", { request: { subscriptionId: context.subscriptionId, viewEpoch: context.viewEpoch, highestAppliedStreamSeq: 1, appliedJournalCursor: "1" } }),
    await attempt("ui_disconnect", { request: { subscriptionId: context.subscriptionId, viewEpoch: context.viewEpoch } }),
    await attempt("ui_action", { request: { action: { kind: "RequestNotificationAuthorization" }, expectedRevision: null, requestId: crypto.randomUUID(), context } }),
    await attempt("plugin:window|start_dragging", {}),
  ];
  const report = { label: "acl-probe", allRefused: results.every((result) => result.refused), results };
  document.title = `ACL-PROBE:${JSON.stringify(report)}`;
}
