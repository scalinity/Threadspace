// Version-specific renderer diagnostics for three 0.186.1 (r186,
// 9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8). SPEC §15.1: a positive WebGPU
// attestation requires the initialized `renderer.backend.isWebGPUBackend ===
// true`. `isWebGPURenderer` names the renderer class only and does not rule
// out its automatic WebGL2 fallback. Any other shape is UNVERIFIED.

import { REVISION, type WebGPURenderer } from "three/webgpu";

export const PINNED_THREE_REVISION = "186";

export type RendererBackend = "WEBGPU" | "WEBGL2_COMPATIBILITY" | "UNVERIFIED";

export interface AdapterSummary {
  vendor: string;
  architecture: string;
  device: string;
  description: string;
}

export interface RendererAttestation {
  rendererClass: string;
  backend: RendererBackend;
  /** The G14 primary-backend criterion: only an attested WebGPU backend passes. */
  webgpuGate: "PASS" | "FAIL";
  forcedCompatibility: boolean;
  threeRevision: string;
  revisionMatchesPin: boolean;
  navigatorGpu: boolean;
  secureContext: boolean;
  origin: string;
  userAgent: string;
  adapter: AdapterSummary | null;
  initDurationMs: number;
}

interface BackendShape {
  isWebGPUBackend?: unknown;
  isWebGLBackend?: unknown;
  device?: { adapterInfo?: Partial<Record<keyof AdapterSummary, unknown>> } | null;
}

function backendOf(renderer: WebGPURenderer): BackendShape | undefined {
  return (renderer as unknown as { backend?: BackendShape }).backend;
}

/** Reads the initialized backend identity. Isolated so the pin's internals stay in one place. */
export function inspectPinnedRendererBackend(renderer: WebGPURenderer): RendererBackend {
  const backend = backendOf(renderer);
  if (backend?.isWebGPUBackend === true) return "WEBGPU";
  if (backend?.isWebGLBackend === true) return "WEBGL2_COMPATIBILITY";
  return "UNVERIFIED";
}

function adapterSummary(renderer: WebGPURenderer): AdapterSummary | null {
  const info = backendOf(renderer)?.device?.adapterInfo;
  if (!info) return null;
  const text = (value: unknown) => (typeof value === "string" ? value : "");
  return {
    vendor: text(info.vendor),
    architecture: text(info.architecture),
    device: text(info.device),
    description: text(info.description),
  };
}

export function attest(renderer: WebGPURenderer, forcedCompatibility: boolean, initDurationMs: number): RendererAttestation {
  const backend = inspectPinnedRendererBackend(renderer);
  const rendererClass = (renderer as unknown as { isWebGPURenderer?: unknown }).isWebGPURenderer === true
    ? "WebGPURenderer"
    : "unknown";
  return {
    rendererClass,
    backend,
    webgpuGate: backend === "WEBGPU" && !forcedCompatibility ? "PASS" : "FAIL",
    forcedCompatibility,
    threeRevision: REVISION,
    revisionMatchesPin: REVISION === PINNED_THREE_REVISION,
    navigatorGpu: typeof navigator !== "undefined" && "gpu" in navigator && navigator.gpu != null,
    secureContext: typeof window !== "undefined" && window.isSecureContext,
    origin: typeof location !== "undefined" ? location.origin : "",
    userAgent: typeof navigator !== "undefined" ? navigator.userAgent : "",
    adapter: backend === "WEBGPU" ? adapterSummary(renderer) : null,
    initDurationMs,
  };
}
