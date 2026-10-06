import { useCallback } from "react";

import { OfficeScene, type SceneModel, type WorkerVisualState } from "@threadspace/scene";

import type { BridgeClient, ViewState } from "../bridge/client";
import { launch } from "../launch";

function workerState(state: ViewState, sessionId: string): WorkerVisualState {
  const open = state.attention.filter((item) => item.sessionId === sessionId && item.resolvedAtMs === null);
  if (open.some((item) => item.acknowledgedAtMs === null)) return "attention";
  return open.length > 0 ? "acknowledged" : "idle";
}

export function sceneModel(state: ViewState): SceneModel {
  return {
    workers: state.sessions.map((session) => ({
      id: session.sessionId,
      label: session.displayName,
      state: workerState(state, session.sessionId),
    })),
    selectedId: state.inspector?.sessionId ?? null,
  };
}

/**
 * Mounts the WebGPU scene with a callback ref. The scene subscribes to the
 * bridge store directly, so projection changes reach it without React
 * re-rendering the canvas.
 */
export function SceneView({ client }: { client: BridgeClient }) {
  const mount = useCallback(
    (canvas: HTMLCanvasElement | null) => {
      if (!canvas) return;
      let disposed = false;
      let scene: OfficeScene | null = null;
      let unsubscribe: (() => void) | null = null;
      OfficeScene.create(canvas, {
        forceWebGL: launch.rendererMode === "webgl2-compatibility",
        reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches,
        onSelect: (sessionId) => client.select(sessionId),
      })
        .then((created) => {
          if (disposed) {
            void created.dispose();
            return;
          }
          scene = created;
          client.setRenderer(created.attestation);
          created.update(sceneModel(client.getSnapshot()));
          unsubscribe = client.subscribe(() => created.update(sceneModel(client.getSnapshot())));
        })
        .catch((error: unknown) => client.setRendererError(error instanceof Error ? error.message : String(error)));
      return () => {
        disposed = true;
        unsubscribe?.();
        void scene?.dispose();
      };
    },
    [client],
  );
  return <canvas ref={mount} className="scene-canvas" aria-label="Office scene: one desk and worker per session" />;
}
