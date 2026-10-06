import { useCallback } from "react";

import {
  RendererLifecycle,
  createThreeRendererPlatform,
  documentVisibilityConfirmation,
  type SceneModel,
  type WorkerVisualState,
} from "@threadspace/scene";

import type { BridgeClient, ViewState } from "../bridge/client";
import { launch } from "../launch";
import { registerRendererLifecycle } from "../qualification/rendererQualification";

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

const lifecycles = new WeakMap<BridgeClient, RendererLifecycle>();

/**
 * The one renderer lifecycle for a bridge client. Construction has no side
 * effects; the lifecycle starts work only when SceneView attaches it.
 * `confirmVisibility` is the single injection point for a native-confirmed
 * hidden/minimized signal.
 */
export function rendererLifecycleFor(client: BridgeClient): RendererLifecycle {
  let lifecycle = lifecycles.get(client);
  if (!lifecycle) {
    lifecycle = new RendererLifecycle({
      platform: createThreeRendererPlatform({
        forceWebGL: launch.rendererMode === "webgl2-compatibility",
        canvasLabel: "Office scene: one desk and worker per session",
        onSelect: (sessionId) => client.select(sessionId),
      }),
      confirmVisibility: documentVisibilityConfirmation,
      qualification: launch.qualificationBuild,
      frameTarget: launch.qualificationBuild ? window : null,
      matchMedia: (query) => window.matchMedia(query),
      onAttested: (attestation) => client.setRenderer(attestation),
      onFallback: (reason) => client.setRendererError(reason),
      report: (report) => client.recordQualificationReport("renderer-lifecycle", report),
    });
    lifecycles.set(client, lifecycle);
  }
  return lifecycle;
}

/**
 * Mounts the renderer lifecycle on a host element with a callback ref. The
 * lifecycle creates a fresh canvas per renderer generation inside the host;
 * projection changes reach it through the bridge store without React
 * re-rendering the scene, and visibility changes are confirmed before acting.
 */
export function SceneView({ client, lifecycle, hidden }: { client: BridgeClient; lifecycle: RendererLifecycle; hidden: boolean }) {
  const mount = useCallback(
    (host: HTMLDivElement | null) => {
      if (!host) return;
      const unregister = launch.qualificationBuild ? registerRendererLifecycle(lifecycle) : null;
      lifecycle.setModel(sceneModel(client.getSnapshot()));
      lifecycle.attach(host);
      const unsubscribe = client.subscribe(() => lifecycle.setModel(sceneModel(client.getSnapshot())));
      const onVisibility = () => lifecycle.setHostVisibility(document.visibilityState === "visible" ? "visible" : "hidden");
      document.addEventListener("visibilitychange", onVisibility);
      return () => {
        document.removeEventListener("visibilitychange", onVisibility);
        unsubscribe();
        lifecycle.detach();
        unregister?.();
      };
    },
    [client, lifecycle],
  );
  return <div ref={mount} className="scene-host" hidden={hidden} />;
}
