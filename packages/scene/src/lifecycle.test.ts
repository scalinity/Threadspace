import { describe, expect, it } from "vitest";

import type { RendererAttestation } from "./backend";
import type { SceneModel } from "./controller";
import { installFrameCounter, type FrameTarget } from "./frameCounter";
import {
  RendererLifecycle,
  type AssetCounts,
  type ConfirmedVisibility,
  type LifecycleReport,
  type LifecycleScene,
  type MediaQueryLike,
  type RendererLossInfo,
  type RendererPlatform,
} from "./lifecycle";

// ------------------------------------------------------------------ fakes

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
}

function deferred<T = void>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Lets every pending microtask and timer-free promise chain run. */
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

const host = {} as HTMLElement;

class FakeFrames implements FrameTarget {
  private next = 1;
  private readonly callbacks = new Map<number, FrameRequestCallback>();

  requestAnimationFrame(callback: FrameRequestCallback): number {
    const handle = this.next++;
    this.callbacks.set(handle, callback);
    return handle;
  }

  cancelAnimationFrame(handle: number): void {
    this.callbacks.delete(handle);
  }

  runFrame(): void {
    const due = [...this.callbacks.values()];
    this.callbacks.clear();
    for (const callback of due) callback(16);
  }
}

class FakeMedia implements MediaQueryLike {
  readonly listeners = new Set<(event: { matches: boolean }) => void>();
  constructor(public matches: boolean) {}
  addEventListener(_type: "change", listener: (event: { matches: boolean }) => void): void {
    this.listeners.add(listener);
  }
  removeEventListener(_type: "change", listener: (event: { matches: boolean }) => void): void {
    this.listeners.delete(listener);
  }
  fire(matches: boolean): void {
    this.matches = matches;
    for (const listener of this.listeners) listener({ matches });
  }
}

/** Counts every `dispose()` call, so a second disposal by the lifecycle is visible. */
interface FakeAsset {
  readonly generation: number;
  disposed: boolean;
  disposeCalls: number;
  dispose(): void;
}

function fakeAsset(generation: number): FakeAsset {
  return {
    generation,
    disposed: false,
    disposeCalls: 0,
    dispose() {
      this.disposed = true;
      this.disposeCalls += 1;
    },
  };
}

/** Mirrors the pinned renderer where it matters: an unbound default loss handler that relies on `this`, and an internal frame loop that starts in `init()` and stops only in `dispose()`. */
class FakeRenderer {
  initCalls = 0;
  disposeCalls = 0;
  disposed = false;
  isDeviceLost = false;
  readonly defaultLossCalls: RendererLossInfo[] = [];
  readonly defaultErrorCalls: unknown[] = [];
  readonly initGate = deferred();
  disposeGate: Deferred<void> | null = null;
  failInit: Error | null = null;
  appCallback: (() => void) | null = null;
  private loopHandle: number | null = null;
  onDeviceLost: (info: RendererLossInfo) => void;
  onError: (info: unknown) => void;

  constructor(
    private readonly env: FakeEnv,
    readonly id: number,
  ) {
    this.onDeviceLost = FakeRenderer.prototype.defaultOnDeviceLost;
    this.onError = FakeRenderer.prototype.defaultOnError;
  }

  defaultOnDeviceLost(this: FakeRenderer, info: RendererLossInfo): void {
    this.isDeviceLost = true;
    this.defaultLossCalls.push(info);
  }

  defaultOnError(this: FakeRenderer, info: unknown): void {
    this.defaultErrorCalls.push(info);
  }

  init(): Promise<this> {
    this.initCalls += 1;
    if (this.failInit) return Promise.reject(this.failInit);
    return this.initGate.promise.then(() => {
      const update = () => {
        this.loopHandle = this.env.frames.requestAnimationFrame(update);
        this.appCallback?.();
      };
      update();
      return this;
    });
  }

  async dispose(): Promise<void> {
    this.disposeCalls += 1;
    this.env.log.push(`dispose:${this.id}`);
    if (this.loopHandle !== null) this.env.frames.cancelAnimationFrame(this.loopHandle);
    this.loopHandle = null;
    if (this.disposeGate) await this.disposeGate.promise;
    this.disposed = true;
    this.env.log.push(`disposed:${this.id}`);
  }
}

class FakeScene implements LifecycleScene<FakeAsset> {
  readonly updates: SceneModel[] = [];
  readonly assets: FakeAsset[] = [];
  releaseGate: Deferred<void> | null = null;
  failApply: Error | null = null;
  released = false;

  constructor(
    readonly renderer: FakeRenderer,
    public reducedMotion: boolean,
  ) {
    renderer.appCallback = () => undefined;
  }

  update(model: SceneModel): void {
    this.updates.push(model);
  }

  setReducedMotion(reduced: boolean): void {
    this.reducedMotion = reduced;
  }

  applyAsset(asset: FakeAsset): void {
    if (this.failApply) throw this.failApply;
    this.assets.push(asset);
  }

  async release(): Promise<void> {
    this.renderer.appCallback = null;
    if (this.releaseGate) await this.releaseGate.promise;
    this.released = true;
  }
}

function attestation(initDurationMs: number): RendererAttestation {
  return {
    rendererClass: "WebGPURenderer",
    backend: "WEBGPU",
    webgpuGate: "PASS",
    forcedCompatibility: false,
    threeRevision: "186",
    revisionMatchesPin: true,
    navigatorGpu: true,
    secureContext: true,
    origin: "test://fake",
    userAgent: "vitest",
    adapter: null,
    initDurationMs,
  };
}

class FakeEnv {
  readonly frames = new FakeFrames();
  readonly media = new FakeMedia(false);
  readonly renderers: FakeRenderer[] = [];
  readonly scenes: FakeScene[] = [];
  readonly assetRequests: Array<{ gate: Deferred<FakeAsset>; signal: AbortSignal }> = [];
  readonly reports: LifecycleReport[] = [];
  readonly attested: RendererAttestation[] = [];
  readonly fallbacks: string[] = [];
  readonly log: string[] = [];
  surfacesDestroyed = 0;
  autoInit = true;
  failNextInit: Error | null = null;
  failNextScene: Error | null = null;
  /** Scenes created while this is set throw it from `applyAsset`. */
  failApply: Error | null = null;
  visibility: ConfirmedVisibility = { visible: true, minimized: false };
  confirm: () => Promise<ConfirmedVisibility> = () => Promise.resolve({ ...this.visibility });

  readonly platform: RendererPlatform<FakeRenderer, FakeAsset> = {
    createRenderer: () => {
      const renderer = new FakeRenderer(this, this.renderers.length + 1);
      if (this.autoInit) renderer.initGate.resolve();
      if (this.failNextInit) {
        renderer.failInit = this.failNextInit;
        this.failNextInit = null;
      }
      this.renderers.push(renderer);
      this.log.push(`create:${renderer.id}`);
      return renderer;
    },
    attest: (_renderer, initDurationMs) => attestation(initDurationMs),
    createScene: (renderer, reducedMotion) => {
      const failure = this.failNextScene;
      if (failure) {
        this.failNextScene = null;
        throw failure;
      }
      const scene = new FakeScene(renderer, reducedMotion);
      scene.failApply = this.failApply;
      this.scenes.push(scene);
      return scene;
    },
    destroySurface: () => {
      this.surfacesDestroyed += 1;
    },
    loadAsset: (signal) => {
      const gate = deferred<FakeAsset>();
      this.assetRequests.push({ gate, signal });
      return gate.promise;
    },
  };

  lifecycle(qualification = true): RendererLifecycle<FakeRenderer, FakeAsset> {
    return new RendererLifecycle<FakeRenderer, FakeAsset>({
      platform: this.platform,
      confirmVisibility: () => this.confirm(),
      qualification,
      frameTarget: this.frames,
      matchMedia: () => this.media,
      onAttested: (attested) => this.attested.push(attested),
      onFallback: (reason) => this.fallbacks.push(reason),
      report: (report) => this.reports.push(report),
    });
  }

  async live(qualification = true): Promise<RendererLifecycle<FakeRenderer, FakeAsset>> {
    const lifecycle = this.lifecycle(qualification);
    lifecycle.attach(host);
    await lifecycle.settled();
    expect(lifecycle.getSnapshot().state).toBe("live");
    return lifecycle;
  }

  renderer(index: number): FakeRenderer {
    const renderer = this.renderers[index];
    if (!renderer) throw new Error(`no renderer ${index}`);
    return renderer;
  }

  scene(index: number): FakeScene {
    const scene = this.scenes[index];
    if (!scene) throw new Error(`no scene ${index}`);
    return scene;
  }

  async show(lifecycle: RendererLifecycle<FakeRenderer, FakeAsset>, visible: boolean, minimized = false): Promise<void> {
    this.visibility = { visible, minimized };
    lifecycle.setHostVisibility(visible && !minimized ? "visible" : "hidden");
    await lifecycle.settled();
  }
}

const naturalLoss: RendererLossInfo = { api: "WebGPU", message: "GPU process reset", reason: "unknown", originalEvent: { secret: "raw" } };

function model(...ids: string[]): SceneModel {
  return { workers: ids.map((id) => ({ id, label: id, state: "attention" as const })), selectedId: null };
}

/** Terminal outcomes; after quiescence they must equal `started`. */
function outcomes(asset: AssetCounts): number {
  return asset.applied + asset.aborted + asset.discardedLate + asset.failed + asset.releasedBeforeScene;
}

/** A deterministic PRNG (mulberry32), so a failing interleaving replays from its seed. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ------------------------------------------------------------------ tests

describe("RendererLifecycle generations", () => {
  it("initializes after visibility is confirmed and reports the attestation", async () => {
    const env = new FakeEnv();
    const lifecycle = env.lifecycle();
    const first = model("a");
    lifecycle.setModel(first);
    lifecycle.attach(host);
    await lifecycle.settled();

    const snapshot = lifecycle.getSnapshot();
    expect(snapshot).toMatchObject({ state: "live", presentation: "3d", generation: 1, liveGeneration: 1 });
    expect(env.renderer(0).initCalls).toBe(1);
    expect(env.attested).toHaveLength(1);
    expect(snapshot.lastAttestation?.backend).toBe("WEBGPU");
    expect(env.scene(0).updates.at(-1)).toBe(first);
  });

  it("creates no renderer while the host is confirmed hidden or minimized", async () => {
    const env = new FakeEnv();
    env.visibility = { visible: true, minimized: true };
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    await lifecycle.settled();
    expect(env.renderers).toHaveLength(0);
    expect(lifecycle.getSnapshot().state).toBe("hidden-disposed");
  });

  it("discards a late init after a confirmed hide and disposes only once init has settled", async () => {
    const env = new FakeEnv();
    env.autoInit = false;
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    await tick();
    const first = env.renderer(0);
    expect(lifecycle.getSnapshot().state).toBe("initializing");
    expect(lifecycle.getSnapshot().initPending).toEqual({ generation: 1, retired: false });

    env.visibility = { visible: false, minimized: false };
    lifecycle.setHostVisibility("hidden");
    await tick();
    expect(first.disposeCalls).toBe(0);
    // Retired while its init is still pending: the witness the native G15 overlap asserts.
    expect(lifecycle.getSnapshot().initPending).toEqual({ generation: 1, retired: true });
    expect(env.reports.some((report) => report.event === "generation-retired" && report.detail?.initialized === false)).toBe(true);

    first.initGate.resolve();
    await lifecycle.settled();
    expect(env.scenes).toHaveLength(0);
    expect(env.attested).toHaveLength(0);
    expect(first.disposeCalls).toBe(1);
    const snapshot = lifecycle.getSnapshot();
    expect(snapshot.state).toBe("hidden-disposed");
    expect(snapshot.liveGeneration).toBeNull();
    expect(snapshot.initPending).toBeNull();
    expect(snapshot.counts.discardedLateInits).toBe(1);
  });

  it("hide then show during init retires the first generation and builds a fresh one after its disposal", async () => {
    const env = new FakeEnv();
    env.autoInit = false;
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    await tick();

    env.visibility = { visible: false, minimized: false };
    lifecycle.setHostVisibility("hidden");
    await tick();
    env.visibility = { visible: true, minimized: false };
    lifecycle.setHostVisibility("visible");
    await tick();
    expect(env.renderers).toHaveLength(1);

    env.autoInit = true;
    env.renderer(0).initGate.resolve();
    await lifecycle.settled();
    expect(env.renderers).toHaveLength(2);
    expect(env.log).toEqual(["create:1", "dispose:1", "disposed:1", "create:2"]);
    expect(env.scenes.map((scene) => scene.renderer.id)).toEqual([2]);
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "live", liveGeneration: 2 });
  });

  it("never reuses a renderer across repeated hide/show cycles", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    for (let cycle = 0; cycle < 5; cycle += 1) {
      await env.show(lifecycle, false);
      expect(lifecycle.getSnapshot().state).toBe("hidden-disposed");
      await env.show(lifecycle, true);
      expect(lifecycle.getSnapshot().state).toBe("live");
    }
    expect(env.renderers).toHaveLength(6);
    expect(env.renderers.every((renderer) => renderer.initCalls === 1)).toBe(true);
    expect(env.renderers.slice(0, 5).every((renderer) => renderer.disposeCalls === 1 && renderer.disposed)).toBe(true);
    expect(env.renderer(5).disposeCalls).toBe(0);
    expect(lifecycle.getSnapshot().counts).toMatchObject({ inits: 6, rebuilds: 5, disposals: 5, recoveryRebuilds: 0 });
  });

  it("awaits renderer disposal before building the next generation", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    const gate = deferred();
    env.renderer(0).disposeGate = gate;

    lifecycle.setMode2d(true);
    await tick();
    expect(lifecycle.getSnapshot().state).toBe("disposing");
    lifecycle.setMode2d(false);
    await tick();
    expect(env.renderers).toHaveLength(1);

    gate.resolve();
    await lifecycle.settled();
    expect(env.log.indexOf("disposed:1")).toBeLessThan(env.log.indexOf("create:2"));
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "live", liveGeneration: 2 });
  });

  it("drops a stale visibility confirmation that resolves out of order", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    const hidden = deferred<ConfirmedVisibility>();
    const visible = deferred<ConfirmedVisibility>();
    const pending = [hidden, visible];
    env.confirm = () => pending.shift()?.promise ?? Promise.reject(new Error("unexpected confirmation"));

    lifecycle.setHostVisibility("hidden");
    lifecycle.setHostVisibility("visible");
    visible.resolve({ visible: true, minimized: false });
    await tick();
    hidden.resolve({ visible: false, minimized: false });
    await lifecycle.settled();

    expect(env.renderer(0).disposeCalls).toBe(0);
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "live", liveGeneration: 1 });
    expect(lifecycle.getSnapshot().counts.staleConfirmations).toBe(1);
  });

  it("an attach/detach/attach sequence leaves exactly one live renderer", async () => {
    const env = new FakeEnv();
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    lifecycle.detach();
    lifecycle.attach(host);
    await lifecycle.settled();
    const undisposed = env.renderers.filter((renderer) => renderer.disposeCalls === 0);
    expect(undisposed).toHaveLength(1);
    expect(lifecycle.getSnapshot().liveGeneration).toBe(undisposed[0]?.id);
  });
});

describe("RendererLifecycle device loss", () => {
  it("visible loss runs the preserved handler, sends allowlisted metadata, disposes and rebuilds once", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    const first = env.renderer(0);

    first.onDeviceLost({ ...naturalLoss, message: `reset\n${"x".repeat(400)}` });
    await lifecycle.settled();

    expect(first.isDeviceLost).toBe(true);
    expect(first.defaultLossCalls).toHaveLength(1);
    expect(first.disposeCalls).toBe(1);
    expect(env.renderers).toHaveLength(2);
    const snapshot = lifecycle.getSnapshot();
    expect(snapshot).toMatchObject({ state: "live", liveGeneration: 2, lossCounts: { natural: 1, injected: 0 } });
    expect(snapshot.counts.recoveryRebuilds).toBe(1);
    const [record] = snapshot.deviceLosses;
    expect(Object.keys(record ?? {}).sort()).toEqual(["api", "atMs", "generation", "injected", "message", "outcome", "reason", "visible"]);
    expect(record).toMatchObject({ generation: 1, api: "WebGPU", reason: "unknown", injected: false, visible: true, outcome: "rebuild" });
    expect(record?.message.length).toBe(160);
    expect(record?.message.startsWith("reset ")).toBe(true);
  });

  it("ignores loss and error callbacks from a retired generation", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    await env.show(lifecycle, false);
    await env.show(lifecycle, true);
    const retired = env.renderer(0);

    retired.onDeviceLost(naturalLoss);
    retired.onError({ api: "WebGPU", type: "GPUValidationError", message: "late" });

    // A late error updates counters without emitting; the report resamples them.
    const snapshot = await lifecycle.runQualificationCommand("report-state");
    expect(snapshot.counts.ignoredLateCallbacks).toBe(2);
    expect(snapshot.deviceLosses).toHaveLength(0);
    expect(snapshot.rendererErrors).toHaveLength(0);
    expect(snapshot).toMatchObject({ state: "live", liveGeneration: 2 });
    expect(env.renderers).toHaveLength(2);
  });

  it("hidden loss defers recreation until the host is visible again", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    const confirmation = deferred<ConfirmedVisibility>();
    env.confirm = () => confirmation.promise;
    lifecycle.setHostVisibility("hidden");

    env.renderer(0).onDeviceLost(naturalLoss);
    await tick();
    expect(env.renderer(0).disposeCalls).toBe(1);
    expect(env.renderers).toHaveLength(1);
    expect(lifecycle.getSnapshot().deviceLosses[0]).toMatchObject({ visible: false, outcome: "deferred-until-visible" });

    confirmation.resolve({ visible: false, minimized: false });
    env.confirm = () => Promise.resolve({ ...env.visibility });
    await lifecycle.settled();
    expect(lifecycle.getSnapshot().state).toBe("hidden-disposed");
    expect(env.renderers).toHaveLength(1);

    await env.show(lifecycle, true);
    expect(env.renderers).toHaveLength(2);
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "live", liveGeneration: 2 });
    expect(lifecycle.getSnapshot().counts.recoveryRebuilds).toBe(1);
  });

  it("a repeated loss stops retrying and falls back; an explicit exit from 2D tries once more", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    env.renderer(0).onDeviceLost(naturalLoss);
    await lifecycle.settled();
    env.renderer(1).onDeviceLost(naturalLoss);
    await lifecycle.settled();

    let snapshot = lifecycle.getSnapshot();
    expect(snapshot).toMatchObject({ state: "failed-fallback", presentation: "fallback", liveGeneration: null });
    expect(snapshot.deviceLosses.map((loss) => loss.outcome)).toEqual(["rebuild", "fallback"]);
    expect(snapshot.fallbackReason).toContain("natural loss");
    expect(env.fallbacks).toHaveLength(1);
    expect(env.renderer(1).disposeCalls).toBe(1);

    await env.show(lifecycle, false);
    await env.show(lifecycle, true);
    expect(env.renderers).toHaveLength(2);

    lifecycle.setMode2d(false);
    await lifecycle.settled();
    snapshot = lifecycle.getSnapshot();
    expect(env.renderers).toHaveLength(3);
    expect(snapshot).toMatchObject({ state: "live", presentation: "3d", fallbackReason: null });
  });

  it("a failed recovery rebuild falls back without another attempt and never disposes the failed renderer", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    env.failNextInit = new Error("device request failed");
    env.renderer(0).onDeviceLost(naturalLoss);
    await lifecycle.settled();

    expect(env.renderers).toHaveLength(2);
    expect(env.renderer(1).disposeCalls).toBe(0);
    expect(env.surfacesDestroyed).toBe(2);
    const snapshot = lifecycle.getSnapshot();
    expect(snapshot).toMatchObject({ state: "failed-fallback", presentation: "fallback" });
    expect(snapshot.counts.initFailures).toBe(1);
    expect(snapshot.fallbackReason).toContain("device request failed");
  });

  it("an initial init failure keeps the operational fallback", async () => {
    const env = new FakeEnv();
    env.failNextInit = new Error("Unable to create WebGPU adapter.");
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    await lifecycle.settled();
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "failed-fallback", presentation: "fallback" });
    expect(env.renderer(0).disposeCalls).toBe(0);
    expect(env.fallbacks[0]).toContain("initialization failed");
  });

  it("records renderer errors separately from device loss and bounds them per generation", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    for (let index = 0; index < 6; index += 1) {
      env.renderer(0).onError({ api: "WebGPU", type: "GPUValidationError", message: `bad bind group ${index}`, originalEvent: {} });
    }
    // Errors past the per-generation cap are counted without emitting, so a
    // per-frame validation error cannot flood reports or re-render the UI.
    expect(lifecycle.getSnapshot().counts.rendererErrors).toBe(4);
    const snapshot = await lifecycle.runQualificationCommand("report-state");
    expect(env.renderer(0).defaultErrorCalls).toHaveLength(6);
    expect(snapshot.counts.rendererErrors).toBe(6);
    expect(snapshot.rendererErrors).toHaveLength(4);
    expect(snapshot.rendererErrors[0]).toMatchObject({ generation: 1, api: "WebGPU", type: "GPUValidationError" });
    expect(snapshot.deviceLosses).toHaveLength(0);
    expect(snapshot.state).toBe("live");
  });
});

describe("RendererLifecycle qualification injection", () => {
  it("labels an injected loss as injected in the record, counts and reports", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    lifecycle.injectDeviceLoss();
    await lifecycle.settled();

    const preserved = env.renderer(0).defaultLossCalls[0];
    expect(preserved).toMatchObject({ api: "WebGPU", message: "qualification-injected", reason: "unknown", injected: true });
    const snapshot = lifecycle.getSnapshot();
    expect(snapshot.deviceLosses[0]).toMatchObject({ injected: true, message: "qualification-injected", outcome: "rebuild" });
    expect(snapshot.lossCounts).toEqual({ natural: 0, injected: 1 });
    expect(env.reports.filter((report) => report.event === "device-loss-injected")).toHaveLength(1);
    expect(env.reports.some((report) => report.event === "device-loss")).toBe(false);
    expect(snapshot).toMatchObject({ state: "live", liveGeneration: 2 });

    lifecycle.injectDeviceLoss();
    await lifecycle.settled();
    expect(lifecycle.getSnapshot().fallbackReason).toContain("injected loss");
    expect(env.fallbacks[0]).toContain("injected loss");
  });

  it("labels a natural loss after an injected one as natural", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    lifecycle.injectDeviceLoss();
    await lifecycle.settled();
    await env.show(lifecycle, false);
    await env.show(lifecycle, true);
    env.renderer(2).onDeviceLost(naturalLoss);
    await lifecycle.settled();
    expect(lifecycle.getSnapshot().deviceLosses.map((loss) => loss.injected)).toEqual([true, false]);
    expect(lifecycle.getSnapshot().lossCounts).toEqual({ natural: 1, injected: 1 });
  });

  it("refuses injection outside qualification builds and without a live renderer", async () => {
    const env = new FakeEnv();
    const production = await env.live(false);
    expect(() => production.injectDeviceLoss()).toThrow(/qualification builds/);
    await expect(production.runQualificationCommand("report-state")).rejects.toThrow(/qualification builds/);

    const qualification = new FakeEnv();
    const lifecycle = await qualification.live();
    lifecycle.setMode2d(true);
    await lifecycle.settled();
    expect(() => lifecycle.injectDeviceLoss()).toThrow(/no live initialized renderer/);
  });

  it("qualification commands resolve with the settled diagnostics", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    expect((await lifecycle.runQualificationCommand("enter-2d")).state).toBe("2d");
    expect((await lifecycle.runQualificationCommand("exit-2d")).state).toBe("live");
    const injected = await lifecycle.runQualificationCommand("inject-device-loss");
    expect(injected).toMatchObject({ state: "live", liveGeneration: 3, lossCounts: { natural: 0, injected: 1 } });
    await lifecycle.runQualificationCommand("report-state", { label: "g15-step-4" });
    expect(env.reports.at(-1)?.detail).toEqual({ command: "report-state", label: "g15-step-4" });
    await expect(lifecycle.runQualificationCommand("reboot-gpu")).rejects.toThrow(/unknown/);
  });
});

describe("RendererLifecycle 2D mode, frames, assets and motion", () => {
  it("2D mode disposes the renderer and exiting rebuilds from the latest CPU model", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    lifecycle.setModel(model("a"));
    lifecycle.setMode2d(true);
    expect(lifecycle.getSnapshot().presentation).toBe("2d");
    await lifecycle.settled();
    expect(lifecycle.getSnapshot().state).toBe("2d");
    expect(env.scene(0).released).toBe(true);
    expect(env.renderer(0).disposed).toBe(true);

    const latest = model("a", "b");
    lifecycle.setModel(latest);
    expect(env.scene(0).updates.at(-1)).not.toBe(latest);
    lifecycle.setMode2d(false);
    await lifecycle.settled();
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "live", presentation: "3d", liveGeneration: 2 });
    expect(env.scene(1).updates[0]).toBe(latest);
    expect(env.attested).toHaveLength(2);
  });

  it("counts the internal frame loop after the callback is cleared and proves zero scheduled frames after disposal", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    const counter = installFrameCounter(env.frames);
    env.frames.runFrame();
    env.frames.runFrame();
    expect(counter.counts()).toMatchObject({ requested: 3, fired: 2, pending: 1 });

    const releaseGate = deferred();
    env.scene(0).releaseGate = releaseGate;
    lifecycle.setMode2d(true);
    await tick();
    expect(env.renderer(0).appCallback).toBeNull();
    env.frames.runFrame();
    env.frames.runFrame();
    expect(counter.counts()).toMatchObject({ requested: 5, pending: 1 });

    releaseGate.resolve();
    await lifecycle.settled();
    expect(counter.counts()).toMatchObject({ requested: 5, cancelled: 1, pending: 0 });
    env.frames.runFrame();
    env.frames.runFrame();

    const report = await lifecycle.runQualificationCommand("report-state");
    expect(report.state).toBe("2d");
    expect(report.frames).toMatchObject({ requested: 5, pending: 0, requestedSinceDisposal: 0 });

    await lifecycle.runQualificationCommand("exit-2d");
    expect(lifecycle.getSnapshot().frames?.requestedSinceDisposal).toBeNull();
    expect(counter.counts().pending).toBe(1);
  });

  it("aborts the asset request on retirement and disposes a completion that arrives afterwards", async () => {
    const env = new FakeEnv();
    const lifecycle = await env.live();
    const [request] = env.assetRequests;
    await env.show(lifecycle, false);
    expect(request?.signal.aborted).toBe(true);

    const late = fakeAsset(1);
    request?.gate.resolve(late);
    await tick();
    expect(late.disposed).toBe(true);
    expect(env.scene(0).assets).toHaveLength(0);
    expect(lifecycle.getSnapshot().asset).toMatchObject({ started: 1, applied: 0, discardedLate: 1 });
    expect(env.reports.some((report) => report.event === "late-asset-discarded")).toBe(true);

    await env.show(lifecycle, true);
    const current = fakeAsset(2);
    env.assetRequests[1]?.gate.resolve(current);
    await tick();
    expect(env.scene(1).assets).toEqual([current]);
    expect(current.disposed).toBe(false);
    await env.show(lifecycle, false);
    expect(current.disposed).toBe(true);
  });

  it("applies an asset that completes during init once the scene exists", async () => {
    const env = new FakeEnv();
    env.autoInit = false;
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    await tick();
    const early = fakeAsset(1);
    env.assetRequests[0]?.gate.resolve(early);
    await tick();
    env.renderer(0).initGate.resolve();
    await lifecycle.settled();
    expect(env.scene(0).assets).toEqual([early]);
    expect(lifecycle.getSnapshot().asset.applied).toBe(1);
  });

  it("follows prefers-reduced-motion changes and releases the listener on detach", async () => {
    const env = new FakeEnv();
    env.media.matches = true;
    const lifecycle = await env.live();
    expect(env.scene(0).reducedMotion).toBe(true);
    expect(lifecycle.getSnapshot().reducedMotion).toBe(true);

    env.media.fire(false);
    expect(env.scene(0).reducedMotion).toBe(false);
    expect(lifecycle.getSnapshot().reducedMotion).toBe(false);

    lifecycle.detach();
    await lifecycle.settled();
    expect(env.media.listeners.size).toBe(0);
    expect(lifecycle.getSnapshot().state).toBe("detached");
    expect(env.renderer(0).disposed).toBe(true);
  });
});

describe("RendererLifecycle asset outcome accounting", () => {
  /** Attaches with init held open and delivers generation 1's asset before any scene exists. */
  async function heldBeforeScene(env: FakeEnv): Promise<{ lifecycle: RendererLifecycle<FakeRenderer, FakeAsset>; early: FakeAsset }> {
    env.autoInit = false;
    const lifecycle = env.lifecycle();
    lifecycle.attach(host);
    await tick();
    const early = fakeAsset(1);
    env.assetRequests[0]?.gate.resolve(early);
    await tick();
    expect(early.disposeCalls).toBe(0);
    expect(env.scenes).toHaveLength(0);
    return { lifecycle, early };
  }

  it("releases an asset received before its scene once when the generation retires before a scene exists", async () => {
    const env = new FakeEnv();
    const { lifecycle, early } = await heldBeforeScene(env);

    env.visibility = { visible: false, minimized: false };
    lifecycle.setHostVisibility("hidden");
    await tick();
    expect(lifecycle.getSnapshot().initPending).toEqual({ generation: 1, retired: true });
    expect(early.disposeCalls).toBe(0);

    env.renderer(0).initGate.resolve();
    await lifecycle.settled();
    expect(env.scenes).toHaveLength(0);
    expect(early.disposeCalls).toBe(1);
    const { asset } = lifecycle.getSnapshot();
    expect(asset).toEqual({ started: 1, applied: 0, aborted: 0, discardedLate: 0, failed: 0, releasedBeforeScene: 1, disposed: 1 });
    expect(outcomes(asset)).toBe(asset.started);
    expect(env.reports.filter((report) => report.event === "asset-released-before-scene")).toHaveLength(1);
  });

  it("releases an asset received during init once when the init rejects", async () => {
    const env = new FakeEnv();
    const { lifecycle, early } = await heldBeforeScene(env);

    env.renderer(0).initGate.reject(new Error("Unable to create WebGPU adapter."));
    await lifecycle.settled();
    const snapshot = lifecycle.getSnapshot();
    expect(snapshot.state).toBe("failed-fallback");
    expect(env.renderer(0).disposeCalls).toBe(0);
    expect(early.disposeCalls).toBe(1);
    expect(snapshot.asset).toEqual({ started: 1, applied: 0, aborted: 0, discardedLate: 0, failed: 0, releasedBeforeScene: 1, disposed: 1 });
    expect(outcomes(snapshot.asset)).toBe(snapshot.asset.started);
  });

  it("releases a held asset once when scene construction fails", async () => {
    const env = new FakeEnv();
    env.failNextScene = new Error("node material compile failed");
    const { lifecycle, early } = await heldBeforeScene(env);

    env.renderer(0).initGate.resolve();
    await lifecycle.settled();
    const snapshot = lifecycle.getSnapshot();
    expect(snapshot.state).toBe("failed-fallback");
    expect(snapshot.fallbackReason).toContain("scene construction failed");
    expect(env.renderer(0).disposeCalls).toBe(1);
    expect(early.disposeCalls).toBe(1);
    expect(snapshot.asset).toEqual({ started: 1, applied: 0, aborted: 0, discardedLate: 0, failed: 0, releasedBeforeScene: 1, disposed: 1 });
    expect(outcomes(snapshot.asset)).toBe(snapshot.asset.started);
  });

  it("releases a held asset once when a device loss retires the generation before its scene", async () => {
    const env = new FakeEnv();
    const { lifecycle, early } = await heldBeforeScene(env);

    env.renderer(0).onDeviceLost(naturalLoss);
    await tick();
    expect(lifecycle.getSnapshot().deviceLosses[0]).toMatchObject({ generation: 1, outcome: "rebuild" });
    env.autoInit = true;
    env.renderer(0).initGate.resolve();
    await lifecycle.settled();
    expect(early.disposeCalls).toBe(1);
    expect(lifecycle.getSnapshot()).toMatchObject({ state: "live", liveGeneration: 2 });

    const current = fakeAsset(2);
    env.assetRequests[1]?.gate.resolve(current);
    await tick();
    expect(env.scene(0).assets).toEqual([current]);
    const { asset } = lifecycle.getSnapshot();
    expect(asset).toEqual({ started: 2, applied: 1, aborted: 0, discardedLate: 0, failed: 0, releasedBeforeScene: 1, disposed: 1 });
    expect(outcomes(asset)).toBe(asset.started);
  });

  it("counts an asset the scene throws on as failed, once, and still disposes it once", async () => {
    // Thrown while the scene is built: the generation falls back as before.
    const building = new FakeEnv();
    building.failApply = new Error("texture upload rejected");
    const { lifecycle, early } = await heldBeforeScene(building);
    building.renderer(0).initGate.resolve();
    await lifecycle.settled();
    expect(lifecycle.getSnapshot().fallbackReason).toContain("texture upload rejected");
    expect(early.disposeCalls).toBe(1);
    expect(lifecycle.getSnapshot().asset).toEqual({ started: 1, applied: 0, aborted: 0, discardedLate: 0, failed: 1, releasedBeforeScene: 0, disposed: 1 });

    // Thrown by a live scene: the generation stays live.
    const env = new FakeEnv();
    const live = await env.live();
    env.scene(0).failApply = new Error("texture upload rejected");
    const asset = fakeAsset(1);
    env.assetRequests[0]?.gate.resolve(asset);
    await tick();
    expect(live.getSnapshot()).toMatchObject({ state: "live", liveGeneration: 1 });
    expect(env.reports.filter((report) => report.event === "asset-failed")).toHaveLength(1);
    await env.show(live, false);
    expect(asset.disposeCalls).toBe(1);
    const counts = live.getSnapshot().asset;
    expect(counts).toEqual({ started: 1, applied: 0, aborted: 0, discardedLate: 0, failed: 1, releasedBeforeScene: 0, disposed: 1 });
    expect(outcomes(counts)).toBe(counts.started);
  });

  it("gives every started asset one outcome and one disposal across seeded random interleavings", { timeout: 120_000 }, async () => {
    const SEED = 0xc12;
    const ITERATIONS = 600;
    const random = seeded(SEED);
    const chance = (p: number) => random() < p;
    const pick = <T>(items: readonly T[]): T | undefined => items[Math.floor(random() * items.length)];
    const totals = {
      steps: 0,
      delivered: 0,
      identityChecks: 0,
      started: 0,
      applied: 0,
      aborted: 0,
      discardedLate: 0,
      failed: 0,
      releasedBeforeScene: 0,
      disposed: 0,
    };

    for (let iteration = 0; iteration < ITERATIONS; iteration += 1) {
      const label = `seed ${SEED} iteration ${iteration}`;
      const env = new FakeEnv();
      env.autoInit = false;
      const lifecycle = env.lifecycle();
      const delivered: FakeAsset[] = [];
      const settledRequests = new Set<number>();
      const settledInits = new Set<FakeRenderer>();
      const pendingRequests = () => env.assetRequests.map((_, index) => index).filter((index) => !settledRequests.has(index));
      const pendingInits = () => env.renderers.filter((renderer) => !settledInits.has(renderer));
      // One request per generation, so request `index` belongs to generation `index + 1`.
      const settleRequest = (index: number, deliver: boolean): void => {
        const request = env.assetRequests[index];
        if (!request) return;
        settledRequests.add(index);
        if (deliver) {
          const asset = fakeAsset(index + 1);
          delivered.push(asset);
          request.gate.resolve(asset);
        } else if (request.signal.aborted) {
          request.gate.reject(new DOMException("The operation was aborted.", "AbortError"));
        } else {
          request.gate.reject(new Error("floor texture request failed: HTTP 500"));
        }
      };
      const settleInit = (renderer: FakeRenderer, succeed: boolean): void => {
        settledInits.add(renderer);
        if (succeed) renderer.initGate.resolve();
        else renderer.initGate.reject(new Error("Unable to create WebGPU adapter."));
      };

      lifecycle.attach(host);
      const steps = 6 + Math.floor(random() * 18);
      for (let step = 0; step < steps; step += 1) {
        const index = pick(pendingRequests());
        const renderer = pick(pendingInits());
        switch (Math.floor(random() * 11)) {
          case 0:
            if (index !== undefined) settleRequest(index, true);
            break;
          case 1:
            if (index !== undefined) settleRequest(index, false);
            break;
          case 2:
            if (renderer) settleInit(renderer, true);
            break;
          case 3:
            if (renderer) settleInit(renderer, false);
            break;
          case 4:
            env.visibility = chance(0.5) ? { visible: false, minimized: false } : { visible: true, minimized: true };
            lifecycle.setHostVisibility("hidden");
            break;
          case 5:
            env.visibility = { visible: true, minimized: false };
            lifecycle.setHostVisibility("visible");
            break;
          case 6:
            lifecycle.setMode2d(true);
            break;
          case 7:
            lifecycle.setMode2d(false);
            break;
          case 8:
            env.renderers.at(-1)?.onDeviceLost(naturalLoss);
            break;
          case 9:
            env.failNextScene = new Error("node material compile failed");
            break;
          default:
            env.failApply = env.failApply ? null : new Error("texture upload rejected");
        }
        // Interleave: the next action runs in the same turn, a few microtasks later, or after everything queued.
        if (chance(0.3)) await tick();
        else for (let hops = Math.floor(random() * 6); hops > 0; hops -= 1) await Promise.resolve();
      }

      // Quiescence: settle every outstanding init and request until the lifecycle starts no more.
      for (let round = 0; ; round += 1) {
        await tick();
        const requests = pendingRequests();
        const inits = pendingInits();
        if (requests.length === 0 && inits.length === 0) break;
        expect(round, label).toBeLessThan(32);
        for (const pending of inits) settleInit(pending, chance(0.85));
        for (const pending of requests) settleRequest(pending, chance(0.7));
      }
      const quiet = lifecycle.getSnapshot();
      expect(quiet.initPending, label).toBeNull();
      expect(quiet.asset.started, label).toBe(env.assetRequests.length);
      expect(outcomes(quiet.asset), label).toBe(quiet.asset.started);
      expect(delivered.every((asset) => asset.disposeCalls <= 1), label).toBe(true);
      // Only the live generation may still hold its asset.
      const held = delivered.filter((asset) => asset.disposeCalls === 0);
      expect(held.length, label).toBeLessThanOrEqual(1);
      if (held[0]) expect(held[0].generation, label).toBe(quiet.liveGeneration);
      expect(quiet.asset.disposed, label).toBe(delivered.length - held.length);

      lifecycle.detach();
      await lifecycle.settled();
      const final = lifecycle.getSnapshot();
      expect(final.state, label).toBe("detached");
      expect(delivered.every((asset) => asset.disposeCalls === 1), label).toBe(true);
      expect(final.asset.disposed, label).toBe(delivered.length);
      expect(outcomes(final.asset), label).toBe(final.asset.started);

      totals.steps += steps;
      totals.delivered += delivered.length;
      totals.identityChecks += 2;
      for (const key of ["started", "applied", "aborted", "discardedLate", "failed", "releasedBeforeScene", "disposed"] as const) {
        totals[key] += final.asset[key];
      }
    }

    // Every outcome occurred, so the identity held on each path, not only the common ones.
    for (const key of ["applied", "aborted", "discardedLate", "failed", "releasedBeforeScene"] as const) {
      expect(totals[key], key).toBeGreaterThan(0);
    }
    console.info(`asset outcome interleavings: seed ${SEED}, ${ITERATIONS} iterations, ${JSON.stringify(totals)}`);
  });
});
