// The five bridge commands. Rejections are normalized: a typed `UiError` from
// the native handler keeps its code; a framework refusal (for example the
// capability ACL) has no code and is reported as IPC_REFUSED.

import { Channel, invoke } from "@tauri-apps/api/core";

import type { UiAckReply } from "../contracts/generated/UiAckReply";
import type { UiAckRequest } from "../contracts/generated/UiAckRequest";
import type { UiActionRequest } from "../contracts/generated/UiActionRequest";
import type { UiActionResult } from "../contracts/generated/UiActionResult";
import type { UiConnectReply } from "../contracts/generated/UiConnectReply";
import type { UiConnectRequest } from "../contracts/generated/UiConnectRequest";
import type { UiDisconnectRequest } from "../contracts/generated/UiDisconnectRequest";
import type { UiErrorCode } from "../contracts/generated/UiErrorCode";
import type { UiQueryRequest } from "../contracts/generated/UiQueryRequest";
import type { UiQueryResult } from "../contracts/generated/UiQueryResult";

export interface BridgeFailure {
  code: UiErrorCode | "IPC_REFUSED";
  retryable: boolean;
  detail: string;
}

export function normalizeFailure(error: unknown): BridgeFailure {
  if (typeof error === "object" && error !== null && "code" in error && typeof error.code === "string") {
    const record = error as { code: UiErrorCode; retryable?: unknown; detail?: unknown };
    return {
      code: record.code,
      retryable: record.retryable === true,
      detail: typeof record.detail === "string" ? record.detail : "",
    };
  }
  return { code: "IPC_REFUSED", retryable: false, detail: String(error) };
}

export class BridgeError extends Error {
  constructor(readonly failure: BridgeFailure) {
    super(`${failure.code}: ${failure.detail}`);
  }
}

async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new BridgeError(normalizeFailure(error));
  }
}

export const ipc = {
  connect: (request: UiConnectRequest, events: Channel<unknown>) => call<UiConnectReply>("ui_connect", { request, events }),
  ack: (request: UiAckRequest) => call<UiAckReply>("ui_ack", { request }),
  disconnect: (request: UiDisconnectRequest) => call<null>("ui_disconnect", { request }),
  query: (request: UiQueryRequest) => call<UiQueryResult>("ui_query", { request }),
  action: (request: UiActionRequest) => call<UiActionResult>("ui_action", { request }),
  /** Raw access for qualification probes of refused/malformed calls. */
  raw: (command: string, args: Record<string, unknown>) => call<unknown>(command, args),
};
