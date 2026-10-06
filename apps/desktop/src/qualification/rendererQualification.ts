// Qualification-only entry point for renderer lifecycle commands (G13/G15).
// The mounted scene registers its lifecycle here in qualification builds
// only; native qualification intents call `runRendererQualificationCommand`.
// The lifecycle itself refuses every command outside qualification builds.

import type { RendererLifecycle, RendererLifecycleSnapshot, RendererQualificationCommand } from "@threadspace/scene";

let active: RendererLifecycle | null = null;

/** Registers the mounted lifecycle; the returned function unregisters it. */
export function registerRendererLifecycle(lifecycle: RendererLifecycle): () => void {
  active = lifecycle;
  return () => {
    if (active === lifecycle) active = null;
  };
}

/**
 * The live canvas's drawing buffer against its CSS size (the pixel ratio
 * actually applied), and where it sits in the viewport, so the harness can
 * measure the scene's own pixels in a window capture.
 */
export interface SurfaceFacts {
  devicePixelRatio: number;
  viewport: { width: number; height: number };
  canvas: { width: number; height: number; clientWidth: number; clientHeight: number; left: number; top: number } | null;
}

function surfaceFacts(): SurfaceFacts {
  const canvas = document.querySelector<HTMLCanvasElement>("canvas.scene-canvas");
  const rect = canvas?.getBoundingClientRect();
  return {
    devicePixelRatio: window.devicePixelRatio,
    viewport: { width: window.innerWidth, height: window.innerHeight },
    canvas:
      canvas === null || rect === undefined
        ? null
        : { width: canvas.width, height: canvas.height, clientWidth: canvas.clientWidth, clientHeight: canvas.clientHeight, left: rect.left, top: rect.top },
  };
}

/**
 * Runs `inject-device-loss`, `enter-2d`, `exit-2d` or `report-state` against
 * the mounted renderer and resolves with the settled diagnostics and the
 * surface facts. `args.label` (1-48 of [a-z0-9-]) tags the resulting
 * `renderer-lifecycle` report.
 */
export async function runRendererQualificationCommand(
  name: RendererQualificationCommand | (string & {}),
  args: Readonly<Record<string, unknown>> = {},
): Promise<RendererLifecycleSnapshot & { surface: SurfaceFacts }> {
  if (active === null) throw new Error("no renderer lifecycle is mounted");
  const snapshot = await active.runQualificationCommand(name, args);
  return { ...snapshot, surface: surfaceFacts() };
}
