// One imperative scene controller over provider-neutral view models
// (SPEC §15.1–15.2). M0A uses a deliberately simple geometric qualification
// skin: a floor, one desk per worker, a capsule worker and a TSL-shaded
// attention marker whose colour and motion follow the projected state.

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
  Vector2,
  WebGPURenderer,
} from "three/webgpu";
import { color, float, mix, oscSine, time, uniform } from "three/tsl";

import { attest, type RendererAttestation } from "./backend";

export type WorkerVisualState = "attention" | "acknowledged" | "idle";

export interface SceneWorker {
  id: string;
  label: string;
  state: WorkerVisualState;
}

export interface SceneModel {
  workers: SceneWorker[];
  selectedId: string | null;
}

export interface OfficeSceneOptions {
  forceWebGL: boolean;
  reducedMotion: boolean;
  onSelect: (workerId: string | null) => void;
}

export interface FrameStats {
  frames: number;
  lastFrameAtMs: number;
  modelRevision: number;
}

// Quiet Editorial palette (warm paper, deep teal, lavender).
const PALETTE = {
  floor: "#EFEBE3",
  desk: "#D8D2C6",
  worker: "#195C5D",
  selected: "#379489",
  attentionA: "#C58AE5",
  attentionB: "#195C5D",
  acknowledged: "#55AAA4",
};

interface WorkerNodes {
  root: Object3D;
  body: Mesh;
  marker: Mesh;
  /** 1 = outstanding attention (pulsing), 0 = acknowledged (steady). */
  pulse: ReturnType<typeof uniform>;
}

export class OfficeScene {
  readonly attestation: RendererAttestation;
  private readonly renderer: WebGPURenderer;
  private readonly scene = new Scene();
  private readonly camera: OrthographicCamera;
  private readonly workers = new Map<string, WorkerNodes>();
  private readonly raycaster = new Raycaster();
  private readonly resizeObserver: ResizeObserver;
  private readonly stats: FrameStats = { frames: 0, lastFrameAtMs: 0, modelRevision: 0 };
  private disposed = false;

  private constructor(
    private readonly canvas: HTMLCanvasElement,
    renderer: WebGPURenderer,
    attestation: RendererAttestation,
    private readonly options: OfficeSceneOptions,
  ) {
    this.renderer = renderer;
    this.attestation = attestation;
    this.camera = new OrthographicCamera(-6, 6, 4, -4, 0.1, 100);
    this.camera.position.set(9, 9, 9);
    this.camera.lookAt(0, 0.6, 0);
    this.buildRoom();
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(canvas);
    this.resize();
    canvas.addEventListener("pointerdown", this.onPointerDown);
    void this.renderer.setAnimationLoop(() => this.frame());
  }

  /** Creates and asynchronously initializes the renderer, then attests its backend. */
  static async create(canvas: HTMLCanvasElement, options: OfficeSceneOptions): Promise<OfficeScene> {
    const started = performance.now();
    const renderer = new WebGPURenderer({ canvas, antialias: true, forceWebGL: options.forceWebGL });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5));
    await renderer.init();
    const attestation = attest(renderer, options.forceWebGL, Math.round(performance.now() - started));
    return new OfficeScene(canvas, renderer, attestation, options);
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

    const floorMaterial = new MeshStandardNodeMaterial({ roughness: 0.95 });
    floorMaterial.colorNode = color(PALETTE.floor);
    const floor = new Mesh(new PlaneGeometry(14, 10), floorMaterial);
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

    const bodyMaterial = new MeshStandardNodeMaterial({ roughness: 0.6 });
    bodyMaterial.colorNode = color(PALETTE.worker);
    const body = new Mesh(new CapsuleGeometry(0.32, 0.6, 6, 16), bodyMaterial);
    body.position.set(0, 0.62, 0.35);
    body.userData.workerId = id;
    root.add(body);

    // TSL: the marker blends between lavender and teal over time while the
    // worker needs attention; once acknowledged the pulse uniform drops to 0
    // and it settles on a steady teal.
    const pulse = uniform(1);
    const markerMaterial = new MeshStandardNodeMaterial({ roughness: 0.35, metalness: 0.1 });
    const wave = this.options.reducedMotion ? float(0.5) : oscSine(time.mul(0.6));
    markerMaterial.colorNode = mix(color(PALETTE.acknowledged), mix(color(PALETTE.attentionB), color(PALETTE.attentionA), wave), pulse);
    markerMaterial.emissiveNode = mix(color("#000000"), color(PALETTE.attentionA).mul(0.35), pulse.mul(wave));
    const marker = new Mesh(new OctahedronGeometry(0.22), markerMaterial);
    marker.position.set(0, 1.55, 0.35);
    marker.userData.workerId = id;
    root.add(marker);

    this.scene.add(root);
    return { root, body, marker, pulse };
  }

  /** Applies a projected model. Missing workers are removed and their GPU resources released. */
  update(model: SceneModel): void {
    if (this.disposed) return;
    const seen = new Set<string>();
    model.workers.forEach((worker, index) => {
      seen.add(worker.id);
      const nodes = this.workers.get(worker.id) ?? this.createWorker(worker.id);
      this.workers.set(worker.id, nodes);
      nodes.root.position.set((index - (model.workers.length - 1) / 2) * 3, 0, 0);
      nodes.marker.visible = worker.state !== "idle";
      nodes.pulse.value = worker.state === "attention" ? 1 : 0;
      const bodyMaterial = nodes.body.material as MeshStandardNodeMaterial;
      bodyMaterial.colorNode = color(worker.id === model.selectedId ? PALETTE.selected : PALETTE.worker);
      bodyMaterial.needsUpdate = true;
    });
    for (const [id, nodes] of this.workers) {
      if (!seen.has(id)) {
        this.release(nodes);
        this.workers.delete(id);
      }
    }
    this.stats.modelRevision += 1;
  }

  private release(nodes: WorkerNodes): void {
    nodes.root.traverse((object) => {
      if (object instanceof Mesh) {
        object.geometry.dispose();
        (object.material as MeshStandardNodeMaterial).dispose();
      }
    });
    this.scene.remove(nodes.root);
  }

  private frame(): void {
    if (this.disposed) return;
    if (!this.options.reducedMotion) {
      for (const nodes of this.workers.values()) {
        nodes.marker.rotation.y += 0.02;
      }
    }
    this.renderer.render(this.scene, this.camera);
    this.stats.frames += 1;
    this.stats.lastFrameAtMs = performance.now();
  }

  private resize(): void {
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

  /** Stops the loop and awaits the pinned renderer's asynchronous disposal. */
  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    this.canvas.removeEventListener("pointerdown", this.onPointerDown);
    this.resizeObserver.disconnect();
    await this.renderer.setAnimationLoop(null);
    for (const nodes of this.workers.values()) this.release(nodes);
    this.workers.clear();
    await this.renderer.dispose();
  }
}
