// Renderer lifecycle (SPEC §15.6) for the pinned three 0.186.1 renderer.
//
// One controller owns every renderer generation. Init, dispose and rebuild
// are serialized through a single queue, and each generation's asynchronous
// steps (init, asset load, loss and error callbacks) carry their own
// generation record: once a generation is retired, nothing it reports can
// revive it, and a disposed renderer is never reused.
//
// Pinned behaviour this controller is built around:
// - `setAnimationLoop(null)` only clears the application callback; the
//   internal requestAnimationFrame loop started by `init()` runs until
//   `dispose()` cancels it. Hidden and 2D states therefore dispose.
// - `dispose()` on a renderer whose `init()` has not settled skips its
//   release block and then calls `setAnimationLoop(null)`, which awaits
//   `init()`: the renderer finishes initializing and its internal loop runs
//   with nothing left to stop it. A generation is therefore disposed only
//   after its `init()` settles, and an init that rejected is never disposed
//   (that path would only re-raise the rejection unhandled).
// - `dispose()` leaves `_initialized` true and `_initPromise` resolved, so a
//   disposed renderer would accept `init()` again and render from released
//   internals. Every rebuild creates a new renderer on a new surface.

import type { RendererAttestation } from "./backend";
import type { SceneModel } from "./controller";
import { installFrameCounter, type FrameCounter, type FrameCounts, type FrameTarget } from "./frameCounter";

export type LifecycleState =
  | "detached"
  | "initializing"
  | "live"
  | "rebuilding"
  | "disposing"
  | "hidden-disposed"
  | "2d"
  | "failed-fallback";

/** What the stage shows: the 3D office, the chosen 2D view, or the 2D view after 3D failed. */
export type Presentation = "3d" | "2d" | "fallback";

export interface ConfirmedVisibility {
  visible: boolean;
  minimized: boolean;
}

/** Confirms the host window's visibility before the lifecycle acts on a hint. */
export type ConfirmVisibility = () => Promise<ConfirmedVisibility>;

export type VisibilityHint = "visible" | "hidden";

/** Loss information in the shape the pinned backends pass to `renderer.onDeviceLost`. */
export interface RendererLossInfo {
  api: string;
  message: string;
  reason: string | null;
  originalEvent?: unknown;
  /** Present only on qualification-injected losses. */
  injected?: boolean;
}

/** The renderer members the lifecycle drives. Method syntax keeps `WebGPURenderer` assignable. */
export interface LifecycleRenderer {
  onDeviceLost(info: RendererLossInfo): void;
  /** Three passes an object at runtime although its typings declare a string. */
  onError(info: unknown): void;
  init(): Promise<unknown>;
  dispose(): Promise<void>;
}

export interface SceneAsset {
  dispose(): void;
}

export interface LifecycleScene<A extends SceneAsset> {
  update(model: SceneModel): void;
  setReducedMotion(reduced: boolean): void;
  applyAsset(asset: A): void;
  /** Clears the application frame callback and releases scene GPU resources. The lifecycle disposes the renderer. */
  release(): Promise<void>;
}

export interface RendererPlatform<R extends LifecycleRenderer, A extends SceneAsset> {
  /** A fresh, uninitialized renderer on a fresh surface inside `host`. */
  createRenderer(host: HTMLElement): R;
  attest(renderer: R, initDurationMs: number): RendererAttestation;
  createScene(renderer: R, reducedMotion: boolean): LifecycleScene<A>;
  /** Removes a retired renderer's surface. */
  destroySurface(renderer: R): void;
  /** One bundled same-scheme resource request per generation; it can still be in flight at close or reload. */
  loadAsset(signal: AbortSignal): Promise<A>;
}

type ChangeListener = (event: { matches: boolean }) => void;

export interface MediaQueryLike {
  readonly matches: boolean;
  addEventListener(type: "change", listener: ChangeListener): void;
  removeEventListener(type: "change", listener: ChangeListener): void;
}

export interface LifecycleReport {
  event: string;
  atMs: number;
  detail: Record<string, unknown> | null;
  diagnostics: RendererLifecycleSnapshot;
}

export interface RendererLifecycleOptions<R extends LifecycleRenderer, A extends SceneAsset> {
  platform: RendererPlatform<R, A>;
  confirmVisibility: ConfirmVisibility;
  /** True only in qualification builds: enables loss injection and qualification commands. */
  qualification: boolean;
  /** Qualification builds pass `window`; its frame scheduling is wrapped before the first renderer exists. */
  frameTarget: FrameTarget | null;
  matchMedia: ((query: string) => MediaQueryLike) | null;
  onAttested?: (attestation: RendererAttestation) => void;
  onFallback?: (reason: string) => void;
  report?: (report: LifecycleReport) => void;
}

export interface DeviceLossRecord {
  generation: number;
  api: "WebGPU" | "WebGL" | "other";
  reason: "unknown" | "destroyed" | "other" | null;
  message: string;
  /** True only for a qualification-injected loss; such a loss is never reported as natural. */
  injected: boolean;
  /** Whether the host was confirmed visible, with no hidden hint awaiting confirmation, when the loss arrived. */
  visible: boolean;
  outcome: "rebuild" | "deferred-until-visible" | "fallback";
  atMs: number;
}

export interface RendererErrorRecord {
  generation: number;
  api: "WebGPU" | "WebGL" | "other";
  type: string;
  message: string;
  atMs: number;
}

export interface TransitionRecord {
  atMs: number;
  generation: number;
  from: LifecycleState;
  to: LifecycleState;
  cause: string;
}

export interface FrameReport extends FrameCounts {
  /** Frames requested since the last renderer disposal completed; null while a renderer generation exists. */
  requestedSinceDisposal: number | null;
}

export interface LifecycleCounts {
  inits: number;
  initFailures: number;
  disposals: number;
  disposeFailures: number;
  /** Renderer generations created after the first. */
  rebuilds: number;
  /** Rebuilds made to recover from a device loss. */
  recoveryRebuilds: number;
  discardedLateInits: number;
  ignoredLateCallbacks: number;
  rendererErrors: number;
  staleConfirmations: number;
  confirmationFailures: number;
}

/**
 * Asset outcomes, for diagnostics only: no lifecycle decision reads them.
 * Once no request is in flight and no generation is initializing, every
 * started asset has exactly one outcome:
 * `started === applied + aborted + discardedLate + failed + releasedBeforeScene`.
 */
export interface AssetCounts {
  started: number;
  applied: number;
  aborted: number;
  discardedLate: number;
  /** The request failed, or the scene threw while applying the asset. */
  failed: number;
  /** Received and held, then released on retirement before any scene applied it. */
  releasedBeforeScene: number;
  /** `dispose()` calls on received assets; each received asset is disposed once. */
  disposed: number;
}

export interface RendererLifecycleSnapshot {
  state: LifecycleState;
  presentation: Presentation;
  /** The most recently created generation; 0 before the first. */
  generation: number;
  /** The generation currently rendering, if any. */
  liveGeneration: number | null;
  /** The current generation while its `init()` has not settled, and whether it is already retired. */
  initPending: { generation: number; retired: boolean } | null;
  visibility: ConfirmedVisibility | null;
  reducedMotion: boolean;
  counts: LifecycleCounts;
  lossCounts: { natural: number; injected: number };
  deviceLosses: DeviceLossRecord[];
  rendererErrors: RendererErrorRecord[];
  lastAttestation: RendererAttestation | null;
  /** Null unless a frame target was supplied (qualification builds). */
  frames: FrameReport | null;
  asset: AssetCounts;
  fallbackReason: string | null;
  transitions: TransitionRecord[];
}

export type RendererQualificationCommand = "inject-device-loss" | "enter-2d" | "exit-2d" | "report-state";

interface Generation<R, A extends SceneAsset> {
  readonly id: number;
  readonly renderer: R;
  /** Created to recover from a device loss; a loss on it is a repeated loss. */
  readonly recovery: boolean;
  readonly abort: AbortController;
  scene: LifecycleScene<A> | null;
  asset: A | null;
  /** Whether the held asset already has its outcome (applied, or failed to apply). */
  assetSettled: boolean;
  retired: boolean;
  initialized: boolean;
  injecting: boolean;
  errorsRecorded: number;
}

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
const MAX_TRANSITIONS = 32;
const MAX_LOSSES = 16;
const MAX_ERRORS = 16;
const MAX_ERRORS_PER_GENERATION = 4;
const MAX_TEXT = 160;
const ERROR_TYPES = new Set(["GPUValidationError", "GPUOutOfMemoryError", "GPUInternalError", "GPUError"]);

function boundedText(value: unknown): string {
  const text = typeof value === "string" ? value : value instanceof Error ? value.message : String(value);
  return text.replace(/[\u0000-\u001f\u007f]/g, " ").slice(0, MAX_TEXT);
}

function allowApi(api: unknown): DeviceLossRecord["api"] {
  return api === "WebGPU" || api === "WebGL" ? api : "other";
}

function allowReason(reason: unknown): DeviceLossRecord["reason"] {
  if (reason === null || reason === undefined) return null;
  return reason === "unknown" || reason === "destroyed" ? reason : "other";
}

function pushBounded<T>(list: T[], item: T, max: number): void {
  list.push(item);
  if (list.length > max) list.splice(0, list.length - max);
}

/** The default confirmation reads `document.visibilityState`; it cannot see minimization. */
export const documentVisibilityConfirmation: ConfirmVisibility = () =>
  Promise.resolve({
    visible: typeof document !== "undefined" && document.visibilityState === "visible",
    minimized: false,
  });

export class RendererLifecycle<R extends LifecycleRenderer = LifecycleRenderer, A extends SceneAsset = SceneAsset> {
  private host: HTMLElement | null = null;
  private visibility: ConfirmedVisibility | null = null;
  private hiddenHintPending = false;
  private visibilitySeq = 0;
  private confirmation: Promise<void> = Promise.resolve();
  private mode2d = false;
  private failure: string | null = null;
  private recoveryPending = false;
  private model: SceneModel = { workers: [], selectedId: null };
  private reducedMotion = false;
  private media: MediaQueryLike | null = null;
  private frames: FrameCounter | null = null;
  private framesAtDisposal: number | null = null;
  private current: Generation<R, A> | null = null;
  private generationCounter = 0;
  private queue: Promise<void> = Promise.resolve();
  private state: LifecycleState = "detached";
  private lastAttestation: RendererAttestation | null = null;
  private readonly counts: LifecycleCounts = {
    inits: 0,
    initFailures: 0,
    disposals: 0,
    disposeFailures: 0,
    rebuilds: 0,
    recoveryRebuilds: 0,
    discardedLateInits: 0,
    ignoredLateCallbacks: 0,
    rendererErrors: 0,
    staleConfirmations: 0,
    confirmationFailures: 0,
  };
  private readonly assetCounts: AssetCounts = {
    started: 0,
    applied: 0,
    aborted: 0,
    discardedLate: 0,
    failed: 0,
    releasedBeforeScene: 0,
    disposed: 0,
  };
  private readonly lossCounts = { natural: 0, injected: 0 };
  private readonly losses: DeviceLossRecord[] = [];
  private readonly errors: RendererErrorRecord[] = [];
  private readonly transitions: TransitionRecord[] = [];
  private readonly listeners = new Set<() => void>();
  private snapshot: RendererLifecycleSnapshot;

  constructor(private readonly options: RendererLifecycleOptions<R, A>) {
    this.snapshot = this.buildSnapshot();
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly getSnapshot = (): RendererLifecycleSnapshot => this.snapshot;

  // ----------------------------------------------------------------- inputs

  /** Mounts the lifecycle on its host element and confirms visibility before the first renderer. */
  attach(host: HTMLElement): void {
    if (this.options.frameTarget !== null && this.frames === null) {
      this.frames = installFrameCounter(this.options.frameTarget);
    }
    this.host = host;
    if (this.options.matchMedia !== null && this.media === null) {
      this.media = this.options.matchMedia(REDUCED_MOTION_QUERY);
      this.reducedMotion = this.media.matches;
      this.media.addEventListener("change", this.onReducedMotion);
    }
    this.emit("attach");
    this.confirm(null);
  }

  /** Unmounts: the current generation is retired and disposed; the CPU model is kept. */
  detach(): void {
    this.host = null;
    this.visibilitySeq += 1;
    this.hiddenHintPending = false;
    this.visibility = null;
    if (this.media !== null) {
      this.media.removeEventListener("change", this.onReducedMotion);
      this.media = null;
    }
    this.retireIfUnwanted();
    this.schedule();
  }

  /** A host visibility hint (e.g. `document.visibilitychange`); acted on only once confirmed. */
  setHostVisibility(hint: VisibilityHint): void {
    this.confirm(hint);
  }

  /** The latest CPU scene model. It is kept while no renderer exists and rebuilds the next one. */
  setModel(model: SceneModel): void {
    this.model = model;
    const entry = this.current;
    if (entry !== null && !entry.retired && entry.scene !== null) entry.scene.update(model);
  }

  /** Enters or leaves 2D mode. Leaving it after a fallback clears the failure and tries 3D once more. */
  setMode2d(on: boolean): void {
    this.mode2d = on;
    if (!on && this.failure !== null) {
      this.failure = null;
      this.recoveryPending = false;
    }
    this.retireIfUnwanted();
    this.emit(on ? "enter-2d" : "exit-2d");
    this.schedule();
  }

  /**
   * Qualification builds only: drives the live renderer's own `onDeviceLost`
   * path, so the preserved Three handler and the recovery controller both
   * run against the real initialized renderer. The loss is labelled injected.
   */
  injectDeviceLoss(): void {
    if (!this.options.qualification) throw new Error("device-loss injection is available only in qualification builds");
    const entry = this.current;
    if (entry === null || entry.retired || entry.scene === null) {
      throw new Error("no live initialized renderer to inject a device loss into");
    }
    const target: LifecycleRenderer = entry.renderer;
    entry.injecting = true;
    try {
      target.onDeviceLost({ api: "WebGPU", message: "qualification-injected", reason: "unknown", originalEvent: null, injected: true });
    } finally {
      entry.injecting = false;
    }
  }

  /** Qualification builds only: runs one command, waits for the lifecycle to settle and returns its diagnostics. */
  async runQualificationCommand(name: string, args: Readonly<Record<string, unknown>> = {}): Promise<RendererLifecycleSnapshot> {
    if (!this.options.qualification) throw new Error("renderer qualification commands are available only in qualification builds");
    switch (name as RendererQualificationCommand) {
      case "inject-device-loss":
        this.injectDeviceLoss();
        break;
      case "enter-2d":
        this.setMode2d(true);
        break;
      case "exit-2d":
        this.setMode2d(false);
        break;
      case "report-state":
        break;
      default:
        throw new Error(`unknown renderer qualification command: ${boundedText(name)}`);
    }
    await this.settled();
    const label = typeof args.label === "string" && /^[a-z0-9-]{1,48}$/.test(args.label) ? args.label : null;
    this.emit(`command-${name}`, { command: name, label });
    return this.snapshot;
  }

  /** Resolves once queued lifecycle work and visibility confirmations have drained. */
  async settled(): Promise<void> {
    for (;;) {
      const queue = this.queue;
      const confirmation = this.confirmation;
      await Promise.all([queue, confirmation]);
      if (queue === this.queue && confirmation === this.confirmation) return;
    }
  }

  // ------------------------------------------------------------- visibility

  private confirm(hint: VisibilityHint | null): void {
    if (this.host === null) return;
    const seq = ++this.visibilitySeq;
    this.hiddenHintPending = hint === "hidden";
    const confirmVisibility = this.options.confirmVisibility;
    this.confirmation = (async () => {
      let confirmed: ConfirmedVisibility;
      try {
        confirmed = await confirmVisibility();
      } catch (error) {
        if (seq !== this.visibilitySeq) return;
        this.hiddenHintPending = false;
        this.counts.confirmationFailures += 1;
        this.emit("visibility-confirmation-failed", { hint, message: boundedText(error) });
        this.schedule();
        return;
      }
      if (seq !== this.visibilitySeq) {
        this.counts.staleConfirmations += 1;
        this.emit("visibility-confirmation-stale", { hint });
        return;
      }
      this.hiddenHintPending = false;
      this.visibility = { visible: confirmed.visible === true, minimized: confirmed.minimized === true };
      this.retireIfUnwanted();
      this.emit("visibility-confirmed", { hint, ...this.visibility });
      this.schedule();
    })();
  }

  private isVisible(): boolean {
    return this.visibility !== null && this.visibility.visible && !this.visibility.minimized;
  }

  /** A live generation is retired only on confirmed state, never on an unconfirmed hint. */
  private mustRetire(): boolean {
    return this.host === null || !this.isVisible() || this.mode2d || this.failure !== null;
  }

  /** A new generation also waits while a hidden hint is still being confirmed. */
  private mayCreate(): boolean {
    return this.host !== null && this.isVisible() && !this.hiddenHintPending && !this.mode2d && this.failure === null;
  }

  private readonly onReducedMotion = (event: { matches: boolean }): void => {
    this.reducedMotion = event.matches;
    const entry = this.current;
    if (entry !== null && !entry.retired && entry.scene !== null) entry.scene.setReducedMotion(event.matches);
    this.emit("reduced-motion", { reducedMotion: event.matches });
  };

  // ------------------------------------------------------- serialized work

  private retireIfUnwanted(): void {
    const entry = this.current;
    if (entry !== null && !entry.retired && this.mustRetire()) this.markRetired(entry);
  }

  /** Synchronous, so no callback of this generation can act after the decision. */
  private markRetired(entry: Generation<R, A>): void {
    entry.retired = true;
    entry.abort.abort();
    this.emit("generation-retired", { generation: entry.id, initialized: entry.initialized });
  }

  private schedule(): void {
    this.queue = this.queue
      .then(() => this.reconcile())
      .catch((error: unknown) => this.emit("lifecycle-error", { message: boundedText(error) }));
  }

  private async reconcile(): Promise<void> {
    for (;;) {
      const entry = this.current;
      if (entry !== null) {
        if (!entry.retired && this.mustRetire()) this.markRetired(entry);
        if (!entry.retired) return;
        await this.retire(entry);
        continue;
      }
      if (this.mayCreate()) {
        await this.build();
        continue;
      }
      this.rest();
      return;
    }
  }

  private async build(): Promise<void> {
    const host = this.host;
    if (host === null) return;
    const { platform } = this.options;
    const recovery = this.recoveryPending;
    this.recoveryPending = false;
    const id = ++this.generationCounter;
    let renderer: R;
    try {
      renderer = platform.createRenderer(host);
    } catch (error) {
      this.fail(`renderer construction failed: ${boundedText(error)}`);
      return;
    }
    const entry: Generation<R, A> = {
      id,
      renderer,
      recovery,
      abort: new AbortController(),
      scene: null,
      asset: null,
      assetSettled: false,
      retired: false,
      initialized: false,
      injecting: false,
      errorsRecorded: 0,
    };
    this.hook(entry);
    this.current = entry;
    this.framesAtDisposal = null;
    const first = this.counts.inits === 0;
    if (!first) this.counts.rebuilds += 1;
    if (recovery) this.counts.recoveryRebuilds += 1;
    this.counts.inits += 1;
    this.enter(first ? "initializing" : "rebuilding", recovery ? "recovery-rebuild" : first ? "init" : "rebuild");
    this.loadAsset(entry);

    const started = performance.now();
    try {
      await renderer.init();
    } catch (error) {
      this.counts.initFailures += 1;
      if (!entry.retired) this.fail(`renderer initialization failed: ${boundedText(error)}`);
      else this.emit("retired-init-failed", { generation: id, message: boundedText(error) });
      return;
    }
    entry.initialized = true;
    if (entry.retired) {
      this.counts.discardedLateInits += 1;
      this.emit("late-init-discarded", { generation: id });
      return;
    }
    try {
      const attestation = platform.attest(renderer, Math.round(performance.now() - started));
      const scene = platform.createScene(renderer, this.reducedMotion);
      entry.scene = scene;
      scene.update(this.model);
      if (entry.asset !== null) this.applyAsset(entry, scene, entry.asset);
      this.lastAttestation = attestation;
      this.enter("live", recovery ? "recovered" : "live");
      this.options.onAttested?.(attestation);
    } catch (error) {
      this.fail(`scene construction failed: ${boundedText(error)}`);
    }
  }

  /** Clear the frame callback, release scene GPU resources, then await the renderer's disposal. */
  private async retire(entry: Generation<R, A>): Promise<void> {
    this.enter("disposing", "retire");
    if (entry.scene !== null) {
      const scene = entry.scene;
      entry.scene = null;
      try {
        await scene.release();
      } catch (error) {
        this.emit("scene-release-failed", { generation: entry.id, message: boundedText(error) });
      }
    }
    if (entry.asset !== null) {
      const asset = entry.asset;
      entry.asset = null;
      this.disposeAsset(asset);
      if (!entry.assetSettled) {
        this.assetCounts.releasedBeforeScene += 1;
        this.emit("asset-released-before-scene", { generation: entry.id });
      }
    }
    if (entry.initialized) {
      try {
        await entry.renderer.dispose();
        this.counts.disposals += 1;
      } catch (error) {
        this.counts.disposeFailures += 1;
        this.emit("dispose-failed", { generation: entry.id, message: boundedText(error) });
      }
    }
    this.options.platform.destroySurface(entry.renderer);
    if (this.current === entry) this.current = null;
    this.framesAtDisposal = this.frames?.counts().requested ?? null;
  }

  private rest(): void {
    let state: LifecycleState;
    if (this.host === null) state = "detached";
    else if (this.failure !== null) state = "failed-fallback";
    else if (this.mode2d) state = "2d";
    else state = "hidden-disposed";
    this.enter(state, "settled");
  }

  private fail(reason: string): void {
    this.failure = reason;
    const entry = this.current;
    if (entry !== null && !entry.retired) this.markRetired(entry);
    this.emit("fallback", { reason });
    this.options.onFallback?.(reason);
  }

  // ------------------------------------------------- generation callbacks

  /** Registered before `init()`; the renderer's existing handlers keep running first. */
  private hook(entry: Generation<R, A>): void {
    const target: LifecycleRenderer = entry.renderer;
    const previousLost = target.onDeviceLost;
    const previousError = target.onError;
    target.onDeviceLost = (info: RendererLossInfo) => {
      try {
        previousLost.call(target, info);
      } catch {
        // The preserved handler failing must not block recovery.
      }
      this.handleLoss(entry, info);
    };
    target.onError = (info: unknown) => {
      try {
        previousError.call(target, info);
      } catch {
        // As above: diagnostics only.
      }
      this.handleError(entry, info);
    };
  }

  private handleLoss(entry: Generation<R, A>, info: RendererLossInfo): void {
    if (entry !== this.current || entry.retired) {
      this.counts.ignoredLateCallbacks += 1;
      this.emit("late-callback-ignored", { generation: entry.id, callback: "device-lost" });
      return;
    }
    const injected = entry.injecting;
    const visible = this.mayCreate();
    this.markRetired(entry);
    let outcome: DeviceLossRecord["outcome"];
    if (entry.recovery) {
      outcome = "fallback";
    } else {
      this.recoveryPending = true;
      outcome = visible ? "rebuild" : "deferred-until-visible";
    }
    const record: DeviceLossRecord = {
      generation: entry.id,
      api: allowApi(info?.api),
      reason: allowReason(info?.reason),
      message: boundedText(info?.message ?? ""),
      injected,
      visible,
      outcome,
      atMs: Date.now(),
    };
    pushBounded(this.losses, record, MAX_LOSSES);
    if (injected) this.lossCounts.injected += 1;
    else this.lossCounts.natural += 1;
    this.emit(injected ? "device-loss-injected" : "device-loss", { loss: record });
    if (outcome === "fallback") {
      this.fail(`repeated device loss on recovered generation ${entry.id} (${injected ? "injected" : "natural"} loss)`);
    }
    this.schedule();
  }

  private handleError(entry: Generation<R, A>, info: unknown): void {
    if (entry !== this.current || entry.retired) {
      this.counts.ignoredLateCallbacks += 1;
      return;
    }
    this.counts.rendererErrors += 1;
    if (entry.errorsRecorded >= MAX_ERRORS_PER_GENERATION) return;
    entry.errorsRecorded += 1;
    const shape = typeof info === "object" && info !== null ? (info as Record<string, unknown>) : null;
    const type = typeof shape?.type === "string" && ERROR_TYPES.has(shape.type) ? shape.type : "other";
    const record: RendererErrorRecord = {
      generation: entry.id,
      api: allowApi(shape?.api),
      type,
      message: boundedText(typeof info === "string" ? info : (shape?.message ?? "")),
      atMs: Date.now(),
    };
    pushBounded(this.errors, record, MAX_ERRORS);
    this.emit("renderer-error", { error: record });
  }

  private loadAsset(entry: Generation<R, A>): void {
    this.assetCounts.started += 1;
    let request: Promise<A>;
    try {
      request = this.options.platform.loadAsset(entry.abort.signal);
    } catch (error) {
      request = Promise.reject(error);
    }
    request.then(
      (asset) => {
        if (entry.retired) {
          this.disposeAsset(asset);
          this.assetCounts.discardedLate += 1;
          this.emit("late-asset-discarded", { generation: entry.id });
          return;
        }
        // Held until retirement; `build()` applies it if no scene exists yet.
        entry.asset = asset;
        if (entry.scene === null) return;
        try {
          this.applyAsset(entry, entry.scene, asset);
        } catch (error) {
          this.emit("asset-failed", { generation: entry.id, message: boundedText(error) });
          return;
        }
        this.emit("asset-applied", { generation: entry.id });
      },
      (error: unknown) => {
        if (entry.abort.signal.aborted) {
          this.assetCounts.aborted += 1;
          this.emit("asset-aborted", { generation: entry.id });
        } else {
          this.assetCounts.failed += 1;
          this.emit("asset-failed", { generation: entry.id, message: boundedText(error) });
        }
      },
    );
  }

  /** Records the held asset's outcome; a scene that throws records `failed` and the error propagates. */
  private applyAsset(entry: Generation<R, A>, scene: LifecycleScene<A>, asset: A): void {
    entry.assetSettled = true;
    try {
      scene.applyAsset(asset);
    } catch (error) {
      this.assetCounts.failed += 1;
      throw error;
    }
    this.assetCounts.applied += 1;
  }

  /** The lifecycle's only call to an asset's `dispose()`: once at late arrival, or once at retirement. */
  private disposeAsset(asset: A): void {
    asset.dispose();
    this.assetCounts.disposed += 1;
  }

  // ------------------------------------------------------------ diagnostics

  private enter(state: LifecycleState, cause: string): void {
    if (state === this.state) return;
    const from = this.state;
    this.state = state;
    pushBounded(this.transitions, { atMs: Date.now(), generation: this.generationCounter, from, to: state, cause }, MAX_TRANSITIONS);
    this.emit(cause, { from, to: state });
  }

  private emit(event: string, detail: Record<string, unknown> | null = null): void {
    this.snapshot = this.buildSnapshot();
    for (const listener of this.listeners) listener();
    this.options.report?.({ event, atMs: Date.now(), detail, diagnostics: this.snapshot });
  }

  private buildSnapshot(): RendererLifecycleSnapshot {
    const entry = this.current;
    const frames = this.frames?.counts() ?? null;
    return {
      state: this.state,
      presentation: this.failure !== null ? "fallback" : this.mode2d ? "2d" : "3d",
      generation: this.generationCounter,
      liveGeneration: entry !== null && !entry.retired && entry.scene !== null ? entry.id : null,
      initPending: entry !== null && !entry.initialized ? { generation: entry.id, retired: entry.retired } : null,
      visibility: this.visibility === null ? null : { ...this.visibility },
      reducedMotion: this.reducedMotion,
      counts: { ...this.counts },
      lossCounts: { ...this.lossCounts },
      deviceLosses: [...this.losses],
      rendererErrors: [...this.errors],
      lastAttestation: this.lastAttestation,
      frames:
        frames === null
          ? null
          : { ...frames, requestedSinceDisposal: this.framesAtDisposal === null ? null : frames.requested - this.framesAtDisposal },
      asset: { ...this.assetCounts },
      fallbackReason: this.failure,
      transitions: [...this.transitions],
    };
  }
}
