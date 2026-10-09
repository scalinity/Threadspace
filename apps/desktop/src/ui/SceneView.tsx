import { useCallback } from "react";

import { RendererLifecycle, createThreeRendererPlatform, documentVisibilityConfirmation } from "@threadspace/scene";

import type { BridgeClient } from "../bridge/client";
import { ipc } from "../bridge/ipc";
import { launch } from "../launch";
import { registerRendererLifecycle } from "../qualification/rendererQualification";
import { sceneModel } from "./worker";

const lifecycles = new WeakMap<BridgeClient, RendererLifecycle>();

/**
 * Native confirmation (SPEC §15.6): the renderer acts only on the office
 * window's AppKit state, read through the bootstrap-scoped WindowState query.
 * If the native query is unavailable, document visibility is the fallback.
 */
async function nativeVisibility(): Promise<{ visible: boolean; minimized: boolean }> {
  try {
    const result = await ipc.query({ query: { kind: "WindowState" }, context: null });
    if (result.kind === "WindowState") {
      return { visible: result.visible && !result.minimized, minimized: result.minimized };
    }
  } catch {
    // fall through to the document's own signal
  }
  return documentVisibilityConfirmation();
}

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
      confirmVisibility: nativeVisibility,
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
