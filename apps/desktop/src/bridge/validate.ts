// Runtime validation of bridge frames and snapshot content. The generated
// TypeScript types describe the contract; these checks enforce it at runtime.

import type { AttentionPage } from "../contracts/generated/AttentionPage";
import type { AttentionView } from "../contracts/generated/AttentionView";
import type { AttentionCounts } from "../contracts/generated/AttentionCounts";
import type { FleetPage } from "../contracts/generated/FleetPage";
import type { FleetSnapshot } from "../contracts/generated/FleetSnapshot";
import type { NativeIntent } from "../contracts/generated/NativeIntent";
import type { ProjectionPatch } from "../contracts/generated/ProjectionPatch";
import type { RouteSummary } from "../contracts/generated/RouteSummary";
import type { SessionView } from "../contracts/generated/SessionView";
import type { UiFrame } from "../contracts/generated/UiFrame";

export const UI_PROTOCOL_VERSION = 1;

type Record_ = Record<string, unknown>;

export class ValidationError extends Error {}

function fail(path: string, expected: string): never {
  throw new ValidationError(`${path}: expected ${expected}`);
}

function object(value: unknown, path: string): Record_ {
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail(path, "object");
  return value as Record_;
}

function string(value: unknown, path: string): string {
  if (typeof value !== "string") fail(path, "string");
  return value;
}

function optionalString(value: unknown, path: string): string | null {
  return value === null ? null : string(value, path);
}

function integer(value: unknown, path: string, min = 0): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < min) fail(path, `integer ≥ ${min}`);
  return value;
}

function optionalInteger(value: unknown, path: string): number | null {
  return value === null ? null : integer(value, path);
}

function boolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") fail(path, "boolean");
  return value;
}

function array<T>(value: unknown, path: string, item: (value: unknown, path: string) => T): T[] {
  if (!Array.isArray(value)) fail(path, "array");
  return value.map((entry, index) => item(entry, `${path}[${index}]`));
}

function oneOf<T extends string>(value: unknown, path: string, allowed: readonly T[]): T {
  if (typeof value !== "string" || !(allowed as readonly string[]).includes(value)) fail(path, allowed.join("|"));
  return value as T;
}

/** Canonical nonnegative decimal cursor within SQLite's signed 64-bit range. */
export function isCursor(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9][0-9]{0,18})$/.test(value) && BigInt(value) <= 9223372036854775807n;
}

function cursor(value: unknown, path: string): string {
  if (!isCursor(value)) fail(path, "canonical cursor");
  return value;
}

/** Numeric, not lexical, comparison of cursors. */
export function compareCursors(left: string, right: string): number {
  const a = BigInt(left);
  const b = BigInt(right);
  return a < b ? -1 : a > b ? 1 : 0;
}

const TURN_STATES = ["UNKNOWN", "QUEUED", "WORKING", "WAITING", "COMPLETED", "INTERRUPTED", "FAILED", "REFUSED"] as const;
const PRESENCE = ["LIVE", "DETACHED", "PARKED", "ENDED", "UNKNOWN"] as const;
const OBSERVATION = ["CURRENT", "STALE", "DISCONNECTED", "CONFLICT", "UNKNOWN"] as const;
const CATEGORIES = [
  "INPUT_REQUIRED",
  "APPROVAL_REQUIRED",
  "TURN_COMPLETE",
  "ERROR",
  "BLOCKED",
  "HANDOFF_READY",
  "OWNER_DECISION_REQUIRED",
] as const;
const NOTIFICATION = ["NOT_REQUESTED", "PENDING", "SUBMITTED", "CONFIRMED_PRESENT", "UNCERTAIN", "FAILED"] as const;
const SURFACE = [
  "EXACT_NATIVE_SURFACE",
  "EXACT_WINDOW",
  "APP_ONLY",
  "URL_DISPATCHED",
  "PROJECT_ONLY",
  "INSPECTOR_ONLY",
  "AMBIGUOUS",
  "UNAVAILABLE",
] as const;
const VERIFICATION = ["CURRENT_NATIVE_REVALIDATED", "NATIVE_BOUND_LAST_KNOWN", "USER_ATTESTED", "UNBOUND", "CONFLICT"] as const;
const READINESS = ["FOREGROUND_COMPATIBLE", "BACKGROUND_JOB", "UNKNOWN"] as const;

function routeSummary(value: unknown, path: string): RouteSummary {
  const v = object(value, path);
  return {
    requestId: string(v.requestId, `${path}.requestId`),
    surfaceResult: oneOf(v.surfaceResult, `${path}.surfaceResult`, SURFACE),
    sessionVerification: oneOf(v.sessionVerification, `${path}.sessionVerification`, VERIFICATION),
    inputReadiness: oneOf(v.inputReadiness, `${path}.inputReadiness`, READINESS),
    reasonCode: string(v.reasonCode, `${path}.reasonCode`),
    focusPerformed: boolean(v.focusPerformed, `${path}.focusPerformed`),
    latencyMs: integer(v.latencyMs, `${path}.latencyMs`),
    recordedAtMs: integer(v.recordedAtMs, `${path}.recordedAtMs`),
  };
}

function session(value: unknown, path: string): SessionView {
  const v = object(value, path);
  const process = v.process === null ? null : object(v.process, `${path}.process`);
  const binding = v.binding === null ? null : object(v.binding, `${path}.binding`);
  return {
    sessionId: string(v.sessionId, `${path}.sessionId`),
    provider: string(v.provider, `${path}.provider`),
    nativeSessionId: string(v.nativeSessionId, `${path}.nativeSessionId`),
    displayName: string(v.displayName, `${path}.displayName`),
    activation: optionalString(v.activation, `${path}.activation`),
    turnState: oneOf(v.turnState, `${path}.turnState`, TURN_STATES),
    executionPresence: oneOf(v.executionPresence, `${path}.executionPresence`, PRESENCE),
    observation: oneOf(v.observation, `${path}.observation`, OBSERVATION),
    process: process && {
      pid: integer(process.pid, `${path}.process.pid`),
      bootId: string(process.bootId, `${path}.process.bootId`),
      startSeconds: string(process.startSeconds, `${path}.process.startSeconds`),
      startMicroseconds: integer(process.startMicroseconds, `${path}.process.startMicroseconds`),
      executableIdentity: string(process.executableIdentity, `${path}.process.executableIdentity`),
    },
    binding: binding && {
      bindingId: string(binding.bindingId, `${path}.binding.bindingId`),
      surfaceKind: string(binding.surfaceKind, `${path}.binding.surfaceKind`),
      proof: string(binding.proof, `${path}.binding.proof`),
      revision: cursor(binding.revision, `${path}.binding.revision`),
      locator: string(binding.locator, `${path}.binding.locator`),
      deviceNumber: optionalInteger(binding.deviceNumber, `${path}.binding.deviceNumber`),
      pid: optionalInteger(binding.pid, `${path}.binding.pid`),
    },
    liveBindings: integer(v.liveBindings, `${path}.liveBindings`),
    lastInvalidation: optionalString(v.lastInvalidation, `${path}.lastInvalidation`),
    providerStatus: optionalString(v.providerStatus, `${path}.providerStatus`),
    providerWaitingFor: optionalString(v.providerWaitingFor, `${path}.providerWaitingFor`),
    lastRoute: v.lastRoute === null ? null : routeSummary(v.lastRoute, `${path}.lastRoute`),
    fixture: boolean(v.fixture, `${path}.fixture`),
    revision: cursor(v.revision, `${path}.revision`),
  };
}

function attention(value: unknown, path: string): AttentionView {
  const v = object(value, path);
  return {
    attentionId: string(v.attentionId, `${path}.attentionId`),
    sessionId: string(v.sessionId, `${path}.sessionId`),
    turnId: optionalString(v.turnId, `${path}.turnId`),
    category: oneOf(v.category, `${path}.category`, CATEGORIES),
    priority: integer(v.priority, `${path}.priority`),
    summary: optionalString(v.summary, `${path}.summary`),
    createdAtMs: integer(v.createdAtMs, `${path}.createdAtMs`),
    acknowledgedAtMs: optionalInteger(v.acknowledgedAtMs, `${path}.acknowledgedAtMs`),
    resolvedAtMs: optionalInteger(v.resolvedAtMs, `${path}.resolvedAtMs`),
    notificationState: oneOf(v.notificationState, `${path}.notificationState`, NOTIFICATION),
    revision: cursor(v.revision, `${path}.revision`),
  };
}

function counts(value: unknown, path: string): AttentionCounts {
  const v = object(value, path);
  return {
    needsAttention: integer(v.needsAttention, `${path}.needsAttention`),
    awaitingAction: integer(v.awaitingAction, `${path}.awaitingAction`),
  };
}

export function parseSnapshot(value: unknown): FleetSnapshot {
  const v = object(value, "snapshot");
  const snapshot: FleetSnapshot = {
    viewRevision: cursor(v.viewRevision, "snapshot.viewRevision"),
    sessions: array(v.sessions, "snapshot.sessions", session),
    attention: array(v.attention, "snapshot.attention", attention),
    counts: counts(v.counts, "snapshot.counts"),
    complete: boolean(v.complete, "snapshot.complete"),
    totalSessions: integer(v.totalSessions, "snapshot.totalSessions"),
    totalAttention: integer(v.totalAttention, "snapshot.totalAttention"),
    sessionsAfter: optionalString(v.sessionsAfter, "snapshot.sessionsAfter"),
    attentionAfter: optionalString(v.attentionAfter, "snapshot.attentionAfter"),
  };
  // A complete view holds every row; a bounded one never hides its totals.
  if (snapshot.complete && (snapshot.sessions.length !== snapshot.totalSessions || snapshot.attention.length !== snapshot.totalAttention)) {
    fail("snapshot.complete", "row counts equal to totals");
  }
  if (snapshot.sessions.length > snapshot.totalSessions || snapshot.attention.length > snapshot.totalAttention) {
    fail("snapshot.totals", "totals at least the rows present");
  }
  return snapshot;
}

function sessionPage(value: unknown, path: string): FleetPage {
  const v = object(value, path);
  return {
    viewRevision: cursor(v.viewRevision, `${path}.viewRevision`),
    rows: array(v.rows, `${path}.rows`, session),
    nextAfter: optionalString(v.nextAfter, `${path}.nextAfter`),
    total: integer(v.total, `${path}.total`),
  };
}

function attentionPage(value: unknown, path: string): AttentionPage {
  const v = object(value, path);
  return {
    viewRevision: cursor(v.viewRevision, `${path}.viewRevision`),
    rows: array(v.rows, `${path}.rows`, attention),
    nextAfter: optionalString(v.nextAfter, `${path}.nextAfter`),
    total: integer(v.total, `${path}.total`),
  };
}

/** Validates a page reply before it may touch the projection. */
export function parsePage(value: unknown): { kind: "FleetPage"; page: FleetPage } | { kind: "AttentionPage"; page: AttentionPage } {
  const v = object(value, "page");
  if (v.kind === "FleetPage") return { kind: "FleetPage", page: sessionPage(v, "page") };
  if (v.kind === "AttentionPage") return { kind: "AttentionPage", page: attentionPage(v, "page") };
  return fail("page.kind", "FleetPage or AttentionPage");
}

function patch(value: unknown, path: string): ProjectionPatch {
  const v = object(value, path);
  return {
    fromCursor: cursor(v.fromCursor, `${path}.fromCursor`),
    toCursor: cursor(v.toCursor, `${path}.toCursor`),
    viewRevision: cursor(v.viewRevision, `${path}.viewRevision`),
    sessionUpserts: array(v.sessionUpserts, `${path}.sessionUpserts`, session),
    attentionUpserts: array(v.attentionUpserts, `${path}.attentionUpserts`, attention),
    tombstones: array(v.tombstones, `${path}.tombstones`, (entry, entryPath) => {
      const t = object(entry, entryPath);
      return { entity: oneOf(t.entity, `${entryPath}.entity`, ["SESSION", "ATTENTION"] as const), id: string(t.id, `${entryPath}.id`) };
    }),
    counts: counts(v.counts, `${path}.counts`),
    pageInvalidations: array(v.pageInvalidations, `${path}.pageInvalidations`, (entry, entryPath) =>
      oneOf(entry, entryPath, ["SESSION", "ATTENTION"] as const),
    ),
  };
}

function intent(value: unknown, path: string): NativeIntent {
  const v = object(value, path);
  const action = object(v.action, `${path}.action`);
  const intentId = string(v.intentId, `${path}.intentId`);
  if (action.kind === "QualificationCommand") {
    return {
      intentId,
      action: { kind: "QualificationCommand", command: string(action.command, `${path}.action.command`), args: action.args },
    };
  }
  if (action.kind !== "OpenAttention") fail(`${path}.action.kind`, "OpenAttention or QualificationCommand");
  return {
    intentId,
    action: {
      kind: "OpenAttention",
      attentionId: string(action.attentionId, `${path}.action.attentionId`),
      sessionId: string(action.sessionId, `${path}.action.sessionId`),
      outstanding: boolean(action.outstanding, `${path}.action.outstanding`),
      source: oneOf(action.source, `${path}.action.source`, ["NOTIFICATION_RESPONSE"] as const),
      route: action.route === null ? null : routeSummary(action.route, `${path}.action.route`),
      observationEnabled: boolean(action.observationEnabled, `${path}.action.observationEnabled`),
    },
  };
}

export function parseFrame(value: unknown): UiFrame {
  const frame = object(value, "frame");
  const header = object(frame.header, "frame.header");
  if (header.protocolVersion !== UI_PROTOCOL_VERSION) fail("frame.header.protocolVersion", String(UI_PROTOCOL_VERSION));
  const parsedHeader = {
    protocolVersion: UI_PROTOCOL_VERSION,
    storeGeneration: string(header.storeGeneration, "frame.header.storeGeneration"),
    coreGeneration: string(header.coreGeneration, "frame.header.coreGeneration"),
    subscriptionId: string(header.subscriptionId, "frame.header.subscriptionId"),
    viewEpoch: string(header.viewEpoch, "frame.header.viewEpoch"),
    streamSeq: integer(header.streamSeq, "frame.header.streamSeq", 1),
    cursor: cursor(header.cursor, "frame.header.cursor"),
  };
  const body = object(frame.body, "frame.body");
  switch (body.kind) {
    case "SnapshotBegin":
      return {
        header: parsedHeader,
        body: {
          kind: "SnapshotBegin",
          viewRevision: cursor(body.viewRevision, "body.viewRevision"),
          chunkCount: integer(body.chunkCount, "body.chunkCount"),
          totalBytes: integer(body.totalBytes, "body.totalBytes"),
        },
      };
    case "SnapshotChunk":
      return {
        header: parsedHeader,
        body: { kind: "SnapshotChunk", index: integer(body.index, "body.index"), data: string(body.data, "body.data") },
      };
    case "SnapshotEnd":
      return { header: parsedHeader, body: { kind: "SnapshotEnd", viewRevision: cursor(body.viewRevision, "body.viewRevision") } };
    case "ProjectionPatch":
      return { header: parsedHeader, body: { kind: "ProjectionPatch", patch: patch(body.patch, "body.patch") } };
    case "BridgeHeartbeat":
      return { header: parsedHeader, body: { kind: "BridgeHeartbeat" } };
    case "NativeIntent":
      return { header: parsedHeader, body: { kind: "NativeIntent", intent: intent(body.intent, "body.intent") } };
    default:
      return fail("frame.body.kind", "known frame kind");
  }
}
