// The renderer side of the bridge (SPEC §18.4–18.5). It installs the Channel
// handler before `ui_connect`, requires contiguous stream sequences, stages a
// snapshot and replaces the projection atomically at SnapshotEnd before
// acknowledging it, applies full-entity patches, and serializes ACKs. A
// stalled or invalid stream is discarded for a fresh view epoch; it is never
// repaired in place.

import { Channel } from "@tauri-apps/api/core";

import type { AttentionCounts } from "../contracts/generated/AttentionCounts";
import type { AttentionView } from "../contracts/generated/AttentionView";
import type { DiagnosticsReport } from "../contracts/generated/DiagnosticsReport";
import type { DiscoverySummary } from "../contracts/generated/DiscoverySummary";
import type { FrameHeader } from "../contracts/generated/FrameHeader";
import type { IntegrationReport } from "../contracts/generated/IntegrationReport";
import type { NativeIntent } from "../contracts/generated/NativeIntent";
import type { ProjectionPatch } from "../contracts/generated/ProjectionPatch";
import type { RouteResult } from "../contracts/generated/RouteResult";
import type { SessionView } from "../contracts/generated/SessionView";
import type { UiAction } from "../contracts/generated/UiAction";
import type { UiActionResult } from "../contracts/generated/UiActionResult";
import type { UiCallContext } from "../contracts/generated/UiCallContext";
import type { UiQuery } from "../contracts/generated/UiQuery";
import type { UiQueryResult } from "../contracts/generated/UiQueryResult";
import type { RendererAttestation } from "@threadspace/scene";
import { launch } from "../launch";
import { BridgeError, ipc, normalizeFailure } from "./ipc";
import { UI_PROTOCOL_VERSION, ValidationError, compareCursors, parseFrame, parsePage, parseSnapshot } from "./validate";
import type { RouteSummary } from "../contracts/generated/RouteSummary";

export type Phase = "connecting" | "hydrating" | "live" | "unavailable";

export interface Inspector {
  attentionId: string | null;
  sessionId: string;
  source: "NOTIFICATION_RESPONSE" | "SELECTION";
  outstandingAtOpen: boolean | null;
  intentId: string | null;
  openedAtMs: number;
  /** The Return the companion attempted for a notification response. */
  route: RouteSummary | null;
  /** False when the response arrived with observation disabled or in maintenance. */
  observationEnabled: boolean | null;
}

/** Bounded-view paging state (SPEC §18.4). */
export interface Paging {
  complete: boolean;
  totalSessions: number;
  totalAttention: number;
  pagesLoaded: number;
  staleRepliesIgnored: number;
  invalidations: number;
}

export type QualificationHandler = (command: string, args: unknown) => Promise<unknown> | unknown;

export interface ConnectionInfo {
  subscriptionId: string;
  viewEpoch: string;
  coreGeneration: string;
  storeGeneration: string;
}

export interface StreamStats {
  lastSeq: number;
  framesApplied: number;
  patchesApplied: number;
  snapshotsApplied: number;
  intentsApplied: number;
  lastFrameAtMs: number;
  hydratedAtMs: number | null;
  connects: number;
}

export interface ViewState {
  phase: Phase;
  detail: string | null;
  connection: ConnectionInfo | null;
  cursor: string;
  viewRevision: string | null;
  sessions: SessionView[];
  attention: AttentionView[];
  counts: AttentionCounts;
  inspector: Inspector | null;
  stream: StreamStats;
  diagnostics: DiagnosticsReport | null;
  integration: IntegrationReport | null;
  renderer: RendererAttestation | null;
  rendererError: string | null;
  notices: string[];
  /** Latest Return result per session, with its evidence chain. */
  routes: Record<string, RouteResult>;
  /** Session whose Return is in flight. */
  routing: string | null;
  discovery: DiscoverySummary | null;
  paging: Paging;
  /** Times the view revalidated after timers were suspended (sleep, hidden). */
  resumes: number;
}

const HYDRATION_DEADLINE_MS = 5_000;
const STALL_MS = 5_000;
/** A watchdog tick this late means timers were suspended (sleep or hidden). */
const SUSPENSION_GAP_MS = 10_000;
const PAGE_LIMIT = 200;
const MAX_NOTICES = 6;

interface Staging {
  viewRevision: string;
  cursor: string;
  chunkCount: number;
  totalBytes: number;
  chunks: string[];
}

interface Active {
  attempt: number;
  viewEpoch: string;
  subscriptionId: string | null;
  channel: Channel<unknown>;
  startedAtMs: number;
  hydrated: boolean;
  /** Snapshot applied and connect reply received; hooks have run. */
  announced: boolean;
  expectedSeq: number;
  staging: Staging | null;
}

const initialState: ViewState = {
  phase: "connecting",
  detail: null,
  connection: null,
  cursor: "0",
  viewRevision: null,
  sessions: [],
  attention: [],
  counts: { needsAttention: 0, awaitingAction: 0 },
  inspector: null,
  stream: {
    lastSeq: 0,
    framesApplied: 0,
    patchesApplied: 0,
    snapshotsApplied: 0,
    intentsApplied: 0,
    lastFrameAtMs: 0,
    hydratedAtMs: null,
    connects: 0,
  },
  diagnostics: null,
  integration: null,
  renderer: null,
  rendererError: null,
  notices: [],
  routes: {},
  routing: null,
  discovery: null,
  paging: { complete: true, totalSessions: 0, totalAttention: 0, pagesLoaded: 0, staleRepliesIgnored: 0, invalidations: 0 },
  resumes: 0,
};

function upsert<T>(list: T[], items: T[], key: (item: T) => string): T[] {
  if (items.length === 0) return list;
  const replacements = new Map(items.map((item) => [key(item), item]));
  const next = list.map((item) => replacements.get(key(item)) ?? item);
  for (const [id, item] of replacements) {
    if (!list.some((existing) => key(existing) === id)) next.push(item);
  }
  return next;
}

export type HydrationListener = (client: BridgeClient) => void;

export class BridgeClient {
  private state: ViewState = initialState;
  private readonly listeners = new Set<() => void>();
  private readonly hydrationListeners: HydrationListener[] = [];
  private active: Active | null = null;
  private attempts = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelayMs = 1_000;
  private ackInFlight = false;
  private ackedThrough = 0;
  private ackPending: FrameHeader | null = null;
  private readonly pendingReports: Array<{ kind: string; report: unknown }> = [];
  private lastWatchdogMs = Date.now();
  /**
   * Qualification-only stream faults (inert unless `launch.qualificationBuild`):
   * drop frames as though lost in transit, withhold or delay ACKs.
   */
  readonly faults = { dropFrames: "none" as "none" | "next" | "all", withholdAcks: false, ackDelayMs: 0, stallNext: false, dropped: 0 };
  private qualificationHandler: QualificationHandler | null = null;
  private pagingEpoch: string | null = null;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  readonly getSnapshot = (): ViewState => this.state;

  private set(partial: Partial<ViewState>): void {
    this.state = { ...this.state, ...partial };
    for (const listener of this.listeners) listener();
  }

  notice(message: string): void {
    const stamp = new Date().toLocaleTimeString();
    this.set({ notices: [`${stamp}  ${message}`, ...this.state.notices].slice(0, MAX_NOTICES) });
  }

  /** Called once at startup, outside React. */
  start(): void {
    setInterval(() => this.watchdog(), 1_000);
    // Hidden→visible: browser timers may have been suspended, so validate
    // the stream explicitly instead of trusting it (SPEC §18.5).
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") void this.revalidate("view became visible");
    });
    void this.connect("initial connection");
  }

  /** Qualification builds only: resubscribe as after a detected stream fault. */
  qualificationReconnect(reason: string): void {
    if (launch.qualificationBuild) this.reconnect(reason);
  }

  /** Qualification builds only: receives QualificationCommand intents. */
  setQualificationHandler(handler: QualificationHandler): void {
    this.qualificationHandler = handler;
  }

  private async revalidate(reason: string): Promise<void> {
    this.set({ resumes: this.state.resumes + 1 });
    const active = this.active;
    if (!active) return;
    if (!active.hydrated || performance.now() - this.state.stream.lastFrameAtMs > STALL_MS) {
      const status = await ipc
        .query({ query: { kind: "ConnectionStatus" }, context: null })
        .then((result) => (result.kind === "ConnectionStatus" ? result.companion.state : "unknown"))
        .catch(() => "query failed");
      if (this.active === active) this.reconnect(`${reason}; stream stale (companion ${status})`);
    }
  }

  onHydrated(listener: HydrationListener): void {
    this.hydrationListeners.push(listener);
  }

  // ------------------------------------------------------------ connection

  private teardown(): void {
    const previous = this.active;
    this.active = null;
    this.ackPending = null;
    if (!previous) return;
    previous.channel.onmessage = () => {};
    if (previous.subscriptionId) {
      void ipc.disconnect({ subscriptionId: previous.subscriptionId, viewEpoch: previous.viewEpoch }).catch(() => {});
    }
  }

  private async connect(reason: string): Promise<void> {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.teardown();
    this.ackedThrough = 0;
    // An injected stream fault belongs to the stream it broke; a new Channel
    // is a new stream. `stallNext` withholds ACKs on exactly one new stream.
    this.faults.dropFrames = "none";
    this.faults.withholdAcks = this.faults.stallNext;
    this.faults.stallNext = false;
    const attempt = ++this.attempts;
    const viewEpoch = crypto.randomUUID();
    const channel = new Channel<unknown>();
    // The handler exists before the command runs: frames may precede its reply.
    channel.onmessage = (raw) => this.onFrame(attempt, raw);
    this.active = {
      attempt,
      viewEpoch,
      subscriptionId: null,
      channel,
      startedAtMs: performance.now(),
      hydrated: false,
      announced: false,
      expectedSeq: 1,
      staging: null,
    };
    this.set({ phase: "connecting", detail: reason, stream: { ...this.state.stream, connects: this.state.stream.connects + 1 } });
    try {
      const reply = await ipc.connect({ protocolVersion: UI_PROTOCOL_VERSION, viewEpoch }, channel);
      if (this.active?.attempt !== attempt) return;
      this.active.subscriptionId = reply.subscriptionId;
      this.reconnectDelayMs = 1_000;
      this.set({
        connection: {
          subscriptionId: reply.subscriptionId,
          viewEpoch: reply.viewEpoch,
          coreGeneration: reply.coreGeneration,
          storeGeneration: reply.storeGeneration,
        },
        phase: "hydrating",
      });
      this.announceIfReady(this.active);
    } catch (error) {
      if (this.active?.attempt !== attempt) return;
      const failure = error instanceof BridgeError ? error.failure : normalizeFailure(error);
      this.teardown();
      this.set({ phase: "unavailable", detail: `${failure.code}: ${failure.detail}`, connection: null });
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    const delay = this.reconnectDelayMs;
    this.reconnectDelayMs = Math.min(this.reconnectDelayMs * 2, 10_000);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.connect("retrying after companion unavailable");
    }, delay);
  }

  private reconnect(reason: string): void {
    this.notice(`Stream reset: ${reason}`);
    void this.connect(reason);
  }

  private watchdog(): void {
    const wall = Date.now();
    const suspended = wall - this.lastWatchdogMs > SUSPENSION_GAP_MS;
    this.lastWatchdogMs = wall;
    if (suspended) {
      // Sleep/wake or a long hidden period: never compare stale timers.
      void this.revalidate("timers resumed after suspension");
      return;
    }
    const active = this.active;
    if (!active) return;
    const now = performance.now();
    if (!active.hydrated && now - active.startedAtMs > HYDRATION_DEADLINE_MS) {
      this.reconnect("hydration deadline exceeded");
      return;
    }
    // The bridge heartbeat runs only while the office is visible.
    if (document.visibilityState !== "visible") return;
    if (active.hydrated && now - this.state.stream.lastFrameAtMs > STALL_MS) {
      // Independent status check: a stalled Channel cannot deliver its own reset.
      void ipc
        .query({ query: { kind: "ConnectionStatus" }, context: null })
        .then((result) => (result.kind === "ConnectionStatus" ? result.companion.state : "unknown"))
        .catch(() => "query failed")
        .then((status) => {
          if (this.active === active) this.reconnect(`no stream progress for 5 s (companion ${status})`);
        });
    }
  }

  // ---------------------------------------------------------------- frames

  private onFrame(attempt: number, raw: unknown): void {
    const active = this.active;
    if (!active || active.attempt !== attempt) return;
    if (launch.qualificationBuild && this.faults.dropFrames !== "none") {
      if (this.faults.dropFrames === "next") this.faults.dropFrames = "none";
      this.faults.dropped += 1;
      return;
    }
    let frame;
    try {
      frame = parseFrame(raw);
    } catch (error) {
      this.reconnect(error instanceof ValidationError ? `invalid frame (${error.message})` : "invalid frame");
      return;
    }
    const header = frame.header;
    if (header.viewEpoch !== active.viewEpoch) {
      this.reconnect("frame for another view epoch");
      return;
    }
    if (active.subscriptionId !== null && header.subscriptionId !== active.subscriptionId) {
      this.reconnect("frame for another subscription");
      return;
    }
    active.subscriptionId ??= header.subscriptionId;
    if (header.streamSeq !== active.expectedSeq) {
      this.reconnect(`stream gap: expected ${active.expectedSeq}, received ${header.streamSeq}`);
      return;
    }
    active.expectedSeq += 1;
    const stream = { ...this.state.stream, lastSeq: header.streamSeq, lastFrameAtMs: performance.now(), framesApplied: this.state.stream.framesApplied + 1 };
    const body = frame.body;
    switch (body.kind) {
      case "SnapshotBegin":
        if (active.hydrated || active.staging) return this.reconnect("unexpected snapshot");
        active.staging = { viewRevision: body.viewRevision, cursor: header.cursor, chunkCount: body.chunkCount, totalBytes: body.totalBytes, chunks: [] };
        this.set({ stream });
        return;
      case "SnapshotChunk":
        if (!active.staging || body.index !== active.staging.chunks.length || header.cursor !== active.staging.cursor) {
          return this.reconnect("snapshot chunk out of order");
        }
        active.staging.chunks.push(body.data);
        this.set({ stream });
        return;
      case "SnapshotEnd":
        return this.applySnapshot(active, header, stream);
      case "ProjectionPatch":
        if (!active.hydrated) return this.reconnect("patch before snapshot");
        return this.applyPatch(header, body.patch, stream);
      case "BridgeHeartbeat":
        if (!active.hydrated) return this.reconnect("heartbeat before snapshot");
        this.set({ stream });
        this.ackFrame(header);
        return;
      case "NativeIntent":
        if (!active.hydrated) return this.reconnect("intent before snapshot");
        return this.applyIntent(header, body.intent, stream);
    }
  }

  private applySnapshot(active: Active, header: FrameHeader, stream: StreamStats): void {
    const staging = active.staging;
    if (!staging || staging.chunks.length !== staging.chunkCount || header.cursor !== staging.cursor) {
      this.reconnect("incomplete snapshot");
      return;
    }
    const json = staging.chunks.join("");
    if (new TextEncoder().encode(json).length !== staging.totalBytes) {
      this.reconnect("snapshot byte count mismatch");
      return;
    }
    let snapshot;
    try {
      snapshot = parseSnapshot(JSON.parse(json));
    } catch (error) {
      this.reconnect(`invalid snapshot (${error instanceof Error ? error.message : "parse"})`);
      return;
    }
    active.staging = null;
    active.hydrated = true;
    const now = performance.now();
    // Atomic replacement of the renderer projection, then the ACK.
    this.set({
      detail: null,
      cursor: header.cursor,
      viewRevision: snapshot.viewRevision,
      sessions: snapshot.sessions,
      attention: snapshot.attention,
      counts: snapshot.counts,
      stream: { ...stream, snapshotsApplied: stream.snapshotsApplied + 1, hydratedAtMs: now },
      paging: {
        ...this.state.paging,
        complete: snapshot.complete,
        totalSessions: snapshot.totalSessions,
        totalAttention: snapshot.totalAttention,
      },
    });
    this.ackFrame(header);
    this.announceIfReady(active);
    if (!snapshot.complete) {
      this.pagingEpoch = active.viewEpoch;
      void this.pageFrom("FleetPage", snapshot.sessionsAfter);
      void this.pageFrom("AttentionPage", snapshot.attentionAfter);
    }
  }

  /**
   * Reads the rest of a bounded view. A reply for an older view epoch is
   * ignored, and a page row never replaces a newer row this view already
   * holds (SPEC §18.4).
   */
  private async pageFrom(kind: "FleetPage" | "AttentionPage", after: string | null): Promise<void> {
    const epoch = this.active?.viewEpoch;
    let next = after;
    while (epoch && this.active?.viewEpoch === epoch && this.pagingEpoch === epoch) {
      let raw: unknown;
      try {
        raw = await ipc.query({ query: { kind, after: next, limit: PAGE_LIMIT }, context: this.context() });
      } catch (error) {
        this.notice(`Paging ${kind} failed: ${error instanceof Error ? error.message : String(error)}`);
        return;
      }
      if (this.active?.viewEpoch !== epoch) {
        this.set({ paging: { ...this.state.paging, staleRepliesIgnored: this.state.paging.staleRepliesIgnored + 1 } });
        return;
      }
      const parsed = parsePage(raw);
      const newer = <T extends { revision: string }>(current: T | undefined, row: T) =>
        current === undefined || compareCursors(row.revision, current.revision) >= 0;
      if (parsed.kind === "FleetPage") {
        const held = new Map(this.state.sessions.map((row) => [row.sessionId, row]));
        const rows = parsed.page.rows.filter((row) => newer(held.get(row.sessionId), row));
        this.set({ sessions: upsert(this.state.sessions, rows, (row) => row.sessionId) });
      } else {
        const held = new Map(this.state.attention.map((row) => [row.attentionId, row]));
        const rows = parsed.page.rows.filter((row) => newer(held.get(row.attentionId), row));
        this.set({ attention: upsert(this.state.attention, rows, (row) => row.attentionId) });
      }
      this.set({ paging: { ...this.state.paging, pagesLoaded: this.state.paging.pagesLoaded + 1 } });
      next = parsed.page.nextAfter;
      if (next === null) {
        this.recordQualificationReport("paging-complete", {
          kind,
          viewEpoch: epoch,
          sessionsHeld: this.state.sessions.length,
          attentionHeld: this.state.attention.length,
          paging: this.state.paging,
        });
        return;
      }
    }
  }

  /** Frames may precede the connect reply; the view is live once both exist. */
  private announceIfReady(active: Active): void {
    if (this.active !== active || active.announced || !active.hydrated || this.state.connection?.viewEpoch !== active.viewEpoch) {
      return;
    }
    active.announced = true;
    this.set({ phase: "live" });
    for (const listener of this.hydrationListeners) listener(this);
    void this.flushReports();
  }

  private applyPatch(header: FrameHeader, patch: ProjectionPatch, stream: StreamStats): void {
    if (patch.fromCursor !== this.state.cursor || compareCursors(patch.toCursor, this.state.cursor) <= 0 || patch.toCursor !== header.cursor) {
      this.reconnect(`patch ${patch.fromCursor}→${patch.toCursor} does not follow cursor ${this.state.cursor}`);
      return;
    }
    const removedSessions = new Set(patch.tombstones.filter((t) => t.entity === "SESSION").map((t) => t.id));
    const removedAttention = new Set(patch.tombstones.filter((t) => t.entity === "ATTENTION").map((t) => t.id));
    this.set({
      cursor: patch.toCursor,
      viewRevision: patch.viewRevision,
      sessions: upsert(this.state.sessions, patch.sessionUpserts, (s) => s.sessionId).filter((s) => !removedSessions.has(s.sessionId)),
      attention: upsert(this.state.attention, patch.attentionUpserts, (a) => a.attentionId).filter((a) => !removedAttention.has(a.attentionId)),
      counts: patch.counts,
      stream: { ...stream, patchesApplied: stream.patchesApplied + 1 },
    });
    this.ackFrame(header);
    if (patch.pageInvalidations.length > 0 && this.active) {
      // The change was too large for entity upserts: re-read affected lists.
      this.set({ paging: { ...this.state.paging, complete: false, invalidations: this.state.paging.invalidations + 1 } });
      this.pagingEpoch = this.active.viewEpoch;
      if (patch.pageInvalidations.includes("SESSION")) void this.pageFrom("FleetPage", null);
      if (patch.pageInvalidations.includes("ATTENTION")) void this.pageFrom("AttentionPage", null);
    }
  }

  private applyIntent(header: FrameHeader, intent: NativeIntent, stream: StreamStats): void {
    const action = intent.action;
    const appliedAtMs = performance.now();
    if (action.kind === "QualificationCommand") {
      this.set({ stream: { ...stream, intentsApplied: stream.intentsApplied + 1 } });
      this.ackFrame(header);
      // Release views never act on qualification commands.
      if (!launch.qualificationBuild || !this.qualificationHandler) return;
      void Promise.resolve()
        .then(() => this.qualificationHandler?.(action.command, action.args))
        .then(
          (result) => this.recordQualificationReport("qualification-command", { intentId: intent.intentId, command: action.command, ok: true, result }),
          (error: unknown) =>
            this.recordQualificationReport("qualification-command", {
              intentId: intent.intentId,
              command: action.command,
              ok: false,
              error: error instanceof Error ? error.message : String(error),
            }),
        );
      return;
    }
    this.set({
      inspector: {
        attentionId: action.attentionId,
        sessionId: action.sessionId,
        source: action.source,
        outstandingAtOpen: action.outstanding,
        intentId: intent.intentId,
        openedAtMs: Date.now(),
        route: action.route,
        observationEnabled: action.observationEnabled,
      },
      stream: { ...stream, intentsApplied: stream.intentsApplied + 1 },
    });
    const routed = action.route ? ` — Return ${action.route.surfaceResult}/${action.route.reasonCode}` : "";
    this.notice(`Opened from notification: attention ${action.attentionId.slice(0, 8)}${routed}`);
    this.ackFrame(header);
    this.recordQualificationReport("notification-intent", {
      intentId: intent.intentId,
      attentionId: action.attentionId,
      sessionId: action.sessionId,
      outstanding: action.outstanding,
      source: action.source,
      route: action.route,
      observationEnabled: action.observationEnabled,
      streamSeq: header.streamSeq,
      cursor: header.cursor,
      hydratedAtMs: this.state.stream.hydratedAtMs,
      appliedAtMs,
      appliedAfterHydration: this.state.stream.hydratedAtMs !== null && appliedAtMs >= this.state.stream.hydratedAtMs,
      inspectorAttentionPresent: this.state.attention.some((item) => item.attentionId === action.attentionId),
      inspectorItemAcknowledged: this.state.attention.find((item) => item.attentionId === action.attentionId)?.acknowledgedAtMs ?? null,
    });
  }

  /** One ACK in flight; later ACKs coalesce (an ACK releases everything up to it). */
  private ackFrame(header: FrameHeader): void {
    if (launch.qualificationBuild && this.faults.withholdAcks) return;
    if (launch.qualificationBuild && this.faults.ackDelayMs > 0) {
      setTimeout(() => {
        // A later frame's ACK may already have covered this one.
        if (header.viewEpoch !== this.active?.viewEpoch || header.streamSeq <= this.ackedThrough) return;
        if (this.ackPending && this.ackPending.streamSeq >= header.streamSeq) return;
        this.ackPending = header;
        if (!this.ackInFlight) void this.flushAcks();
      }, this.faults.ackDelayMs);
      return;
    }
    this.ackPending = header;
    if (!this.ackInFlight) void this.flushAcks();
  }

  private async flushAcks(): Promise<void> {
    this.ackInFlight = true;
    try {
      while (this.ackPending) {
        const header = this.ackPending;
        this.ackPending = null;
        if (!this.active || header.viewEpoch !== this.active.viewEpoch) continue;
        try {
          await ipc.ack({
            subscriptionId: header.subscriptionId,
            viewEpoch: header.viewEpoch,
            highestAppliedStreamSeq: header.streamSeq,
            appliedJournalCursor: header.cursor,
          });
          this.ackedThrough = header.streamSeq;
        } catch (error) {
          const failure = error instanceof BridgeError ? error.failure : normalizeFailure(error);
          if (this.active && header.viewEpoch === this.active.viewEpoch) {
            this.reconnect(`ACK refused (${failure.code})`);
          }
        }
      }
    } finally {
      this.ackInFlight = false;
    }
  }

  // ---------------------------------------------------------- operations

  context(): UiCallContext {
    const connection = this.state.connection;
    if (!connection || !this.active?.hydrated || connection.viewEpoch !== this.active.viewEpoch) {
      throw new Error("not connected");
    }
    return {
      subscriptionId: connection.subscriptionId,
      viewEpoch: connection.viewEpoch,
      coreGeneration: connection.coreGeneration,
      storeGeneration: connection.storeGeneration,
    };
  }

  isLive(): boolean {
    return this.state.phase === "live" && this.active?.announced === true;
  }

  async query(query: UiQuery): Promise<UiQueryResult> {
    return ipc.query({ query, context: this.context() });
  }

  async action(action: UiAction, expectedRevision: string | null = null, requestId = crypto.randomUUID()): Promise<UiActionResult> {
    return ipc.action({ action, expectedRevision, requestId, context: this.context() });
  }

  async acknowledge(attentionId: string): Promise<void> {
    const item = this.state.attention.find((entry) => entry.attentionId === attentionId);
    try {
      const result = await this.action({ kind: "AcknowledgeAttention", attentionId }, item?.revision ?? null);
      if (result.kind === "CommandCommitted") {
        this.notice(`Acknowledged ${attentionId.slice(0, 8)} — ${result.receipt.status} at cursor ${result.receipt.cursor}`);
      }
    } catch (error) {
      this.notice(`Acknowledge failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  /** "Mark handled": explicit resolution with a reason (SPEC §7.2). */
  async resolve(attentionId: string, reason: string): Promise<void> {
    const item = this.state.attention.find((entry) => entry.attentionId === attentionId);
    try {
      const result = await this.action({ kind: "ResolveAttention", attentionId, reason }, item?.revision ?? null);
      if (result.kind === "CommandCommitted") {
        this.notice(`Marked handled ${attentionId.slice(0, 8)} — ${result.receipt.status}`);
      }
    } catch (error) {
      this.notice(`Mark handled failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  /** Enable/Stop Observation run the native bootstrap ordering (SPEC §19.5). */
  async setObservation(enabled: boolean): Promise<void> {
    const connection = this.state.connection;
    const context = connection ?? { subscriptionId: crypto.randomUUID(), viewEpoch: crypto.randomUUID(), coreGeneration: "", storeGeneration: "" };
    try {
      const result = await ipc.action({
        action: enabled ? { kind: "EnableObservation" } : { kind: "StopObservation" },
        expectedRevision: null,
        requestId: crypto.randomUUID(),
        context,
      });
      if (result.kind === "ObservationChanged") this.notice(`Observation ${result.enabled ? "enabled" : "stopped"} (${result.service.status})`);
    } catch (error) {
      this.notice(`${enabled ? "Enable" : "Stop"} observation failed: ${error instanceof Error ? error.message : String(error)}`);
    }
    await this.refreshDiagnostics();
  }

  /** Return-to-Agent. The companion revalidates and focuses; this only asks. */
  async returnTo(sessionId: string, chosenBindingId: string | null = null): Promise<void> {
    if (this.state.routing !== null) return;
    this.set({ routing: sessionId });
    try {
      const result = await this.action({
        kind: "ReturnToSession",
        sessionId,
        chosenBindingId,
        expectedBindingRevision: null,
      });
      if (result.kind === "Routed") {
        this.set({ routes: { ...this.state.routes, [sessionId]: result.result } });
      }
    } catch (error) {
      this.notice(`Return failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      this.set({ routing: null });
    }
  }

  async refreshEvidence(): Promise<void> {
    try {
      const result = await this.action({ kind: "RefreshEvidence" });
      if (result.kind === "EvidenceRefreshed") {
        this.set({ discovery: result.summary });
        if (result.summary.error) this.notice(`Discovery: ${result.summary.error}`);
      }
    } catch (error) {
      this.notice(`Refresh failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async refreshDiagnostics(): Promise<void> {
    try {
      const [diagnostics, integration] = await Promise.all([
        this.query({ kind: "Diagnostics" }),
        this.query({ kind: "IntegrationStatus" }),
      ]);
      this.set({
        diagnostics: diagnostics.kind === "Diagnostics" ? diagnostics : this.state.diagnostics,
        integration: integration.kind === "IntegrationStatus" ? integration : this.state.integration,
      });
    } catch (error) {
      this.notice(`Diagnostics unavailable: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async requestSetup(kind: "RequestNotificationAuthorization" | "RequestTerminalAutomation"): Promise<void> {
    try {
      const result = await this.action({ kind });
      if (result.kind === "NotificationAuthorization") {
        this.notice(`Notifications ${result.granted ? "granted" : "not granted"} (${result.settings.authorizationStatus})`);
      } else if (result.kind === "TerminalAutomation") {
        this.notice(`Terminal automation: ${result.automation} (${result.statusCode})`);
      }
      await this.refreshDiagnostics();
    } catch (error) {
      this.notice(`${kind} failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  // ------------------------------------------------------- local selection

  select(sessionId: string | null): void {
    if (sessionId === null) {
      this.set({ inspector: null });
      return;
    }
    const outstanding = this.state.attention.find((item) => item.sessionId === sessionId && item.acknowledgedAtMs === null);
    this.set({
      inspector: {
        attentionId: outstanding?.attentionId ?? null,
        sessionId,
        source: "SELECTION",
        outstandingAtOpen: outstanding ? true : null,
        intentId: null,
        openedAtMs: Date.now(),
        route: null,
        observationEnabled: null,
      },
    });
  }

  openAttention(attentionId: string): void {
    const item = this.state.attention.find((entry) => entry.attentionId === attentionId);
    if (!item) return;
    this.set({
      inspector: {
        attentionId,
        sessionId: item.sessionId,
        source: "SELECTION",
        outstandingAtOpen: item.resolvedAtMs === null,
        intentId: null,
        openedAtMs: Date.now(),
        route: null,
        observationEnabled: null,
      },
    });
  }

  // -------------------------------------------------------------- renderer

  setRenderer(attestation: RendererAttestation): void {
    this.set({ renderer: attestation, rendererError: null });
    this.recordQualificationReport("renderer-attestation", attestation);
  }

  setRendererError(message: string): void {
    this.set({ rendererError: message });
    this.recordQualificationReport("renderer-error", { message });
  }

  /** Qualification builds only; reports wait for a hydrated subscription. */
  recordQualificationReport(kind: string, report: unknown): void {
    if (!launch.qualificationBuild) return;
    this.pendingReports.push({ kind, report });
    void this.flushReports();
  }

  private async flushReports(): Promise<void> {
    if (!this.isLive()) return;
    while (this.pendingReports.length > 0) {
      const next = this.pendingReports.shift();
      if (!next) break;
      try {
        await this.action({ kind: "RecordQualificationReport", reportKind: next.kind, report: next.report as never });
      } catch (error) {
        this.notice(`Qualification report ${next.kind} not recorded: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
  }
}
