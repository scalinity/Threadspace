import { describe, expect, it } from "vitest";

import { attentionView, sessionView } from "./testViews";
import { WORKER_STATE_TEXT, sceneModel, workerAttention, workerState } from "./worker";

const A = "00000000-0000-4000-8000-00000000000a";
const B = "00000000-0000-4000-8000-00000000000b";

describe("one persistent worker per session", () => {
  it("keeps a worker for every session, with or without attention", () => {
    const sessions = [sessionView({ sessionId: A }), sessionView({ sessionId: B, turnState: "WORKING" })];
    const model = sceneModel({ sessions, attention: [], inspector: null });
    expect(model.workers.map((worker) => worker.id)).toEqual([A, B]);
  });

  it("keeps the worker present after its turn completes", () => {
    const working = sceneModel({ sessions: [sessionView({ sessionId: A, turnState: "WORKING" })], attention: [], inspector: null });
    const completed = sceneModel({ sessions: [sessionView({ sessionId: A, turnState: "COMPLETED" })], attention: [], inspector: null });
    expect(working.workers).toHaveLength(1);
    expect(working.workers[0]?.state).toBe("working");
    expect(completed.workers).toHaveLength(1);
    expect(completed.workers[0]?.id).toBe(A);
    expect(completed.workers[0]?.state).toBe("idle");
  });

  it("keeps a completed worker with its completion item as attention, and after resolution as idle", () => {
    const session = sessionView({ sessionId: A, turnState: "COMPLETED" });
    const open = attentionView({ sessionId: A, category: "TURN_COMPLETE" });
    expect(workerState(session, [open])).toBe("attention");
    expect(workerState(session, [{ ...open, acknowledgedAtMs: 5 }])).toBe("acknowledged");
    expect(workerState(session, [{ ...open, acknowledgedAtMs: 5, resolvedAtMs: 9 }])).toBe("idle");
    expect(workerState(session, [{ ...open, resolvedAtMs: 9 }])).toBe("idle");
  });

  it("marks the selected worker from the inspector", () => {
    const inspector = {
      attentionId: null,
      sessionId: B,
      source: "SELECTION" as const,
      outstandingAtOpen: null,
      intentId: null,
      openedAtMs: 0,
      route: null,
      observationEnabled: null,
    };
    const model = sceneModel({ sessions: [sessionView({ sessionId: A }), sessionView({ sessionId: B })], attention: [], inspector });
    expect(model.selectedId).toBe(B);
  });
});

describe("worker state precedence", () => {
  it("shows an ended execution as ended history, even with open attention", () => {
    const session = sessionView({ sessionId: A, executionPresence: "ENDED", observation: "DISCONNECTED", turnState: "COMPLETED" });
    const open = attentionView({ sessionId: A });
    expect(workerState(session, [open])).toBe("ended");
    expect(workerAttention(session, [open])).toBe("needs");
  });

  it("shows uncertain observation as stale on a provider session, not on a fixture", () => {
    for (const observation of ["STALE", "DISCONNECTED", "UNKNOWN", "CONFLICT"] as const) {
      expect(workerState(sessionView({ observation, turnState: "WORKING" }), [])).toBe("stale");
      expect(workerState(sessionView({ observation, turnState: "WORKING", fixture: true }), [])).toBe("working");
    }
    expect(workerState(sessionView({ observation: "CURRENT" }), [])).toBe("idle");
  });

  it("shows working for a working or queued turn", () => {
    expect(workerState(sessionView({ turnState: "WORKING" }), [])).toBe("working");
    expect(workerState(sessionView({ turnState: "QUEUED" }), [])).toBe("working");
  });

  it("shows waiting for a waiting turn or an open owner wait, acknowledged or not", () => {
    expect(workerState(sessionView({ turnState: "WAITING" }), [])).toBe("waiting");
    for (const category of ["INPUT_REQUIRED", "APPROVAL_REQUIRED", "BLOCKED"] as const) {
      const item = attentionView({ category });
      expect(workerState(sessionView({ turnState: "WORKING" }), [item])).toBe("waiting");
      expect(workerState(sessionView(), [{ ...item, acknowledgedAtMs: 3 }])).toBe("waiting");
      expect(workerState(sessionView(), [{ ...item, resolvedAtMs: 4 }])).toBe("idle");
    }
  });

  it("lets new work dominate the state while older attention stays on the worker", () => {
    const session = sessionView({ turnState: "WORKING" });
    const earlier = attentionView({ category: "TURN_COMPLETE" });
    expect(workerState(session, [earlier])).toBe("working");
    expect(workerAttention(session, [earlier])).toBe("needs");
    expect(workerAttention(session, [{ ...earlier, acknowledgedAtMs: 2 }])).toBe("awaiting");
    expect(workerAttention(session, [{ ...earlier, resolvedAtMs: 2 }])).toBe("none");
  });

  it("ignores attention that belongs to another session", () => {
    expect(workerState(sessionView({ sessionId: A }), [attentionView({ sessionId: B })])).toBe("idle");
  });

  it("has text for every state", () => {
    expect(Object.keys(WORKER_STATE_TEXT).sort()).toEqual(["acknowledged", "attention", "ended", "idle", "stale", "waiting", "working"]);
  });
});
