// Qualification-only entry point for renderer lifecycle commands (G13/G15).
// The mounted scene registers its lifecycle here in qualification builds
// only; native qualification intents call `runRendererQualificationCommand`.
// The lifecycle itself refuses every command outside qualification builds.

import type { RendererLifecycle, RendererLifecycleSnapshot, RendererQualificationCommand } from "@threadspace/scene";

import { launch } from "../launch";
import { type GpuLike, type InitBarrierState, createInitBarrier } from "./initBarrier";

let active: RendererLifecycle | null = null;

/**
 * Installed on `GPU.prototype` when this module is first evaluated, before the
 * scene mounts and creates its first renderer; a release build never wraps
 * the adapter request.
 */
const initBarrier = createInitBarrier((globalThis as unknown as { GPU?: { prototype: GpuLike } }).GPU?.prototype, launch.qualificationBuild);

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

type Reported = RendererLifecycleSnapshot & { surface: SurfaceFacts; initBarrier: InitBarrierState; atMs: number };

/**
 * Runs `inject-device-loss`, `enter-2d`, `exit-2d` or `report-state` against
 * the mounted renderer and resolves with the settled diagnostics and the
 * surface facts. `args.label` (1-48 of [a-z0-9-]) tags the resulting
 * `renderer-lifecycle` report.
 *
 * `arm-init-barrier` / `release-init-barrier` drive the init barrier and
 * `peek-state` reads the diagnostics without waiting for the lifecycle to
 * settle, so a held init can be observed while it is pending.
 */
export async function runRendererQualificationCommand(
  name: RendererQualificationCommand | "arm-init-barrier" | "release-init-barrier" | "peek-state" | (string & {}),
  args: Readonly<Record<string, unknown>> = {},
): Promise<Reported> {
  if (active === null) throw new Error("no renderer lifecycle is mounted");
  const lifecycle = active;
  const peek = (): Reported => ({ ...lifecycle.getSnapshot(), surface: surfaceFacts(), initBarrier: initBarrier.state(), atMs: Date.now() });
  switch (name) {
    case "arm-init-barrier":
      initBarrier.arm();
      return peek();
    case "release-init-barrier":
      initBarrier.release();
      return peek();
    case "peek-state":
      return peek();
    default: {
      await lifecycle.runQualificationCommand(name, args);
      return peek();
    }
  }
}
