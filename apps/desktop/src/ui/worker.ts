// One persistent worker per Session (MILESTONES M2). The worker's visual
// state is a projection of its SessionView and open attention, following the
// precedence of SPEC §6.3; it is never stored. A completed turn leaves the
// worker in place, and an ended execution keeps it as history.

import type { SceneModel, WorkerAttention, WorkerVisualState } from "@threadspace/scene";

import type { ViewState } from "../bridge/client";
import type { AttentionView } from "../contracts/generated/AttentionView";
import type { SessionView } from "../contracts/generated/SessionView";
import { openItemsFor } from "./attention";

/** Observation states that cannot vouch for current activity. */
const UNCERTAIN_OBSERVATION = new Set<SessionView["observation"]>(["STALE", "DISCONNECTED", "CONFLICT", "UNKNOWN"]);

/** Attention categories that mean the provider is waiting on the owner. */
const WAITING_CATEGORIES = new Set<AttentionView["category"]>(["INPUT_REQUIRED", "APPROVAL_REQUIRED", "BLOCKED"]);

export const WORKER_STATE_TEXT: Record<WorkerVisualState, string> = {
  working: "Working",
  waiting: "Waiting for you",
  attention: "Needs attention",
  acknowledged: "Acknowledged, awaiting action",
  idle: "Idle",
  ended: "Ended (history)",
  stale: "Coverage uncertain",
};

/**
 * Precedence: an ended execution is history; uncertain observation stops a
 * live worker from looking confidently current; then owner waits, current
 * work, unacknowledged and acknowledged attention, and finally idle.
 */
export function workerState(session: SessionView, attention: readonly AttentionView[]): WorkerVisualState {
  if (session.executionPresence === "ENDED") return "ended";
  if (!session.fixture && UNCERTAIN_OBSERVATION.has(session.observation)) return "stale";
  const open = openItemsFor(attention, session.sessionId);
  if (session.turnState === "WAITING" || open.some((item) => WAITING_CATEGORIES.has(item.category))) return "waiting";
  if (session.turnState === "WORKING" || session.turnState === "QUEUED") return "working";
  if (open.some((item) => item.acknowledgedAtMs === null)) return "attention";
  if (open.length > 0) return "acknowledged";
  return "idle";
}

/** Open attention independent of the worker state: new work never erases older attention (SPEC §15.3). */
export function workerAttention(session: SessionView, attention: readonly AttentionView[]): WorkerAttention {
  const open = openItemsFor(attention, session.sessionId);
  if (open.some((item) => item.acknowledgedAtMs === null)) return "needs";
  return open.length > 0 ? "awaiting" : "none";
}

/** Every session keeps its worker, whatever its attention or turn. */
export function sceneModel(state: Pick<ViewState, "sessions" | "attention" | "inspector">): SceneModel {
  return {
    workers: state.sessions.map((session) => ({
      id: session.sessionId,
      label: session.displayName,
      state: workerState(session, state.attention),
      attention: workerAttention(session, state.attention),
    })),
    selectedId: state.inspector?.sessionId ?? null,
  };
}
