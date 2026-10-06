// The real renderer platform for `RendererLifecycle`: one fresh canvas and
// one fresh `WebGPURenderer` per generation. A fresh canvas matters on the
// WebGL2 compatibility path, where the pinned backend's `dispose()` calls
// `WEBGL_lose_context.loseContext()`; the same canvas would hand the next
// renderer an already-lost context.

import { RepeatWrapping, Texture, WebGPURenderer } from "three/webgpu";

import floorGrainUrl from "../assets/floor-grain.png?no-inline";
import { attest } from "./backend";
import { type FloorTexture, OfficeScene } from "./controller";
import type { RendererPlatform } from "./lifecycle";

export interface ThreePlatformOptions {
  forceWebGL: boolean;
  canvasLabel: string;
  onSelect: (workerId: string | null) => void;
}

export function createThreeRendererPlatform(options: ThreePlatformOptions): RendererPlatform<WebGPURenderer, FloorTexture> {
  return {
    createRenderer(host) {
      const canvas = document.createElement("canvas");
      canvas.className = "scene-canvas";
      canvas.setAttribute("aria-label", options.canvasLabel);
      host.append(canvas);
      const renderer = new WebGPURenderer({ canvas, antialias: true, forceWebGL: options.forceWebGL });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5));
      return renderer;
    },
    attest(renderer, initDurationMs) {
      return attest(renderer, options.forceWebGL, initDurationMs);
    },
    createScene(renderer, reducedMotion) {
      return new OfficeScene(renderer, { reducedMotion, onSelect: options.onSelect });
    },
    destroySurface(renderer) {
      renderer.domElement.remove();
    },
    async loadAsset(signal) {
      // A same-scheme request (tauri:// when packaged) that can still be in
      // flight at close or reload; the lifecycle aborts it on retirement and
      // disposes anything that completes afterwards.
      const response = await fetch(floorGrainUrl, { signal });
      if (!response.ok) throw new Error(`floor texture request failed: HTTP ${response.status}`);
      const bitmap = await createImageBitmap(await response.blob());
      const grain = new Texture(bitmap);
      grain.wrapS = RepeatWrapping;
      grain.wrapT = RepeatWrapping;
      grain.needsUpdate = true;
      return {
        texture: grain,
        dispose() {
          grain.dispose();
          bitmap.close();
        },
      };
    },
  };
}
