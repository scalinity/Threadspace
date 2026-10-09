// One imperative scene controller over provider-neutral view models
// (SPEC §15.1–15.2). A deliberately simple geometric skin: a floor, one desk
// per worker, a capsule worker whose colour, opacity and motion follow its
// projected state, and a TSL-shaded marker for owner needs. The marker
// follows open attention independently of the state, so new work never
// erases older attention (SPEC §15.3).
//
// An OfficeScene is built on an already initialized renderer and lives for
// exactly one renderer generation; `RendererLifecycle` creates, attests and
// disposes renderers (SPEC §15.6).

import {
  BoxGeometry,
  CapsuleGeometry,
  Color,
  DirectionalLight,
  HemisphereLight,
  Mesh,
  MeshStandardNodeMaterial,
  Object3D,
  OctahedronGeometry,
  OrthographicCamera,
  PlaneGeometry,
  Raycaster,
  Scene,
  type Texture,
  Vector2,
  type WebGPURenderer,
} from "three/webgpu";
import { color, float, mix, oscSine, texture, time, uniform, uv, vec2 } from "three/tsl";

import type { LifecycleScene, SceneAsset } from "./lifecycle";

export type WorkerVisualState = "working" | "waiting" | "attention" | "acknowledged" | "idle" | "ended" | "stale";

/** Open attention at the worker's desk: some needing attention, only items awaiting action, or none. */
export type WorkerAttention = "needs" | "awaiting" | "none";

export interface SceneWorker {
  id: string;
  label: string;
  state: WorkerVisualState;
  attention: WorkerAttention;
}

export interface SceneModel {
  workers: SceneWorker[];
  selectedId: string | null;
}

export interface OfficeSceneOptions {
  reducedMotion: boolean;
  onSelect: (workerId: string | null) => void;
}

export interface FrameStats {
  frames: number;
  lastFrameAtMs: number;
  modelRevision: number;
}

/** The bundled floor-grain texture, owned by one renderer generation. */
export interface FloorTexture extends SceneAsset {
  readonly texture: Texture;
}

// Quiet Editorial palette (warm paper, deep teal, lavender).
const PALETTE = {
  floor: "#EFEBE3",
  desk: "#D8D2C6",
  working: "#195C5D",
  settled: "#379489",
  muted: "#736E67",
  selectedGlow: "#C58AE5",
  attentionA: "#C58AE5",
  attentionB: "#195C5D",
  acknowledged: "#55AAA4",
};

/**
 * The body carries activity and presence. Working is deep teal (and moves
 * unless motion is reduced); a provider wait is lavender; the settled states
 * share the lighter teal and differ by marker; a stale worker is muted, and
 * an ended one is a faint, still selectable trace at its desk.
 */
const BODY: Record<WorkerVisualState, { color: string; opacity: number }> = {
  working: { color: PALETTE.working, opacity: 1 },
  waiting: { color: PALETTE.attentionA, opacity: 1 },
  attention: { color: PALETTE.settled, opacity: 1 },
  acknowledged: { color: PALETTE.settled, opacity: 1 },
  idle: { color: PALETTE.settled, opacity: 1 },
  stale: { color: PALETTE.muted, opacity: 0.7 },
  ended: { color: PALETTE.muted, opacity: 0.3 },
};

const BODY_Y = 0.62;

interface WorkerNodes {
  root: Object3D;
  body: Mesh;
  marker: Mesh;
  state: WorkerVisualState;
  bodyColor: ReturnType<typeof uniform>;
  /** 1 = selected (lavender glow), 0 = not. */
  selected: ReturnType<typeof uniform>;
  /** 1 = outstanding attention (pulsing), 0 = acknowledged (steady). */
  pulse: ReturnType<typeof uniform>;
  /** 1 = the provider is waiting on the owner (steady lavender call). */
  call: ReturnType<typeof uniform>;
}

export class OfficeScene implements LifecycleScene<FloorTexture> {
  private readonly renderer: WebGPURenderer;
  private readonly canvas: HTMLCanvasElement;
  private readonly scene = new Scene();
  private readonly camera: OrthographicCamera;
  private readonly workers = new Map<string, WorkerNodes>();
  private readonly raycaster = new Raycaster();
  private readonly resizeObserver: ResizeObserver;
  private readonly stats: FrameStats = { frames: 0, lastFrameAtMs: 0, modelRevision: 0 };
  /** 1 = idle motion on, 0 = reduced motion. Shared by every marker's TSL graph. */
  private readonly motion = uniform(1);
  private readonly floorMaterial = new MeshStandardNodeMaterial({ roughness: 0.95 });
  private reducedMotion: boolean;
  private released = false;

  /** Builds the room on an initialized renderer and starts its application frame callback. */
  constructor(
    renderer: WebGPURenderer,
    private readonly options: OfficeSceneOptions,
  ) {
    this.renderer = renderer;
    this.canvas = renderer.domElement;
    this.reducedMotion = options.reducedMotion;
    this.motion.value = options.reducedMotion ? 0 : 1;
    this.camera = new OrthographicCamera(-6, 6, 4, -4, 0.1, 100);
    this.camera.position.set(9, 9, 9);
    this.camera.lookAt(0, 0.6, 0);
    this.buildRoom();
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(this.canvas);
    this.resize();
    this.canvas.addEventListener("pointerdown", this.onPointerDown);
    void this.renderer.setAnimationLoop(() => this.frame());
  }

  get frameStats(): FrameStats {
    return { ...this.stats };
  }

  private buildRoom(): void {
    this.scene.background = new Color(PALETTE.floor);
    this.scene.add(new HemisphereLight(0xfcfcfb, 0xd8d2c6, 2.2));
    const sun = new DirectionalLight(0xffffff, 1.6);
    sun.position.set(5, 10, 4);
    this.scene.add(sun);

    this.floorMaterial.colorNode = color(PALETTE.floor);
    const floor = new Mesh(new PlaneGeometry(14, 10), this.floorMaterial);
    floor.rotation.x = -Math.PI / 2;
    this.scene.add(floor);
  }

  private createWorker(id: string): WorkerNodes {
    const root = new Object3D();
    root.name = id;

    const deskMaterial = new MeshStandardNodeMaterial({ roughness: 0.8 });
    deskMaterial.colorNode = color(PALETTE.desk);
    const desk = new Mesh(new BoxGeometry(1.8, 0.8, 1.0), deskMaterial);
    desk.position.set(0, 0.4, -0.7);
    root.add(desk);

    // Body colour and selection are uniforms, so a state change never
    // recompiles the material.
    const bodyColor = uniform(new Color(PALETTE.settled));
    const selected = uniform(0);
    const bodyMaterial = new MeshStandardNodeMaterial({ roughness: 0.6, transparent: true });
    bodyMaterial.colorNode = bodyColor;
    bodyMaterial.emissiveNode = color(PALETTE.selectedGlow).mul(selected.mul(0.4));
    const body = new Mesh(new CapsuleGeometry(0.32, 0.6, 6, 16), bodyMaterial);
    body.position.set(0, BODY_Y, 0.35);
    body.userData.workerId = id;
    root.add(body);

    // TSL: the marker blends between lavender and teal over time while the
    // worker needs attention; once acknowledged the pulse uniform drops to 0
    // and it settles on a steady teal. A provider wait overrides both with a
    // steady, larger lavender call. Reduced motion holds the blend still while
    // keeping the three colours distinct.
    const pulse = uniform(1);
    const call = uniform(0);
    const markerMaterial = new MeshStandardNodeMaterial({ roughness: 0.35, metalness: 0.1 });
    const wave = mix(float(0.5), oscSine(time.mul(0.6)), this.motion);
    const attentionColor = mix(color(PALETTE.acknowledged), mix(color(PALETTE.attentionB), color(PALETTE.attentionA), wave), pulse);
    markerMaterial.colorNode = mix(attentionColor, color(PALETTE.attentionA), call);
    markerMaterial.emissiveNode = color(PALETTE.attentionA).mul(pulse.mul(wave).add(call.mul(0.5)).mul(0.35));
    const marker = new Mesh(new OctahedronGeometry(0.22), markerMaterial);
    marker.position.set(0, 1.55, 0.35);
    marker.userData.workerId = id;
    root.add(marker);

    this.scene.add(root);
    return { root, body, marker, state: "idle", bodyColor, selected, pulse, call };
  }

  /** Applies a projected model. Missing workers are removed and their GPU resources released. */
  update(model: SceneModel): void {
    if (this.released) return;
    const seen = new Set<string>();
    model.workers.forEach((worker, index) => {
      seen.add(worker.id);
      const nodes = this.workers.get(worker.id) ?? this.createWorker(worker.id);
      this.workers.set(worker.id, nodes);
      nodes.root.position.set((index - (model.workers.length - 1) / 2) * 3, 0, 0);
      nodes.state = worker.state;
      const body = BODY[worker.state];
      (nodes.bodyColor.value as Color).set(body.color);
      (nodes.body.material as MeshStandardNodeMaterial).opacity = body.opacity;
      nodes.selected.value = worker.id === model.selectedId ? 1 : 0;
      if (worker.state !== "working") nodes.body.position.y = BODY_Y;
      const waiting = worker.state === "waiting";
      nodes.marker.visible = waiting || worker.attention !== "none";
      nodes.marker.scale.setScalar(waiting ? 1.35 : 1);
      nodes.call.value = waiting ? 1 : 0;
      nodes.pulse.value = worker.attention === "needs" ? 1 : 0;
    });
    for (const [id, nodes] of this.workers) {
      if (!seen.has(id)) {
        this.releaseObject(nodes.root);
        this.scene.remove(nodes.root);
        this.workers.delete(id);
      }
    }
    this.stats.modelRevision += 1;
  }

  /** Stops idle motion (marker spin, pulse and work motion) without changing any state-driven appearance. */
  setReducedMotion(reduced: boolean): void {
    this.reducedMotion = reduced;
    this.motion.value = reduced ? 0 : 1;
    if (reduced) {
      for (const nodes of this.workers.values()) nodes.body.position.y = BODY_Y;
    }
  }

  /** Adds the bundled paper grain to the floor. The texture stays owned by its generation's asset. */
  applyAsset(asset: FloorTexture): void {
    if (this.released) return;
    const grain = texture(asset.texture, uv().mul(vec2(7, 5))).r;
    this.floorMaterial.colorNode = color(PALETTE.floor).mul(grain.mul(0.12).add(0.88));
    this.floorMaterial.needsUpdate = true;
  }

  private releaseObject(root: Object3D): void {
    root.traverse((object) => {
      if (object instanceof Mesh) {
        object.geometry.dispose();
        (object.material as MeshStandardNodeMaterial).dispose();
      }
    });
  }

  private frame(): void {
    if (this.released) return;
    if (!this.reducedMotion) {
      const seconds = performance.now() / 1000;
      for (const nodes of this.workers.values()) {
        // Uncertain and historical workers stay still (SPEC §15.3).
        if (nodes.state !== "stale" && nodes.state !== "ended") nodes.marker.rotation.y += 0.02;
        if (nodes.state === "working") nodes.body.position.y = BODY_Y + 0.05 * Math.sin(seconds * 4);
      }
    }
    this.renderer.render(this.scene, this.camera);
    this.stats.frames += 1;
    this.stats.lastFrameAtMs = performance.now();
  }

  private resize(): void {
    if (this.released) return;
    const width = Math.max(1, this.canvas.clientWidth);
    const height = Math.max(1, this.canvas.clientHeight);
    const aspect = width / height;
    const halfHeight = 3.2;
    this.camera.left = -halfHeight * aspect;
    this.camera.right = halfHeight * aspect;
    this.camera.top = halfHeight;
    this.camera.bottom = -halfHeight;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(width, height, false);
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    const bounds = this.canvas.getBoundingClientRect();
    const pointer = new Vector2(
      ((event.clientX - bounds.left) / bounds.width) * 2 - 1,
      -((event.clientY - bounds.top) / bounds.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(pointer, this.camera);
    const hit = this.raycaster.intersectObjects([...this.workers.values()].flatMap((nodes) => [nodes.body, nodes.marker]))[0];
    const workerId = hit?.object.userData.workerId;
    this.options.onSelect(typeof workerId === "string" ? workerId : null);
  };

  /**
   * Clears the application frame callback and releases this scene's GPU
   * resources. The internal Three loop keeps running until the lifecycle
   * awaits `renderer.dispose()`.
   */
  async release(): Promise<void> {
    if (this.released) return;
    this.released = true;
    this.canvas.removeEventListener("pointerdown", this.onPointerDown);
    this.resizeObserver.disconnect();
    await this.renderer.setAnimationLoop(null);
    this.releaseObject(this.scene);
    this.workers.clear();
    this.scene.clear();
  }
}
