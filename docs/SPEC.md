# THREADSPACE — Engineering and Product Specification

**Architecture date:** October 5, 2026, America/New_York  
**Status:** M0A/M0B accepted on `8558854`; **M0C ACCEPTED** by the final independent review of `7cc386f240eb56403fdd3326566bda816f5c2f6d` on application build `fd02d6a` and merged to `main` at `cd9e376`; G01–G17 PASS under their accepted scopes. **M1 is a remediation candidate** on branch `m1`: the first independent review required remediation of eight groups of findings, which are closed there pending independent re-review ([manifest](../evidence/M1/manifest.json), [remediation](../evidence/M1/remediation/README.md), [D-0007](decisions/D-0007-m1-canonical-engine.md), [D-0008](decisions/D-0008-tao-0.37.0-window-release-containment.md)). Accepted qualification decisions remain normative; the [M0C checklist](../evidence/M0C/gate-checklist.md) controls platform gate status.  

**Companion plan:** [MILESTONES.md](MILESTONES.md)  
**Primary target:** Daniel's Apple Silicon Mac, macOS 26 or later.

## 0. Document contract and evidence basis

This document specifies a personal, local-first spatial command center for existing AI-agent workflows. It selects the architecture, defines the correctness contracts, and identifies the exact conditions under which each integration is supported. MUST, MUST NOT, and SHALL describe requirements. Performance numbers are acceptance targets, not measurements already achieved.

The research used current official provider documentation, public release-pinned source, terminal documentation/source, and focused competitor source inspection. The research cutoff is October 5, 2026; release/source snapshots were independently rechecked for that cutoff during the final audit on October 6 UTC. Mutable documentation describes the content retrieved during this research, not an independently archived historical snapshot.

Two provider reference points materially inform the design:

- Claude Code **2.1.290**, released October 5, was the researched candidate; M0C qualifies the installed **2.1.291** observer profile under [D-0005](decisions/D-0005-claude-2.1.291-observer-semantics.md). The researched release-pinned public declaration snapshot identifies **2.1.277**. Generated declarations from the exact installed build and recorded runtime fixtures control compatibility; a larger version number is not automatic certification. [Claude release][C9] [Mod reference][C7] [Release-pinned declarations][C8_PIN] [Public declarations][C8]
- Codex **0.160.1**, release commit **d27764b82f7118f674371e6d6e76271d9d606edb**, is the researched release. Some generic documentation lagged that release. Where they conflict, this document cites the released schema and implementation, with actual installed CLI and desktop runtimes qualified separately. [Codex release][O1] [Released hook schema][O2]

The original research was not native certification. Committed M0A/M0B evidence establishes the accepted substrate and real Claude/Terminal identity-and-return path; M0C establishes platform reliability on the target Mac (G17 PASS). M1 makes the canonical journal, contracts and reducer the store's only write path and qualifies them with deterministic synthetic evidence; its candidate awaits independent review.

### 0.1 Frozen decisions

| Area | Selected architecture |
| --- | --- |
| Desktop | Tauri 3, Rust, React, TypeScript; native macOS window behavior |
| Graphics | Direct Three.js `WebGPURenderer` using actual WebGPU; TSL/node materials |
| Observer lifetime | Independent signed, bundled `ThreadspaceAgent.app` with a thin AppKit/UserNotifications bridge and Rust core |
| Supervision | One `SMAppService.loginItem(identifier:)` AppKit companion; system relaunch on crash/nonzero exit; no parallel LaunchAgent registration |
| Hook transport | Small Rust capture executable → private Unix stream socket → durable journal; bounded local spool on failure |
| Persistence | SQLite, one writer, WAL, `synchronous=FULL`, versioned normalized event journal and materialized projections |
| Claude | Passive command registration + supported native JSON inventory + version-qualified observer mod |
| Codex | Native hooks; passive connection to an existing shared daemon where present; mode-specific routing guarantees |
| Session correlation | Provider identity joined to verified execution evidence and native surface identity; no cwd/recency binding cascade |
| Attention | Durable, individually addressable owner-action records; acknowledgement separate from resolution |
| Frontend state | Snapshot plus revisioned deltas over a typed Tauri Channel; renderer consumes provider-neutral view models |
| MCP | Optional semantic enrichment, never objective lifecycle authority |
| Remote/cloud | Endpoint-aware identity; user-owned SSH collector and original-URL adapters in M10 |
| ChatGPT | Separate capability profiles; original-URL/manual integration first, consumer lifecycle experimental |
| Primary release | Direct macOS distribution; Apple Silicon first; no cloud service, telemetry, or subscription required |

### 0.2 Scope and release boundary

**The first usable local MVP is M0A → M0B → M0C → M1–M6.** It observes ordinary externally launched Claude CLI sessions in Terminal.app after one explicit integration setup, keeps workers through completed turns, represents subagents, preserves attention, returns to verified current Claude surfaces, survives restarts, and integrates Codex with truthful coverage and routing tiers.

M7–M15 complete the project/worktree world, terminal extensions, semantic MCP, remote/browser groundwork, ChatGPT experiment, visual polish, stress qualification, full acceptance, and packaging.

A capability marked experimental is implemented behind a feature flag with its own evidence and visible coverage label. Unsupported consumer ChatGPT lifecycle, absent Ghostty properties, and missing shared-daemon Codex client identity are not silently substituted into MVP promises.

## 1. Product definition

### 1.1 Vision

Threadspace is an ambient operating layer over Daniel's existing coding-agent workflow. Projects become persistent places. Sessions become recognizable workers. Subagents form visible relationships. Outstanding owner actions remain visible until handled.

The central operation is:

> Open the office, understand what the fleet is doing and who needs attention, select a worker, and return to the original working surface with an honest statement of how precisely it was identified.

The user continues using Terminal, Claude Code, Codex, worktrees, native apps, and cloud surfaces. Threadspace does not need to launch or own those processes.

In everyday terms, the system maintains three separate records: **who the worker is, what its current piece of work is doing, and where its connection is running**. Finishing a response changes the second record. Closing a terminal changes the third. Neither erases the first. An attention list is the user's durable inbox for all the workers' requests.

### 1.2 Product principles

1. **Observe existing work.** One-time integration installation is allowed; launching every session through Threadspace is not a prerequisite.
2. **Report evidence at its actual strength.** Native completion, a permission preflight, a disconnected observer, and an uncertain inference are distinct.
3. **Preserve identity.** Resume restores the same conceptual worker when the provider returns the same persistent identity.
4. **Return precisely.** Exact native-tab focus and verification of the conversation currently in that tab are separate claims.
5. **Keep owner actions durable.** Closing the office, losing a banner, or restarting cannot discard accepted attention.
6. **Use space to improve comprehension.** The office complements a fast attention drawer, fleet list, inspectors, and keyboard switcher.
7. **Keep the system local and lightweight.** The 3D scene can stop completely while observation and notifications continue.
8. **Expose limitations without surrendering usefulness.** A correctly labeled bound tab, original conversation URL, or unknown state is useful.
9. **Separate provider semantics from presentation.** The scene knows canonical state and capabilities, not Claude/Codex hook names.
10. **Validate difficult assumptions early.** Correlation, current-session checks, replay, and native application behavior precede extensive art production.

### 1.3 Goals and non-goals

Goals include concurrent external sessions; same-repository and same-cwd separation; multiple worktrees; top-level and subordinate actors; native turn, activity, waiting and termination observations; exact native routing where supported; durable attention; restart/reconnect recovery; synthetic testing without inference; and comfortable operation with dozens of sessions.

The MVP does not execute coding tasks, own PTYs, approve tools, send prompts, merge changes, replace native terminals, parse terminal text for lifecycle, inspect undocumented ChatGPT databases, purchase infrastructure, or install system-wide privileged services. A future launcher can use the same contracts but cannot become the foundation of observation.

No character species or commercial product name is frozen. The neutral qualification skin is a replaceable geometry asset, not a branding decision.

### 1.4 Focused competitive findings

These are inspected implementation choices, not runtime benchmarks or claims of unique invention.

| Project | Relevant evidence | Threadspace decision |
| --- | --- | --- |
| AI Agent Session Center | Its pinned matcher selects the newest of several CONNECTING same-cwd candidates; native hooks coexist with timer/spinner approval inference. | Ambiguous candidates remain unbound. Tool duration and terminal text cannot certify approval. |
| CCC, Amir Fish implementation | Exact Terminal/iTerm TTY focus and PID birth checks already exist; app-only fallback shares a generic success result. | Adopt strong native primitives and return explicit location and conversation-verification results. |
| This Office | Transcript activity fills desks; startup seeds older files at EOF and loaded office state resets waiting/busy flags. | Scene restoration and actual lifecycle/attention reconstruction are separate systems. |
| Pixel Agents | Current source includes external hooks, standalone operation, routing maps, buffering and external restoration. | Credit mature observation; restored identity does not automatically restore a native focus handle. |
| Termhive | Current runtime uses PTYs for several providers and app-server threads for Codex. | Execution context must support both terminal and nonterminal runtimes. |
| Codeg | ACP/native imports preserve IDs and filter subordinate sessions. | History import, live presence and actor role remain distinct. |
| co:lana | Official product describes managed interactive PTYs and resume; internals were not verified. | No assertions about uninspected correlation correctness. |

Threadspace's differentiation is the enforced combination of identity, lifecycle provenance, reconciliation, attention and routing quality. External observation, 3D offices, subagents, and TTY focus individually have prior art. Source snapshots and exact relevant files are listed in Section 23. [Competitor sources][P1] [CCC routing][P2] [This Office][P3] [Pixel Agents][P4] [Termhive][P5] [Codeg][P6] [co:lana][P7]

## 2. System architecture and boundaries

~~~mermaid
flowchart TB
  subgraph NativeSources["Existing provider runtimes"]
    Claude["Claude hooks, mod and inventory"]
    Codex["Codex hooks and native daemon"]
    Remote["Later remote endpoints"]
  end
  Capture["Bounded capture relay"]
  Adapters["Provider adapters and reconciliation"]
  Journal["SQLite journal and single writer"]
  Projection["Session, actor and attention projections"]
  Native["Native routing and notifications"]
  Bridge["Tauri Rust bridge"]
  UI["React fleet and Three.js office"]
  Claude --> Capture
  Codex --> Capture
  Remote --> Capture
  Claude --> Adapters
  Codex --> Adapters
  Capture --> Journal
  Adapters --> Journal
  Journal --> Projection
  Projection --> Native
  Projection --> Bridge
  Bridge --> UI
  UI --> Bridge
  Bridge --> Native
~~~

### 2.1 Executables and ownership

**Threadspace.app** owns the Tauri window, native menu integration, React controls, Three.js scene, and a narrow Rust client bridge. It is not the journal writer. Closing or quitting this application does not stop enabled observation.

**ThreadspaceAgent.app** (display name “Threadspace Agent”) is a signed nested LSUIElement app bundle with an AppKit run loop. A thin Swift/Objective-C bridge exposes UserNotifications, AppleEvents/native surface operations and accessibility preferences. The installed outer app owns ServiceManagement registration/status/unregistration in a native bootstrap path that does not require a WebView; the companion owns observation and reports its own health. Rust owns ingestion, providers, reconciliation, journal, projections, attention and routing plans. It contains no WebView.

**threadspace-hook** is a small native Rust executable installed at a stable user-scoped location. It captures allowlisted metadata and process evidence, sends bounded records, spools when necessary, and exits. It never waits for the renderer or external network. The same binary supports bounded mod-batch ingestion and a narrow diagnostic/installation interface.

**threadspace-mcp**, added in M9, is a stdio semantic adapter. Its connection is enrolled into a verified session scope or it refuses session-specific tools.

An independent observer avoids coupling provider progress to a WebView crash or renderer load. Tauri 3's development CLI terminates the app's process tree on restart; a Tauri-spawned sidecar cannot provide the required independent supervision. The native companion is launched independently through ServiceManagement in development and production. [Tauri 3 runtime][N1] [Development process lifetime][N16]

### 2.2 Repository layout

~~~text
apps/desktop/                 React, TypeScript, Tauri application
apps/agent-macos/             bundled native companion and thin Apple bridges
crates/contracts/             canonical schemas and generated TypeScript types
crates/state-engine/          pure reducers, evidence resolution, attention
crates/journal/               SQLite, checkpoints, migrations, outbox
crates/relay/                 Unix IPC, capture executable, spool
crates/provider-claude/       hooks, mod bridge, native inventory
crates/provider-codex/        hook parsing, daemon observer, native pagination
crates/surfaces/              routing model and adapter interfaces
crates/surfaces-macos/        Terminal, iTerm, Ghostty, process/TTY support
crates/project-index/         repository and worktree identity
crates/semantic-mcp/          optional semantic tools and enrollment
packages/scene/               provider-neutral Three.js scene/controller
packages/provider-mod/        pinned Claude observer mod
fixtures/                     synthetic and redacted native fixtures
tests/native/                 macOS provider, routing and packaged-app tests
docs/compatibility/           qualified versions, schemas, native dictionaries
docs/decisions/               material implementation deviations and decisions
evidence/                     milestone manifests and reproducible evidence
~~~

This layout assigns ownership; it is not a requirement to create every crate, table or optional integration in M0/M1. Create modules as their milestone needs them, and promote the working qualification code into those owners. M0 uses only the small persisted fixture and real adapters needed by its gates; M1 completes the canonical engine without discarding the proven vertical slice.

Rust contracts are authoritative for IPC and stored data. Generate TypeScript definitions and JSON Schemas from those contracts. Provider wire schemas remain separate, versioned inputs. The frontend cannot write SQLite or apply provider events directly.

### 2.3 Functional boundaries

- Provider adapters emit supported observations and normalized facts. They do not create avatars, notifications, or terminal-focus scripts.
- The state engine is deterministic over accepted facts, checkpoints, and owner commands. It performs no network, filesystem or OS calls.
- Reconciliation obtains evidence and submits facts; it does not patch projection tables behind the reducer.
- Surface adapters inventory, validate and focus native surfaces. They cannot submit prompts or grant approvals.
- The attention router decides owner-action lifecycle; the notification service only transports notification attempts.
- Scene/controller code consumes `FleetViewModel` and `AttentionViewModel`. Provider-specific branching is limited to branding and explicit capability descriptions.
- Native permissions are requested at the feature boundary that needs them. Missing permissions degrade that feature without disabling the event journal.

## 3. Canonical entities, identity axes and terminology

### 3.1 Entities

| Entity | Purpose and durable key |
| --- | --- |
| Endpoint | A logical observed machine/runtime authority; random persisted UUID, not hardware serial |
| ProviderNamespace | One provider profile/store authority on an endpoint; independent profiles never merge implicitly |
| Project | User-facing persistent project/place UUID |
| Repository | A particular local Git common directory or explicit remote repository record |
| Worktree | A checkout with its own root/gitdir; branch is mutable metadata |
| Session | Persistent provider conversation/thread UUID, unique on namespace and native session ID |
| Actor | A principal or subordinate worker/loop; may reference its own provider Session |
| ExecutionInstance | One logical activation of a session/actor in a runtime; independent of durable identity |
| ProcessIncarnation | Endpoint, boot, PID, kernel birth time; one process can serve many sessions |
| Turn | A native model/agent turn, scoped to session and actor; distinct from an input request |
| InputAttempt | Submitted input and its provenance/acceptance; can queue or be rejected |
| Activity | A tool/item attempt with native occurrence ID when available |
| SourceSurface | Native terminal pane/tab, app window, browser tab, URL, or remote surface |
| SurfaceBinding | Versioned relation between session/activation and source surface, with proof and invalidation |
| AttentionItem | Durable owner-action record tied to a turn, request, wait category or decision |
| SemanticAnnotation | Agent/user-reported title, phase, checkpoint, blocker or handoff |
| ObservationSource | A producer with capability profile, epoch, provenance and delivery coverage |
| Layout/Avatar | Stable project/worker presentation, independent of process lifetime |

### 3.2 Core pseudotypes

~~~typescript
type UUID = string;
type OpaqueId = string;
type DecimalU64 = string; // JSON-safe; never order decimal strings lexically.

interface SessionKey {
  provider: "claude" | "codex" | "chatgpt" | "synthetic" | string;
  namespaceId: UUID;
  nativeSessionId: OpaqueId;
}
interface ProcessKey {
  endpointId: UUID;
  bootId: string;
  pid: number;
  startSeconds: DecimalU64;
  startMicroseconds: number;
}
interface ExecutionRef {
  executionId: UUID;
  sessionId: UUID;
  actorId: UUID;
  activation: DecimalU64;
  nativeRuntimeId?: OpaqueId;
  processes: ProcessKey[];
  mode: "terminal_embedded" | "shared_daemon" | "supervised_background"
      | "desktop" | "remote" | "headless";
}
interface TurnRef {
  turnId: UUID;
  sessionId: UUID;
  actorId: UUID;
  executionId?: UUID;
  nativeTurnId?: OpaqueId;
  identityKind: "NATIVE" | "LOCAL_PROVISIONAL";
}
interface SessionState {
  identity: SessionKey;
  recordState: "KNOWN" | "ARCHIVED" | "PROVIDER_DELETED";
  executionPresence: "LIVE" | "DETACHED" | "PARKED" | "ENDED" | "UNKNOWN";
  observation: "CURRENT" | "STALE" | "DISCONNECTED" | "CONFLICT" | "UNKNOWN";
  turnStates: Record<UUID, TurnState>;
  activeActivityIds: UUID[];
  waitConditions: WaitCondition[];
  unresolvedAttentionIds: UUID[];
  activeExecutionIds: UUID[];
  viewRevision: DecimalU64;
}
type TurnState = "UNKNOWN" | "QUEUED" | "WORKING" | "WAITING"
               | "COMPLETED" | "INTERRUPTED" | "FAILED" | "REFUSED";
~~~

`recordState` describes a durable record. `executionPresence` describes currently known execution/attachment. `observation` describes what Threadspace knows about it. They never substitute for one another.

### 3.3 Provider-specific identity normalization

**Claude:** a root Session uses the full native `session_id`. A subordinate Actor normally uses `(root session, native agent_id)` and can have several runs/turns. `agent_type` alone does not establish subordinate role. Native mod `turnId`, conventional `prompt_id` and display `turn_id` are separate namespaces unless a qualified observation explicitly links them. [Claude schemas][C1] [Mod declarations][C8]

**Codex:** persistent identity is `Thread.id`. Hook `session_id` is the live tree/root identity; a child's `agent_id` is its concrete thread ID. `Thread.parentThreadId` is the immediate parent, while `forkedFromId` is historical fork lineage. A provider-supported change to an owner-addressable root retains the persistent thread ID; selecting or attempting to resume a parent-owned child is not evidence that such a role change occurred. [Codex identity implementation][O3] [Thread model][O4]

A Session has at most one principal presentation identity. A provider thread represented as a subordinate Actor and later as a root retains that identity; the current role changes. Historical parentage does not permanently force a worker into a child-only view.

## 4. Session identity, correlation, and reconciliation

This section is the binding contract for the hardest subsystem. It replaces an opportunistic heuristic cascade with native identity, explicit registration, proof-bearing relations and conservative reconciliation.

### 4.1 Durable session identity

Create a random canonical Session UUID on the first accepted identification of `(provider, namespaceId, nativeSessionId)`. Enforce that tuple with a database unique constraint. Every later hook, mod event, snapshot, resume and historical read resolves through it.

Provider namespaces are persisted logical profile/store identities. Resolve a configured profile to its canonical config directory and endpoint; retain path aliases when the same directory moves. Two independent config homes or endpoints do not merge because they contain the same copied provider UUID. An explicit user-approved migration/alias operation can preserve identity across an intentional move; a copy remains distinct by default.

Cwd, repository, branch, model, prompt text, display name, PID and transcript basename are never session keys. Native IDs remain opaque even if they currently look like UUIDs.

### 4.2 Execution identity and activation

Use `ProcessKey` for a native process, with executable identity recorded separately. Revalidate executable path/code identity because `exec` can replace a provider within the same PID and kernel birth. Never use PID alone.

A logical activation begins when a provider opens/resumes a session in a runtime. A session switch A→B→A within one process creates distinct activation records where the native source proves those transitions. Compaction and plugin reload do not inherently create activations.

One daemon process can host many threads. One Claude process can host several actors. A session can have more than one live attachment or even conflicting concurrent resumes. Store these relations explicitly; never enforce “one process equals one session.”

For simultaneous activations of one persistent session, preserve one worker with a multiple-attachments badge and a surface chooser. Do not silently choose the newest activation. Historical turns retain their originating activation.

### 4.3 Source epochs are not execution epochs

A mod reload, observer reconnect or helper restart can create a new source epoch while the provider execution remains unchanged. A database ingest sequence is a local commit cursor. A callback capture sequence is a producer-local observation order. A provider turn ID is a native object identity. These fields must never be reused interchangeably.

At callback entry, capture an immutable context containing source epoch, entry sequence, known activation, native actor/turn IDs and any supplied session ID. Bind each native turn to its original session/activation at a verified start or known step.

After an asynchronous `next(e)` or host API call, use that immutable mapping. Never attach a delayed completion from A to whichever session B is current at callback return. A first-seen late event lacking provable ownership stays unresolved instead of borrowing the current session. Test A→B→A, plugin reload, delayed child results and concurrent actors.

### 4.4 Deterministic discovery path A: native inventory

For qualified Claude, `claude agents --json --all` is a supported external inventory. It can identify live interactive sessions predating Threadspace, plus background jobs. Its conditional full `sessionId` and live `pid` supply the native association to corroborate with process evidence. The selected direct CLI return path requires a qualified `kind=interactive` row. A background worker PID/PTY is not the identity of its attached terminal client. The short background `id` is not the full resumable session ID. [Native Claude inventory][C3]

Algorithm:

1. Use one bounded native inventory request to enumerate candidates; retain its interval, profile and source epoch. This first response alone does not bind a returned PID to its subsequently sampled incarnation.
2. Sample kernel birth, executable and controlling-device evidence for only the returned candidate PIDs.
3. Make one further bounded inventory request for that batch, bracketed by those incumbent process samples and fresh post-response samples. Accept a join only when the expected full session ID/PID and qualified runtime mode still agree and ProcessKey/executable/controlling TTY remain unchanged. An old row must not be joined to a PID reused before the first kernel sample. Provider `startedAt` is not kernel birth.
4. Keep changed, absent or newly encountered PIDs provisional until a later scheduler pass; do not loop until a fictional globally stable snapshot appears. Resolve accepted Session keys and upsert their proven ExecutionInstances.
5. Inventory native terminal surfaces; find a unique surface by the validated character-device join in Section 13.3. Recheck process and terminal-app incarnations around this operation.
6. Commit provider identity, execution evidence, project references and any completed surface proof as one registration transaction. Lifecycle ingestion can commit without waiting for a surface.
7. Rows lacking full identity remain provisional candidates. Background/supervised rows still establish their supported session/job facts, but exact attachment routing requires a separately qualified client association.

This bracketing closes Threadspace's PID-sampling race; it cannot convert a stale provider cache into current-session authority. The public inventory has no published revision/refresh barrier. M0B must qualify actual current interactive mappings through exit and supported in-place switches. If the build supplies only a last-known association, disable `currentSessionProcessLookup`, retain the lower proof tier, and keep the decisive exact-current-session gate BLOCKED pending a supported repair. Repeated reads or elapsed freshness do not substitute for that capability.

For supported shared-daemon Codex, native loaded-thread enumeration identifies threads, not their originating TUI. It therefore establishes session/lifecycle identity without necessarily establishing a local surface. That boundary is deliberate.

### 4.5 Deterministic discovery path B: command registration

The installed command hook supplies native session metadata to `threadspace-hook`. On macOS both researched providers detach command hooks from their own controlling terminal. Their stdin is also JSON, not terminal input. Reading `tty(0)` or `/dev/tty` in the hook is invalid. [Claude hook process behavior][C1] [Codex command runner][O5]

The capture helper:

1. Captures its own ProcessKey immediately.
2. Walks at most 24 ancestors with per-edge validation: sample the child ProcessKey/PPID, sample the parent, then re-read the child and parent. Require unchanged child identity/PPID and parent incarnation; reject vanished, reparented or cyclic chains. Recheck the selected provider ancestor. A stable final PID cannot repair an unproven intermediate edge.
3. Selects a provider runtime only through a qualified adapter rule: known executable identity and runtime mode, not a process name substring.
4. Reads the selected runtime with `proc_pidinfo(PROC_PIDTBSDINFO)`, requiring the exact expected structure length. Public fields include controlling-device number `e_tdev`, `e_tpgid`, `pbi_pgid` and kernel start seconds/microseconds. Denied/short reads and no-controlling-terminal/`NODEV` results leave that evidence unavailable; the API does not return a TTY pathname.
5. Sends the native event, captured ancestry and process/TTY evidence in one observation envelope.
6. Lets the companion independently corroborate live evidence and finalize the registration.

Detachment from a controlling terminal does not itself erase ancestry. If the qualified runtime cannot be reached through a safely captured lineage, retain session metadata and try native inventory; never jump to an unrelated ancestor. The nearest provider ancestor can be a shared daemon without a source TTY. That is a valid discovered session with no deterministic originating terminal. The helper must not walk farther to the daemon's historical startup terminal and label it as the client's source. [Apple process definitions][A1]

### 4.6 Registration transaction

~~~typescript
interface RegistrationEvidence {
  observationId: UUID;
  sessionKey: SessionKey;
  actorNativeId?: OpaqueId;
  activationHint?: OpaqueId;
  providerMode: ExecutionRef["mode"];
  process?: ProcessKey;
  executableIdentity?: string;
  controllingTty?: { deviceNumber: DecimalU64; path?: string }; // path only after validated native surface join
  ancestry?: Array<{ key: ProcessKey; parentPid: number }>;
  nativeSurfaceId?: OpaqueId;
  launchNonce?: UUID;
  nativeInventoryRef?: UUID;
  projectEvidence?: UUID;
  capturedAt: CaptureClock;
  bindingMethod: "NATIVE_INVENTORY" | "HOOK_ANCESTRY"
               | "EXPLICIT_LAUNCH" | "USER_PAIRING";
}
~~~

Kernel capture may supply controlling-device evidence without a pathname. Add `path` only after native surface enumeration and character-device validation; a missing path does not discard a valid device observation. One transaction records the observation, identity upsert, execution relation, binding proof and derived changes. Duplicate registration is idempotent. Identity can commit with a null surface; a slow or unavailable native terminal inventory must not block lifecycle ingestion.

Raw hook claims about PID/native window IDs do not bypass validation. On a direct Unix connection, same-user peer credentials and peer PID corroborate the sender. A replaying spool reader identifies itself, not the original capture process; historical capture evidence keeps its original provenance.

### 4.7 Optional shell registration

Shell integration is an enhancement, not a requirement for the Claude Terminal fast path. At an explicit provider launch it records a random nonsecret launch nonce, shell ProcessKey, current TTY, optional native terminal identifier, selected provider profile and explicit resume ID if supplied. The provider inherits the nonce.

A hook can join the nonce to the registration only when endpoint, boot, process lineage, current TTY and runtime mode agree. Inherited stale environment values are not enough. A tmux server can preserve old environment; a Codex shared daemon uses its own startup environment. Both require their respective adapter rules.

Installing a shell shim is explicit and reversible. It preserves argument boundaries, signals and exit status through `exec`; it does not parse arbitrary shell history. An optional Codex shim may add documented `--no-daemon` only after the user chooses that execution mode. It must preserve explicit remote/daemon preferences and must not silently change existing launches. [Codex launch policy][O6]

### 4.8 Same-cwd and worktree rules

Two native session IDs in the same cwd are two Sessions. Three sessions in one repository are three workers. A new worktree is a workspace relation, not a new provider identity. A directory change never rekeys a Session.

Cwd, repository, branch, recency, model, terminal title and prompt similarity can rank candidate labels for a manual chooser. They cannot promote a candidate into a binding, even when only one candidate is visible. Invisible or undiscovered sessions can exist.

### 4.9 Resume, clear, fork and process replacement

| Observed situation | Required behavior |
| --- | --- |
| Same native session resumed in a different terminal | Same Session/avatar; new proven activation/surface. Retire the previous route only when its attachment is proven ended/superseded; coexisting eligible attachments require a chooser. |
| New returned session ID after clear/fork | New Session; preserve explicit historical relationship if supplied |
| Compaction in same session/runtime | Keep identity and activation unless native source explicitly proves a switch |
| Plugin reload | New source epoch; same session/execution unless independently changed |
| PID reused after exit | New ProcessKey; cannot revive old binding |
| Provider executable replaced within same PID | Invalidate provider-process proof and requalify |
| Same native session open in two runtimes | One Session, multiple activations/surfaces; explicit selection |
| Background worker evicted/restarted by supervisor | Update worker-process presence; keep logical job/session if native inventory says it persists |
| Parent turn completes while child runs | Parent attention can coexist with live child work |
| Stored child resumed as a root | Preserve persistent identity; update current role, retain historical parent |
| Remote reconnection | Same namespace/native identity when authenticated endpoint continuity is established; new transport epoch |

The native provider can refuse concurrent transcript writers. A refused resume creates no new activation, worker or displaced route. Qualify actual supported attachments and retain synthetic multiple-attachment coverage even when a real concurrent-write attempt is refused. [Claude concurrent-open boundary][C3]

An old `SessionEnd` must close its matched activation only. When a legacy event cannot distinguish two rapid same-process activations, retain it as ambiguous; do not close the newer one.

### 4.10 Source-surface proof has independent axes

~~~typescript
type SurfaceResult =
  | "EXACT_NATIVE_SURFACE" | "EXACT_WINDOW" | "APP_ONLY"
  | "URL_DISPATCHED" | "PROJECT_ONLY" | "INSPECTOR_ONLY"
  | "AMBIGUOUS" | "UNAVAILABLE";

type SessionVerification =
  | "CURRENT_NATIVE_REVALIDATED" | "NATIVE_BOUND_LAST_KNOWN"
  | "USER_ATTESTED" | "UNBOUND" | "CONFLICT";

type InputReadiness = "FOREGROUND_COMPATIBLE" | "BACKGROUND_JOB" | "UNKNOWN";

interface RoutingProof {
  bindingId: UUID;
  bindingRevision: DecimalU64;
  surfaceResult: SurfaceResult;
  sessionVerification: SessionVerification;
  inputReadiness: InputReadiness;
  evidenceIds: UUID[];
  checkedAt: number;
}
~~~

A surface can be exact while its current conversation is unverified. This occurs when a TUI switches threads in place without changing PID, TTY or tab.

For direct interactive Claude, start from the registered ProcessKey and bracket a fresh native inventory lookup with process samples: requested session ID still maps to that incarnation, its executable/TTY still agree, and the selected native tab and binding generation remain valid. Repeat current-provider/process/surface checks after focus. These checks attest the observed instants, not an atomic freeze of another application.

Embedded Codex native ancestry can earn exact surface plus `NATIVE_BOUND_LAST_KNOWN`. A hook queue drain does not establish a loss-free upstream barrier. Current-session verification requires a real native lookup/handshake, not a freshness timeout or lack of observed conflicts.

### 4.11 Explicit pairing and its ceiling

Default shared-daemon Codex does not expose a supported current TUI-client→thread→TTY mapping in the researched release. Its client metadata and daemon process cannot distinguish equivalent concurrent terminals. [Daemon contract][O7] [TUI connection source][O8]

Provide a pairing flow: select a live terminal surface, paste the Session UUID shown by Codex `/status`, and confirm that association. Store `USER_ATTESTED` proof. The action is labeled **Focus paired tab — current thread unverified**.

Pairing is a historical assertion. In-place `/new` or `/resume` can invalidate its current-thread meaning without changing the native tab. Explicit launch argv such as `codex resume <id>` has the same ceiling after launch. Neither gets automatic current-session promotion. Notifications open the inspector/choice flow for these bindings; they do not silently claim an exact-current-session return or auto-acknowledge attention. [Codex status card][O9]

### 4.12 TTY, terminal and multiplexer lifetime

A TTY pathname is a locator, not a durable identity. Store endpoint/boot, device number, process incarnation and native surface generation. Revalidate all of them when routing. A closed/recreated tab cannot inherit an old session because its path looks familiar.

Terminal tab indexes and window IDs are hints for diagnostics only. iTerm/Ghostty native IDs retain their application-specific namespaces and validity. A terminal app restart invalidates live surface generations.

tmux introduces two layers: provider→tmux pane and selected tmux client→native terminal. Bind server ProcessKey/socket, pane ID, pane TTY and a specific outer client. Multiple clients require a choice/pin; detached panes have no outer window. Do not route by tmux session name alone. Nested tmux/SSH combinations outside the qualified profile remain unresolved. [tmux reference][A6]

### 4.13 Evidence authority and allowed fallbacks

| Class | Evidence | Permitted use |
| --- | --- | --- |
| AUTHORITATIVE for its stated fact | Native session/turn IDs; qualified native completion; native request resolution; supported provider snapshot; kernel process death/birth; explicit owner command | Establish exactly the fact the source proves |
| STRONG EVIDENCE | Revalidated ancestry; controlling TTY joined to native inventory; inherited launch nonce with matching process proof; provider-referenced transcript path | Corroborate execution/surface/project relations |
| WEAK EVIDENCE | Cwd, branch, title, recency, process-name guess, transcript mtime, terminal-output patterns | Rank manual candidates or explain diagnostics; never certify lifecycle or automatic exact routing |
| UNSUPPORTED / DO NOT USE in MVP | Private ChatGPT databases/endpoints; notification databases; undocumented provider state-file schemas as stable APIs; shared-link snapshots as original sessions | No dependency or automatic observation authority |

Authority is fact-specific. `PermissionRequest` is authoritative that a permission check occurred; it does not prove a human dialog remains open. A native snapshot is authoritative for what that source reported, not an exact historical replay or instantaneous global truth.

### 4.14 Reconciliation procedure

Run reconciliation on companion startup, provider connection/reload, source-sequence gap, process exit, sleep/wake, schema mismatch, route conflict, and bounded periodic health checks.

1. Restore the latest verified checkpoint and replay all accepted journal entries, including owner commands and notification records.
2. Reconnect native provider streams and begin buffering live observations before enumerating snapshots.
3. Enumerate supported provider sessions and process/surface inventories with request intervals and source epochs.
4. Revalidate existing activations; adopt only deterministic native joins. Keep historical identities and unresolved attention.
5. Recover available native history since each stored coverage anchor. Separate first history enrollment from a previously enrolled gap. For Codex, use bounded `thread/turns/list` pages, qualify terminal evidence by Section 12.6, revisit nonterminal turns, and merge only established native identities as native turns.
6. Apply buffered events and snapshots through the same reducer. A snapshot without a native revision remains an interval observation.
7. Mark coverage current only for the domains actually repaired. Retain explicit gaps where provider history is missing, ephemeral, deleted or unsupported.
8. Restore UI subscriptions from the resulting revision. Do not replay old entrance animations or notification banners as though all recovered events just happened.

Pagination must reach a known anchor or exhaust the source, not merely fetch one recent page. New native thread IDs seen during enumeration join the work queue. Detect cursor repetition/expiry and bound each iteration; keep catch-up pending rather than silently complete. [Codex turn pagination][O10]

### 4.15 Snapshot races and negative evidence

A snapshot with no native sequence/cursor has request-start and request-end bounds. No event arriving during that interval does not prove none occurred. A delayed event can invalidate a snapshot later.

Use native revisions/barriers when the interface provides them. Otherwise:

- Reject a response from an older observer epoch or superseded request.
- Never overwrite a concrete terminal turn outcome with a generic busy/idle field.
- Requery on a conflicting event; preserve conflicting evidence and mark uncertainty.
- Never infer durable deletion, exact request resolution, historical successful completion or source rebind from absence in a list.
- Permit a current activity observation to repair the activity display while leaving the historical turn outcome unknown.
- Check session activation again before an exact-current-session route.

Periodic polling obtains fresh evidence; elapsed time itself never becomes a completion or termination fact.

### 4.16 Late installation and irrecoverable gaps

With integrations already installed, closing Threadspace's UI changes nothing about capture. If the companion is unavailable, successfully published spool records replay later.

If Threadspace is installed after agents started, Claude's native inventory can identify existing sessions where IDs are present. New hook/mod activity enriches them. A running process whose current provider ID cannot be obtained remains a provisional discovery candidate.

On first enrollment with no prior Threadspace coverage, record an initial attention baseline with native scope/anchors and the import context. Imported preexisting outcomes have unknown handledness; do not invent hundreds of unread owner actions. They remain inspectable history, and the owner can explicitly promote a relevant output into attention. Current positively observed waits and separately captured live outcomes still use normal attention policy. This baseline is not an inferred resolution, a wall-clock cutoff or permission to discard missed outputs from an already-enrolled gap.

A record killed before durable acceptance may be unrecoverable. An unavailable relay plus failed spool can lose even the loss marker. Native histories repair only what they actually expose. The product must display unknown/coverage gaps rather than claim every missed event is detectable or recoverable.

## 5. Event journal, schemas and ordering

### 5.1 Observations, facts and derived state

An **observation** is a captured source occurrence. A **fact** is its normalized meaning at the source's actual scope. **Derived state** is the reducer's projection. **Attention** is a separate projection plus owner commands. **Semantic annotations** are self-reported content.

One observation can yield zero or several native-keyed fact drafts. Pure adapter normalization does not allocate or look up random canonical UUIDs. Inside the existing single-writer admission transaction, resolve/allocate canonical identities, record stable ID assignments and fact IDs, persist the resolved facts, then reduce them. Unprovable ownership remains an unresolved draft/diagnostic rather than borrowing a current session. The reducer receives resolved accepted facts and reads neither randomness nor the system clock. This is an admission step, not another service. Unsupported fields are ignored or retained in a bounded schema-diagnostic summary; unvalidated payloads never become state.

~~~typescript
interface CaptureClock {
  endpointId: UUID;
  bootId?: string;
  monotonicNs?: DecimalU64;
  wallTimeMs: number;
  clockQuality: "LOCAL_MONOTONIC" | "REMOTE_REPORTED" | "RECEIPT_ONLY";
}
interface ObservationEnvelope {
  schemaVersion: 1;
  observationId: UUID;             // retained through retry/spooling
  sourceId: UUID;
  sourceEpoch: UUID;
  sourceSequence?: DecimalU64;      // only when that source really supplies it
  sequenceMeaning?: "NATIVE" | "OBSERVER_CAPTURE";
  callbackEntrySequence?: DecimalU64;
  callbackResultSequence?: DecimalU64;
  adapterId: string;
  adapterVersion: string;
  providerVersion?: string;
  nativeEvent: string;
  sessionKey?: SessionKey;
  actorNativeId?: OpaqueId;
  nativeTurnId?: OpaqueId;
  nativePromptId?: OpaqueId;
  nativeOccurrenceId?: OpaqueId;
  activationRef?: UUID;
  capturedAt: CaptureClock;
  evidence: EvidenceRef[];
  payload: AllowedProviderMetadata;
}
interface CausalPoint {
  sourceId: UUID;
  sourceEpoch: UUID;
  orderDomain: OpaqueId;            // actual native or qualified observer counter scope
  sequence?: DecimalU64;
  nativePredecessorKeys?: OpaqueId[];
}
interface NativeFactDraft {
  kind: CanonicalFactKind;
  nativeRefs: NativeObjectRefs;     // namespace/session/actor/turn/activation evidence
  provenance: EvidenceClass;
  payload: ValidatedNativePayload;
}
interface CanonicalFact {
  factId: UUID;
  observationId: UUID;
  kind: CanonicalFactKind;
  sessionId?: UUID;
  actorId?: UUID;
  executionId?: UUID;
  turnId?: UUID;
  nativeOccurrenceKey?: string;
  provenance: EvidenceClass;
  payloadVersion: 1;
  payload: CanonicalPayload;
}
~~~

`EvidenceClass` includes `PROVIDER_EVENT`, `PROVIDER_SNAPSHOT`, `KERNEL`, `USER_ATTESTED`, `DERIVED`, `SEMANTIC_SELF_REPORT` and `UI_INFERRED`. This enum is provenance, not a universal scalar confidence score.

### 5.2 Canonical fact vocabulary

| Fact kind | Required meaning |
| --- | --- |
| SESSION_IDENTIFIED | Native durable/session identity learned or revisited |
| SESSION_RECORD_CHANGED | Archive/delete visibility fact or explicit local owner archive |
| EXECUTION_ATTACHED | Logical activation/attachment established with evidence |
| EXECUTION_ENDED | Matched execution ended; durable identity remains |
| PROCESS_OBSERVED / PROCESS_EXIT_OBSERVED | Kernel process evidence, independent of logical job lifetime |
| OBSERVATION_LINK_CHANGED | Observer connection/coverage state, not provider outcome |
| INPUT_SUBMITTED / INPUT_ACCEPTED / INPUT_REJECTED | Separate input attempt, accepted/queued entry and rejection |
| TURN_STARTED | Native turn began; provisional attempts do not automatically qualify |
| TURN_STEP_OBSERVED | Correlated native step/activity, including child turns lacking start event |
| RESPONSE_BOUNDARY_OBSERVED | Legacy Stop/output boundary that may be continued/vetoed |
| OUTPUT_READY | Native answer-ready notification; does not necessarily mean runtime quiescent |
| TURN_OUTCOME_OBSERVED | Native completed, interrupted, failed or refused turn outcome |
| ACTIVITY_PROPOSED / ACTIVITY_STARTED / ACTIVITY_FINISHED | Tool/item phases; proposal is not actual execution |
| PERMISSION_CHECK_OBSERVED | A permission preflight/decision occurred; no automatic human-wait claim |
| WAIT_STATE_OBSERVED | Native exact request or aggregate wait-category state |
| REQUEST_RESOLVED | A particular native request resolved, when its identity is available |
| ACTOR_IDENTIFIED / ACTOR_RELATION_OBSERVED | Stable actor and explicitly supported parent/tree/role relation |
| ACTOR_RUN_ENDED | A particular subordinate run ended; actor identity can be reused |
| WORKSPACE_OBSERVED | Cwd, repository/worktree or accessible-root observation |
| SURFACE_BINDING_RECORDED / SURFACE_BINDING_INVALIDATED | Versioned binding proof and invalidation |
| PROVIDER_SNAPSHOT_OBSERVED | Bounded native snapshot with scope, interval and coverage |
| OBSERVATION_GAP_DETECTED | Known gap; does not imply all possible loss is detectable |
| SEMANTIC_ANNOTATION_RECORDED | Self-reported/user semantic content |
| ATTENTION_ACKNOWLEDGED / ATTENTION_RESOLVED / ATTENTION_SNOOZED | Durable owner command outcome |
| NOTIFICATION_DELIVERY_RECORDED | Submission/known OS result, not proof the user saw a banner |
| LAYOUT_UPDATED / SETTINGS_CHANGED | Relevant persisted owner preferences and world layout changes |

Every fact has a payload schema and adapter/source preconditions. Facts do not carry executable commands, arbitrary scripts or provider decision outputs. Mod dispatch provenance is part of those preconditions: a plausible payload/native ID alone cannot earn PROVIDER_EVENT authority. Qualified Claude 2.1.291 does not expose direct lifecycle raises through the plugin-facing `$`; nonengine lifecycle-shaped inputs remain a synthetic defence for other profiles. Capture host-stamped dispatch origin at callback entry and require qualified native semantics or independent native corroboration for the particular fact. Unqualified dispatches remain bounded observation diagnostics, not native outcomes. [D-0005](decisions/D-0005-claude-2.1.291-observer-semantics.md)

### 5.3 Idempotency

Transport deduplication uses `observationId`. An ACK timeout followed by a spool retry reuses the same ID. Facts derived from an already-accepted observation must not be produced twice.

Where native occurrence IDs exist, use a semantic key scoped to provider namespace, session/actor, activation where necessary, turn, occurrence ID and phase. Tool completion and request resolution merge into their matching objects regardless of arrival order.

A content hash, same message text or a time window is not a universal deduplication key. Identical permission checks and repeated notifications can be genuine separate occurrences. For sources without native occurrence IDs, preserve separate raw observations and make state transitions idempotent at the appropriate episode/object scope.

### 5.4 Partial order and stale-event handling

The journal's `ingest_seq` orders local durable commits. It does not reconstruct native causal order and is monotonic, not gap-free: SQLite allocation/retention can leave gaps. Treat database cursors as opaque increasing positions; only qualified source/relay sequences and the explicitly contiguous per-subscription `streamSeq` use gap detection. SQLite integer cursors stay within its signed 64-bit range even though the wire uses validated nonnegative decimal strings. [SQLite AUTOINCREMENT][D_AUTOINC]

The reducer uses:

1. Native object identities and explicit predecessor/parent relationships.
2. Native sequence within its source epoch where supplied.
3. Immutable observer callback-entry/result order within the same module epoch.
4. Matched activation/process generations.
5. Snapshot intervals and qualified source revisions.
6. Explicit unknown/conflict when these do not establish an order.

A terminal outcome closes its matching native turn. A late earlier tool start cannot make that turn currently working again. A new native turn can make the same session working while the old completion remains in attention. Incomparable contradictory outcomes are retained and flagged; last wall-clock timestamp does not choose a winner.

Compare causal points as BEFORE, AFTER, EQUAL or INCOMPARABLE within their declared source/epoch/order domain or through explicit native predecessor relations. Independently numbered entry/result counters are not directly comparable; a qualified single module capture counter may order both, but does not by itself prove original human-gesture order through upstream middleware. Cross-epoch order needs a native relation, not wall time. No general vector-clock system is required.

Do not manufacture sequence numbers across independently spawned command hooks. The capture helper can number its own work, but that number is not provider execution order.

### 5.5 Deterministic reducer contract

~~~text
reduce(previous_projection, accepted_fact, reducer_version)
    -> new_projection
     + derived_attention_changes
     + notification_outbox_changes
     + reconciliation_requests
~~~

Exact replay of the same admitted journal/checkpoint reproduces recorded canonical IDs, durable domain state and owner-command results. Independent admissible arrival-permutation tests compare a semantic projection normalized by native keys, relation identity and stable attention scope, excluding allocated UUID values, ingest/revision cursors, receipt timestamps and allocation-order presentation metadata. They still require identical identity relationships, confirmed outcomes, uncertainty and unresolved attention. Do not demand identical newly allocated UUIDs or first-free desk assignments across separate admissions. Owner commands are journal entries with stable IDs; rebuilding cannot resurrect acknowledged, snoozed or resolved actions.

Projection changes, attention changes, outbox entries and the applied journal cursor commit atomically. OS side effects run after commit and record their outcome separately. The reducer never calls provider controls, launches a model, or performs focus itself.

The reducer keeps each record's evidence as sets (outcomes, end reasons, wait positives, clear barriers and owner decisions, resolution causes) and re-derives every displayed state from that evidence, so arrival order cannot decide a state that the evidence decides; an execution's attach mode/presence, the human follow-up frontier and a session's observer link state remain last-observation fields until M2, whose exit converts them to evidence sets before it adds competing producers. A wait with known turn identity belongs to that turn; a wait without it is session-scoped. Owner decisions on a wait item are kept with the evidence they were made on, so a late clear that repartitions episodes never reopens a condition the owner handled, and a new wait after a clear is not covered. The public read model shows the canonical session state. Admission alone allocates identities for never-seen native keys and records them; records the reducer derives (attention items, wait episodes, outbox intents) take name-based UUIDs of their canonical scope. In the semantic comparison, presentation metadata includes display names, summaries, route records and surface status, the outbox is compared by its currently eligible intents, a wait item whose episode a late, earlier clear emptied (resolved as superseded, holding no evidence or owner state) is omitted, and owner decisions are always compared by the evidence they govern. One row builder per record both writes and hashes the materialized projection, so the tables are checkable against the state at any time ([D-0007](decisions/D-0007-m1-canonical-engine.md)).


## 6. Lifecycle and presence semantics

### 6.1 Turn state machine

~~~mermaid
stateDiagram-v2
  direction TB
  [*] --> UNKNOWN
  UNKNOWN --> QUEUED: accepted input queued
  UNKNOWN --> WORKING: native start or identified step
  QUEUED --> WORKING: native start
  WORKING --> WAITING: confirmed wait condition
  WAITING --> WORKING: native wait cleared and work active
  WORKING --> COMPLETED: native completed outcome
  WAITING --> COMPLETED: native completed outcome
  WORKING --> INTERRUPTED: native interruption
  WAITING --> INTERRUPTED: native interruption
  WORKING --> FAILED: native terminal failure
  WAITING --> FAILED: native terminal failure
  WORKING --> REFUSED: native terminal refusal
~~~

This diagram describes one turn. A follow-up after terminal completion creates a new native Turn associated with the same Session; it does not rewrite the completed turn as working. Input steering an active turn may retain that native Turn ID. Follow provider identity and never invent a new Turn for every InputAttempt. Missing-start recovery can create a turn directly in a proven terminal state. A queued input with no known future native turn ID remains an InputAttempt until linked. [Codex steering/turn protocol][O10_TURN]

| Observation | Turn effect | Presence effect | Attention effect |
| --- | --- | --- | --- |
| Input submitted, not yet accepted | Record attempt; show preparing/queued evidence | None | None |
| Verified accepted human follow-up | Link/queue input; native start controls working | None | Resolve only causally prior eligible outputs |
| Native turn start/step | Working for that turn/actor | Corroborates live runtime | Old unread items remain unless separately resolved |
| Tool proposal | Activity proposed | None | No approval inference |
| Confirmed native wait | Waiting condition added; other actors may still work | None | Exact-request or aggregate-category item |
| Legacy Stop | Response boundary retained | None | No final-completion item by itself |
| Native output-ready notification | Output availability recorded | None | Eligible owner-facing output item may coexist with finishing/compacting |
| Native terminal turn outcome | Matching turn becomes completed/interrupted/failed/refused | None | Appropriate item only for an owner-facing outcome or escalation |
| Observer disconnect | Preserve last observed turn state with stale label | Observation disconnected | Integration-health indicator in setup/Diagnostics; no session owner action solely from transport loss |
| Proven interactive runtime close | Preserve turn outcome or unknown loss | Matched execution ended | Unresolved owner actions remain |
| Background worker process ends | Close process record; consult native job state | Job can remain parked/live | No invented failure |
| Resume | Preserve old turns; observe new/current turn | New activation or restored attachment | Unresolved semantic decisions persist |

`WAITING` requires native wait evidence linked to that particular turn/actor. Session-level inventory waits stay session-scoped: a completed parent with a waiting child retains its completed Turn, and siblings do not all become WAITING. Unspecified silence leaves the last state plus evidence age. A semantic external-wait/blocker annotation adds a visibly reported phase beside the observed turn state; it cannot change the canonical TurnState to WAITING.

### 6.2 Presence and observer health

Execution presence is derived from all relevant execution records:

- **LIVE:** a current runtime/actor is positively observed.
- **DETACHED:** native execution continues without an attached interactive surface.
- **PARKED:** the provider retains a resumable background job/runtime while no worker process is active.
- **ENDED:** all relevant activations have explicit close/exit evidence and no native live/parked continuation is known.
- **UNKNOWN:** evidence cannot settle current presence.

Connection state is separate. A disconnected app-server observer does not prove the daemon, thread or agent stopped. A missing heartbeat marks observation stale; it does not create `EXECUTION_ENDED`.

Kernel process exit has mode-specific consequences. For an embedded interactive CLI it can establish that runtime's end. For a provider supervisor, an idle worker may exit while its logical background job persists or transfers children/tasks to a replacement. Update only the ownership relations the qualified provider profile establishes. Never cascade termination through all descendants simply because one PID disappeared. [Claude background supervision][C3]

### 6.3 Visual state precedence

A worker's visual state is a projection, not the stored truth. It combines current work, strongest owner need, presence and observation quality.

1. Show stale/disconnected/conflict as a visible overlay and stop representing uncertain activity as confidently current.
2. Show confirmed approval and input needs distinctly, including counts or aggregate-scope labels.
3. Show terminal failure/refusal requiring action without hiding other live children.
4. Animate work while any relevant current turn is working; keep unread-output markers visible.
5. Show completed output/owner attention when no current work dominates.
6. Show ordinary idle/parked state when positively established.
7. Show departure only for qualified execution/session closure, never a Stop or completed response.

An actor can work while retaining an earlier unacknowledged result. Parent completion does not make active children disappear. A closed worker can leave an attention marker at its desk and remain in the attention queue.

### 6.4 Subagents, teammates and role

A subordinate Actor is upserted by its native identity and owning provider scope. A new run reuses that Actor; it does not automatically create another character. Store explicit immediate-parent, tree-root, teammate and fork relationships separately.

Claude conventional subordinate hooks do not by themselves provide every nested immediate parent. Use qualified mod spawn/list evidence. Codex tree-root hook IDs do not establish an immediate parent; use native thread metadata. Unresolved parentage appears in an “unresolved relationship” group, never silently as an independent root.

Internal helper agents, prompt-suggestion agents and similar transient loops are recorded when observed but collapsed by default. Agent teams are an opt-in supported profile, not a universal Desktop feature. The app never enables provider experimental teams on the user's behalf.

A child run finishing resolves its activity and may leave the visible child area. A child asking for input or still running remains visible even after the parent turn ends. The principal worker remains through ordinary turn completion.

## 7. Attention router and native notifications

### 7.1 Attention schema and identity

Owner-facing eligibility is explicit. Root or independently user-addressed outcomes can create owner attention. Determine eligibility from the originating turn/run/request's ownership and addressing evidence, not the Actor's current role. A child later becoming a root cannot retroactively promote its old parent-owned outputs; a later explicit escalation is its own fact. Ordinary parent-owned subagent completions and handled failures update activity/graph/history without creating an owner item. A subordinate creates owner attention only for a confirmed owner-addressed wait, explicit handoff/decision, or a qualified escalation. Five eligible owner-facing completions produce five addressable items; twenty internal helper completions do not produce twenty unsolicited owner notifications.

~~~typescript
type AttentionCategory =
  | "INPUT_REQUIRED" | "APPROVAL_REQUIRED" | "TURN_COMPLETE"
  | "ERROR" | "BLOCKED" | "HANDOFF_READY" | "OWNER_DECISION_REQUIRED";

type AttentionScope =
  | { kind: "EXACT_REQUEST"; nativeRequestId: OpaqueId }
  | { kind: "SESSION_WAIT_CATEGORY"; episodeId: UUID; waitKind: string }
  | { kind: "TURN_OUTPUT"; turnId: UUID }
  | { kind: "OWNER_DECISION"; decisionId: UUID };

interface AttentionItem {
  id: UUID;
  sessionId: UUID;
  actorId?: UUID;
  projectId?: UUID;
  category: AttentionCategory;
  scope: AttentionScope;
  createdByFactId: UUID;
  createdAtMs: number;
  causalPoint: CausalPoint;
  priority: number;
  summary?: string;
  summaryAuthority: "NATIVE_METADATA" | "SELF_REPORTED" | "USER" | "NONE";
  bindingId?: UUID;
  acknowledgedAtMs?: number;
  resolvedAtMs?: number;
  resolutionReason?: string;
  snoozedUntilMs?: number;
  notificationState: "NOT_REQUESTED" | "PENDING" | "SUBMITTED"
                   | "CONFIRMED_PRESENT" | "UNCERTAIN" | "FAILED";
}
~~~

A completion item is unique for the relevant session/actor/native turn/output purpose. `OUTPUT_READY` and a later completed outcome upgrade one turn-output item; they do not create two. Five distinct eligible owner-facing completed turns produce five addressable items, even if the visual list groups them.

Exact requests use native request IDs. Native wait flags without those IDs produce aggregate wait-category episodes. Repeated snapshots of one active episode do not create new items. If a gap conceals multiple identical waits, Threadspace does not invent their count or outcome. Scope each aggregate episode to namespace, Session, known actor/execution or native job, and wait category. Persist its positive witness and qualified clear barrier for that scope/generation. A late positive ordered before a clear is historical and cannot reopen current attention; a clear cannot resolve a newer activation's episode. Incomparable evidence remains uncertain and cannot automatically clear attention. Missing inventory rows are not positive wait-cleared evidence.

### 7.2 Creation, acknowledgement, resolution and snooze

**Creation** follows a confirmed actionable condition, a native output-ready/final outcome, or an explicit semantic request. A permission preflight alone does not create confirmed approval attention.

**Acknowledgement** means Daniel has deliberately engaged with an item. A successful `EXACT_NATIVE_SURFACE` route with `CURRENT_NATIVE_REVALIDATED` and `FOREGROUND_COMPATIBLE` input readiness can acknowledge automatically. A backgrounded/stopped provider or unknown foreground job cannot auto-acknowledge merely because its containing tab was focused. Other routing results require explicit acknowledgement. Acknowledgement does not erase an unresolved action.

**Resolution** means the action is no longer outstanding under its defined policy. Explicit owner resolution always records a reason. Native request resolution closes only that request. A fresh native aggregate wait clearing resolves the observed category episode as “wait ended,” not “approved.”

**Snooze** changes presentation/notification eligibility until a persisted time. It does not change provider state or resolve the item. A higher-severity new condition creates or updates its own item; it does not inherit an unrelated snooze.

The UI has “Needs attention” for unacknowledged unsnoozed items and “Awaiting action” for acknowledged unresolved items. Both counts are visible. A completion can be explicitly marked handled; error, handoff and owner-decision records show their distinct resolution rules.

### 7.3 Human-follow-up auto-resolution

Auto-resolution is deliberately narrow and is enabled only when `automaticHumanFollowupResolution=true` for a positively qualified original-order witness. Qualified Claude 2.1.291 has `acceptedInputProvenance=true` but `automaticHumanFollowupResolution=false` / NOT_SUPPORTED: its installed `prompt.submit` contract exposes original origin and the active-at-submission turn, but no positive original-submission order witness. Callback arrival, a missing `turnId`, or an assumed first/prepend seat cannot supply that ordering. Explicit Mark handled preserves the owner's resolution path; M1 implements the durable command and M2 qualifies the native control. This unavailable convenience does not block the native observer/MVP; exact current-session Return and qualified native outcomes remain required. [D-0005](decisions/D-0005-claude-2.1.291-observer-semantics.md)

When enabled, all of these conditions apply:

1. The input must be verified as actually accepted/enqueued by the provider.
2. Its original native provenance must be an allowed human origin.
3. Its original submission must be causally after the output being handled.
4. It must belong to the same persistent Session and applicable actor context.
5. The item must be an eligible previous output, not an unrelated approval, blocker or explicit owner decision.

Store input entry and result/acceptance points separately. Preserve native `prompt.submit.turnId` as the turn running at original submission, separately from a later new turn; that input cannot resolve the active turn's subsequent output. Callback entry/result order is this observer's order, not proof of original human-gesture order through upstream middleware delay or reload. If original causal order is unprovable, retain explicit resolution for that observation. An Enter queued before completion cannot resolve the later output just because acceptance returns afterward. If completion occurs before a human follow-up but spool replay delivers them in reverse, reduction must still resolve the old output after both facts arrive.

Reevaluate eligible outputs both when accepted-input evidence is learned and when a later-arriving output is upserted. Persist the compact accepted-human causal frontier/evidence in each actually comparable session/actor/source scope, including checkpoint retention, so reverse spool delivery produces the same resolved item. Do not retain only a transient list of items that existed when acceptance arrived.

For Claude mod input, a success-shaped result from `next(e)` is insufficient: downstream middleware can answer without reaching core. Require engine dispatch provenance, original `composer`/`bridge` origin, a settled successful core trace proving entry, and no drop. Preserve only allowlisted proof flags, not text or full traces. Missing or incomparable evidence disables automatic resolution and leaves explicit owner controls available. [Mod middleware][C5] [Native declaration contract][C8]

Scheduled prompts, task notifications, peer messages, SDK input, plugin submissions, unclassified input and presentation-only “as user” behavior cannot clear owner actions merely because a prompt event fired. Codex hook-only mode does not invent human acceptance provenance it cannot verify.

### 7.4 Priority and queue ordering

Default base priorities are: actionable terminal error/blocked 90; approval 80; required input 70; explicit owner decision 65; handoff 60; completed output 40. User pinning takes precedence. Within a category, oldest unresolved item sorts first; bounded aging can add up to ten points without changing its category or inventing urgency.

A retrying tool error does not become a terminal ERROR unless the provider reports terminal failure or an explicit blocker requests attention. Model-written urgency is self-reported and cannot silently jump the queue.

The queue supports project/provider filtering, next/previous item, acknowledge, handled, snooze, open inspector and the best available return action. Grouping and filtering never delete underlying items.

### 7.5 Notification ownership and actions

Native notifications are owned by the bundled companion through `UNUserNotificationCenter`. The companion queries actual authorization/settings and handles responses through the native delegate. It does not infer macOS authorization from a generic plugin returning granted. Native behavior is qualified under the system-started login-item app identity in M0A, with recovery/cold-start cases completed in M0C. [Apple notification authorization][N18] [Notification responses][N19]

Assign and strongly retain the notification delegate before the AppKit app finishes launching. Queue only validated internal IDs/actions until journal readiness, and complete every native callback. A notification-driven companion launch honors the persisted observation-enabled preference and writer lock: forward to a verified incumbent if present, otherwise show the outer app's inspector/service-state flow when observation is disabled or unavailable. It must not silently register supervision, start capture or acknowledge an action. [Delegate lifecycle][N_NOTIFICATION_DELEGATE]

Notification payloads contain internal attention/session IDs and a schema version, not commands, terminal text or secrets. On response, the companion reloads current state, checks whether the item remains outstanding, plans the current route and either:

- performs the verified current-session return;
- opens the app's inspector/paired-surface choice for lower verification;
- opens a grouped attention list for a batch notification;
- shows that an already resolved item is handled.

No notification approves a tool, types into a terminal, or resumes a closed provider session.

### 7.6 Durable outbox and duplicate control

Create notification intent in the same transaction as attention. Use stable request identifiers. Record OS submission outcome separately; OS acceptance is not proof Daniel saw a banner. Projection rebuild performs no OS side effects. Admission/recovery context distinguishes live delivery from startup/spool/history catch-up: hold catch-up output intents until currently recoverable eligibility/accepted-input evidence has been reconciled, then permit at most one recovery summary for newly recovered unresolved actions, preserving all individual items. If catch-up remains uncertain, show the durable queue/coverage state and hold its historical banners; unrelated live attention continues normally.

Immediately before OS submission, reread each item and suppress pending intent that is resolved, snoozed or otherwise ineligible under the configured acknowledgement/delivery policy. Rebuild a pending group from its still-eligible members and persist the disposition. A banner already submitted before late resolving evidence may have appeared; record that result and revalidate its click. Final attention convergence cannot retroactively prevent every stale banner.

On crash between submission and outcome recording, query pending/delivered identifiers when supported. If the result remains ambiguous, mark UNCERTAIN and retain the attention item rather than replaying a banner storm. Exactly-once visual notification delivery is not promised.

Coalesce a burst of completions into one summary banner over a short presentation window, while preserving all individual queue items. A group click opens the queue. Default banner suppression while the relevant app/attention surface is already foreground does not acknowledge or resolve items.

Focus mode, denied notifications, muted banners and closed windows affect delivery only. The attention queue remains authoritative. Notification→Threadspace→actual native target routing is a packaged-app acceptance test.

## 8. Local relay and fail-open capture

### 8.1 Selected transport

Use a private Unix `SOCK_STREAM` protocol with length-prefixed versioned JSON frames. The normal capture path has no TCP listener and no network dependency. Maintain separate `events.sock` and `control.sock` endpoints in one private runtime directory.

Create a short random directory such as `/tmp/ts.<uid>.<random>/` with mode 0700 using symlink-safe creation and ownership checks. Sockets are mode 0600; paths fit macOS `sockaddr_un.sun_path[104]`. Store the actual endpoint/generation locator in the user's application-support directory using atomic replacement.

On accepted connections, verify effective UID with `getpeereid` and, for direct provenance, sample `LOCAL_PEERPID` plus ProcessKey. A forwarded connection or spool replay does not preserve original peer identity. Do not claim a peer token proves the origin of previously stored records. [Apple Unix socket definitions][A7]

The control endpoint accepts typed application operations from the registered UI/native client. The event endpoint cannot execute focus, shell, approval or provider-control actions.

### 8.2 Capture sequence

1. Generate a stable observation UUID in canonical lowercase hyphenated form (receipts name it as given); capture minimal native IDs and process context.
2. Parse only allowlisted metadata with bounded input size/depth/time. Never echo raw provider input.
3. Attempt the current private endpoint with a bounded connect/write/receipt budget.
4. Receive `COMMITTED` or `ALREADY_COMMITTED` only after the journal transaction commits.
5. On unavailability/timeout, publish a spool record with the same observation UUID.
6. For conventional hook invocation, exit 0 with no stdout/control JSON and no routine stderr, including on delivery failure. The private mod-batch receipt contract is separate (Section 8.3).

The provider's configured hook timeout is an outer limit, not the ordinary operating budget. The capture helper's normal target is p95 ≤25 ms; its application-controlled wall budget is 250 ms, with a shorter 100 ms shutdown path. A healthy local socket operation uses a 20 ms connect budget and at most 75 ms waiting for durable receipt. Disk/kernel stalls cannot be given an absolute userspace scheduling guarantee; the provider's own outer timeout remains necessary.

Capture failure never returns a deny/block/continue decision, never retries provider execution and never waits for UI readiness.

### 8.3 Mod delivery

The qualified Claude mod enqueues small sanitized observations at callback entry/result and drains in the background. It uses documented host `$.process.run` with an argv array, JSON stdin and explicit `timeoutMs` to invoke the trusted capture helper. No shell interpolation is used.

Conventional provider-hook invocation remains silent and always exits 0. A separate `mod-batch` subcommand of the same executable returns a bounded typed receipt to the calling mod; this is not provider hook-control output. Its per-observation UUID status is `COMMITTED`, `ALREADY_COMMITTED`, `LOCAL_SPOOLED` or `NOT_ACCEPTED`. `LOCAL_SPOOLED` means successful publication in the actual local-spool durability domain, not journal commit. Validate receipt version, submitted UUIDs, one result per record and a 16 KiB response cap. Remove accepted records from mod memory only on a valid receipt; retain/retry unknown or unaccepted records with the original UUIDs within existing queue limits. Exit code 0, missing/malformed receipt and timeout are not acceptance. Partial commits followed by timeout safely replay through idempotency.

Retry unknown/unaccepted mod batches off the provider callback path using the qualified host `$.clock.after`, with one pending retry timer and exponential backoff from 250 ms to five seconds. Reset on successful receipt and cancel on module retirement/reload; one in-flight process alone does not prevent a tight failed-delivery loop. Allow at most one in-flight drain per module. Bound the queue to 2,048 records or 8 MiB, batches to 128 records/64 KiB, and schedule a drain promptly after enqueue. Batch splitting preserves source sequence. Idle modules do not run a frame-rate timer.

The inspected mod HTTP API supports Unix socket paths but does not expose the same caller timeout fields; it is not the selected primary drain mechanism. Do not invent a fetch AbortSignal/timeout option. Mod reload/host death can discard pre-ACK memory. A bounded normal-end drain reduces that window; it does not promise impossible zero-wait/zero-loss delivery. [Mod host APIs][C6] [ProcessRunInit declarations][C8]

### 8.4 Durable spool and receipt meaning

Spool layout uses exclusive temporary files and atomic ready-file publication:

~~~text
capture-spool/
  quota.lock
  pending/<observation-uuid>.<writer-nonce>.tmp
  ready/<observation-uuid>.<bytes>.json
  dropped/<observation-uuid>.<event-or-reason>
  quarantine/<reason>/<observation-uuid>.json
~~~

Write a complete bounded record, flush according to the selected platform policy, close it, and atomically rename into ready. Each capture is its own process, so the bound check and the rename happen under one advisory `quota.lock` (`flock`) held only for the recount, the decision and the rename; the kernel releases it if its holder dies, so a killed publisher leaves no reservation. A publisher waits for it at most 50 ms of its 250 ms budget; one that cannot take it in time does not publish and leaves a `spoolbusy` loss marker. The record bound counts ready files; the byte bound counts their real sizes, never pending temporaries. The reader ignores temporary/partial files. Normal replay removes a ready record only after a durable journal receipt. The explicit local-spool age/quota cleanup below is an exception for records not yet accepted by the companion: the expiry/coverage loss is journaled before the record leaves ready or a saturation marker is removed, and it is a loss, not successful delivery. A record the journal refuses on its own (`NOT_ACCEPTED`) moves to quarantine, so it cannot hold back the records around it. ACK loss safely causes duplicate transport delivery.

**Durability domains:**

- Journal ACK means SQLite commit under WAL/FULL and the qualified macOS storage policy.
- A completed atomic spool publication survives ordinary process/helper crashes under the tested filesystem.
- Sudden power-loss guarantees require successful tested flush/fsync/fullfsync behavior; rename alone does not establish them.
- If the deadline or storage failure prevents durable publication, the record is not durably accepted.
- Relay and spool failure can lose the event and even its loss marker. Diagnostics are best effort, not omniscient.

Bound spool capacity to 256 MiB or 100,000 records, whichever is reached first; bound record age to seven days unless the owner explicitly preserves a diagnostic bundle. Drop optional high-volume activity detail before identity/outcome/owner-action records. Reserved health capacity improves gap reporting but cannot guarantee it under total disk failure. Saturation is visible as degraded capture once evidence reaches the companion.

### 8.5 Ingress limits and burst behavior

A normalized observation is at most 16 KiB; a transport frame is at most 64 KiB. Provider raw input is parsed with a configured 8 MiB byte cap, depth cap 32 and time budget; larger/slow/malformed inputs fail open. Strip prompt bodies, tool inputs/results, elicitation content and full assistant text by default. Required IDs are extracted before optional metadata is discarded when the parser can do so safely.

The companion groups commits for at most 10 ms or 256 facts, whichever comes first. Backpressure returns a typed nonblocking failure to the helper; the helper spools. Do not use unbounded queues in either process.

Schema mismatch, invalid UTF-8, unsupported event shape and oversized input create bounded diagnostics without logging raw payloads. Unknown additive fields are tolerated. Unknown discriminators cannot drive lifecycle.

## 9. Persistence and reconstruction

### 9.1 Database decision

SQLite is the durable local store. One companion process owns writes under a singleton lock. Use WAL, `foreign_keys=ON` and `synchronous=FULL`; qualify macOS flush policy and storage behavior in native tests. WAL/FULL establishes stronger commit durability than NORMAL, but application acceptance remains explicit about the storage/OS domain. [SQLite WAL][D1] [Synchronous policy][D2]

Use prepared statements and versioned migrations. Materialize bounded snapshots/pages in short read transactions and release statements/transactions before waiting for IPC, rendering, OS calls or ACKs; long readers can prevent WAL recycling. Retain automatic WAL checkpointing initially and qualify any tuning. Monitor database, WAL, checkpoint and backup sizes separately. The frontend, hooks, MCP and provider adapters do not independently write the database.

### 9.2 Logical schema

| Table | Important columns/constraints |
| --- | --- |
| endpoints | UUID, logical identity, boot observation, transport capabilities |
| provider_namespaces | UUID, provider, endpoint, canonical profile reference, compatibility profile |
| projects / repositories / worktrees | UUIDs, canonical path references, filesystem evidence, aliases, mutable metadata |
| sessions | UUID; UNIQUE(namespace_id,native_session_id); record state; optional project |
| actors | UUID; provider actor key; optional own Session; current role |
| actor_relations | child, related actor/session, relation kind, evidence, validity interval |
| process_incarnations | endpoint, boot, PID, birth; executable identity; exit evidence |
| executions | UUID, session/actor, activation, runtime mode/native ID, lifecycle |
| execution_processes | many-to-many execution/process links with role and validity |
| inputs / turns / activities | native IDs and scopes; acceptance/outcomes/provenance |
| source_surfaces / surface_bindings | native locator, endpoint/app and surface generation, proof, revision, validity |
| actors / actor_relations / inputs / activities | the corresponding canonical records |
| observation_sources | adapter/profile, source epoch, sequence/coverage, diagnostics |
| observations | UUID, ingest cursor, source/native metadata, sanitized payload |
| facts | UUID, observation FK, kind, canonical and native object refs, resolved fact JSON |
| identity_assignments | native key → canonical ID, entity, allocating cursor; one row per native key |
| admission_diagnostics | bounded unresolved-draft and unsupported-event records; never state |
| session_projection / actor_projection | current materialized view, revision, evidence quality |
| attention_items | durable scope/key, reason/priority, ack/resolve/snooze |
| wait_scopes | aggregate wait scope, category, generation and derived episodes |
| source_coverage | per source epoch: seen sequence ranges, gaps and reported gaps |
| attention_commands | command UUID, target, verb, actor, causal/receipt metadata |
| semantic_annotations | session scope, typed content, self-report/user provenance |
| notification_outbox / notification_attempts | stable request ID, attention refs, delivery uncertainty |
| provider_coverage | native turn anchors/cursors, in-progress IDs, gap intervals, initial import baseline and compact accepted-input causal evidence |
| remote_receipts | endpoint/relay epoch, contiguous committed relay sequence, acceptance domain |
| layouts / avatar_assignments / settings | durable presentation/user preferences |
| projection_checkpoints | reducer/schema version, through-cursor, compressed state, checksum |
| schema_migrations | ordered migration IDs, application version, checksum |

Representative constraints:

~~~sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  namespace_id TEXT NOT NULL REFERENCES provider_namespaces(id),
  native_session_id TEXT NOT NULL,
  record_state TEXT NOT NULL,
  project_id TEXT REFERENCES projects(id),
  UNIQUE(namespace_id, native_session_id)
) STRICT;

CREATE TABLE observations (
  ingest_seq INTEGER PRIMARY KEY AUTOINCREMENT,
  observation_id TEXT NOT NULL UNIQUE,
  source_id TEXT NOT NULL,
  source_epoch TEXT NOT NULL,
  source_sequence TEXT,
  captured_wall_ms INTEGER NOT NULL,
  received_wall_ms INTEGER NOT NULL,
  payload_version INTEGER NOT NULL,
  payload_json TEXT NOT NULL
) STRICT;

CREATE TABLE attention_commands (
  command_id TEXT PRIMARY KEY,
  attention_id TEXT NOT NULL,
  action TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  observation_id TEXT NOT NULL UNIQUE REFERENCES observations(observation_id)
) STRICT;
~~~

Complete generated schema definitions live in `crates/contracts` and `crates/journal`. The table map is normative; column evolution remains migration-controlled. Source sequence fields are nullable because some providers do not supply them.

### 9.3 Checkpoints, retention and bounded growth

The logical journal is append-only within retained history. Rebuild from a verified checkpoint plus immutable subsequent entries. Retention is explicit compaction, never undocumented deletion of inconvenient evidence.

Default budgets are 30 days and 1 GiB of ordinary detailed event-history payload, whichever is reached first. They are not a hard size cap on the entire SQLite store: protected unresolved attention, compact identity/causal/dedup evidence, owner-command receipts and required checkpoints may exceed that history budget and are reported separately. Never evict accepted unresolved actions to meet it. Logical row deletion, freelist reuse and physical file reclamation are distinct; storage exhaustion refuses durable admission/ACK and uses the existing bounded spool/loss behavior. Keep at least two verified checkpoints. Before deleting an older segment, prove a checkpoint reconstructs its surviving identities, activations, native dedup anchors, coverage, unresolved attention, owner actions and notification state.

Never prune unresolved attention or the identities/proofs needed to interpret it. Preserve compact confirmed turn outcomes, qualified source watermarks, aggregate clear barriers and accepted-human causal frontiers after detailed activity expires. A new reducer version must migrate the retained checkpoint representation; it cannot assume expired raw observations remain available to re-normalize. Dedup retention must cover the seven-day local spool window; remote relay receipt watermarks persist across compaction as specified in Section 17.1. An event older than the retained causal coverage cannot silently mutate current state; classify it as historical/needs reconciliation.

Do not retain full transcripts. Store a provider transcript reference only where useful, with the provider profile and evidence version. Unsupported internal file schemas are not runtime dependencies.

### 9.4 Crash, corruption and update handling

- On ordinary crash, replay accepted journal entries and owner commands; drain ready spool records idempotently.
- On an incomplete migration, rollback or enter explicit recovery mode. Never start a second database under a different path and claim the original state was restored.
- On corruption, preserve the original database and recover from the newest verified consistent SQLite backup plus retained journal/checkpoint evidence; a checkpoint stored only inside the corrupted database is not an independent backup. Report unrecovered coverage; do not synthesize successful histories.
- Before a schema-changing update, use SQLite's consistent backup API (or a separately qualified consistent snapshot method), not a copy of the live main file alone, and record version compatibility. [SQLite backup][D_BACKUP]
- Older binaries refuse a newer unsupported schema or reducer checkpoint and leave capture spooling available. The refusal writes nothing: a read-only preflight checks both before any pragma, migration or bootstrap write, reading a WAL-mode store's `-wal` without rebuilding its `-shm` and creating no sidecar. The one exception is SQLite rolling back a hot rollback journal, which restores the last committed bytes and is required before the store can be read at all.
- A singleton writer lock and protocol handshake prevent old/new companions from writing concurrently.
- UI reload is not a database restart.

### 9.5 Startup sequence

This is the enabled-observation startup path; disabled/control-only and maintenance starts follow Section 19.5 before capture admission. Start in RECOVERING. Acquire the writer lock; validate schema/checkpoint; restore projections; open private ingress; ingest queued captures; connect native provider observers; reconcile sessions, processes and surfaces; establish coverage; publish a revisioned UI snapshot.

Accept valid new observations during catch-up. Buffer/serialize projection updates through the one writer. Show recovered attention promptly, with stale badges where live checks are unfinished. Do not wait for the 3D scene before making the queue usable.

## 10. Provider adapter contract

~~~typescript
interface ProviderCapabilities {
  profileId: string;
  testedVersions: string[];
  stableSessionIdentity: boolean;
  nativeTurnIdentity: boolean;
  nativeTurnOutcome: boolean;
  acceptedInputProvenance: boolean;
  automaticHumanFollowupResolution: boolean; // separately proven original causal order
  exactRequestIdentity: boolean;
  aggregateHumanWait: boolean;
  liveInventory: boolean;
  replayableTurnHistory: boolean;
  immediateParentIdentity: boolean;
  currentSessionProcessLookup: boolean;
  supportedSurfaces: string[];
}
interface ProviderAdapter {
  probe(config: ProviderConfig): Promise<CapabilityReport>;
  normalize(observation: ObservationEnvelope): NativeFactDraft[];
  discover(cursor?: string): Promise<DiscoveryPage>;
  reconcile(request: ReconcileRequest): Promise<EvidenceBatch>;
  verifyCurrentSession(request: SessionVerificationRequest): Promise<Verification>;
  installationPlan(): Promise<InspectableConfigPlan>;
}
~~~

The actual Rust interface uses typed enums/results and cancellation/budget controls. `normalize` is pure. Discovery/reconciliation may be asynchronous but cannot call provider task-start, approval or resume APIs in observer mode.

Each profile records:

- exact researched and tested binary/schema versions;
- supported surface/runtime mode;
- required fields and qualified semantic interpretations;
- maximum event/frame limits;
- source precedence per fact domain;
- unavailable capabilities and resulting UI labels;
- installation and removal ownership;
- evidence fixture hashes and native acceptance result.

A capabilities probe is not a model inference call. Unknown versions start in LIMITED coverage until compatible schemas and behavior qualify. The UI shows the profile and actual coverage, not a single misleading “connected” badge.

## 11. Claude integration

### 11.1 Selected integration profiles

**CLAUDE_NATIVE_OBSERVER** is the primary profile: conventional passive registration, supported native inventory, and a pinned observer mod. It combines independently qualified native turn outcomes, actor graph and current session→process mapping with explicit capability flags for accepted-human provenance and automatic follow-up resolution. Accepted input does not by itself establish original causal order. An unavailable automatic-resolution facet does not remove otherwise qualified native observation or routing.

**CLAUDE_CLASSIC_LIMITED** uses safe command hooks plus native inventory when the mod is absent or incompatible. It still discovers sessions and shows native reported activity/waits. It does not invent definitive per-turn outcomes or human acceptance from weaker fields.

**CLAUDE_REMOTE** receives the same normalized contracts from an installed remote collector; local hooks cannot magically inspect a cloud process.

The installed user can manually enter `claude` in an ordinary independently opened Terminal tab. Neither profile requires a Threadspace launcher. An integration installed after a session began must verify actual reload/accessibility behavior; installing files alone does not certify that running process received them.

### 11.2 Conventional hook field matrix

Common fields include `session_id`, `transcript_path`, `cwd` and `hook_event_name`, with context-dependent `prompt_id`, `permission_mode`, `effort`, `agent_id` and `agent_type`. Current documentation also lists `scratchpad_dir`. Conventional input does not generally supply a native event UUID, ordered global sequence, PID, TTY or terminal ID. [Hook reference][C1]

| Hook | Relevant fields | Normalized use and limitations |
| --- | --- | --- |
| SessionStart | source; model/agent_type/session_title when available | Identify/register native session. startup/resume/clear/compact/fork semantics use returned IDs; not every event is a new execution. |
| SessionEnd | reason | End the matched logical incarnation; reasons include clear, resume, logout, prompt_input_exit, other. Do not delete conversation identity. |
| UserPromptSubmit | prompt; optional source in inspected declarations | Record submission. Can be scheduled/system/peer work and can be blocked. Do not retain prompt text. |
| Stop | stop_hook_active; optional last_assistant_message/background metadata | Response boundary. Other hooks can demand continuation. Never SessionEnd. |
| StopFailure | error, optional error_details | Native terminal API-error evidence; retain structured category without raw sensitive text. |
| Notification | notification_type, message, optional title | Supplementary native notification evidence; type availability/timing varies by surface. |
| PermissionRequest | tool_name, tool_input, suggestions, optional mcp_server | Permission preflight. No tool_use_id in this payload; not proof of an open human dialog. |
| PermissionDenied | tool_use_id, tool_name, reason | Auto-mode denial only; not all human/policy denial paths. |
| SubagentStart | agent_id, agent_type | Upsert actor/run; also occurs on resumes and repeated in-process teammate work. |
| SubagentStop | agent_id/type, agent_transcript_path, stop_hook_active | Vetoable run boundary; native mod outcome/list provides stronger finality. |
| TeammateIdle | teammate_name, team_name | Proposed idle boundary; names are not stable actor keys. |
| TaskCreated / TaskCompleted | task_id, subject, optional teammate/team | Proposed task transition; other hooks can veto. Task identity is not turn identity. |
| CwdChanged | old_cwd, new_cwd | Update workspace annotation without changing Session identity. |
| DirectoryAdded | directory, source | Add accessible root; does not necessarily change project of record. |
| WorktreeCreate | name | MUST NOT install an observer handler: registration replaces provider creation behavior. |
| WorktreeRemove | worktree_path | MUST NOT install an observer handler: successful handler claims responsibility for cleanup. |
| Elicitation | server, mode, optional elicitation_id/url/schema | Input preflight; ID may be absent; never log submitted form content. |
| ElicitationResult | action, optional elicitation_id/content | Proposed response can be altered by other hooks; exact outcome needs qualified evidence. |
| MessageDisplay | turn_id, message_id, index, final, delta | Display batches, not a general turn boundary. Excluded from MVP hot capture. |
| PreToolUse | tool_use_id, name/input | Proposed tool attempt, not guaranteed execution. |
| PostToolUse | tool_use_id, response, optional duration_ms | Successful tool result; no implied turn completion. |
| PostToolUseFailure | tool_use_id, error, optional is_interrupt | Tool failure/interruption, not necessarily whole-turn failure. |
| PostToolBatch | tool_calls with IDs/responses | Parallel batch completed; individual arrival order need not be total. |
| PreCompact / PostCompact | native compaction context | Phase annotation, not new identity. |
| PreModelSwitch / PostModelSwitch | version-specific model metadata | Branding/phase metadata only after field qualification. |

The primary profile installs minimal command handlers needed for registration and supplementary signals; mod observation supplies high-volume tool/turn facts. Classic-limited mode installs its supported tool hooks. Do not duplicate identical capture handlers across settings and plugin scopes.

Never install passive command or mod interception for WorktreeCreate/WorktreeRemove. Observe resulting directories through native session/cwd metadata and Git inventory.

### 11.3 Native inventory

Call `claude agents --json --all` in the configured profile. The supported output includes `cwd`, `kind`, `startedAt`; full `sessionId`/name when set; live `pid`/`status`; `waitingFor` when waiting; and background short `id`/`state`. [Agent-view reference][C3]

Map live `busy` to session activity evidence, `waiting` plus `waitingFor` to session-scoped native wait categories, and `idle` to reported idle. Inventory rows supply no turn/actor ID; they cannot reopen a completed parent Turn or set every child WAITING. `permission prompt` maps to aggregate APPROVAL_REQUIRED with a tool-permission subtype; `sandbox request` to aggregate APPROVAL_REQUIRED with a sandbox-decision subtype; `input needed` to INPUT_REQUIRED. `worker request` and `dialog open` remain generic session INPUT_REQUIRED without guessed actor/request identity. Background `working`, `blocked`, `done`, `failed` and `stopped` describe job/task state, separate from process presence. A background blocked row without a more specific wait reason uses the existing aggregate episode scope with `waitKind=JOB_BLOCKED` and its native job/execution context, creating BLOCKED attention without an inferred approval, canonical Turn WAITING or failed native Turn. [Native wait categories][C3]

`done` can mean ready for the next prompt while no worker process remains. `working` can include autonomous waiting between steps. Preserve these meanings. Do not parse `~/.claude/jobs/<id>/state.json` or supervisor roster files as stable APIs.

At startup and after gaps, enumerate all relevant profiles without cwd filtering. Record runtime/version provenance separately from the inventory command's binary version; an updated CLI cannot certify an older already-running process. A bare `claude` launch configured to open agent view is not evidence of a direct interactive conversation, and Threadspace must not change that preference to force qualification. For routing, take a targeted fresh verification from a full/qualified inventory and validate process incarnation. Inventory may start the provider's supervisor as a documented administrative side effect; M0B must establish behavior and ensure it never creates model work or changes the user's task/session.

### 11.4 Observer mod implementation

The public mod runs in a restricted host environment. It does not have Node globals, `process.pid`, unrestricted filesystem/network globals, or ordinary timer globals. Use qualified host APIs and generated declarations. [Mod API][C6] [Reference][C7]

Apply provenance checks before interpreting the following events. In qualified 2.1.291, `EventCalls` is engine/test-kit functionality; the plugin-facing `$` cannot directly raise turn start/step/complete or session start/end. Snapshot `next.origin` at callback entry; native lifecycle authority requires host-stamped engine/core origin plus qualified semantics and core settlement, or independent native corroboration. Preserve legitimate plugin-origin tool/spawn activity only when actual execution/relation is established; engine dispatch alone does not prove a downstream middleware performed a requested spawn. Host methods including `$.agent.list()` and `$.session.id()` are middleware-interceptable and need core-backed proof or a lower evidence tier. [D-0005](decisions/D-0005-claude-2.1.291-observer-semantics.md)

Load the pinned mod from a Threadspace-owned copy: loading writes generated declarations and `tsconfig.json` into its folder. After reload, identity learned through `$.session.id()` remains lower-tier until separate qualified native proof restores authority. The M0C observer does not promote an unchanged ID merely because another event arrives; M2 must qualify that restoration. Immutable callback-entry ownership never changes with reload or later identity proof.

| Mod event/API | Qualified contract |
| --- | --- |
| session.start | Module bootstrap only; does not refire for every logical clear/resume |
| classic.SessionStart | Refresh logical session activation and metadata |
| prompt.submit | Capture original origin/entry context; observe settled core result after next |
| turn.start | Native main-turn identity/start |
| turn.step | Identified native steps, including child turns; preserve async-generator forwarding |
| turn.complete | Native turnId, agentId when present, reason and interruption; no session deletion |
| tool.call | Native call ID; observe proposal and unchanged downstream result |
| tool.check | Permission decision preflight; ask can target classifier/headless host as well as human |
| agent.spawn | Capture the proposal's pinned parent/spawn identity; establish an actual returned actor/run/relation only from a settled successful core spawn trace or separately qualified core-backed actor inventory |
| session.end | Logical end with native session/resume identity |
| session.attach/detach | Client attachment metadata; not native macOS tab identity |
| $.agent.list() | Actor graph/status snapshot only at the qualified core-backed evidence tier; a middleware-supplied array is not independent native corroboration |
| $.session.id()/version() | Bootstrap/profile metadata, not a substitute for immutable in-flight turn ownership |

Mod `turn.complete` reason maps answer→COMPLETED, aborted→INTERRUPTED, error→FAILED, refusal→REFUSED. It does not prove every background child stopped. Qualify Stop-veto/automatic-continuation behavior: separate native turns may belong to one continuing owner request; do not generate misleading final-owner alerts for internal continuations.

Observer middleware always calls `next(e)` once with the original event, returns its exact result unchanged and propagates provider exceptions. Catch observer failures separately. Never catch a provider exception and call next again. Streaming observation must forward every chunk with the correct native generator contract.

Require engine dispatch origin, protected original prompt origin and settled core trace for accepted-human input. A lower middleware returning a plausible object without calling core cannot create INPUT_ACCEPTED. If an earlier middleware hides events or a build lacks proof fields, lower the affected capability and retain explicit owner controls. M0C's 2.1.291 native sessions exercise SDK/task-notification origins and successful main/child outcomes; composer/bridge provenance is declaration/test-kit evidence. No positive original-order witness qualifies, so automaticHumanFollowupResolution stays disabled. M2 qualifies interactive human input, the native Mark handled control and its additional outcome cases.

### 11.5 Approval, input and notification nuance

Permission preflights can be auto-resolved. Confirm current human wait through native inventory or a qualified exact native request source. Missing request IDs create aggregate category episodes.

Delayed `idle_prompt` is not a timer-based lifecycle boundary. Background notification types can refer to job outcomes without carrying a reliable target identity in their English text; use native inventory, not regex extraction. Model refusals, login/quota waits and terminal API failure remain distinct error categories.

Transcript writes can lag hooks; missing latest text is not missing completion. Store previews only when enabled, using the appropriate event field and sanitization rather than an assumption that the transcript is already flushed.

### 11.6 Surface coverage

Local CLI supports the primary Terminal path. Desktop Code and VS Code panel can run hooks/mods while lacking a controlling TTY; they need separate surface adapters. Claude Desktop Chat/Cowork is not automatically a Claude Code session.

Remote Control executes on the original host; browser/mobile detach is not termination. SSH/cloud execution needs an endpoint collector and actual plugin availability in that environment. Shared event schemas do not guarantee local hook files reach cloud sessions. [Desktop configuration][C10] [Remote Control][C11] [Cloud Code][C12]

Commands such as `claude attach` or `claude --desktop --resume` alter attachment/resume behavior. They are not automatic focus substitutes and remain explicit later actions.

## 12. Codex integration

### 12.1 Supported runtime modes

**CODEX_SHARED_DAEMON** is the ordinary 0.160.x TUI default when daemon auto-start is enabled, without embedded fallback. Threadspace observes an already-running daemon with read-only protocol operations plus native hooks; it never starts the daemon for observation. The mode provides thread/turn/wait evidence when qualified, but no guaranteed originating TUI identity. [D-0004](decisions/D-0004-codex-runtime-modes-and-daemon-probe.md)

**CODEX_EMBEDDED** applies to `--no-daemon` or the release's other explicit daemon exclusions. It observes execution through native hooks, optional legacy notify and process ancestry. It supports native bound surfaces but does not manufacture missing terminal-outcome or current-thread lookup capabilities.

**CODEX_DESKTOP_LOCAL** is qualified separately for the desktop-bundled runtime. The M0C-observed app-servers use stdio and expose no passive endpoint; this profile is limited to supported hooks/notify. Its executable/schema can differ from the standalone CLI. App activation and a verified current conversation route are separate capabilities.

Threadspace never launches a provider-owned execution merely to observe it and never calls `thread/resume` to acquire prettier telemetry.

### 12.2 Released hook schema

The researched release supplies common `session_id`, nullable `transcript_path`, `cwd` and `hook_event_name`. Most turn/tool hooks include native `turn_id`, with optional subordinate `agent_id`/`agent_type`. There is no universal event UUID, ordered sequence or source-terminal client ID. [Released schema][O2]

| Hook | Important additional fields | Canonical meaning |
| --- | --- | --- |
| SessionStart | model, permission_mode, source | Session context registration; source may be startup/resume/clear/compact/fork |
| SessionEnd | reason, currently other | Runtime/session closure; not persistent thread deletion |
| UserPromptSubmit | turn_id, prompt, actor metadata | Submission attempt; other hooks can block |
| PreToolUse | turn_id, tool_use_id, name/input | Proposed activity |
| PermissionRequest | turn_id, name/input; no tool_use_id | Permission preflight, not exact request identity |
| PostToolUse | turn_id, tool_use_id, response | Tool result; not necessarily terminal success |
| PreCompact / PostCompact | turn_id, trigger | Compaction phase |
| SubagentStart | turn_id, agent_id/type | Concrete child thread/run |
| SubagentStop | turn_id, agent_id/type, transcript path, stop_hook_active | Child response boundary |
| Stop | turn_id, stop_hook_active, last assistant text | Vetoable stop attempt |
| Interrupt | turn_id, model, permission_mode | Native main-turn interruption |

Do not invent Codex counterparts for Claude's StopFailure, Notification, PermissionDenied, CwdChanged, Elicitation or team task events. Released source confirms asynchronous hooks, but the selected objective capture path uses the small bounded synchronous relay for predictable durable receipt. Existing trust requirements for exact hook definitions remain visible; installation never bypasses provider trust.

### 12.3 Identity and actor graph

Map `Thread.id` to persistent Session identity. Treat `Thread.sessionId` and hook `session_id` as live-tree membership, not a substitute for concrete child identity. Map hook `agent_id` to the subordinate thread ID. Recover immediate `parentThreadId` through native thread metadata; a tree-root relation is not an immediate-parent relation. [Thread identity][O3] [Thread model][O4]

`ephemeral` threads can disappear without persistent history. Record their identities/runs while observed but do not claim later recovery if the provider no longer retains them. `canAcceptDirectInput` is experimental-only and unavailable to the selected nonexperimental passive observer; do not infer direct-input permission from its absence. In the researched multi-agent V2 profile, parent-owned ThreadSpawn children reject ordinary direct input/resume; do not promote one to root because a user selected it. An explicit parent-context action targets the proven parent Session and is labeled accordingly. Exact-current-parent focus cannot be reported as exact-current-child or auto-acknowledge a child item unless native evidence establishes that parent surface as the item's actual interaction surface. Otherwise retain the child inspector and explicit acknowledgement. [Child input ownership][O_CHILD_INPUT] [D-0004](decisions/D-0004-codex-runtime-modes-and-daemon-probe.md)

### 12.4 Passive app-server observer

Connect only to a configured/discovered authorized existing local daemon socket. Learn its version from the observer's `initialize` response, independently of the CLI and desktop builds. Do not periodically invoke the CLI against the owner's `CODEX_HOME`: even `--version` creates temporary helper directories and runs a janitor. Explicit setup diagnostics may invoke it with the documented scope; the M0C probe used a disposable home. The local wire is a WebSocket upgrade over AF_UNIX followed by JSON-RPC, not raw newline-delimited JSON. Initialization is a connection, not a task-start/resume request. M0C found no answering daemon; a live handshake and runtime/store qualification remain M6 work. [Daemon contract][O7] [Released client handshake][O_DAEMON_CLIENT] [D-0004](decisions/D-0004-codex-runtime-modes-and-daemon-probe.md)

Observer sequence:

1. Start a bounded buffering receive loop, connect the Unix socket, complete its WebSocket upgrade, send `initialize` with a distinct observer client identity, await the response, then send `initialized`. Do not advertise experimental API or interactive UI/auth/attestation/MCP capabilities this passive observer does not implement.
2. Register status handling before enumeration and qualify the answering daemon/runtime/store profile.
3. Enumerate `thread/loaded/list` with an explicit bounded `limit` and its cursor until complete; its default is unbounded. Then use metadata-only `thread/read` with request `includeTurns: false`; `Thread.historyMode` is a response field, not a request argument.
4. Consume globally broadcast `thread/status/changed`.
5. On changes/gaps, read bounded native turn pages and relevant metadata.
6. Reconnect and reconcile without changing provider runtime lifetime.

The released broadcaster sends thread status globally; full item/turn streams remain subscription-dependent. Do not assume all notifications reach a passive connection. Do not subscribe by resuming a thread, answer server requests, create turns, archive/delete threads, or change configuration from this observer. [Global status broadcaster][O11] [Read implementation][O12]

### 12.5 Native status and turn mapping

| Native data | Interpretation |
| --- | --- |
| ThreadStatus notLoaded | Runtime unloaded; persistent thread can remain |
| idle | Native current activity idle; inspect turn outcome separately |
| active | Active work; activeFlags can include simultaneous wait categories |
| waitingOnApproval | Aggregate native approval wait, not exact request ID |
| waitingOnUserInput | Aggregate native input wait |
| systemError | Runtime error state; retain concrete turn errors separately |
| turn/started | Confirmed native turn start when actually received |
| turn/completed / qualified recorded history outcome | Completed, interrupted or failed native outcome, using the provenance rules below |
| Unqualified returned history status | Provider-reconstructed or recovery-normalized metadata; not automatically a native terminal occurrence |
| error with willRetry=true | Retrying error; not terminal failure |
| thread/closed | Runtime closure/unload |
| thread archived/unarchived/deleted | Distinct durable record/visibility facts |
| item started/completed | Correlated activity when actually subscribed/received |
| serverRequest/resolved | Exact request resolution only when native request identity is available |

Do not turn `idle` into “succeeded,” or a terminal-looking history status into a recorded native occurrence. Read and qualify the relevant turn's evidence under Section 12.6. A passive aggregate waiting flag does not establish the exact approval count or which tool the owner accepted.

### 12.6 Recovering missed turns

Native pagination is a capability of a qualified reachable app-server/store, not an automatic property of every Codex profile. Embedded CLI does not by itself expose such an endpoint. If history is unavailable, retain the coverage gap; do not start another app-server or resume a thread just to obtain it.

**Recorded outcomes versus reconstructed history.** The released legacy history builder can create synthetic turn shells defaulting to Completed from message history without a native terminal event. Both history modes can also normalize InProgress to Interrupted when the effective thread status is not active, without recording an interruption. The public wire exposes `Thread.historyMode` and optional start/completion markers, but no universal outcome-provenance discriminator. Do not infer native identity from a UUID-shaped string. [Legacy builder][O_HISTORY_BUILDER] [Response normalization][O12]

Isolate these predicates in the exact 0.160.1 compatibility adapter and qualify the actual runtime/store implementation. D-0004 accepts installed macOS CLI 0.160.0's byte-identical cited source, not blanket runtime certification; the separately selected daemon package 0.159.3 remains unqualified. An answering unqualified daemon has LIMITED coverage until M6 qualification:

| Evidence | Permitted recovery |
| --- | --- |
| Matching qualified native terminal event/Interrupt plus turn ownership | Confirm that native outcome |
| Qualified local `historyMode=paginated` row, Completed or Failed | Recover recorded native-history outcome with PROVIDER_SNAPSHOT provenance; the pinned store derives these from native TurnComplete |
| Qualified paginated Interrupted with a non-null `completedAt` terminal marker | Recover recorded interruption under that source contract |
| Interrupted without terminal marker or matching native corroboration | Preserve provider-reported recovery classification, leave definitive native outcome uncertain; do not invent cancellation cause/time |
| Legacy row with independent matching qualified native terminal outcome and turn ownership | Recover that corroborated outcome |
| Legacy `startedAt` present, even with `completedAt`, without matching terminal corroboration | A native start/ID may be established by the pinned builder; terminal classification remains strong/reconstructed history evidence and cannot create confirmed completion attention |
| Other legacy rows, unknown history mode or unqualified store/runtime | Display reconstructed history; no native outcome, native causal anchor or completion attention solely from that row |

The paginated allowance depends on the inspected local materialization/projection path. The legacy builder can attach an unmatched terminal event to its current turn, so even both start and completion markers do not prove that the terminal event belongs to the returned turn. No public wire discriminator repairs that match. Marker **presence** can support only the specific structural fact established by that decoder; timestamp magnitude/order is never used as causal proof. A known native turn ID alone, or notify's OUTPUT_READY, does not prove a default-Completed history shell is final. Represent unproven shells as LOCAL_PROVISIONAL/history metadata, not stable native dedup/coverage anchors. Later concrete evidence can settle the matching unknown outcome; it does not overwrite a contradictory confirmed outcome without conflict handling. [Native history projection][O_HISTORY_PROJECTION] [Local materialization][O_HISTORY_MATERIALIZATION] [Turn-store writes][O_HISTORY_STORE]

Use `thread/turns/list` with `threadId`, bounded `limit`, opaque `cursor`, `sortDirection` and `itemsView: "notLoaded"` where supported by the pinned schema. Persist native turn anchors and separately recheck previously in-progress turns. Do not repeatedly request unbounded `thread/read(includeTurns=true)` history. [Turn pagination][O10]

Page until a known anchor/exhaustion, merging by native turn ID and handling concurrent new turns. Five recoverable recorded native outcomes from a previously enrolled profile, with page size two, must produce five historical outcomes; attention then follows actual accepted-input causality. First history enrollment follows the unknown-handledness baseline in Section 4.16 and is not this offline-recovery case. A fixture with no later accepted human follow-ups retains all five eligible output items. A stale/repeated cursor or missing ephemeral history produces a coverage gap, not a fabricated repair.

History mode affects cost. The released legacy-history path can rebuild an entire rollout for each page, so a bounded response does not bound provider CPU or file-reading work. Coalesce recovery requests, allow at most one history request in flight per namespace, and rate-limit legacy requests to one per second while catching up. Do not repeatedly poll unchanged legacy histories. On slow/error responses, back off from five seconds to 60 seconds and retain pending coverage. A local response timeout does not prove provider computation was cancelled; prevent overlapping replacement requests while the original request remains outstanding. Qualify large legacy histories separately from indexed-store history. [Released history implementation][O12]

### 12.7 Stop, notify and completion

The released turn loop evaluates Stop hooks before deciding whether to continue. Therefore Stop normalizes to RESPONSE_BOUNDARY_OBSERVED. [Turn loop][O13]

The optional legacy `notify` command receives one JSON string as its final argv element, not stdin. Its payload includes `type: agent-turn-complete`, `thread-id`, `turn-id`, cwd, optional client and message fields. It runs after Stop continuation decisions but can precede post-turn compaction. Normalize it to OUTPUT_READY, retaining possible finishing activity. Native final turn outcome has higher precision. [Legacy notifier][O14]

Preserve an existing notify command through an inspectable dispatcher if this optional capture is enabled. Forward its original argv semantics, never shell-interpolate the JSON, and strip message bodies from Threadspace's record. Hook-only embedded mode openly has reduced coverage where native terminal failure/wait/acceptance cannot be established.

### 12.8 Source routing boundaries

For embedded CLI, native hook ancestry/launch registration can bind thread activation to the terminal. The hook itself has no controlling TTY. Root activation changes and subordinate events must not overwrite each other's binding.

For shared daemon, the daemon's startup environment is not per-TUI environment. Hook ancestry points to the daemon. Client info does not expose a current TUI PID/TTY; source/originator labels and model-context `window_id` are not macOS surface identifiers. [Daemon environment][O7] [TUI metadata][O8]

Ship these truthful actions:

| Binding | Offered action | Result ceiling |
| --- | --- | --- |
| Embedded native binding | Return to bound native tab | Exact surface + native last-known session relation unless fresh native verification exists |
| Shared daemon, unbound | Open inspector and pair a surface | No guessed terminal |
| Explicit UUID/manual pairing | Focus paired tab — current thread unverified | Exact surface + user-attested association |
| Explicit resume argv | Focus registered launch surface | Initial intent/last-known relation; not permanent current-thread proof |
| Verified future client→thread API | Return to current session | Enabled only after a new compatibility profile passes native tests |
| Remote/cloud surface | Open registered native URL/surface | URL/native-surface capability actually verified |

The first Codex E2E proves lifecycle, identity, independent sessions and the routing tier each mode can establish. It must not falsely inherit Claude's fresh inventory verification. Reduced routing certainty is an external interface boundary with a concrete usable product path.


## 13. Return-to-Agent and native surface adapters

Return-to-Agent is a native operation against current evidence, not a frontend URL handler disguised as successful focus. The companion owns the request, validates the target again and returns a structured result. The office's worker, attention drawer, command switcher and notification actions all use this one operation.

### 13.1 Surface contract

~~~typescript
interface SurfaceCapabilities {
  canIdentifyNativeSurface: boolean;
  canVerifyCurrentProviderSession: boolean;
  canFocusApp: boolean;
  canFocusWindow: boolean;
  canFocusTabOrPane: boolean;
  canRouteByTty: boolean;
  canRouteByNativeId: boolean;
  canRouteByUrl: boolean;
}
interface SurfaceAdapter {
  inventory(endpointId: UUID): Promise<SurfaceInventory>;
  validate(binding: SurfaceBinding): Promise<BindingValidation>;
  route(request: NativeRouteRequest): Promise<RouteResult>;
}
interface NativeRouteRequest {
  requestId: UUID;
  sessionId: UUID;
  attentionId?: UUID;
  chosenBindingId?: UUID;
  expectedBindingRevision?: DecimalU64;
  allowedFallback: "NONE" | "WINDOW" | "APP" | "PROJECT";
}
interface RouteResult {
  requestId: UUID;
  surfaceResult: "EXACT_NATIVE_SURFACE" | "EXACT_WINDOW" | "APP_ONLY"
    | "URL_DISPATCHED" | "PROJECT_ONLY" | "INSPECTOR_ONLY"
    | "AMBIGUOUS" | "UNAVAILABLE";
  sessionVerification: "CURRENT_NATIVE_REVALIDATED" | "NATIVE_BOUND_LAST_KNOWN"
    | "USER_ATTESTED" | "UNBOUND" | "CONFLICT";
  inputReadiness: "FOREGROUND_COMPATIBLE" | "BACKGROUND_JOB" | "UNKNOWN";
  bindingId?: UUID;
  validatedBindingRevision?: DecimalU64;
  reasonCode: string;
}
~~~

These capabilities describe the installed adapter/runtime combination. An adapter can identify an exact terminal pane without being able to verify which provider conversation currently occupies it. `CAN_IDENTIFY_EXACT_SESSION` is therefore represented by the stronger `canVerifyCurrentProviderSession` capability, never inferred from tab selection.

Adapters return app ProcessKey, native IDs, TTY/device identity, accessibility/automation requirements, inventory interval and dictionary/API version. Cache inventory for presentation, but refresh it for routing. Never serialize an AppleScript object reference and reuse it after an application restart.

### 13.2 Route decision and fallback

The native operation follows these steps:

1. Resolve the canonical session and, if present, the still-existing attention item. A stale or resolved notification can open its inspector but cannot target some newly active session.
2. Enumerate current eligible bindings. If multiple live attachments exist, require an explicit choice; currentness is not established by choosing the most recent.
3. Revalidate provider identity, activation, executable, ProcessKey, controlling TTY and native surface as far as that adapter supports.
4. For a verified target, perform the narrow native focus operation and read back the resulting selected surface.
5. Recheck the session/binding revision and available provider-currentness evidence after focus. Record a result, including any conflict detected during the operation.
6. Auto-acknowledge the selected attention item only under the rules in Section 7. An exact tab with an unverified current conversation does not satisfy that condition.

The default fallback is `NONE`. If an exact operation cannot be established, open the inspector with explicit actions for the evidenced window, app or project. An owner-selected action may authorize the corresponding fallback for that request. Never automatically cycle through other applications or open project folders after an unrelated target vanishes.

Within an authorized request, priority is exact current native surface, evidenced containing window, evidenced application, explicit project action, then inspector. URLs are a separate dispatch result: submitting a URL to macOS does not prove which browser tab or conversation becomes visible.

Routes are idempotent with respect to stored attention acknowledgement; repeating focus itself is harmless. Native automation has a two-second per-attempt budget and can fail with `AUTOMATION_DENIED`, `TARGET_GONE`, `SESSION_CHANGED`, `MULTIPLE_ATTACHMENTS`, `NO_LIVE_MAPPING` or `READBACK_FAILED`. The budget is one deadline on the monotonic clock, set when the request arrives: for a notification Return, when the companion receives the response, before it waits in any queue, so a Return queued behind another spends its budget rather than renewing it, and one whose budget is spent before it starts does no native work. A forwarded response carries how long ago its first instance received it; every native step (lookups, enumeration, the focus script and its settling, readback and post-focus revalidation) waits only for what remains of it, and a step still running at the deadline is stopped. A timeout records uncertainty, not success: a result reached after the deadline is `TIMEOUT`, never an exact surface or current verification, however good its late evidence, and a focus already issued stays recorded as performed. A queued attempt must check that the user's request is still current before moving focus.

### 13.3 Terminal.app

The selected MVP adapter uses the installed Terminal AppleScript dictionary and a fixed, bundled AppleScript invoked by a bounded native process worker. Apple documents Terminal scripting; the actual macOS dictionary is the qualification contract for tab properties and selection. [Terminal scripting][A2]

For a route, re-enumerate every live window and tab and obtain its current `tty` pathname from the installed dictionary. `stat` that path, require a character device, and compare normalized `st_rdev` with the provider's `e_tdev`; `st_dev` identifies the containing filesystem and is not this join. Do not open/read the terminal device. Require exactly one eligible tab and unchanged provider/Terminal application incarnations around enumeration. [Apple device metadata][A_STAT] Select that live tab object, unminimize and raise its current containing window, activate Terminal, then read back the selected tab's TTY and native frontmost application. Showing a window in another Space, such as its own fullscreen Space, completes asynchronously, so readback may wait read-only, for no longer than the route's remaining budget, for the target to lead Terminal's window order; it must then still match exactly. Revalidate the provider process and session around these steps.

A cached window ID and tab ordinal are hints only. Terminal provides no stable tab ID established by this research. Tab moves and reorder are handled by fresh TTY enumeration. A reused `/dev/ttys...` name cannot validate an old binding after its ProcessKey or activation ends.

Scripts are fixed resources; values are passed as data arguments to `osascript`, not interpolated into AppleScript source or a shell command. The worker uses argv execution, a bounded output buffer and a timeout. Keep native work off the renderer thread and serialize competing focus requests. No `do script`, newline, synthetic keypress, `fg`, resume command or terminal-output parsing is part of focus.

A backgrounded/stopped provider can still have the correct controlling TTY while a shell or another process group is foreground. Report `FOREGROUND_COMPATIBLE` only when current foreground-process-group and process-state evidence is compatible with the provider accepting input; a stopped/backgrounded provider or an unproven foreground job yields `BACKGROUND_JOB` or `UNKNOWN`. These lower readiness results cannot auto-acknowledge attention. Never send terminal input to “repair” foreground state.

M0B proves the packaged companion's identity/TTY/tab join, automation/readback and stale-binding rejection first; M0C completes Spaces/fullscreen, minimized restoration and selection/readback race qualification. Full Terminal.app restart remains unexecuted/BLOCKED on the owner environment and is assigned to isolated M15 native qualification by [D-0006](decisions/D-0006-m0c-environment-limits-and-window-shells.md). A Terminal process-generation change still invalidates every affected binding.

### 13.4 iTerm2, Ghostty and multiplexers

| Adapter | Selected native identity and focus strategy | Qualification boundary |
| --- | --- | --- |
| iTerm2 | Native session unique ID plus TTY; enumerate window/tab/session, select the session and its tab/window, activate and read back. | The split pane is the surface. Select every containing level; a pane-only selection does not imply frontmost window. |
| Ghostty | AppleScript terminal UUID and native focus command. If the installed dictionary exposes `tty`, use it for the passive process-to-surface join. | Shipped 1.3.1 is the researched stable build; TTY/PID additions are merged for the open 1.4.0 milestone. Probe the actual dictionary. |
| tmux | Server generation/socket, pane ID/TTY, and a specifically identified attached client/outer TTY; select the pane/window for that client and then focus its outer terminal. | Multiple attached clients require selection; detached panes have no current outer surface. |
| Native Codex/Claude desktop | App activation and only documented, build-qualified surface selectors. | A provider thread ID or model-facing `window_id` does not establish a native window ID. |
| Browser | Opted-in extension's browser profile/tab identity plus actual conversation URL, otherwise native URL dispatch. | Runtime tab IDs are ephemeral; current tab URL and conversation must be revalidated. |

iTerm's AppleScript and Python APIs support separate pane, tab and window operations. The first adapter uses AppleScript to avoid a Python dependency. The documented `iterm2:reveal?sessionid=...` route can dispatch a full valid identifier, but without native readback it is only `URL_DISPATCHED`. [iTerm scripting][A3] [iTerm Python activation][A8] [iTerm URL scheme][A9]

Ghostty AppleScript has existed since 1.3.0. The merged TTY/PID change is real, but merged main source is not proof it ships in the installed application. `GHOSTTY_SURFACE_ID` is not interchangeable with the AppleScript terminal UUID. An older installation without a passive reverse mapping uses explicitly linked native UUIDs or app-only routing. No guessed UUID conversion or frontmost-window correlation is allowed. [Ghostty scripting][A4] [TTY/PID change][A5] [Current download][A10]

tmux pane IDs are meaningful inside their server generation. `pane_pid` names the initial pane process, not necessarily the current provider or foreground process group. A detached pane, multiple outer clients and a nested mux are explicit routing states. MVP does not qualify nested mux chains; M8 must either prove the complete chain or leave it unresolved. [tmux manual][A6]

### 13.5 Native permissions

Apple-event automation requires the usage description and automation entitlement on both the event-sending companion and its containing application for the qualified profile. TCC attributes Terminal consent to the outer application, while the companion remains the only sender; test that actual arrangement after identity changes. A successful Terminal-launched script is insufficient. Use public AppKit/AppleEvents before Accessibility. [D-0003](decisions/D-0003-apple-event-consent-attribution.md) [Usage description][A11] [Automation entitlement][A12]

Accessibility is requested only for a separately enabled AX adapter or a specific native readback that needs it. Denial is a supported degraded state. Do not request screen recording, Full Disk Access, root privileges or input monitoring for the selected MVP. macOS may restrict foreground activation across Spaces; acceptance records the actual result instead of simulating success with input injection.

## 14. Projects, repositories and worktrees

### 14.1 Identity algorithm

Project identity is a persisted random UUID with a human-editable name. The initial automatic grouping is one Project per particular local Git repository/common directory. Several linked worktrees share that Project. The owner can explicitly group several repositories or directories into one Project without changing session identity.

For a verified cwd, run bounded native Git metadata commands with argv arguments, no shell evaluation and no writes. Resolve the worktree root, absolute gitdir, common gitdir, bare status and current branch/ref. Parse bounded outputs. Branch is nullable and mutable; detached HEAD is a normal state. Git's `rev-parse` and worktree plumbing provide these relationships. [Git repository paths][G1] [Git worktrees][G2]

Persist Repository and Worktree UUIDs, ordinary native URL bookmarks and path aliases. Within a verified boot/filesystem context, compare volume identity and current file-resource identity plus creation metadata. A rename can retain the mapping when that continuity is proven. Apple's file-resource identifier is explicitly not persistent across system restarts; an archived value cannot establish cross-reboot identity. On restart, resolve the native bookmark and revalidate the resulting Git structure and volume/context. A stale or unresolved bookmark is a relocation candidate requiring corroboration or owner confirmation. Paths alone are fallback locators. [Apple file identity][G3] [URL bookmarks][G3_BOOKMARK]

A deleted/recreated directory, changed volume identity, uncertain file-ID reuse or an unavailable mounted volume triggers revalidation. Never merge solely because a path or inode matches an old record. A clone with the same remote URL is a distinct Repository; a copied checkout is not automatically a move. Remote URLs are display/grouping suggestions only and must redact embedded credentials.

For non-Git directories, create a Project around the first verified root using the same persisted UUID and filesystem continuity rules. The owner can adjust that boundary. Unsupported filesystem identity is represented explicitly; retain the old place as unavailable until the owner confirms a relocation rather than assigning it to an unrelated recreated folder.

### 14.2 Session context and placement

A Session has a stable home Project, a current working context, optional additional directories and historical context changes. Initial verified repository context selects its home. A native CwdChanged event updates current context and worktree/branch metadata, but does not silently teleport a worker to a different Project. Offer “Move worker's home” when the new repository differs. An additional directory is not another session.

Worktrees appear as labeled desk clusters inside the Project. Separate top-level sessions in one worktree receive separate stable desks. The same session resumed in another worktree retains its avatar; show the current worktree and a context-change marker.

### 14.3 Stable layout

Persist project transforms, desk assignments, avatar seed/skin and owner overrides independently of runtime presence. Use deterministic free-slot allocation: reuse a Session's reserved desk; otherwise choose the first unoccupied stable slot in its worktree cluster, expanding that cluster without repacking existing desks.

New projects append to a stable grid of rooms. User movement overrides automatic placement. Removing an inactive Project hides it from the current office but retains its identity, unresolved attention and history. Auto-layout is an explicit owner action with undo, not a response to every event.

Ended workers leave their live desk but keep their reservation and history. Unresolved attention remains as a visible desk marker and in the queue. Resuming the same Session returns that worker; a new native identity receives a distinct worker even at the same path.

## 15. The Three.js office and operational interface

### 15.1 Graphics architecture

Use React and TypeScript for the application shell and operational DOM. One imperative `SceneController` owns Three.js objects and consumes immutable provider-neutral view models. React components never reconstruct the complete scene on every native event. Use direct Three.js initially; an additional React renderer is not needed for this application.

Import `WebGPURenderer` and node-material classes from `three/webgpu`, and TSL functions from `three/tsl`. Initialize asynchronously before rendering. Use the renderer's WebGPU path as the normal target; its WebGL2 backend is a diagnosed compatibility path only. No `WebGLRenderer` foundation, legacy `ShaderMaterial` shader pipeline, or old WebGL postprocessing chain belongs in the selected design. [Three renderer][R1] [Three WebGPU guide][R2] [TSL][R3] [WebGPU exports][R_WEBGPU_EXPORTS] [TSL exports][R_TSL_EXPORTS]

A minimal initialization shape is:

~~~typescript
import { WebGPURenderer } from "three/webgpu";

const renderer = new WebGPURenderer({ canvas, antialias: true });
await renderer.init();
const backend = inspectPinnedRendererBackend(renderer);
diagnostics.recordBackend(backend);
await renderer.setAnimationLoop(renderFrame);
~~~

`inspectPinnedRendererBackend` is a small version-specific diagnostics adapter. At pinned 0.186.1, require the initialized `renderer.backend.isWebGPUBackend === true` for a positive WebGPU result; isolate this source-dependent read and test it against a forced compatibility-backend run. An unknown backend shape is UNVERIFIED, never a passing WebGPU attestation. `isWebGPURenderer` identifies the renderer class and does not rule out its automatic WebGL2 fallback. Do not claim success based only on `navigator.gpu`, a successful adapter request or Safari support. [Renderer implementation][R4]

Expose renderer class, actual backend, Three revision, webview/runtime version, OS/build, origin/security context, adapter availability, device-loss count and frame statistics in Diagnostics. Do not collect hardware serials or send diagnostics remotely. WebGPU qualification must run inside the actual Tauri 3 Wry/WKWebView application, including the installed package.

### 15.2 Scene model and update flow

Use canonical IDs as keys for project nodes, desks, workers and relationships. An incoming projection revision updates a compact scene model. Animation interpolation runs independently of event frequency. Appearance changes derive from current state and attention; physics or animation completion cannot alter session state.

Instanced meshes handle repeated furniture and simple background geometry. Pool workers and effects, reuse materials/textures and dispose removed GPU resources. Use frustum culling, distance-based detail, a visible-avatar budget and clustered room badges for far-away workers. The attention drawer includes all items even when an avatar is culled.

A worker's stable avatar seed derives from its canonical presentation identity. `CharacterSkin` declares an asset version, geometry/material factory, bounds/selection anchor, state-pose mapping and explicit asynchronous resource cleanup. `AvatarInstance` contains only canonical presentation ID, skin/seed, scene objects and animation state; it cannot own provider lifecycle. Allow replaceable skins and asset packs; no fixed creature or art style is a correctness dependency. Retain a simple geometric skin for synthetic and native tests.

Tool activity is a bounded visual summary—tool category, active count and optional sanitized label. Parallel tools appear as a count/short list, not an unlimited swarm of effects. Do not send token streams or tool output to the renderer to animate activity.

### 15.3 Visual language

| Objective or attention condition | Office presentation |
| --- | --- |
| Live turn WORKING | Seated work loop; active workstation; optional tool badge |
| Native input wait | Readable question marker, attention icon and gentle call-for-owner gesture |
| Approval required | Distinct approval symbol and label; never distinguished by color alone |
| Completed turn, owner action outstanding | Worker remains, relaxes/raises hand; persistent completion marker |
| Interrupted/refused/failed turn | Different labeled outcome; unresolved owner action stays visible |
| Observation stale/disconnected | Coverage badge and subdued motion; does not impersonate an ended session |
| Subagent active | Related desk/position and selectable relationship edge; parent label |
| Subagent run terminal | Finish gesture, then leave active area if no continuing run; history remains |
| Execution ended | Short exit animation; retain desk/attention marker |
| Session resumed | Same worker returns to its reserved place |

Status precedence follows Section 6, while the attention count is independent. A working agent can still have an earlier unhandled completion. New work must not visually erase older attention.

The world is a compact, stylized office with an orthographic/isometric default camera. Click selects, double-click or the explicit Return button invokes routing, drag empty space pans, wheel zooms within useful limits. A selected worker has a persistent inspector. Camera motion does not change focus in another application.

### 15.4 Operational 2D surfaces

The application remains useful with 3D disabled or unavailable:

- A virtualized fleet list with provider, project/worktree, title, current turn, presence, coverage, attention count and routing quality.
- An attention drawer with Needs Attention and Awaiting Action views, priority sorting, explicit acknowledgement/snooze/resolution and one-item Return.
- A session inspector with current and historical turns, source evidence, actor graph, outstanding actions, selected surface and reconciliation status.
- A keyboard switcher that searches titles/projects/providers and ranks attention first; it routes only after explicit selection.
- Project navigation, connection/setup health and Diagnostics.

Use the same native queries and actions in 2D and 3D. No key function exists only on a canvas hit target. The inspector's evidence view explains why a session is considered working or why current routing cannot be verified; it does not expose raw provider secrets.

### 15.5 Native feel and accessibility

Use a native macOS window with traffic lights, standard fullscreen/minimize and a titlebar integrated with the React toolbar as specified in Section 18. Adopt system light/dark mode, readable typography, quiet selection states, reduced motion and native keyboard conventions. Keep a visible connection/coverage indicator.

DOM controls expose labels, focus order and VoiceOver summaries. Provide textual equivalents for every state, provider, relationship and attention category. Reduced motion disables idle character loops, camera easing and pulsing effects while preserving state changes. Sounds are off by default. A compact operational layout replaces continuous animation for high-load or low-power use.

### 15.6 Hidden windows, rendering failure and GPU recovery

At the pinned Three.js revision, `setAnimationLoop(null)` clears the application callback but leaves an internal requestAnimationFrame loop running. On a confirmed native hidden/minimized transition or entry into 2D mode, clear the application callback, release scene GPU resources and await disposal of the initialized renderer. Preserve the CPU scene model and canonical projection; do not call private `_animation` methods or treat browser throttling as zero scheduled work. On visible return, hydrate current state, create a fresh renderer, initialize and attest its backend, and rebuild the scene. Serialize init/dispose/rebuild by generation so a late initialization or callback cannot revive a retired/hidden renderer. A disposed renderer is never reused. [Internal animation scheduler][R_ANIMATION] [Renderer disposal][R_RENDERER]

Keep the React projection connected at a coalesced low rate when useful; the companion continues full ingestion, durable attention and notifications regardless. Count internal scheduled Three work, not only application render callbacks, in the hidden-office gate.

Register generation-bound `renderer.onDeviceLost` handling before initialization, preserving the renderer's existing handler, and send only allowlisted loss metadata to the recovery controller. Handle `renderer.onError` separately; a validation error is not automatically device loss. On an actual loss while visible, retire/dispose the renderer and attempt one bounded rebuild from hydrated CPU state. While hidden, defer recreation until visible. Initialization failure or repeated recovery failure retains operational DOM controls and diagnostics, never a journal reset or session-end event.

Qualification may inject the loss callback against a real initialized native renderer to test bounded recovery; label that evidence as injected. Intentional `GPUDevice.destroy()` does not prove the natural loss callback works: this pinned backend suppresses loss reason `destroyed`. Record an actually observed GPU loss separately when available. [Pinned backend][R7]

WebGL2 fallback is visibly labeled `WEBGL2_COMPATIBILITY`. It can keep the office usable on an unqualified environment, but **does not pass M0A's WebGPU requirement on the intended target Mac**. Never enable hidden/private WKWebView preferences to force a backend.

## 16. Optional MCP semantic enrichment

### 16.1 Responsibility and transport

Implement a small `threadspace-mcp` stdio executable backed by the companion's private semantic endpoint. Its capability is semantic annotation, not provider control. It is optional and first ships in M9.

Use the official MCP protocol and SDK version qualified at M9; record the negotiated protocol and exact dependency lock. Stdio keeps the personal integration local and avoids another localhost HTTP service. Do not use model-provided session IDs as authorization. [MCP specification][S1] [MCP stdio transport][S2]

On connection, the native bootstrap resolves its provider ancestry and requests a connection-scoped enrollment from the companion. Enrollment binds endpoint, namespace, Session, Actor when proven, activation, native source evidence, expiry and allowed tools. Every automatic enrollment requires a qualified provider-scoped connection/per-call identity or a fresh native current-session lookup; ancestry and absence of an observed switch are insufficient. Initial automatic MCP support is restricted to the qualified Claude CLI profile, where native current inventory corroborates the provider process/session before each mutating semantic call. A session switch invalidates the old enrollment. Embedded Codex needs a separately qualified current-session scope before automatic MCP binding can be enabled.

Enrollment is immutable for the lifetime of its stdio connection. Never retarget connection A to Session B when current inventory changes; invalidate it and refuse scoped mutations until a new independently proven connection or trusted call scope exists. Initial inventory proves a root Session/process relation at sampled instants, not which subordinate made a shared MCP call. Leave Actor unspecified unless separately proven; missing actor metadata is not proof of a root caller.

An ambiguous shared daemon cannot enroll a global MCP process as one of its client sessions merely from daemon ancestry. Disable automatic semantic binding for that mode until an actually scoped provider integration exists. Owner-created linked/manual workers may receive explicitly owner-scoped annotations, labeled as such; that does not establish provider-currentness. Connection tokens are not shown to the model, written into prompts or accepted as arbitrary tool arguments.

### 16.2 Tool contract

| Tool | Required fields | Effect |
| --- | --- | --- |
| `threadspace_set_task_title` | title, idempotencyKey | Set a semantic title, up to 120 characters |
| `threadspace_set_phase` | phase, optional detail, idempotencyKey | Update reported phase; enum plus bounded display text |
| `threadspace_report_checkpoint` | summary, optional evidence references, idempotencyKey | Append a checkpoint annotation |
| `threadspace_request_attention` | reason, summary, idempotencyKey | Create a semantic owner-action item |
| `threadspace_report_blocker` | summary, optional dependency, idempotencyKey | Create a semantic BLOCKED item |
| `threadspace_declare_handoff_ready` | summary, optional checklist, idempotencyKey | Create a semantic HANDOFF_READY item |

Bound each call to 8 KiB; summaries to 1,000 characters; reference/checklist arrays to 20 entries. References are inert data until the owner selects a validated URL/path action. Reject unknown fields that could imply execution, arbitrary notification titles or routing commands.

Every accepted semantic change becomes a journaled `SEMANTIC_ANNOTATION_RECORDED` or appropriately scoped attention fact with `SEMANTIC_SELF_REPORT` provenance, source enrollment and optional proven turn reference. A repeated idempotency key on one enrollment/scope returns the original result; conflicting reuse is rejected.

A semantic “finished” report cannot emit native `TURN_OUTCOME_OBSERVED`, clear native approval waits or mark an execution ended. “312 tests pass” is displayed as agent-reported text, not independently verified test evidence. Semantic attention is deduplicated by its explicit key and does not overwrite native attention.

### 16.3 Failure behavior

If enrollment, the companion or IPC is unavailable, return a bounded tool error identifying that no annotation was recorded. Provider lifecycle continues through its native hooks/inventory. Rate-limit enrichment to 10 calls/second per enrollment with a burst of 20; aggregate checkpoints in the UI. The tools expose no prompt-submission, shell, approval, merge or session-launch operation.

## 17. Remote execution and ChatGPT capability tiers

### 17.1 Endpoint-aware remote architecture

All durable identity includes a ProviderNamespace and endpoint authority. A remote PID/TTY has meaning only on that endpoint. Never compare a remote PID with a local macOS PID or map a cloud cwd to a local project without an explicit link.

M10 implements a **user-owned collector over an existing SSH connection**. The Mac companion initiates a noninteractive, host-key-verified OpenSSH connection to an explicitly configured host and runs the fixed remote `threadspace-remote serve --protocol=1` command. The remote collector reads its local private provider relay/spool and exchanges framed, versioned observations and acknowledgements over stdio. There is no public listener or required hosted backend. SSH configuration remains owned by the user; Threadspace does not copy private keys or bypass host verification. [OpenSSH client][S3]

Install the remote collector and provider hooks only through a separate explicit setup action for that endpoint. The collector's native provider capabilities are qualified independently of the Mac. Its supervisor, restart policy and data directory are recorded in the endpoint profile. M10 initially qualifies a second macOS endpoint; other OS collectors remain separate capability profiles rather than borrowing macOS process facts.

Remote capture commits an admitted record to its bounded durable spool and returns REMOTE_SPOOLED to the capture layer. This is distinct from MAC_COMMITTED, which follows the Mac journal transaction. Only MAC_COMMITTED releases a remote pending record. An admitted unacknowledged record is never silently age-deleted; seven days marks an overdue/coverage warning. When byte/count quota is full, reject new durable admissions while the provider hook still fails open, and report lost coverage when possible. Explicit owner discard is a recorded loss, not successful delivery.

Assign a monotonic relay sequence within a persisted relay epoch, retaining the original observation UUID. Replays preserve both. The Mac commits a contiguous relay receipt watermark atomically with accepted observations and retains that compact watermark across journal compaction. Reject sequence gaps for resynchronization and conflicting reuse; acknowledge a duplicate prefix without recreating facts. An explicit owner discard replaces a pending payload with a compact loss marker at its original relay sequence, so later records can advance through a recorded coverage gap instead of becoming permanently blocked by a missing sequence. This permits remote retries older than the local seven-day dedup window. A restarted collector retains its relay epoch/sequence; a replaced collector is explicitly re-enrolled. Network delay remains observation lag, never evidence a provider has finished or died.

Use a 1 MiB maximum remote batch, 64 KiB frames and a remote spool capped at 256 MiB or 100,000 records. Seven days is an overdue threshold, not pending-record expiry. Apply the same redaction and explicit loss accounting as local capture. The remote transport must not upload transcripts by default. Stop forwarding on endpoint identity mismatch. Retire/re-enroll an endpoint after a deliberate reinstall or key/identity change instead of merging unrelated session stores.

### 17.2 Remote surfaces

A remote session can have a provider-native browser URL, an explicitly linked local SSH/mux surface, or no accessible current surface. Its routing result uses the same proof axes as local sessions. The administrative SSH connection used by the collector is not the original work surface.

M10 records original provider URLs and supports native dispatch. A later opted-in browser extension can prove an exact local browser tab after validating its URL/profile. Remote project associations are explicit links to existing Projects or separate remote Projects. Remote state uses endpoint badges and coverage/latency diagnostics without changing the lifecycle vocabulary.

### 17.3 ChatGPT findings and selected behavior

Consumer ChatGPT Chat, local-only Work/Codex and cloud Work are different capability profiles even when a current desktop application contains them. Official documentation distinguishes those surfaces. No verified public interface in this research exposes arbitrary consumer chat lifecycle with the same semantics as the qualified Claude/Codex sources. [Desktop surface distinction][H1] [Current hook support boundaries][H2]

| Profile | Implemented/qualified tier | Available behavior | Unavailable claim |
| --- | --- | --- | --- |
| Qualified local-only Work/Codex | A, native provider after build/account qualification | Supported local native hooks and Codex protocol capabilities | Every Chat pane automatically shares those hooks |
| Native provider with known surface but incomplete events | B, partial lifecycle | Only supported events; separately qualified native/URL routing | Full lifecycle merely because the app can be focused |
| Consumer Chat web or desktop linked conversation | C, manual semantic worker | Original conversation URL, owner title/project, manual attention and URL/app routing | Authoritative completion/input/approval subscription |
| Opted-in DOM/AX observation experiment | D, UI-inferred | Visible state evidence with source tab/window and confidence label | Hidden/background UI inference as native lifecycle |
| Personal cloud Work | C or a separately proven semantic integration | Original URL and explicitly scoped semantic annotations | Local/plugin command hooks in its cloud orchestrator |
| Managed enterprise cloud hooks | Outside personal baseline | A future separately qualified provider profile | Availability on a personal account |

Current hook documentation explicitly distinguishes local-only execution from cloud orchestration. Running tools on a local computer does not by itself make local command hooks available to personal cloud Work. [Current hooks][H2]

A linked conversation requires its **original working URL**, supplied by the owner or captured by an opted-in browser integration. A ChatGPT share link represents shared conversation content, not the original live work session; reject it as an exact Return-to-Agent locator and explain how to link the original conversation. [Shared-link semantics][H3]

Native macOS release notes describe completion notifications and preferences for chat-link handling, but do not provide a public third-party notification subscription. Work with Apps brings app context into ChatGPT; it is not an outgoing lifecycle feed for Threadspace. [macOS release notes][H4] [Work with Apps][H5]

Do not invent `chatgpt://` or `codex://` conversation schemes. Use verified original HTTPS URLs and installed application activation. Without a qualified browser extension/native selector, label the result URL-dispatched or app-only. Never read private conversation databases, authentication cookies, hidden network endpoints or macOS notification databases as an MVP dependency.

### 17.4 Experimental adapter acceptance

M11 delivers a useful linked ChatGPT worker with manual annotations and Return by original URL. An optional visible-UI experiment is isolated behind an explicit flag and limited to a selected tab/window. It must retain `UI_INFERRED` evidence, survive tab/profile changes without cross-binding, and show unsupported/unknown when the tab is hidden, suspended or structurally unrecognized.

Closing a ChatGPT tab does not mean its turn completed or was cancelled. DOM wording, localization, missing spinners and elapsed time cannot upgrade it to Tier A. The absence of a public lifecycle source is a contained product limitation, not a blocker for the local Claude/Codex MVP.


## 18. Tauri 3 platform, IPC and application lifecycle

### 18.1 Accepted research candidate and dependency policy

**Tauri 3 is the desktop architecture.** The source-verified candidate is `3.0.0-alpha.4`, published October 1, 2026, at commit `a8703ee487c659efbebb27c799752d523a6d09a1`. It remains an alpha regardless of an inconsistent upstream release metadata flag. The supporting packages have deliberately different alpha suffixes. [Core release][T1] [Exact workspace][T2]

M0 rechecks the candidate and resolves the following coherent set into the application's own lockfiles. If a later build is deliberately selected at kickoff, it must replace this set through a recorded compatibility decision and pass the same gate. An automatic package-manager upgrade cannot make that decision.

| Component | Exact qualification candidate |
| --- | --- |
| `tauri` | `=3.0.0-alpha.4` |
| `tauri-runtime-wry` | `=3.0.0-alpha.4` |
| `tauri-build` | `=3.0.0-alpha.3` |
| Resolved `tauri-runtime`, `tauri-utils`, `tauri-macros`, `tauri-codegen` | `3.0.0-alpha.3` each |
| `tauri-plugin` build infrastructure, if actually needed | `3.0.0-alpha.3` |
| Project-local `@tauri-apps/cli` | `3.0.0-alpha.4` |
| Equivalent Rust `tauri-cli`, recorded for provenance | `3.0.0-alpha.4`; do not mix a second global CLI into builds |
| `@tauri-apps/api` | `3.0.0-alpha.2` |
| CLI's resolved `tauri-bundler` / `tauri-macos-sign` | `3.0.0-alpha.3` / `3.0.0-alpha.2` |
| Wry / Tao | `0.57.0` / `0.37.0`, locked transitively |
| Rust build toolchain | `1.99.0`, edition 2024, target `aarch64-apple-darwin` |
| Tauri declared MSRV | `1.95`; this is not the selected build-toolchain pin |
| Three.js | `0.186.1`, official r186 source commit `9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8` |
| SQLite engine | `3.53.4`, linked into the native companion |
| Deployment floor | macOS `26.0`; qualify the actual minor/build and WebKit runtime |

The core, CLI, npm and runtime manifests establish the Tauri set. Wry remains a WKWebView implementation on macOS; Tauri 3 does not automatically bundle a newer WebKit. Rust 1.99.0 is a published compatible toolchain candidate, not a compilation result from this research. [Runtime source][T4] [Package/lock sources][T_PACKAGES] [Rust release][T_RUST] [Wry source][T_WRY]

The official r186 tag's package version is **0.186.1**, so do not infer 0.186.0 from the release name. Its current graphics APIs include `RenderPipeline` for postprocessing and asynchronous renderer disposal; use the pinned implementation instead of older examples. [Three package][R_PACKAGE] [RenderPipeline][R_PIPELINE] [Renderer lifecycle][R_RENDERER]

SQLite 3.53.4 includes the documented WAL-reset corruption fix. Pin the actual linked engine and verify `sqlite_version()` and `sqlite_source_id()` in Diagnostics; relying on an unknown system SQLite or a Rust crate name is insufficient. M0 pins the Rust binding/build inputs so this exact native engine is used. [SQLite release][D4] [WAL caveat and fixed versions][D1]

Commit `Cargo.lock`, `package-lock.json`, `rust-toolchain.toml` and `docs/compatibility/platform-lock.json`. The platform lock records exact Node/npm, React/TypeScript/Vite, Rust SQLite binding, Xcode/SDK, macOS build, provider versions, bundle IDs, native dictionary hashes, source SHAs and renderer proof. Resolve unlisted application libraries once at M0 and record exact versions; do not write “latest” in the reproducible build recipe.

Use exact direct prerelease pins; validate the transitive family from `cargo metadata`/`cargo tree` and the npm lock. CI uses lock-preserving installation, no floating `@next`, no global CLI substitution, no branch-based dependency and no automatic prerelease drift. A later deliberate dependency-update task repeats the affected M0/native regression suite before replacing the accepted lock; one that moves Tauri, tao or Wry first removes the D-0008 window-release containment and runs the view-recovery gate without it.

### 18.2 Tauri 3 application structure and native boundary

Tauri 3 separates runtime selection from core. Add the direct Wry runtime crate and select it explicitly:

~~~toml
[dependencies]
tauri = "=3.0.0-alpha.4"
tauri-runtime-wry = "=3.0.0-alpha.4"

[build-dependencies]
tauri-build = "=3.0.0-alpha.3"
~~~

~~~rust
tauri::Builder::default()
    .runtime(tauri_runtime_wry::Wry::default())
    .manage(bridge_state)
    .invoke_handler(tauri::generate_handler![
        ui_connect, ui_ack, ui_disconnect, ui_query, ui_action
    ])
    .run(tauri::generate_context!())
    .expect("Threadspace runtime failed");
~~~

The default runtime type is `DynRuntime`; `Wry` is the runtime attributes selector and `WryRuntime` is the concrete runtime. Missing runtime configuration is an error. Core/runtime dependency direction and runtime-specific extension traits changed from the prior major. [Tauri 3 changes][T3] [Wry runtime][T4]

Keep all Tauri-specific code in `apps/desktop/src-tauri` and one audited macOS window adapter. The native core, provider adapters, persistence and surface discovery crates do not depend on Tauri. Managed Tauri `State` holds a bridge client, subscription registry, native UI intent queue and window settings—not a second authoritative state engine.

Current Wry-specific operations use the corresponding extension traits; runtime mismatch is a typed error. Main-thread dispatch uses the current `Manager` APIs. Never block the UI thread waiting for work scheduled to that same thread. Borrowed macOS webview pointers remain confined to the adapter and are not treated as owned Objective-C objects. One narrow exception, scoped to tao `0.37.0` and lapsing with that pin: recovery sets `releasedWhenClosed` on the retiring office window to release the reference tao 0.37.0 leaves unowned ([D-0008](decisions/D-0008-tao-0.37.0-window-release-containment.md)). [Tauri 3 changes][T3]

Remove old `macos-private-api` / `app.macOSPrivateApi` settings, removed runtime aliases and old `tauri` runtime feature assumptions. Do not import CEF helpers or Chromium entitlements into the selected Wry build. Tauri 2 examples are comparison material only.

### 18.3 Selected IPC mechanisms

The actual alpha.4 source supports frontend `invoke` and `Channel` from `@tauri-apps/api/core`, Rust commands and `tauri::ipc::Channel<T>`. Use those public primitives. The internal channel namespace still contains an unimplemented rename TODO; Threadspace never calls that internal namespace directly. [Frontend core][T8] [Native channels][T9]

| Operation | Mechanism and contract |
| --- | --- |
| Connect/hydrate | `ui_connect(request, events: Channel<UiFrame>)` |
| Applied progress | `ui_ack(subscriptionId, viewEpoch, highestAppliedStreamSeq, appliedJournalCursor)` |
| Intentional detach | `ui_disconnect(subscriptionId, viewEpoch)`, best effort |
| Queries/status/history | `ui_query(query, context)` with a typed, bounded result; bootstrap ConnectionStatus may omit context |
| Owner actions | `ui_action(action, expectedRevision, requestId, context)` |
| Native window/menu signals | Native handlers; ephemeral Tauri events only if useful |
| Provider state/attention propagation | One ordered Channel per current renderer incarnation |
| Provider ingestion | Private native socket/companion; never through the renderer |
| External URL/custom scheme | Native intent validation and queue, not provider event transport |

Commands use discriminated Rust enums and generated TypeScript types plus native runtime validation. A TypeScript generic on `invoke<T>` is not wire validation. Errors have stable codes, retryability and bounded user-readable detail.

`UiQuery` variants are `ConnectionStatus`, `FleetPage`, `AttentionPage`, `SessionDetail`, `ProjectDetail`, `Diagnostics` and `IntegrationStatus`. Responses include core/store generation, projection/row revisions and page continuation where relevant. Limit data queries to four in flight per native view and replies to 64 KiB; paginate larger results. Keep bootstrap status/ACK paths small and independently available. No arbitrary SQL, file-read path or shell query is exposed.

`UiAction` variants include `ReturnToSession`, `AcknowledgeAttention`, `ResolveAttention`, `SnoozeAttention`, `UpdateLayout`, `UpdatePreferences`, `SetProjectHome`, `LinkSurface`, `RefreshEvidence` and the explicit setup/disable/uninstall operations in Section 19. Each variant validates ownership, IDs, size, current revision and allowed semantics in Rust. A stale action returns conflict/current state without applying to another worker.

At office construction, put an application-owned native incarnation marker in that WebView's public `Manager::resources_table()` and retain its identity in the desktop registry. At entry to every command, including bootstrap and `ui_connect`, retrieve the marker from the actual incoming native WebView and match it against the active registry. Carry it through queued work and revalidate before effects. `Webview` equality is label-only in this pin; label existence and internal `Webview::is_registered()` are not this check. Use public `Resource`/`ResourceTable` operations behind the Tauri compatibility adapter. [Native WebView implementation][T_ACL_DISPATCH] [Public resources][T_RESOURCES]

For each subscribed query/action, `UiCallContext` carries the issued subscription ID, view epoch, core generation and store generation. The native handler validates it against the authenticated current webview registration, captures that incarnation at command entry, and carries it through queued work. Revalidate before accepting mutations or beginning focus; retirement rejects unaccepted old commands, invalidates queued focus and prevents stale query replies from updating the new view. ConnectionStatus and explicit service setup/recovery use a separate native bootstrap scope bound to the current webview, even before a companion subscription exists.

Persist command IDs and immutable payload fingerprints for durable owner actions. After authenticating the current native caller/context, check for an existing committed request before new-mutation preconditions: identical payload returns its recorded result; conflicting reuse is rejected. A new action validates its targeted domain object's revision, not the whole fleet revision unless the operation intentionally targets the fleet. A lost response/retry is not rejected solely because the first successful application advanced that object. An already durably accepted owner command remains committed and replayable by request ID even if its renderer closes; retirement does not roll it back. Read-only queries need no journal entry. Native focus results are recorded independently of a durable acknowledgement.

### 18.4 Snapshot, stream order and bounded flow control

A renderer creates a random `viewEpoch` and installs its Channel handler **before** invoking `ui_connect`. Events may arrive before the command promise resolves. The reply confirms registration; readiness comes only from a valid applied snapshot.

The companion performs one serialized `AttachView` operation: capture a consistent projection at committed cursor S, register subsequent updates, and enqueue the snapshot before all changes after S. The UI bridge has one serialized sender per subscription. Concurrent provider callbacks and Tauri callbacks cannot race unsynchronized writes into that sender.

Every `UiFrame` contains protocol version, store generation, core process generation, subscription ID, view epoch and a monotonic application `streamSeq`. Journal cursors use validated nonnegative decimal strings within SQLite's signed 64-bit range (Section 5.4); compare numerically, not lexically. The durable store generation changes on a replaced/restored store; core generation changes on companion restart.

Use `SnapshotBegin`, bounded `SnapshotChunk`, `SnapshotEnd`, `ProjectionPatch` and `BridgeHeartbeat` frames. A patch has the view revision/cursor range, full entity upserts or explicit tombstones, attention counts and page invalidations. Animation interpolation is not part of the durable stream.

**Selected flow-control policy: bounded complete initial snapshot.** Enforce at most 512 KiB and ten frames, each at most 64 KiB, before sending `SnapshotBegin`. The unacknowledged sender window is 32 frames or 2 MiB. Thus the complete snapshot fits without intermediate application ACKs. Stage it separately, validate all chunks, then replace the frontend projection atomically at `SnapshotEnd` and ACK its stream sequence and cursor.

If the complete requested view exceeds that cap, build a bounded initial view with explicit total counts, available rows and continuation tokens. Older attention/history and overflowing detail are paged through `ui_query`. Never silently omit an outstanding count or pretend a partial collection is complete. Page responses carry row/view revisions; older pages cannot overwrite newer subscribed rows. Patches to unloaded entities are full upserts, page invalidations or count changes, never partial updates applied to a nonexistent base.

ACKs include the highest **applied stream sequence and cursor**. Heartbeat/control frames may share a cursor, so cursor-only acknowledgement cannot release a frame-count window. Validate ACK monotonicity and require the acknowledged stream sequence to have actually been sent by that subscription, with the cursor represented by that frame. A newer paged-query cursor cannot advance stream acknowledgement. Reject future/impossible ACKs. A Channel send result is not application acknowledgement. When the window is exhausted or ACKs stall, retire the subscription and require a fresh snapshot instead of buffering indefinitely. The journal continues independently.

### 18.5 Reload, disconnect and recovery

Tauri Channels order their internal messages by index, but a missing internal delivery can stall later messages. Therefore an in-stream reset is insufficient as the only recovery path. [Native Channel ordering][T9] [Frontend ordering][T8]

Use a two-second bridge heartbeat while visible, a five-second initial hydration deadline and an independent `ui_query(ConnectionStatus)` check after five seconds without valid stream progress. These timers measure bridge liveness only. On timeout, generation mismatch, invalid schema, missing application sequence or a retired subscription, discard that Channel and call `ui_connect` with a new view epoch/subscription.

Native page load/close handlers retire old subscriptions and renderer epochs even if JavaScript did not unsubscribe; retired renderer epochs cannot reconnect. Native construction identity and renderer document/subscription epoch remain separate. Per-builder page/close callbacks capture their original native marker and ignore retired generations; alpha.4 can resolve a late callback's supplied WebView by label, so reading only that supplied handle is insufficient. For this compatibility profile, every main-document replacement/reload after initial load retires the native incarnation and recreates the actual office view before further connects or mutating bootstrap operations are admitted. This also rejects an old document's first-ever delayed `ui_connect`, whose epoch was not previously registered. Ordinary Vite HMR without document replacement is unaffected. A new webview under label `office` is a new incarnation. Old commands/frames cannot mutate the new view's state; already accepted durable commands retain their documented commit semantics. No callback handle becomes a durable identity.

The following cache-reclamation rule is an alpha.4 compatibility policy isolated in the desktop adapter, not a stable Tauri-wide API guarantee. A failed large Channel fetch can leave framework data in a per-webview cache. Dropping a Channel or reloading the page does not purge it; actual native webview/window removal does. Account for potentially unconsumed query/command callback replies as well as stream frames. If retiring a subscription with potentially unconsumed sent data, destroy and recreate the actual office view before admitting another full subscription. Preserve bounds, visibility and pending native intents, suppress last-window exit only during this controlled recovery, and wait for old registration removal before reusing its label. Do not call private cache APIs. User-requested Quit still wins. M0C proves content/cache release on this path, but each recovery retains an empty native window shell. This rate-limited, linearly accumulating C-04 defect is accepted only with mandatory M1 closure under [D-0006](decisions/D-0006-m0c-environment-limits-and-window-shells.md); lifetime resource growth is not claimed bounded. [Channel cache lifetime][T9] [Native removal cleanup][T_MANAGER]

On hidden→visible, sleep→wake or core reconnection, validate status explicitly; browser timers may have been suspended. Mark the displayed data synchronizing until current hydration is applied. A renderer outage changes observation presentation only; it cannot end provider sessions, clear attention or lose hook endpoints.

The native UI bridge also bounds its companion connection. A companion restart reconnects to the new authenticated locator/generation, reports unavailable while connecting and hydrates again. A Tauri UI restart never starts a duplicate journal writer.

### 18.6 Permissions, capabilities and content boundary

Generate a nonempty application command manifest with the exact Tauri 3 build API:

~~~rust
fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "ui_connect", "ui_ack", "ui_disconnect", "ui_query", "ui_action",
    ]);
    let attributes = tauri_build::Attributes::new().app_manifest(manifest);
    tauri_build::try_build(attributes).expect("Threadspace Tauri build failed");
}
~~~

This matters: current local custom commands bypass app-command ACL when there is no app manifest. A restrictive-looking capability file alone does not establish that boundary. [Build manifest][T_ACL_BUILD] [App dispatch][T_ACL_DISPATCH]

Use the one local `office` webview and explicitly select `office-local` through `app.security.capabilities`:

~~~json
{
  "identifier": "office-local",
  "description": "Threadspace views and typed owner actions.",
  "local": true,
  "webviews": ["office"],
  "permissions": [
    "allow-ui-connect", "allow-ui-ack", "allow-ui-disconnect",
    "allow-ui-query", "allow-ui-action",
    "core:window:allow-start-dragging",
    "core:window:allow-internal-toggle-maximize"
  ],
  "platforms": ["macOS"]
}
~~~

Omit remote origins and the `windows` matcher. Window and webview matches are OR, so adding a broad window matcher would widen the grant. Do not add a blanket local deny capability for other windows: the current deny path is origin-wide and can deny the office too. Missing allow rules deny unlisted views. [Capability schema][T_ACL_CAP] [Runtime authority][T_ACL_AUTH]

No generic shell, filesystem, SQL, notification or broad core default permission is needed for these five app commands. The two scoped core permissions implement titlebar drag/double-click zoom only ([D-0002](decisions/D-0002-titlebar-drag-permissions.md)). The framework's internal Channel transport has its own ownership handling; do not claim the five app permissions replace it or add invented internal Channel permission strings.

Serve only bundled local frontend assets in production. Cancel external document navigation/new windows in native handlers. Permit only the exact configured dev origin during development; do not authorize arbitrary localhost pages. Open explicit validated external links through native `NSWorkspace`. Apply a restrictive CSP validated with the selected runtime/Three build; use no remote script/CDN assets or renderer secrets.

Custom protocols are limited to the runtime's own application assets and IPC. No additional streaming custom scheme or general local file protocol is required. Use normal same-scheme bundled paths for models/textures/fonts, avoiding an unnecessary cross-scheme dependency.

### 18.7 Plugin compatibility and selected native implementations

Official Tauri 3 plugin releases already exist. At the inspected v3 workspace commit `d9be6d0492fb6746637ba64497237b2116aaec90`, the following Rust plugins and their six JS packages were released at alpha.2 on September 30. Their dependency ranges permit the selected alpha.4 family; that is manifest evidence, not a native execution pass. [Official v3 plugin workspace][T10]

| Capability | Rust plugin candidate | JS candidate | Threadspace selection |
| --- | --- | --- | --- |
| Notifications | `tauri-plugin-notification =3.0.0-alpha.2` | `@tauri-apps/plugin-notification 3.0.0-alpha.2` | Direct companion UserNotifications |
| Shell/process | `tauri-plugin-shell =3.0.0-alpha.2` | `@tauri-apps/plugin-shell 3.0.0-alpha.2` | Bounded Rust process workers/native routing |
| Deep linking | `tauri-plugin-deep-link =3.0.0-alpha.2` | `@tauri-apps/plugin-deep-link 3.0.0-alpha.2` | Native URL registration + Tauri RunEvent |
| Single instance | `tauri-plugin-single-instance =3.0.0-alpha.2` | None | Native launch/forwarding and core single-writer protocol |
| Window state | `tauri-plugin-window-state =3.0.0-alpha.2` | `@tauri-apps/plugin-window-state 3.0.0-alpha.2` | Native persisted settings |
| Store/settings | `tauri-plugin-store =3.0.0-alpha.2` | `@tauri-apps/plugin-store 3.0.0-alpha.2` | Existing native settings/journal owner |
| SQL | `tauri-plugin-sql =3.0.0-alpha.2` | `@tauri-apps/plugin-sql 3.0.0-alpha.2` | Native SQLite domain operations |

**No optional Tauri plugin is selected for the initial application.** This minimizes duplicated ownership and prerelease coupling. It does not claim plugins are unavailable. If a plugin is later selected, exact-pin its Rust/JS pair and prove it against the accepted runtime before adding it.

The v3 shell plugin removed `open` and its permissions; use `NSWorkspace` rather than importing an old shell-open pattern. The v3 store reload semantics reset defaults before disk merge, another reason not to assume unchanged APIs. The notification plugin's inspected macOS permission methods return granted without querying actual system authorization, so the chosen native query remains necessary. [Plugin source][T10] [Notification permission implementation][N17]

### 18.8 Window, menus, activation and deep links

Ship one primary `office` WebviewWindow. Settings, diagnostics and inspectors are panels within it. Multiple native office windows are not an MVP feature; the per-view IPC design remains valid if later added.

Use an opaque content surface with a native transparent/overlay titlebar, standard traffic lights and native fullscreen/minimize/zoom. Extend the toolbar into the titlebar using the accepted public Tauri 3/Wry and AppKit APIs. Keep draggable regions explicit and exclude buttons/inputs. No removed private-api switch or whole-window transparency is required.

Handle native activation/reopen on the main thread. Tauri alpha.4 provides `RunEvent::Opened { urls }` and macOS `RunEvent::Reopen`. Maintain a native pending-intent queue so launch-time notification/deep-link intent cannot be lost before React subscribes. [Native application events][T_APP]

Register one Threadspace-owned scheme for internal attention/session links using bundle metadata. Accept only defined path variants and UUIDs; the URI carries an identifier, not an executable command or permission to acknowledge. Re-read current state before dispatch. Native notification responses use the same internal intent structure without relying on a URL scheme.

Persist desktop window bounds/display/appearance in native main-app preferences, owned by the desktop shell so first-run and observation-disabled UI can restore them. World layout, project selection and camera belong to the companion's journaled layout domain; do not create a second writer for them. Validate restored bounds against current displays, constrain offscreen positions and honor reduced motion. Do not persist native window pointers or a stale provider focus action.

Native menus provide Open Office, Attention, Refresh Evidence, Settings and Quit Threadspace. Quit closes the UI; Stop Observation is a separate explicit command. No tray dependency is selected for MVP; a menu-bar extra may be added later through the native companion only if it improves use.

The desktop shell owns an independent native UI-instance lock and owner-only incumbent locator, including ProcessKey and a narrow private activation socket. A second launch verifies the incumbent and forwards only a validated open/attention intent, through the core when available or this direct native UI endpoint when observation is disabled/recovering. The incumbent opens the office/queues the intent and the second process exits. Stale locators are reclaimed only after process/lock validation; a live but unavailable incumbent produces a bounded failure. Do not start a disabled companion merely to obtain a UI arbiter. Finder/open behavior alone is not the singleton guarantee. The companion has a separate exclusive journal-writer lock.

### 18.9 Companion supervision, resources and signing

Use one `SMAppService.loginItem(identifier:)` for the real `ThreadspaceAgent.app` nested under `Threadspace.app/Contents/Library/LoginItems/`. Its `CFBundleIdentifier` is the registration identifier; it is an `LSUIElement` AppKit app with a strongly retained UserNotifications delegate and no WebView. The installed outer app owns registration, authoritative ServiceManagement status and unregistration through a native bootstrap path; these operations are not performed as though the companion were the containing app. The bootstrap path can run without constructing React or a WebView. [Login-item API][N_LOGIN_ITEM] [App layout][N15] [Agent app presentation][N_LSUIELEMENT]

Apple documents immediate launch, subsequent-login launch, and system relaunch after a login-item helper crashes or exits nonzero. This provides the required independent lifetime without a custom LaunchAgent plist, `BundleProgram` or a second notification process. Do not also register `SMAppService.agent`. An enabled healthy companion stays in its AppKit run loop; an unexpected fatal exit is nonzero. A deliberate successful exit is permitted only after observation/service stop or during OS logout/shutdown, not as an idle optimization. Crash-loop recovery stays alive in a bounded diagnostic state when possible. M0C must still prove force-kill restart with no UI and no new hook; source documentation does not pass that test. [Registration/relaunch contract][N_SERVICE_REGISTER]

This app-context choice also avoids depending on UserNotifications behavior from a bare LaunchAgent: Apple has documented that limitation in a support case. The chosen nested application still needs actual permission, banner and response proof in M0A; neither `LSUIElement` nor successful registration alone certifies notifications. [Apple notification context report][N_AGENT_NOTIFICATION]

Use `bundle.macOS.files` to copy the complete helper app relative to `Contents`. The v3 bundler copies custom paths before outer signing but does **not** automatically enroll arbitrary helper apps in its signing-target list. Build and sign the companion with its own entitlements first, copy it under `Library/LoginItems`, then sign the outer application; verify both independently. No post-sign bundle mutation. [Exact macOS bundler][T6]

Tauri 3 development restarts terminate the application's descendant process tree. Register the development companion from a stable development host bundle with distinct bundle IDs, stores, sockets and service identity; ServiceManagement starts it independently. A Tauri-spawned sidecar or direct shell child does not replace supervision. [CLI lifetime][T5]

The outer app and companion have separate minimal entitlements. Both carry the qualified Automation entitlement/usage description: the outer app is the TCC consent subject and the companion is the only Apple-event sender ([D-0003](decisions/D-0003-apple-event-consent-attribution.md)). Standard WebKit graphics do not justify blanket unsigned-memory/library-validation exceptions. Direct distribution has no App Sandbox or privileged-helper prerequisite. A stable local signing identity must qualify on Daniel's Mac; Developer ID/notarization belongs to the later public-distribution gate.

Enforce minimum macOS `26.0` in dev preflight and native startup as well as bundle configuration. Dev resources may resolve to source paths while production uses bundle paths, so package acceptance runs with the checkout/dev server unavailable. [Configuration][T7] [Resources][T3]

Supervision is positive evidence, never a default. A companion process is supervised only when launchd is its parent and its launchd job label is the companion bundle identifier, which is the login item's own job. A LaunchServices start such as a notification cold start, a missing or unrecognized label, or that label without launchd as the parent is unsupervised. The persisted observation preference records what the owner wants; effective capture admission also requires positive supervision, no maintenance phase and an awake machine. An unsupervised instance that holds the writer lock is control-only: it keeps the preference, records owner commands and opens the inspector for notification responses, but never polls providers or admits capture. Its idle lifetime is bounded; an active control connection may keep it alive. When the login item's companion starts and finds the lock held, it asks the incumbent to yield; an unsupervised incumbent yields, and the login item's companion takes the lock before opening admission. A supervised incumbent refuses. An unsupervised instance asked to enable observation refuses and stays until the login item's companion claims the store.

An accepted notification, navigation or inspector response always has exactly one durable owner until its lifecycle completes; process lifetime is never that owner. When a live response is received, its response record is committed in the store before any work on it; a response is durably accepted once its record, or its intent in the pending-intent backlog, is committed, and not before. A response that cannot be recorded is still offered to the backlog; if neither commits it is reported as not accepted and the attention item stays outstanding. Ownership moves only by committing the next owner first: a record is retired only after its intent is in a committed backlog, and a failed storage step leaves the previous owner authoritative and is retried when a view hydrates or consumes and at the next start. A Return is planned only for a recorded response and keeps its record until the resulting intent commits; a record that a starting writer finds, interrupted Return included, opens the inspector and never replays focus. The backlog holds at most 256 accepted, unconsumed intents in acceptance order and sends a view at most 32 unconsumed at a time; at its bound, as at the 256-record bound, a response is refused before acceptance, and accepted work is never evicted. A view's consumption is durable once the backlog without the intent commits, and the companion reports it done only then; if that commit fails the companion reports it, does not deliver the intent again, and retries the removal with every later commit, and a repeated consumption retries it and is done only if it commits, so only a restart before any successful commit can deliver it again, and the shell, which remembers what its views applied, does not apply it twice. The backlog remembers the last 256 consumed IDs so a lingering record is not applied again. Each commit writes a temporary file, syncs, renames and syncs the directory: a failure before the rename keeps the previous file; after the rename the state survives a process crash, and a failed directory sync is reported as unconfirmed against power loss, which is not qualified. A malformed backlog is moved aside and kept, a malformed record likewise, and an unreadable backlog is never overwritten: backlog commits are refused until it can be read. Every voluntary exit of a writer, a yield or an idle exit, first stops accepting, finishes what it accepted and waits for any response being recorded. An intent is named by its notification request, so one response yields one intent however often it is found.

If macOS disables the background item, report observation unavailable and keep hooks bounded/spooling; do not launch an unsupervised duplicate. Stop/update preparation occurs while the companion is still running, because `unregister()` can terminate it. Section 19.5 controls the prepare → unregister → verify exit/lock release sequence. [Unregister behavior][N_SERVICE_UNREGISTER]

### 18.10 Native risks and the early platform gate

WebKit source establishes macOS 26 as the WebGPU floor, but only the actual Tauri 3 app can prove the target path. OS restrictions such as Lockdown Mode are not bypassed with hidden preferences. [WebKit floor/preferences][T_WEBKIT]

Known reports guide concrete tests. They are not claimed reproductions on alpha.4/Wry0.57.0:

| Report | Test/containment selected |
| --- | --- |
| Wry #1822: asynchronous scheme response after task cancellation | Burst IPC while reloading/closing/sleeping; independent observer survives a UI crash |
| Wry #1848: stale presented frames despite live DOM/AX | Compare actual packaged screenshots/visible motion with state counters |
| Wry #1778: cross-scheme assets on a macOS beta | Use same-scheme assets; qualify any extra asset protocol before use |
| Wry #1730: secondary webview teardown | One office view initially; repeated close/recreate tests |
| Tauri #15471: transparency-related GPU cost | Opaque main content and measured native titlebar |
| Tauri #15315: .icon bundling failures | Standard .icns initial icon; qualify richer assets separately |

[Scheme race][T11] [Presented-frame report][T11_FRAMES] [Asset report][T11_ASSETS] [Teardown report][T11_CLOSE] [Transparency report][T11_POWER] [Icon report][T11_ICON]

**M0 is three executable phases, not one prerequisite wall.** M0A proves the pinned shell, independent companion, basic native IPC/storage/notifications and actual WebGPU. M0B immediately proves the real manually launched Claude → session → process incarnation → TTY → exact Terminal tab → Return path, before extended recovery/stress work. M0C then closes the remaining platform obligations within the explicit accepted D-0004–D-0006 scope. Each phase has its own evidence and exit review; M1 requires the final M0C gate. Bounded spike code is reused, while extensive art and unqualified architecture-dependent features wait. [M0 traceability and phase gates](MILESTONES.md)

Together the phases preserve all seventeen user-required properties: minimal Tauri 3 build; dev and package launch; request/response IPC; sustained native streaming; native notifications; notification interaction; required macOS execution mechanisms; Terminal inspection/routing; renderer-independent endpoints; SQLite persistence; restart restoration; sleep/wake integrity; WebGPURenderer initialization in the real webview; actual WebGPU backend; sustained packaged animation; native titlebar/window behavior; and no unresolved alpha issue invalidating the core architecture.

If a specific blocker reproduces, retain its exact package/OS versions and minimal reproduction, identify the affected requirement and implement a bounded repair within the Tauri 3 boundary before passing the gate. The research establishes a source-supported Tauri 3 candidate, not an already-passed platform. The native companion app/notification context and current-session correlation remain explicit early GO/NO-GO tests. No Tauri 2 baseline or “migrate later” plan is authorized.


## 19. Setup, configuration, privacy and upgrades

### 19.1 First-run flow

First run opens a usable empty office and a concise setup panel. It explains that observation continues through a native background companion, then provides explicit actions to enable it, connect a provider and enable notifications/Terminal routing. Do not require optional ChatGPT, MCP, remote hosts, other terminals or visual customization before the local workflow works.

The setup sequence is:

1. Check the actual OS/build and accepted native compatibility profile.
2. Register/check the independent companion and verify its authenticated IPC, SQLite and health.
3. Discover configured provider binaries/profiles without launching inference or altering tasks.
4. Present the exact provider integration change and its ownership/rollback information.
5. Apply the explicitly selected integration atomically, then run bounded non-inference diagnostics.
6. Request native notification authorization and Terminal automation when the corresponding feature is enabled.
7. Show each capability separately: native events, live inventory, turn semantics, actor relationships, current-session routing and background observation.

The application does not repeatedly ask for permission already granted in the session or OS. OS-required permission prompts remain native and their actual states are visible. Denial explains the affected feature and offers a direct retry/settings path without making an unrelated capability unavailable.

### 19.2 Idempotent and reversible provider installation

Each adapter produces an inspectable configuration plan containing detected provider version/profile, files touched, existing hashes, parsed additions/removals, owned identifiers, executable paths, verification and rollback. Plans contain no secrets in UI/logs.

Use the provider's supported user/plugin configuration surface. Preserve foreign hook arrays, commands, matchers, MCP servers and ordering. A parser/serializer must preserve meaningful configuration; if lossless merging is not possible, stop that specific edit and show the exact proposed replacement instead of rewriting unrelated data.

Before writing, reread and compare the current hash. Back up the original bytes with owner-only permissions, write to a same-directory temporary file, validate syntax/ownership, then atomically replace. Record the applied hash and exact entries Threadspace owns. Two installs produce one owned integration. Remove only still-matching owned entries on uninstall; if the user changed them, show a conflict rather than erase foreign edits.

For Claude, install the selected command hooks and qualified observer mod without overriding `WorktreeCreate` or `WorktreeRemove`. Do not enable experimental teams, replace the user's prompt, add blocking decisions or assume global settings reach every desktop/cloud runtime.

For Codex, install the supported native hook configuration for the qualified profile. Preserve an existing legacy notify command through an inspectable bounded dispatcher if that path is enabled; do not silently overwrite it. Default shared-daemon observation remains supported. Any optional `--no-daemon` shim is separately selected, plainly explains the mode change, and respects explicit remote/daemon choices.

Store stable executable references in a user-owned Threadspace integration directory. The installed helper has an explicit version and protocol compatibility range; update it by staged atomic replacement so hooks do not point into a deleted build checkout. Never put a provider token, transcript text or raw shell string into the configured invocation.

### 19.3 Native process and filesystem policy

Rust owns subprocess work. Resolve configured executable paths at setup, retain identity, spawn argv without a shell, bound input/output, apply timeouts and record exit status. The short-lived process worker cannot block the Tauri main thread. Route cancellations invalidate focus work that has not begun; ongoing OS calls return their actual outcome.

Use kernel process inspection and exit observation for already-known ProcessKeys, supplemented by bounded resampling. A kqueue-style exit notification is evidence for that process incarnation; it does not supply a provider turn outcome. On PID reuse, executable replacement, wake or observer restart, revalidate rather than reuse stale handles. [Apple process metadata][A1] [Process exit observation][N_KQUEUE]

Use one reconciliation scheduler per provider namespace, with no overlapping full inventory requests. Initial targets are a five-second live inventory interval, a two-second focused wait-detail interval when needed, and a 30-second inactive/health interval. Native event arrivals, process exits and route requests trigger targeted refreshes. Bound inventory execution and back off repeated failures; timers obtain evidence, never generate lifecycle outcomes themselves.

File access stays in the native core and only follows verified profile/project/owned-data paths. Resolve symlinks and ownership before writing. Do not grant the renderer a generic filesystem bridge. A project path supplied in an event does not authorize reading arbitrary files beneath it.

### 19.4 Local data and diagnostics

Default stored data is identity, lifecycle, activity category, process/TTY evidence, project references, bounded semantic text, attention and delivery status. Prompt bodies, full completions, tool arguments/results, source files, credentials and environment dumps are excluded.

Logs use structured error codes and shortened internal IDs. Paths and user-supplied labels can be redacted in a diagnostic export. Export previews list included files/fields; no background upload or telemetry is enabled. Native notification bodies are concise and can use a privacy mode that omits project/task text.

The private socket/UID boundary prevents other local users from accessing the service. It is not a security boundary against all malicious software already running as Daniel. The event port still cannot execute native actions: control actions require the registered native client role, typed validation and current object authorization. Semantic model content never becomes code.

Cap log rotation to five files of 10 MiB each by default, outside the journal's retention budget. Diagnostic evidence fixtures contain synthetic or deliberately redacted metadata. Permissions-denied, parser errors and event gaps are visible health facts without secret-bearing payloads.

### 19.5 Restart, update and uninstall

An explicit `EnableObservation` action may first register/start the single companion in control-only mode, with provider polling and capture admission closed. After authenticating the installed outer bootstrap and verifying service authorization, that companion alone acquires the existing writer lock, performs any authorized pending migration, durably records the enable command/preference and opens observation. Notification clicks, ordinary activation and registration alone never change the enabled preference or authorize capture; failed service authorization leaves observation unavailable. This uses the existing control channel, settings and writer, not another service or store.

A clean UI restart restores window/layout and subscribes to the independent companion. A clean companion restart preserves store generation, changes core process generation, restores the checkpoint/journal and reconciles; callbacks from the old connection are rejected.

On sleep, suspend polling/rendering work and record observer interruption where possible. On wake, resample clocks/boot/process evidence, reconnect providers, reconcile missed native history and create fresh UI subscriptions as necessary. Do not compare monotonic clocks across boots or turn a clock jump into a session outcome.

M15 provides a manual staged update path first. Build/verify the new nested companion and outer app and preserve the owner's observation-enabled preference. The outer native bootstrap requests `PrepareMaintenance` while the old companion is still supervised. The single writer durably records preparation, gates new admission so hooks spool, finishes already accepted work, creates a consistent pre-migration backup, returns `PREPARED`, and remains alive holding its lock. No required draining or backup work remains after that receipt; it may still process a verified maintenance-cancellation command. Only then may the outer app unregister the service and await the actual result: Apple documents that unregistering a running helper terminates it. Verify old ProcessKey exit and lock release before replacing the bundle or opening the store with new code. [Unregister semantics][N_SERVICE_UNREGISTER]

If preparation, backup, unregistration or exit verification fails, abort replacement and retain the old bundle/store. A companion restarted during a pending maintenance transaction restores its quiescent state and cannot resume writes automatically. Record maintenance phase/target version atomically in an owned locator outside the replaceable bundle, so bootstrap can resume the exact handoff or cancel maintenance through the old compatible writer; do not infer completion from a missing process. After replacement when observation was enabled, the outer bootstrap records the verified target and authorized migration phase, registers/starts the updated companion in quiescent maintenance mode, and waits for that companion alone to acquire the writer lock, validate the target/store and migrate with capture admission closed. On its ready response, complete maintenance and admit capture/drain the spool. The outer app never writes SQLite. If observation was disabled and no writer was running, verify disabled/unregistered service state and absence of a writer, replace the bundle without requesting preparation from a nonexistent helper, and defer migration until the next explicitly enabled supervised start. A crash between any phases is recoverable from the recorded phase and verified binary/schema identities. Capture remains bounded/spooled during downtime; an old binary refuses a newer incompatible schema.

Stop Observation uses the same prepare-before-unregister ordering without migration. Uninstall removes owned integration entries and prepares the current companion before unregistering; it must not expect a killed helper to flush or clean up afterward. A notification cold-start during maintenance or disabled observation may forward/open the current inspector, but cannot acquire a second writer or reenable capture.

After the ordered stop is verified, uninstall removes owned hook/locator files and offers an explicit choice to keep or delete local history. Default is keep history. Do not remove provider histories, repositories, foreign hooks or the user's original terminal sessions.

## 20. Performance and operational budgets

These are engineering acceptance targets to measure on the actual target Apple Silicon Mac. The evidence manifest records chip, memory, macOS/build, display/DPR, power mode, exact artifact, provider load and measurement method. They are not existing benchmark results.

### 20.1 Reference workloads

**Normal office:** 32 principal sessions, 64 subordinate actors, eight Projects, up to 64 visible animated workers, a 1440×900 logical viewport and DPR capped at 1.5. Include mixed work/completion/input/approval states and five independent owner-facing completions.

**Capacity office:** 100 principal sessions, 300 subordinate actors and 20 Projects, with the same 64-worker visible budget. The fleet/attention views expose every record through virtualized/paged data. Far-away or hidden workers are summarized; no lifecycle fact is discarded because its avatar is not rendered.

**Transport stress:** 200 normalized observations/second for 15 minutes, plus a 2,000/second five-second burst. Payload sizes and distributions are recorded; test typical 1 KiB observations and separately test maximum-size rejection/capacity paths. Synthetic load does not consume model inference.

### 20.2 Targets and stop conditions

| Measurement | Target |
| --- | --- |
| Healthy capture executable lifetime | p95 ≤25 ms; application wall budget 250 ms, shutdown path 100 ms |
| Receipt path | 20 ms connect budget; ≤75 ms durable receipt wait within total capture budget |
| Local captured observation → committed state | p95 ≤100 ms under normal load |
| Committed projection → applied visible DOM state | p95 ≤100 ms while visible and hydrated |
| Native event → visible state/scene update | p95 ≤250 ms on the healthy normal path; report provider scheduling separately |
| Verified normal Terminal route | p95 ≤750 ms after permissions are already granted; hard per-attempt budget two seconds |
| Recovered attention view | ≤two seconds from native core readiness for a 100,000-event reference store |
| Usable 3D office | ≤four seconds from UI launch in the warm reference case; cold GPU/permission costs separately recorded |
| Normal scene | 60 FPS target; p95 frame time ≤20 ms; no sustained frame freeze |
| Idle companion CPU | ≤1% of one CPU core over a five-minute quiet window |
| Quiet visible UI CPU | ≤3% of one core with idle loops suspended |
| Hidden office | No scheduled Three render frames; observer/notifications continue |
| Companion resident memory | ≤100 MiB quiet, ≤150 MiB normal stress steady state |
| UI + attributable WebKit process memory | ≤600 MiB normal office; ≤1 GiB capacity office |
| Warm memory growth | <20 MiB retained growth after eight-hour equivalent repeated create/dispose/reconnect workload and stabilization |
| Catch-up | Drain 50,000 typical spooled observations within 60 seconds on the reference Mac without blocking new captures |
| Journal/log/spool growth | Enforce Section 8/9/19 budgets with truthful overflow/retention behavior |

Use monotonic clocks within one machine/boot for latency. Remote latency displays capture-to-receipt estimates separately from local commit/apply time; unsynchronized clocks do not produce exact end-to-end claims.

Correctness is the gate above throughput. At overload, shed optional activity detail or retire a lagging UI subscription before threatening provider progress or durable accepted attention. Failed durable acceptance is visible as degraded coverage when detectable. A benchmark passes only if its event, dedup, attention and routing invariants also pass.

### 20.3 Adaptive work

Coalesce scene/projection presentation into at most one patch per 50 ms when busy while preserving journal facts and final entity revisions. Waiting/attention changes are included in the next scheduled patch, not held behind decorative animation. Batches remain within the IPC window.

Disable shadows/postprocessing and reduce DPR/animation detail before sacrificing operational UI responsiveness. Postprocessing, if added, uses the pinned WebGPU `RenderPipeline`/TSL path. Its absence is acceptable; silently changing the primary renderer is not.

Avoid full process/terminal scans on every tool event. Reuse valid ProcessKey evidence, subscribe to process exit, invalidate on observed changes and refresh for an explicit route. Git metadata is cached by worktree and refreshed on native cwd/change evidence or bounded repository checks, never on every frame.

### 20.4 Diagnostics required for acceptance

Diagnostics shows dependency/build identity, core/store generations, provider profiles, native coverage, queue depths, last durable cursor, spool age/size, reconciliation anchors, unresolved conflicts, route proof axes, actual notification authorization and actual graphics backend.

Display capture/commit/apply latency distributions, loss/overflow counters, per-provider event rates, active subscription bytes/frames, IPC reconnect reasons, renderer/device-loss history and memory/frame samples. Export evidence through an explicit action. A green connection indicator cannot hide LIMITED provider coverage or a WebGL2 fallback.

## 21. Invariants and verification strategy

### 21.1 Normative invariants

| ID | Invariant |
| --- | --- |
| INV-01 | One namespace/native session tuple resolves to one canonical Session. |
| INV-02 | Distinct sessions can share cwd/repository/branch without merging. |
| INV-03 | PID alone, TTY pathname alone and a terminal tab ordinal are never durable identity. |
| INV-04 | Process reuse or executable replacement invalidates old process proof. |
| INV-05 | A native session resume retains its worker; a new identity does not inherit it. |
| INV-06 | Session identity, turn state, execution presence and observer health remain separate. |
| INV-07 | Stop/output boundary cannot itself end an execution or certify an unvetoed terminal outcome. |
| INV-08 | A terminal outcome applies to its own turn/activation; stale activity cannot regress it. |
| INV-09 | Missing/ambiguous parent metadata cannot silently promote a child to a root. |
| INV-10 | A parent turn or worker-process exit cannot automatically end independently continuing children/jobs. |
| INV-11 | Native/observer epochs, provider turn IDs and journal cursors retain separate namespaces. |
| INV-12 | Late asynchronous callback results use immutable original identity, not current-session globals. |
| INV-13 | Duplicate accepted observations and retried owner commands are idempotent. |
| INV-14 | Valid permutations of equivalent causal evidence converge, including reverse replay around owner follow-up. |
| INV-15 | A native snapshot without revision is an interval observation; list absence is not durable deletion. |
| INV-16 | Reconciliation claims only the history/coverage it actually inspected to a valid anchor. |
| INV-17 | Exact surface selection and current-provider-session verification are separate results. |
| INV-18 | Ambiguity never causes automatic same-cwd/newest/frontmost routing. |
| INV-19 | Low-proof route, URL/app activation or unverified/background input readiness cannot silently auto-acknowledge attention. |
| INV-20 | Focus cannot type, approve, resume or launch provider work. |
| INV-21 | Every eligible owner action remains addressable despite banner grouping or avatar culling. |
| INV-22 | Acknowledged and resolved are different; reboot/rebuild retains both. |
| INV-23 | Auto-resolution requires actually accepted, causally later human input in the appropriate scope. |
| INV-24 | Parent-owned subordinate results do not flood owner attention without an eligible escalation. |
| INV-25 | Semantic/UI-inferred content cannot overwrite native lifecycle or approval state. |
| INV-26 | Provider hook capture fails open and never waits for the renderer/network. |
| INV-27 | A durable receipt follows the journal/projection/attention transaction; retries reuse observation identity. |
| INV-28 | UI quit/crash/reload and GPU failure cannot stop enabled native capture or destroy accepted state. |
| INV-29 | One companion owns SQLite writes; checkpoint/retention preserves unresolved state, command receipts, native dedup and causal-resolution coverage. |
| INV-30 | Source loss before durable acceptance is not misrepresented as recoverable or impossible. |
| INV-31 | IPC old epochs, gaps and backpressure recover through bounded resubscription without mutating provider truth. |
| INV-32 | Snapshot bounds are enforced before transmission; pagination never hides outstanding counts. |
| INV-33 | Tauri 3 is the implementation target; no Tauri 2 plugin/runtime is introduced as a hidden dependency. |
| INV-34 | The active initialized backend is diagnosed; WebGL2 compatibility cannot pass the target WebGPU gate. |
| INV-35 | Remote process/TTY identity is endpoint-scoped; consumer ChatGPT capabilities are not invented. |
| INV-36 | Configuration changes are owned, inspectable, idempotent and reversible without erasing foreign settings. |

### 21.2 Four distinct test layers

| Layer | Harness | What it proves | What it cannot certify |
| --- | --- | --- | --- |
| Ordinary frontend/browser | TypeScript unit tests and Playwright with synthetic bridge | View reducers, pagination, keyboard/DOM UI, projection sequencing, canvas controls | Tauri IPC, WKWebView WebGPU, TCC, native focus |
| Tauri application integration | Exact Tauri 3 mock runtime/IPC facilities plus real native bridge tests | Commands, serialization, ACL, view epochs, subscriptions, native shell integration | Mock success alone does not prove actual OS/webview behavior |
| Native macOS integration | Rust/Swift harness, real provider processes, installed terminal dictionaries, XCTest/XCUITest/AppleEvents | Process/TTY identity, native routing, service supervision, permissions, provider modes | A debug-only test artifact does not replace installed-release acceptance |
| Packaged-app acceptance | Installed exact Tauri 3 artifact with source checkout/dev server unavailable | Actual WKWebView/WebGPU, displayed pixels, notifications/actions, bundle identity, sleep/restart, first workflow | Success on one OS/provider profile does not certify all future releases |

Tauri alpha.4 includes `mock_builder`, `MockRuntime`, `get_ipc_response` and `assert_ipc_response`. Use these for the command layer. [Tauri 3 test module][T12]

Current WebdriverIO macOS integration exists, but the inspected 1.4.0 native test plugins depend on Tauri 2. They are not a compatible Tauri 3 test dependency. The selected native harness uses Apple's application-launch/UI/screenshot facilities plus narrow qualification assertions in a dedicated build. Test-only commands are absent from the release build. [Current incompatible test manifest][T13] [Apple UI testing][N_XCTEST]

Every native run records exact executable path, bundle ID, build commit and binary hash. Do not accidentally control a different installed app with the same bundle ID. Validate actual presented pixels as well as DOM/AX state, because a live accessibility tree can coexist with stale visual frames.

### 21.3 Synthetic provider and failure harness

The synthetic adapter emits all canonical facts and representative native provider observations, with deterministic seed, logical source clocks, boot/process generations, actor relationships and injected delivery order. It requires no API key or inference.

The harness has a pure reducer path, a real private-relay/SQLite path and a Tauri native stream path. Fixture time is explicitly advanced; tests do not rely on wall-clock sleeps to create completion. Native adapters can be replaced by controlled inventories in synthetic runs without labeling those tests native acceptance.

Required fixture families include:

- Normal turn, tool phases, parallel tools, exact/aggregate waits, successful/refused/interrupted/failed outcomes, follow-up, resume and execution end.
- Fifteen concurrent roots with five in one repository; three same-cwd sessions; multiple worktrees; concurrent subordinate runs and teammates.
- A→B→A session changes inside one process, old SessionEnd, delayed result after switch, source/mod reload and one durable session with multiple attachments.
- PID reuse, same-PID executable replacement, TTY reuse, terminal restart, moved tabs, changed foreground job and copied stale environment tokens.
- Duplicate observation/ACK loss, duplicate native occurrence, reordered native steps, incomparable conflicting outcomes and missing observations with/without available history.
- Five recorded offline Codex outcomes spanning multiple pages; first import versus enrolled-gap recovery; legacy markerless Completed shells, unmatched terminal B attached to legacy start A despite both markers, and normalized Interrupted without terminal markers; repeated/expired cursors; a nonterminal turn ending after its original page was passed.
- Lower mod middleware returning without core acceptance/spawn, synthetic nonengine turn.complete with a real turn ID (direct plugin raises are unavailable on 2.1.291), unqualified host-list results, non-engine lifecycle dispatch, autonomous/upstream-delayed prompt origins, input queued before completion, accepted human follow-up delivered before its earlier completion, and partial mod-batch receipts/retries.
- Parent-owned child completion, child owner wait, parent completion while child continues and an actually promoted former child whose delayed old completion remains parent-owned.
- Relay failure, capture timeout, disk full, atomic spool interruption, crash before/after durable commit, migration failure and checkpoint corruption.
- Channel frame loss/stall, repeated native view recreation/cache cleanup, old-epoch queued actions and query replies, oversized initial snapshots, stale paged rows, renderer reload during a burst and companion restart with UI open.
- Remote reconnect/clock skew, outage beyond seven days, quota rejection versus admitted records, durable relay watermark deduplication, missing local surface, ChatGPT URL-only worker and unsupported UI inference.

Property tests permute only causally permissible relations; they do not pretend order is unknowable when a source supplied a real sequence. Assert final entity identity, turn outcomes, attention state, coverage and allowed routing results. Preserve minimized failing seeds.

### 21.4 Native scenarios and acceptance evidence

M0A proves the substrate, M0B proves exact direct-Claude return early, and M0C completes platform reliability before broad application implementation. M2–M6 prove the first meaningful end-to-end workflow as the capabilities arrive. M14 repeats all 31 original workflow steps against the integrated installed artifact, with Codex's documented routing ceiling.

Native routing scenarios use real manually launched Terminal.app sessions: same cwd, tab moves, window rearrangement, minimized/fullscreen/Spaces, resume elsewhere, in-place conversation switch, target close during focus and permissions denial. A script returning exit code zero without selected-TTY/provider readback cannot pass.

Notification acceptance includes actual delivery while the office is unfocused/closed, default interaction into Threadspace, eligible Return to the current target and an old notification after the tab's conversation changes. Focus/notification permission denial must preserve attention.

Reliability scenarios kill the UI and companion separately, continue provider activity, restart without duplicates, sleep/wake, disconnect a native stream and replay the spool. Kill the helper while the UI is closed and no new hook arrives; the system must relaunch the registered companion independently. A new hook cannot be the hidden restart mechanism.

Graphics evidence comes from the packaged app: actual WebGPU backend, a sustained animated scene, live state-to-pixel changes, device-loss/2D recovery, repeated renderer create/dispose, hidden-window suspension, display/DPR changes and fresh UI restoration. Safari/Chrome/Vite results remain frontend evidence only.

### 21.5 Evidence and gate rules

Each milestone stores a manifest with commit, dependency lock hashes, schema/reducer versions, hardware/OS/provider profiles, commands, fixture seeds, expected/actual outcomes, relevant sanitized logs, screenshots/video and timing/memory summaries.

Gate results are `PASS`, `FAIL`, `BLOCKED` or `NOT_SUPPORTED` for an explicitly optional capability. A required gate cannot be passed by relabeling failure as optional. Accepted decisions D-0004–D-0006 define specific profile/facet limits, their preserved native NOT_RUN/BLOCKED/MANUAL_EXTERNAL_REQUIRED status and later closure; they do not certify execution or waive unrelated requirements. Gate PASS applies only to the resulting explicit scope. A documented unsupported ChatGPT capability does not fail the Claude MVP; a failed Tauri 3 WebGPU requirement on the intended target does.

Tests target meaningful invariants and native risks, not every trivial layout detail. Broader repeats occur for changed dependencies, a concrete unresolved risk or a release gate. Once a gate has the required evidence, continue to the next milestone.

## 22. Compatibility decisions, remaining external boundaries and release scope

### 22.1 Binding implementation decisions

The following decisions control implementation at the failure boundaries:

| Challenged assumption | Frozen repair |
| --- | --- |
| Hook subprocess owns the provider TTY | Inspect qualified ancestor/native inventory; hooks can be detached |
| One process implies one current conversation | Durable Session + activation + ProcessKey; daemon modes separated |
| Same tab proves same current session | Independent current-session verification axis |
| Late callback can use current global session ID | Immutable callback-entry context and turn ownership |
| Stop proves completion/end | Response boundary separate from final outcome and execution presence |
| Snapshots are instantaneous and exhaustive | Interval/revision/coverage model; no absence-based deletion |
| Latest historical turn is enough after downtime | Native paginated coverage to anchors and revisit incomplete turns |
| A success-shaped mod return proves human acceptance | Verified core dispatch/acceptance and original human provenance |
| All subagent results need the owner | Explicit owner-facing attention eligibility |
| MCP “blocked” controls lifecycle | Semantic annotation beside native observed state |
| Atomic rename gives absolute loss-free capture | Defined durable-ACK domain, bounded spool and honest pre-ACK loss |
| UI owns notifications/relay | One independently supervised AppKit login-item companion, with documented crash relaunch and no custom LaunchAgent job |
| Unregister leaves a helper alive to drain | Prepare/quiesce/backup first; unregister can terminate it |
| A PID row can be sampled only after inventory | Bracket a fresh provider association with incumbent ProcessKey evidence |
| Clearing the render callback stops all Three work | At r186 dispose on hide/2D and rebuild a fresh renderer on return |
| Tauri package suffixes are uniform | Exact verified alpha.4/alpha.3/alpha.2 compatibility set |
| Existing Tauri plugins/test adapters are automatically v3 | Manifest/source check and native gate; no optional plugins initially |
| A stalled Channel can deliver its own reset | Independent invoke status/reconnect and bounded epochs/windows |
| Snapshot chunking is automatically safe | Complete bounded snapshot fits the ACK window; explicit paging |
| Custom helper files are automatically signed | Explicit nested signing before outer bundling/signing |
| DOM/AX proves live graphics | Actual packaged-pixel and backend evidence |
| File-resource identifier persists across reboot | Durable UUID/bookmark and revalidation; no stale-ID authority |

### 22.2 External boundaries that remain explicit

- **Native qualification:** M0A/M0B/M0C are accepted; the final M0C disposition is G17 PASS in its checklist. Built-in-display qualification does not certify physical external-monitor disconnect/reconnect, deferred to M15 by D-0006. M1 builds on accepted `main` (`cd9e376`); its candidate is pending independent review.
- **Tauri prerelease drift:** current source supports the architecture, but later alphas may change APIs. Exact locks and the native boundary contain that risk; changes require deliberate regression.
- **Claude mod compatibility:** the public snapshot and exact installed runtime may differ. Generate/pin installed declarations and qualify callback semantics. Original human-input acceptance and positive original causal order are different capabilities; absent a sufficient order witness, automatic follow-up resolution stays disabled and explicit Mark handled remains available without blocking otherwise qualified native observation.
- **Codex shared-daemon routing:** the researched public interface lacks a live TUI-client→thread→TTY registry. Preserve lifecycle visibility and explicit pairing/last-known routes; do not claim current-session verification.
- **Ghostty release boundary:** native TTY reverse mapping depends on the installed dictionary containing the merged properties. Earlier versions retain native-ID/manual/app routing without automatic TTY joins.
- **Consumer ChatGPT lifecycle:** no verified equivalent public stream was found for arbitrary Chat conversations/personal cloud Work. Manual original-URL workers and optional semantic/UI experiments are the selected scope.
- **OS permissions/distribution:** native permissions and a trusted distribution signing identity are external to the specification. Their denial/unavailability has defined behavior and explicit package gates.

These are bounded compatibility conditions with usable paths or required early proofs. None is permission to silently redesign around Tauri 2, fabricate provider APIs, scrape private stores or hide uncertain routing.

### 22.3 Delivery scope

M0A–M0C and M1–M6 produce a useful local observer with a simple real WebGPU office, Claude lifecycle/correlation, subagents, persistent attention, native notifications, Terminal return and Codex integration at its true capability levels.

M7–M15 complete stable worktree spaces, additional terminals, MCP, remote groundwork, the optional ChatGPT tier, polished operational/3D UX, stress qualification, integrated acceptance and release readiness. Optional integrations do not delay release of already-qualified local functionality. The later milestones still deliver their documented capability rather than replacing an unsupported feature with a fictitious success.

Future orchestration, owned PTYs, launching, prompt submission, approvals, merges, team collaboration, public multi-tenant relay, Windows/Linux desktop parity and App Store distribution are outside this implementation plan. They must not become hidden prerequisites for the observe-first system.


## 23. Source register and compatibility evidence

Sources were checked for the October 5, 2026 architecture cutoff, with retrieval continuing on October 6 UTC. **Release/source pin** means the cited source is tied to the stated release or commit. **Current documentation** means the official page was read during this research; it can change and is not an archived historical snapshot. **Issue report** means a concrete upstream report that motivates a qualification case, without establishing that Threadspace or its selected version reproduces it. **Competitor source** establishes an inspected implementation choice, not provider behavior or a measured comparison.

These sources support the external facts behind this specification. Threadspace's schemas, authority rules, limits, benchmarks and acceptance gates are design decisions. No source citation substitutes for native execution: this research did not compile the selected application on macOS, certify its plugins, test TCC or notifications, attest its presented frames, or measure its performance.

### 23.1 Provider evidence

| Area | Sources | Evidence scope |
| --- | --- | --- |
| Claude conventional hooks and installation | [Hook reference][C1], [installation guide][C2] | Current official event inputs, timing, configuration and handler effects. A hook's existence does not make every event an authoritative final outcome. |
| Claude external inventory | [Agent view][C3] | Current supported JSON inventory, interactive/background distinction, supervisor and job boundaries. |
| Claude observer mods | [Overview][C4], [events][C5], [host APIs][C6], [reference][C7], [public declarations][C8] | Current early-access documentation. The public declaration at the [2.1.290 release commit][C8_PIN] identifies 2.1.277; declarations generated by the exact installed build govern qualification. The separate public `main` URL is mutable. |
| Claude release and execution surfaces | [2.1.290 release][C9], [desktop][C10], [Remote Control][C11], [cloud Code][C12], [agent teams][C13] | Release candidate plus current official surface/configuration contracts; shared terminology does not certify identical deployment coverage. |
| Codex release and hook schema | [0.160.1 release][O1], [released schema][O2], [current hooks reference][H2] | Release/source pin `d27764b82f7118f674371e6d6e76271d9d606edb`. The released schema and implementation take precedence over older generic documentation where they conflict. |
| Codex session and child identity | [Session implementation][O3], [thread model][O4], [hook runtime][O3_HOOK] | Same released commit; tree membership, concrete child thread ID and immediate parent are separate fields. |
| Codex hook execution and embedded mode | [Command runner][O5], [Unix child command][O5_PTY], [launch policy][O6], [CLI flags][O6_CLI] | Same released commit; command input/process behavior and explicit `--no-daemon` option. |
| Codex passive observation and routing ceiling | [Daemon contract][O7], [client contract][O7_CLIENT], [TUI metadata][O8], [initialization model][O8_INIT], [status card][O9] | Same released commit; existing daemon behavior and fields actually available for binding. A missing supported client-to-TTY association remains a capability limit. |
| Codex pagination and observation delivery | [Thread/turn pagination][O10], [status broadcaster][O11], [outgoing broadcaster][O11_OUT], [read implementation][O12], [legacy builder][O_HISTORY_BUILDER], [native projection][O_HISTORY_PROJECTION], [turn model][O10_TURN], [notification model][O10_NOTIFY] | Same released commit; global status does not subscribe to every notification. Reconstructed legacy completion and normalized interruption require different authority from recorded native outcomes. |
| Codex stop and completion timing | [Turn loop][O13], [Stop processing][O13_STOP], [legacy notifier][O14] | Same released commit; Stop continuation and notifier delivery are distinct from a final native turn outcome. |
| ChatGPT, Work and Classic | [Desktop migration][H1], [hook boundaries][H2], [share-link semantics][H3], [macOS release notes][H4], [Work with Apps][H5] | Current OpenAI documentation. The reviewed interfaces do not establish an outbound lifecycle subscription for arbitrary consumer ChatGPT conversations. This is a bounded research conclusion, not proof that every possible private interface is absent. |

### 23.2 Tauri 3, WebKit and graphics evidence

| Area | Sources | Evidence scope |
| --- | --- | --- |
| Core candidate and workspace | [Core alpha.4 release][T1], [tag ref][T1_REF], [workspace manifest][T2], [core changelog][T3], [official app template][T_TEMPLATE] | Core commit `a8703ee487c659efbebb27c799752d523a6d09a1`. Tauri 3 is the selected prerelease family. The workspace declares Rust minimum 1.95; the candidate toolchain is [Rust 1.99.0][T_RUST]. |
| Wry runtime and exact package topology | [Runtime implementation][T4], [runtime manifest][T4_MANIFEST], [runtime release][T4_RELEASE], [JS API manifest][T_API], [JS CLI manifest][T_CLI_PACKAGE], [build manifest][T_BUILD] | Release/source pin. Runtime registration is explicit; `tauri`/runtime-wry/CLI alpha.4, build alpha.3 and JS API alpha.2 are intentionally different package versions. Use the complete compatibility contract in Section 18. |
| Migration and development lifecycle | [CLI changelog][T5], [v3 migration commit][T_MIGRATION], [process-tree termination change][T_DEV_TREE], [private-API flag removal][T_PRIVATE_FLAG] | Release/source pin. A development restart can terminate the app's child process tree; independent system supervision is an architectural requirement. Removing a feature flag does not prove every dependency path is free of private APIs. |
| Native bundle layout and signing | [macOS bundler][T6], [configuration types][T7], [child-process source][T_SHELL] | Release/source pin. Arbitrary `macOS.files` copies are not automatically enrolled as nested signing targets. Pre-sign the companion, preserve it during copy, then sign/notarize and verify the outer bundle. [N1][N1] and [N16][N16] now point to Tauri 3 source, not v2 guidance. |
| Invoke, Channel and application lifecycle | [JS core][T8], [Rust Channel][T9], [command machinery][T_COMMAND], [app/RunEvent][T_APP], [frontend events][T_EVENTS] | Release/source pin. Transport ordering does not establish application acknowledgement, reload recovery, bounded flow control or durable delivery. |
| Capabilities and native window APIs | [App-manifest generation][T_ACL_BUILD], [app-command dispatch][T_ACL_DISPATCH], [capability types][T_ACL_CAP], [runtime authority][T_ACL_AUTH], [window implementation][T_WINDOW], [CSP/configuration][T7] | Release/source pin. A nonempty app manifest is necessary for the selected local custom-command ACL; window/webview allow matches are OR and deny behavior is origin-wide in this pin. App argument validation and companion authorization remain required. |
| Plugin alternatives | [Plugin workspace][T10], [v3 notification implementation][N17], [v3 shell implementation][T_SHELL] | Inspected commit `d9be6d0492fb6746637ba64497237b2116aaec90`; reviewed native plugin candidates are 3.0.0-alpha.2. Manifest-level compatibility is not compilation or native proof. The notification implementation still returns `Granted` without querying actual macOS permission. |
| Actual macOS WebView | [Pinned Wry WKWebView source][T_WRY], [Wry 0.57.0 release][T_WRY_RELEASE], [Tao ref][T_TAO], [WebKit macOS 26 floor change][T_WEBGPU_FLOOR], [WebKit preferences][T_WEBGPU_PREFS], [Safari 26 release][N2] | Wry commit `792d0359ba6501a4fc360ece17de2ae42329a47c`; WebKit change `61fa3ded21da55eeeabd33ef06b8be26972269f5`. These establish the native engine and platform direction; dev and packaged app WebGPU gates remain mandatory. |
| Three.js candidate | [r186 release][R5], [exact package manifest][R6], [npm 0.186.1 record][R10] | Release/source pin `9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8`. The package version is **0.186.1**; do not derive a different package version from the release label. |
| Three.js renderer contract | [Renderer docs][R1], [WebGPU guide][R2], [TSL reference][R3], [pinned renderer][R4], [pinned backend][R7], [base Renderer][R8], [RenderPipeline][R9], [WebGPU exports][R_WEBGPU_EXPORTS], [TSL exports][R_TSL_EXPORTS] | Documentation is mutable; implementation files use the pinned commit. The renderer can fall back to WebGL2. Await initialization, inspect the actual backend, and attest presented output. Node material classes/`RenderPipeline` come from `three/webgpu`; TSL functions come from `three/tsl`. `Renderer.dispose()` is asynchronous in this pin. |
| Native testing boundary | [Tauri 3 test module][T12], [WDIO plugin manifest][T13], [WDIO WebDriver manifest][T13_DRIVER], [current testing guide][T_TEST_DOCS], [XCUIApplication][T_XCUI], [build selection][T_XCUI_BUNDLE], [screenshots][T_XCUI_SCREENSHOT] | Core mocks are release-pinned. WDIO snapshot `fb13c9343a24c8d45d396f2258698f49f34ec435` has Tauri 2 dependencies and is not the selected Tauri 3 acceptance harness. The current v2-branded guide describes the ecosystem, not v3 compatibility. Native screenshots complement AX/DOM checks. |

The following reports are **qualification inputs**, not confirmed Tauri 3 failures: [stopped WKURLSchemeTask race][T11], [stale presented frames despite live DOM/AX][T11_FRAMES], [cross-scheme images on a macOS beta][T11_SCHEME], [secondary webview destruction][T11_DESTROY], [transparent-window GPU cost][T11_TRANSPARENT], and [Liquid Glass icon packaging][T11_ICON]. Their reporter versions and reproduction limits are retained in the compatibility risk register. The older [Tauri WebGPU discussion][N3] motivates checking development and packaged execution separately; it does not certify the selected runtime.

### 23.3 macOS, storage and protocol evidence

| Area | Sources | Evidence scope |
| --- | --- | --- |
| Process incarnation and local socket provenance | [XNU process fields][A1], [process implementation][A1_IMPL], [boot-session source][A1_BOOT], [Unix socket definitions][A7], [socket implementation][A7_IMPL], [getpeereid][A7_UID] | Current Apple source plus archived API manual. Process birth/boot and peer credentials require target-machine validation; a forwarded/spooled record does not inherit its original sender's live peer provenance. |
| Terminal.app and iTerm | [Terminal scripting][A2], [installed dictionaries as API contract][A2_DICT], [iTerm AppleScript][A3], [iTerm Python session API][A8], [iTerm URL scheme][A9] | Current official documentation. Terminal's installed dictionary and actual focus/readback tests establish supported native properties. URL dispatch is not native selection readback. |
| Ghostty and tmux | [Ghostty scripting][A4], [TTY/PID change][A5], [current dictionary][A5_DICT], [1.3.0 release][A4_RELEASE], [download][A10], [surface/environment source][A5_SURFACE], [tmux manual][A6] | Current docs/source and merged change. A merged implementation is not proof that a particular installed Ghostty release includes it. tmux IDs require server/client context. |
| Native service and notification lifecycle | [SMAppService][N14], [login-item registration][N_LOGIN_ITEM], [relaunch][N_SERVICE_REGISTER], [unregister][N_SERVICE_UNREGISTER], [helper layout][N15], [notification permission][N18], [responses][N19], [delegate lifecycle][N_NOTIFICATION_DELEGATE] | Current Apple contracts select one independent AppKit login-item companion. Crash/nonzero relaunch is documented; unregistration can terminate it. App identity, actual restart, banners/callbacks and disable/update behavior remain native qualification gates. |
| Native UX and permissions | [AppleEvents usage string][A11], [automation entitlement][A12], [AX trust][N28], [Reduce Motion][N23], [reduced-motion criteria][N24], [URL scheme registration][N_URL_TYPES], [Tauri window source][T_WINDOW] | Current public APIs. Permission prompts, VoiceOver, monitor restoration, URL open events and window presentation are native acceptance cases, not inferred from successful compilation. |
| Git and filesystem continuity | [git-rev-parse][G1], [git-worktree][G2], [Apple file-resource identifier][G3], [URL bookmarks][G3_BOOKMARK] | Current official documentation. Apple's file-resource identifier is **not persistent across system restarts**; it is boot-scoped corroboration, not a durable Repository UUID. Bookmarks can follow moves/renames on supporting volumes; resolution still requires continuity checks. |
| SQLite durability and release | [WAL][D1], [synchronous][D2], [WAL-reset fix][D3], [3.53.4 release][D4], [macOS fullfsync][D5] | Current documentation plus exact native candidate **3.53.4**, released July 24, 2026. Record the actual loaded SQLite version/source ID and compile options. This release contains the WAL-reset fix; durability still depends on the configured VFS/OS/storage path. |
| MCP and remote transport | [MCP specification][S1], [stdio binding][S2], [OpenSSH client][S3], [OpenSSH configuration][S3_CONFIG] | Current official protocol/client documentation. Record the protocol revision and exact SDK at M9; current documentation URLs do not implicitly pin an SDK. SSH remains an explicit configured remote connection. |

### 23.4 Focused competitor source inventory

These exact inspected files support the compact comparison in Section 1.4. No competitor was executed or benchmarked in this research.

| Implementation | Snapshot and inspected files |
| --- | --- |
| AI Agent Session Center | `16df09c4498ec0c8a0a63e77f9617d48109363d2`: [session matcher][P1], [approval detector][P1_APPROVAL]. |
| CCC — Amir Fish implementation | `91017d02682d9c19ad496f626e8cf624961ea30f`: [Terminal/iTerm focus][P2], [process identity][P2_PROCESS], [external discovery][P2_EXTERNAL]. |
| This Office | `08443d04cc2d0ba83f1444741d8da5ed11f93911`: [transcript watcher][P3], [office restoration][P3_STATE]. |
| Pixel Agents | `3537e140c2094761beae748592aeb92ece8edfdd`: [agent runtime][P4], [state persistence][P4_STATE], [session routing][P4_ROUTER]. |
| Termhive | `c2b794fd296affbec80557e7334c9a0860859ea4`: [runtime dispatch][P5], [Codex integration][P5_CODEX]. |
| Codeg | `98035964c0f3811dc80a5e9291b89cb6c0570fa0`: [native import filtering][P6], [ACP session dispatch][P6_ACP]. |
| co:lana | [Official product site][P7]; no source-level inspection or native routing guarantee was established. |

[C1]: <https://code.claude.com/docs/en/hooks> "Claude Code hook reference"
[C2]: <https://code.claude.com/docs/en/hooks-guide> "Claude Code hooks guide"
[C3]: <https://code.claude.com/docs/en/agent-view> "Claude Code agent view and JSON inventory"
[C4]: <https://code.claude.com/docs/en/plugins/mods/overview> "Claude Code mods overview"
[C5]: <https://code.claude.com/docs/en/plugins/mods/events> "Claude Code mod event middleware"
[C6]: <https://code.claude.com/docs/en/plugins/mods/api> "Claude Code mod host APIs"
[C7]: <https://code.claude.com/docs/en/plugins/mods/reference> "Claude Code mod reference"
[C8]: <https://raw.githubusercontent.com/anthropics/claude-code/main/mods/types/claude-code.d.ts> "Public Claude Code mod declarations, mutable main"
[C9]: <https://github.com/anthropics/claude-code/releases/tag/v2.1.290> "Claude Code 2.1.290 release"
[C10]: <https://code.claude.com/docs/en/desktop> "Claude Code desktop configuration and execution surfaces"
[C11]: <https://code.claude.com/docs/en/remote-control> "Claude Code Remote Control"
[C12]: <https://code.claude.com/docs/en/claude-code-on-the-web> "Claude Code on the web"
[C13]: <https://code.claude.com/docs/en/agent-teams> "Claude Code agent teams"

[O1]: <https://github.com/openai/codex/releases/tag/rust-v0.160.1> "Codex 0.160.1 release"
[O2]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/hooks/src/schema.rs> "Released Codex hook schemas"
[O3]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/session/session.rs> "Released Codex session identity"
[O3_HOOK]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/hook_runtime.rs> "Released Codex hook identity normalization"
[O4]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/v2/thread_data.rs> "Released Codex thread wire model"
[O5]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/hooks/src/engine/command_runner.rs> "Released Codex command hook runner"
[O5_PTY]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/utils/pty/src/child_command.rs> "Released Codex child command process setup"
[O6]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/daemon_startup.rs> "Released Codex daemon launch policy"
[O6_CLI]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/cli.rs> "Released Codex CLI flags"
[O7]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-daemon/README.md> "Released Codex daemon contract"
[O7_CLIENT]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-client/README.md> "Released Codex app-server client contract"
[O8]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/app_server_session.rs> "Released Codex TUI app-server metadata"
[O8_INIT]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/v1.rs> "Released Codex initialization client metadata"
[O9]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/tui/src/status/card.rs> "Released Codex status card Session identifier"
[O10]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/v2/thread.rs> "Released Codex thread and turn pagination protocol"
[O10_TURN]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/v2/turn.rs> "Released Codex turn protocol"
[O10_NOTIFY]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/v2/notification.rs> "Released Codex notification protocol"
[O11]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/thread_status.rs> "Released Codex global thread-status broadcaster"
[O11_OUT]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/outgoing_message.rs> "Released Codex outgoing broadcaster"
[O12]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/request_processors/thread_processor.rs> "Released Codex thread read implementation"
[O13]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/core/src/session/turn.rs> "Released Codex Stop continuation order"
[O13_STOP]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/hooks/src/events/stop.rs> "Released Codex Stop hook processing"
[O14]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/hooks/src/legacy_notify.rs> "Released Codex legacy completion notifier"

[H1]: <https://help.openai.com/en/articles/20001276-moving-to-the-new-chatgpt-desktop-app> "ChatGPT desktop migration and surface distinction"
[H2]: <https://learn.chatgpt.com/docs/hooks> "Current Codex and Work hook support boundaries"
[H3]: <https://help.openai.com/en/articles/7925741-chatgpt-sharedlinks-faq/> "ChatGPT shared-link semantics"
[H4]: <https://help.openai.com/en/articles/9703738-chatgpt-macos-app-release-notes> "ChatGPT macOS app release notes"
[H5]: <https://help.openai.com/en/articles/10119604> "ChatGPT Work with Apps"

[T1]: <https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.4> "Tauri 3.0.0-alpha.4 release"
[T1_REF]: <https://api.github.com/repos/tauri-apps/tauri/git/ref/tags/tauri-v3.0.0-alpha.4> "Official Tauri alpha.4 tag ref"
[T2]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/Cargo.toml> "Tauri 3 pinned workspace manifest"
[T3]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/CHANGELOG.md> "Tauri 3 pinned core changelog"
[T4]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-runtime-wry/src/lib.rs> "Tauri 3 pinned Wry runtime implementation"
[T4_MANIFEST]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-runtime-wry/Cargo.toml> "Tauri 3 pinned Wry runtime manifest"
[T4_RELEASE]: <https://github.com/tauri-apps/tauri/releases/tag/tauri-runtime-wry-v3.0.0-alpha.4> "Tauri runtime-wry alpha.4 release"
[T5]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-cli/CHANGELOG.md> "Tauri 3 pinned CLI changelog"
[T6]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-bundler/src/bundle/macos/app.rs> "Tauri 3 pinned macOS bundle construction and signing targets"
[T7]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-utils/src/config.rs> "Tauri 3 pinned MacConfig and CSP types"
[T8]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/packages/api/src/core.ts> "Tauri 3 pinned JS invoke and Channel"
[T9]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/ipc/channel.rs> "Tauri 3 pinned Rust Channel implementation"
[T10]: <https://github.com/tauri-apps/plugins-workspace/blob/d9be6d0492fb6746637ba64497237b2116aaec90/Cargo.toml> "Pinned Tauri 3 plugin workspace"
[T11]: <https://github.com/tauri-apps/wry/issues/1822> "Reported stopped WKURLSchemeTask race; not a Tauri 3 reproduction"
[T11_FRAMES]: <https://github.com/tauri-apps/wry/issues/1848> "Reported stale macOS presented frames; not a Tauri 3 reproduction"
[T11_SCHEME]: <https://github.com/tauri-apps/wry/issues/1778> "Reported macOS beta cross-scheme image restriction"
[T11_DESTROY]: <https://github.com/tauri-apps/wry/issues/1730> "Reported secondary webview destruction crash"
[T11_TRANSPARENT]: <https://github.com/tauri-apps/tauri/issues/15471> "Reported transparent-window GPU overhead"
[T11_ICON]: <https://github.com/tauri-apps/tauri/issues/15315> "Reported Liquid Glass icon packaging issue"
[T12]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/test/mod.rs> "Tauri 3 pinned mock and IPC test APIs"
[T13]: <https://github.com/webdriverio/desktop-mobile/blob/fb13c9343a24c8d45d396f2258698f49f34ec435/packages/tauri-plugin/Cargo.toml> "Inspected WDIO plugin Tauri 2 dependency"
[T13_DRIVER]: <https://github.com/webdriverio/desktop-mobile/blob/fb13c9343a24c8d45d396f2258698f49f34ec435/packages/tauri-plugin-webdriver/Cargo.toml> "Inspected embedded WebDriver plugin Tauri 2 dependency"
[T_TEMPLATE]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-cli/templates/app/src-tauri/src/lib.rs> "Official Tauri 3 explicit runtime app template"
[T_API]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/packages/api/package.json> "Pinned Tauri JS API alpha.2 manifest"
[T_CLI_PACKAGE]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/packages/cli/package.json> "Pinned Tauri JS CLI alpha.4 manifest"
[T_CLI_RELEASE]: <https://github.com/tauri-apps/tauri/releases/tag/tauri-cli-v3.0.0-alpha.4> "Tauri native CLI alpha.4 release"
[T_API_RELEASE]: <https://github.com/tauri-apps/tauri/releases/tag/%40tauri-apps/api-v3.0.0-alpha.2> "Tauri JS API alpha.2 release"
[T_BUILD]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-build/Cargo.toml> "Pinned tauri-build manifest"
[T_LOCK]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/Cargo.lock> "Upstream Tauri workspace dependency resolution, not an application lockfile"
[T_PACKAGES]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/Cargo.lock> "Pinned Tauri package family and upstream lock resolution"
[T_MIGRATION]: <https://github.com/tauri-apps/tauri/commit/49dc7cc39017f63b81adb09303d9b82d5b71207a> "Tauri 3 CLI migration implementation"
[T_DEV_TREE]: <https://github.com/tauri-apps/tauri/commit/0dd3c561acb62e1968bcedaeaa12a80e6981e27f> "Tauri development app process-tree termination change"
[T_PRIVATE_FLAG]: <https://github.com/tauri-apps/tauri/commit/b9a77ebb8ab57480a354ec47aa45c6898ec9bfda> "Tauri 3 removal of macos-private-api feature and setting"
[T_COMMAND]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/ipc/command.rs> "Tauri 3 command argument and response machinery"
[T_APP]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/app.rs> "Tauri 3 Builder and native RunEvent"
[T_EVENTS]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/packages/api/src/event.ts> "Tauri 3 frontend event implementation"
[T_CAPABILITIES]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-utils/src/acl/capability.rs> "Tauri 3 capability types"
[T_AUTHORITY]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/ipc/authority.rs> "Tauri 3 runtime IPC authority"
[T_ACL_BUILD]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-build/src/acl.rs> "Tauri 3 app manifest and capability generation"
[T_ACL_DISPATCH]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/webview/mod.rs> "Tauri 3 app-command ACL dispatch and navigation handlers"
[T_ACL_CAP]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-utils/src/acl/capability.rs> "Tauri 3 capability matching schema"
[T_ACL_AUTH]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/ipc/authority.rs> "Tauri 3 runtime allow and deny semantics"
[T_ACL_ATTRIBUTES]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-build/src/lib.rs> "Tauri 3 build attributes and try_build"
[T_ACL_SLUGS]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-utils/src/acl/build.rs> "Tauri 3 app-command permission identifier generation"
[T_WINDOW]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/window/mod.rs> "Tauri 3 native window and monitor APIs"
[T_SHELL]: <https://github.com/tauri-apps/plugins-workspace/blob/d9be6d0492fb6746637ba64497237b2116aaec90/plugins/shell/src/process/mod.rs> "Tauri 3 shell child-process and sidecar implementation"
[T_WRY]: <https://github.com/tauri-apps/wry/blob/792d0359ba6501a4fc360ece17de2ae42329a47c/src/wkwebview/mod.rs> "Wry 0.57.0 WKWebView implementation"
[T_WRY_RELEASE]: <https://github.com/tauri-apps/wry/releases/tag/wry-v0.57.0> "Wry 0.57.0 release"
[T_TAO]: <https://api.github.com/repos/tauri-apps/tao/git/ref/tags/tao-v0.37.0> "Official Tao 0.37.0 tag ref"
[T_RUST]: <https://github.com/rust-lang/rust/releases/tag/1.99.0> "Rust 1.99.0 release"
[T_WEBGPU_FLOOR]: <https://github.com/WebKit/WebKit/commit/61fa3ded21da55eeeabd33ef06b8be26972269f5> "WebKit macOS 26 WebGPU availability change"
[T_WEBGPU_PREFS]: <https://github.com/WebKit/WebKit/blob/61fa3ded21da55eeeabd33ef06b8be26972269f5/Source/WTF/Scripts/Preferences/UnifiedWebPreferences.yaml> "WebKit WebGPU preferences at the floor-change commit"
[T_WEBGPU_DEFAULTS]: <https://github.com/WebKit/WebKit/blob/61fa3ded21da55eeeabd33ef06b8be26972269f5/Source/WebKit/Shared/WebPreferencesDefaultValues.h> "WebKit WebGPU availability defaults"
[T_WEBKIT]: <https://github.com/WebKit/WebKit/commit/61fa3ded21da55eeeabd33ef06b8be26972269f5> "WebKit macOS 26 WebGPU floor and preference changes"
[T11_ASSETS]: <https://github.com/tauri-apps/wry/issues/1778> "Reported macOS beta cross-scheme asset restriction"
[T11_CLOSE]: <https://github.com/tauri-apps/wry/issues/1730> "Reported secondary webview teardown crash"
[T11_POWER]: <https://github.com/tauri-apps/tauri/issues/15471> "Reported transparent-window GPU overhead"
[T_TEST_DOCS]: <https://v2.tauri.app/develop/tests/webdriver/> "Current Tauri testing ecosystem documentation, not Tauri 3 compatibility proof"
[T_XCUI]: <https://developer.apple.com/documentation/xcuiautomation/xcuiapplication> "Apple XCUIApplication"
[T_XCUI_BUNDLE]: <https://developer.apple.com/documentation/xcuiautomation/xcuiapplication/init(bundleidentifier:)> "Apple application selection by bundle identifier"
[T_XCUI_SCREENSHOT]: <https://developer.apple.com/documentation/xcuiautomation/xcuiscreenshot> "Apple XCUITest screenshot capture"

[R1]: <https://threejs.org/docs/pages/WebGPURenderer.html> "Current Three.js WebGPURenderer reference"
[R2]: <https://threejs.org/manual/en/webgpurenderer.html> "Current Three.js WebGPU guide"
[R3]: <https://github.com/mrdoob/three.js/wiki/Three.js-Shading-Language> "Official Three.js TSL reference"
[R4]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/webgpu/WebGPURenderer.js> "Pinned Three.js renderer fallback and class identity"
[R5]: <https://github.com/mrdoob/three.js/releases/tag/r186> "Three.js r186 release"
[R6]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/package.json> "Pinned Three.js 0.186.1 package manifest"
[R7]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/webgpu/WebGPUBackend.js> "Pinned Three.js WebGPU backend and device handling"
[R8]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/common/Renderer.js> "Pinned Three.js initialization, errors and async disposal"
[R9]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/common/RenderPipeline.js> "Pinned Three.js node render pipeline"
[R10]: <https://registry.npmjs.org/three/0.186.1> "Three.js 0.186.1 npm record"
[R_WEBGPU_EXPORTS]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/Three.WebGPU.js> "Pinned WebGPU and node material exports"
[R_TSL_EXPORTS]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/Three.TSL.js> "Pinned TSL function exports"
[R_PACKAGE]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/package.json> "Pinned Three.js 0.186.1 package manifest"
[R_PIPELINE]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/common/RenderPipeline.js> "Pinned Three.js RenderPipeline implementation"
[R_RENDERER]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/common/Renderer.js> "Pinned Three.js renderer lifecycle and asynchronous disposal"

[A1]: <https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h> "Apple public process metadata structures"
[A1_IMPL]: <https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c> "Apple process metadata implementation"
[A1_BOOT]: <https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sysctl.c> "Apple boot-session sysctl implementation"
[A2]: <https://support.apple.com/guide/terminal/automate-tasks-using-applescript-and-terminal-trml1003/mac> "Apple Terminal scripting guide"
[A2_DICT]: <https://developer.apple.com/library/archive/documentation/LanguagesUtilities/Conceptual/MacAutomationScriptingGuide/AboutScriptingTerminology.html> "Apple application scripting dictionaries"
[A3]: <https://iterm2.com/documentation-scripting.html> "Official iTerm2 AppleScript documentation"
[A4]: <https://ghostty.org/docs/features/applescript> "Official Ghostty AppleScript documentation"
[A4_RELEASE]: <https://ghostty.org/docs/install/release-notes/1-3-0> "Ghostty 1.3.0 release notes"
[A5]: <https://github.com/ghostty-org/ghostty/pull/11922> "Ghostty merged TTY and PID AppleScript change"
[A5_DICT]: <https://github.com/ghostty-org/ghostty/blob/main/macos/Ghostty.sdef> "Current Ghostty AppleScript dictionary"
[A5_SURFACE]: <https://github.com/ghostty-org/ghostty/blob/main/src/Surface.zig> "Current Ghostty surface and environment implementation"
[A6]: <https://github.com/tmux/tmux/blob/master/tmux.1> "Official tmux manual source"
[A7]: <https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/un.h> "Apple Unix socket peer options and path size"
[A7_IMPL]: <https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/uipc_usrreq.c> "Apple Unix socket peer PID implementation"
[A7_UID]: <https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/getpeereid.3.html> "Apple getpeereid effective credential contract"
[A8]: <https://iterm2.com/python-api/session.html> "Official iTerm2 Python session API"
[A9]: <https://iterm2.com/documentation-url-scheme.html> "Official iTerm2 URL scheme"
[A10]: <https://ghostty.org/download> "Official current Ghostty download"
[A11]: <https://developer.apple.com/documentation/bundleresources/information-property-list/nsappleeventsusagedescription> "AppleEvents usage description"
[A12]: <https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.automation.apple-events> "Hardened runtime AppleEvents entitlement"
[A_LAUNCHD]: <https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html> "Apple per-user launchd agent background"

[N1]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-runtime-wry/src/lib.rs> "Tauri 3 Wry runtime source; replaces historical Tauri 2 process-model reference"
[N2]: <https://webkit.org/blog/17333/webkit-features-in-safari-26-0/> "Official WebKit Safari 26.0 feature release"
[N3]: <https://github.com/tauri-apps/tauri/issues/6381> "Historical upstream Tauri WebGPU discussion"
[N14]: <https://developer.apple.com/documentation/servicemanagement/smappservice> "Apple SMAppService"
[N15]: <https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos> "Apple helper bundle layout and migration"
[N15_AGENT]: <https://developer.apple.com/documentation/servicemanagement/smappservice/agent(plistname:)> "Apple SMAppService agent registration"
[N16]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri-cli/CHANGELOG.md> "Tauri 3 development process-tree lifetime"
[N17]: <https://github.com/tauri-apps/plugins-workspace/blob/d9be6d0492fb6746637ba64497237b2116aaec90/plugins/notification/src/desktop.rs> "Pinned Tauri 3 notification desktop permission behavior"
[N18]: <https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications> "Apple notification authorization and settings"
[N19]: <https://developer.apple.com/documentation/usernotifications/handling-notifications-and-notification-related-actions> "Apple native notification responses"
[N23]: <https://developer.apple.com/documentation/appkit/nsworkspace/accessibilitydisplayshouldreducemotion> "AppKit Reduce Motion preference"
[N24]: <https://developer.apple.com/help/app-store-connect/manage-app-accessibility/reduced-motion-evaluation-criteria/> "Apple reduced-motion evaluation criteria"
[N28]: <https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions> "Apple Accessibility trust and prompt API"
[N_URL_TYPES]: <https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleurltypes> "Apple app URL scheme registration"
[N_SERVICE]: <https://developer.apple.com/documentation/servicemanagement/smappservice/agent(plistname:)> "Apple SMAppService agent registration"
[N_LAUNCHD]: <https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos> "Apple modern helper layout and LaunchAgent registration"

[G1]: <https://git-scm.com/docs/git-rev-parse> "Official Git repository path and identity plumbing"
[G2]: <https://git-scm.com/docs/git-worktree> "Official Git linked worktree documentation"
[G3]: <https://developer.apple.com/documentation/foundation/urlresourcekey/fileresourceidentifierkey> "Apple file-resource identifier and non-persistence across restarts"
[G3_BOOKMARK]: <https://developer.apple.com/documentation/foundation/nsurl/bookmarkdata(options:includingresourcevaluesforkeys:relativeto:)> "Apple URL bookmarks and move/rename continuity"
[D1]: <https://sqlite.org/wal.html> "Official SQLite WAL documentation"
[D2]: <https://sqlite.org/pragma.html#pragma_synchronous> "Official SQLite synchronous durability policy"
[D3]: <https://sqlite.org/wal.html#the_wal_reset_bug> "Official SQLite WAL-reset corruption fix and affected circumstances"
[D4]: <https://sqlite.org/releaselog/3_53_4.html> "SQLite 3.53.4 release, source ID and amalgamation hash"
[D5]: <https://sqlite.org/pragma.html#pragma_fullfsync> "SQLite macOS fullfsync setting"
[S1]: <https://modelcontextprotocol.io/specification/latest> "Current official MCP specification"
[S2]: <https://modelcontextprotocol.io/specification/latest/basic/transports/stdio> "Current official MCP stdio binding"
[S3]: <https://man.openbsd.org/ssh.1> "Official OpenSSH client manual"
[S3_CONFIG]: <https://man.openbsd.org/ssh_config> "Official OpenSSH configuration and host verification"

[P1]: <https://github.com/coding-by-feng/ai-agent-session-center/blob/16df09c4498ec0c8a0a63e77f9617d48109363d2/server/sessionMatcher.ts> "Inspected AI Agent Session Center session matcher"
[P1_APPROVAL]: <https://github.com/coding-by-feng/ai-agent-session-center/blob/16df09c4498ec0c8a0a63e77f9617d48109363d2/server/approvalDetector.ts> "Inspected AI Agent Session Center approval inference"
[P2]: <https://github.com/amirfish1/claude-command-center/blob/91017d02682d9c19ad496f626e8cf624961ea30f/ccc_server/session_graph.py> "Inspected CCC native terminal routing"
[P2_PROCESS]: <https://github.com/amirfish1/claude-command-center/blob/91017d02682d9c19ad496f626e8cf624961ea30f/ccc_server/process_identity.py> "Inspected CCC process incarnation checks"
[P2_EXTERNAL]: <https://github.com/amirfish1/claude-command-center/blob/91017d02682d9c19ad496f626e8cf624961ea30f/ccc_server/external_sessions.py> "Inspected CCC external session discovery"
[P3]: <https://github.com/keysforthewin/thisoffice/blob/08443d04cc2d0ba83f1444741d8da5ed11f93911/server/src/watcher.ts> "Inspected This Office transcript watcher"
[P3_STATE]: <https://github.com/keysforthewin/thisoffice/blob/08443d04cc2d0ba83f1444741d8da5ed11f93911/server/src/office.ts> "Inspected This Office state restoration"
[P4]: <https://github.com/pixel-agents-hq/pixel-agents/blob/3537e140c2094761beae748592aeb92ece8edfdd/server/src/agentRuntime.ts> "Inspected Pixel Agents external agent runtime"
[P4_STATE]: <https://github.com/pixel-agents-hq/pixel-agents/blob/3537e140c2094761beae748592aeb92ece8edfdd/server/src/agentStateStore.ts> "Inspected Pixel Agents persistence"
[P4_ROUTER]: <https://github.com/pixel-agents-hq/pixel-agents/blob/3537e140c2094761beae748592aeb92ece8edfdd/server/src/sessionRouter.ts> "Inspected Pixel Agents session routing and buffering"
[P5]: <https://github.com/0x0funky/TermHive/blob/c2b794fd296affbec80557e7334c9a0860859ea4/src/daemon/runtime.ts> "Inspected Termhive runtime dispatch"
[P5_CODEX]: <https://github.com/0x0funky/TermHive/blob/c2b794fd296affbec80557e7334c9a0860859ea4/src/daemon/codex-agents.ts> "Inspected Termhive Codex thread integration"
[P6]: <https://github.com/xintaofei/codeg/blob/98035964c0f3811dc80a5e9291b89cb6c0570fa0/src-tauri/src/db/service/import_service.rs> "Inspected Codeg import and child filtering"
[P6_ACP]: <https://github.com/xintaofei/codeg/blob/98035964c0f3811dc80a5e9291b89cb6c0570fa0/src-tauri/src/acp/agent_session.rs> "Inspected Codeg ACP session dispatch"
[P7]: <https://colana.ai/> "Official co:lana product descriptions"

[N_KQUEUE]: <https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kqueue.2.html> "Apple kqueue process exit observation"
[N_XCTEST]: <https://developer.apple.com/documentation/xcuiautomation/xcuiapplication> "Apple native application UI testing"

[T_MANAGER]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/manager/mod.rs> "Tauri 3 native window and webview removal cache cleanup"

[C8_PIN]: <https://github.com/anthropics/claude-code/blob/e8ae451830fb8d1d8edf853c7830ff6c650d3ee3/mods/types/claude-code.d.ts> "Claude 2.1.290 release-pinned public declarations, generated by 2.1.277"
[A_STAT]: <https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/stat.2.html> "Apple stat and character-device st_rdev"
[R_ANIMATION]: <https://github.com/mrdoob/three.js/blob/9b4a2ac29c63ccb43fd51c5661f2f873ac2c39b8/src/renderers/common/Animation.js> "Pinned Three.js internal frame scheduler and stop/dispose"
[T_RESOURCES]: <https://github.com/tauri-apps/tauri/blob/a8703ee487c659efbebb27c799752d523a6d09a1/crates/tauri/src/resources/mod.rs> "Tauri 3 public native Resource and ResourceTable APIs"
[N_LOGIN_ITEM]: <https://developer.apple.com/documentation/servicemanagement/smappservice/loginitem(identifier:)> "Apple application login-item registration"
[N_SERVICE_REGISTER]: <https://developer.apple.com/documentation/servicemanagement/smappservice/register()> "Apple immediate/login launch and crash/nonzero helper relaunch"
[N_SERVICE_UNREGISTER]: <https://developer.apple.com/documentation/servicemanagement/smappservice/unregister()> "Apple unregister terminates a running helper"
[N_LSUIELEMENT]: <https://developer.apple.com/documentation/bundleresources/information-property-list/lsuielement> "Apple background agent application presentation"
[N_AGENT_NOTIFICATION]: <https://developer.apple.com/forums/thread/804854> "Apple staff guidance on UserNotifications application context; not a Threadspace reproduction"
[N_NOTIFICATION_DELEGATE]: <https://developer.apple.com/documentation/usernotifications/unusernotificationcenter/delegate> "Apple notification delegate setup before application launch completes"

[D_AUTOINC]: <https://sqlite.org/autoinc.html> "SQLite AUTOINCREMENT monotonicity, gaps and integer range"
[D_BACKUP]: <https://sqlite.org/backup.html> "SQLite consistent online backup API"
[O_DAEMON_CLIENT]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-daemon/src/client.rs> "Released Codex AF_UNIX WebSocket connection and initialization handshake"
[O_DAEMON_IMPL]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-daemon/src/lib.rs> "Released read-only existing daemon version/socket probe"
[O_HISTORY_BUILDER]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/thread_history.rs> "Released legacy history fallback turn shells and native boundary markers"
[O_HISTORY_PROJECTION]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server-protocol/src/protocol/thread_history_projection.rs> "Released projection from native turn boundary events"
[O_HISTORY_MATERIALIZATION]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/thread-store/src/local/thread_history_materialization.rs> "Released local paginated history materialization"
[O_HISTORY_STORE]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/thread-store/src/local/thread_history.rs> "Released paginated turn-row writes"
[O_CHILD_INPUT]: <https://github.com/openai/codex/blob/d27764b82f7118f674371e6d6e76271d9d606edb/codex-rs/app-server/src/request_processors/thread_input.rs> "Released parent-owned child direct-input boundary"
