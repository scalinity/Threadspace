// Qualification builds only (G15 overlap witness, SPEC §15.6): a one-shot
// gate on `navigator.gpu.requestAdapter`, the first await inside the pinned
// Three WebGPU backend's `init()`. Once armed, the next adapter request is
// issued natively at once, but its result is withheld until the harness
// releases it (or `MAX_HOLD_MS` passes), so that renderer generation's
// `init()` is genuinely pending while the window is hidden. Release builds
// never install it: `arm` refuses and `requestAdapter` stays untouched.

export interface GpuLike {
  requestAdapter(options?: unknown): Promise<unknown>;
}

export interface InitBarrierRelease {
  heldAtMs: number;
  releasedAtMs: number;
  releasedBy: "COMMAND" | "TIMEOUT";
  /** Whether the native adapter request had already settled when its result was released. */
  adapterSettledWhileHeld: boolean;
}

export interface InitBarrierState {
  installed: boolean;
  armed: boolean;
  held: { heldAtMs: number } | null;
  releases: InitBarrierRelease[];
}

export interface InitBarrier {
  arm(): InitBarrierState;
  release(): InitBarrierState;
  state(): InitBarrierState;
}

const MAX_HOLD_MS = 60_000;
const MAX_RELEASES = 8;

export function createInitBarrier(gpu: GpuLike | undefined, enabled: boolean, now: () => number = Date.now): InitBarrier {
  let armed = false;
  let held: { heldAtMs: number } | null = null;
  let finish: ((by: InitBarrierRelease["releasedBy"]) => void) | null = null;
  const releases: InitBarrierRelease[] = [];
  const installed = enabled && gpu !== undefined;
  const state = (): InitBarrierState => ({ installed, armed, held: held === null ? null : { ...held }, releases: releases.map((entry) => ({ ...entry })) });

  if (installed) {
    const original = gpu.requestAdapter.bind(gpu);
    gpu.requestAdapter = (options?: unknown) => {
      const request = original(options);
      if (!armed) return request;
      armed = false;
      const heldAtMs = now();
      held = { heldAtMs };
      let settled = false;
      request.then(
        () => (settled = true),
        () => (settled = true),
      );
      return new Promise((resolve) => {
        const timer = setTimeout(() => finish?.("TIMEOUT"), MAX_HOLD_MS);
        finish = (releasedBy) => {
          clearTimeout(timer);
          finish = null;
          held = null;
          releases.push({ heldAtMs, releasedAtMs: now(), releasedBy, adapterSettledWhileHeld: settled });
          if (releases.length > MAX_RELEASES) releases.shift();
          resolve(request);
        };
      });
    };
  }

  return {
    arm() {
      if (!installed) throw new Error("the renderer init barrier exists only in qualification builds with WebGPU");
      if (held !== null) throw new Error("a renderer init is already held");
      armed = true;
      return state();
    },
    release() {
      if (finish === null) throw new Error("no renderer init is held");
      finish("COMMAND");
      return state();
    },
    state,
  };
}
