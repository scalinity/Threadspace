# M2 — Manually launched Claude vertical slice: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native) for the ordered core and superpowers:subagent-driven-development for the isolated tasks marked *parallel*. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A real Claude Code session started independently in Terminal is observed, shown as one persistent worker, follows its native turns and attention needs, survives completion, accepts a follow-up as the same worker, and returns to its exact current Terminal tab on an explicit Return.

**Architecture:** The pinned observer mod (`packages/provider-mod`) drains sanitized records through `threadspace-hook mod-batch` (now bundled, staged into a Threadspace-owned integration directory) into the companion, where a new observer adapter in `provider-claude` turns them into canonical facts with provenance tiers. The reducer (version 3) turns the three last-observation fields of D-0007 §10 into evidence sets and gates lower-tier (post-reload, host-read) lifecycle evidence on independent kernel/inventory corroboration. Discovery maps inventory waits to session-scoped wait episodes with ordered inventory points. The UI shows one worker per Session with coverage, attention, Mark handled and an exact Return.

**Tech Stack:** Rust (workspace crates), SQLite 3.53.4 (D-0001), Tauri 3.0.0-alpha.4 / tao 0.37.0 / Wry 0.57.0 (D-0008), React + Three.js WebGPURenderer, Claude Code hooks-module API (2.1.295 declarations), macOS AppleScript/kernel APIs.

**Spec:** `docs/MILESTONES.md` (M2), `docs/SPEC.md` (§5.2, §7.3, §8.2–§8.5, §11.2–§11.4, §13.2, §15.3–§15.5, §19.2, §20.2), D-0005, D-0007, D-0008, the M2 brief (owner instruction, 2026-10-09).

## Settled decisions

| # | Decision | Reason |
| --- | --- | --- |
| S1 | Requalify the observer for Claude Code **2.1.295** (installed CLI); keep 2.1.291 (D-0005). Every other version is `LIMITED` (incompatible profile). | Running sessions are 2.1.292/2.1.294/2.1.295; a newer CLI does not certify older processes. 92 of 97 provider-semantics declarations are identical; the plugin-facing `turn`/`session`/`process` nouns and `PromptSubmitInput` are unchanged; `claude plugin test` 33/33 on 2.1.295. Native facets are requalified in Task 9. |
| S2 | Provider version comes from the kernel: the helper's ancestry walk reads the calling Claude process (pid, birth, executable path under `~/.local/share/claude/versions/<v>`). The mod's `$.session.version()` claim is recorded, never trusted alone. | `$.session.version()` is middleware-interceptable (D-0005 item 11b). |
| S3 | Qualification sessions use the owner's normal Claude login with **per-session activation**: `CLAUDE_CODE_PLUGIN_DIRS=<owned mod copy>` and `--settings <installer-written session settings>`. `~/.claude` is never modified. Install/reinstall/remove cycles run against disposable config directories. | Owner decision (2026-10-09). A disposable `CLAUDE_CONFIG_DIR` or `HOME` is logged out (measured). |
| S4 | Native qualification runs on the **dev channel** (`Threadspace Dev.app`, `ai.scalinity.threadspace.dev.agent`). Production stays on the verified reducer-2 install. | Owner decision. M2 brings reducer 3; a reducer-2 build refuses a reducer-3 store. |
| S5 | **REDUCER_VERSION = 3.** A store with an older checkpoint upgrades by re-deriving from its earliest checkpoint (M0 baseline or genesis) through every later journal entry as catch-up, rebased on committed rows, so the result is independent of where later checkpoints sit. | Evidence sets need per-observation points an older checkpoint does not hold; D-0007 §6 requires placement independence. |
| S6 | Mod-batch receipt is `{ receiptVersion: 1, results: [...] }` (SPEC §8.3 "one result per record", qualified mod); the Rust contract field changes from `receipts` to `results`. The helper input is the mod's object envelope. | The Rust helper and the qualified mod disagree today; the mod is the qualified side. |
| S7 | Lower tier: facts whose session identity comes from a host read (`sessionIdSource: "session.id"`, i.e. after a reload) or from a non-engine dispatch carry `EvidenceClass::HostRead`. The reducer applies their turn outcomes and input verdicts only when the fact's provider process (helper ancestry) is a kernel-proven process of that Session's execution (discovery). Otherwise they are retained, not applied. | D-0005: authority after reload needs separate native proof; no reattribution. |
| S8 | Conflict precedence when causally-latest evidence disagrees: presence `DETACHED > PARKED > LIVE`; mode the lowest `ExecutionMode`; device/runtime id `None`; input verdict rejected; observer link `DISCONNECTED > CONFLICT > STALE > UNKNOWN > CURRENT`. A conflict is visible as coverage uncertainty. | Deterministic, conservative, order-independent (D-0007 §1). |

## Global constraints

- Pins: tao `0.37.0`, wry `0.57.0`, tauri `3.0.0-alpha.4`, tauri-runtime-wry `3.0.0-alpha.4` (D-0008 guard must keep passing).
- SQLite `3.53.4` source id `2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc`.
- `acceptedInputProvenance=true` only inside the qualified profiles; `automaticHumanFollowupResolution=false`; explicit Mark handled stays the resolution path.
- Never register `WorktreeCreate` / `WorktreeRemove`; never add blocking decisions; conventional hooks exit 0 silently.
- Capture executable p95 ≤ 25 ms; 250 ms wall budget (100 ms shutdown path); receipt ≤ 16 KiB; batch ≤ 128 records / 64 KiB; queue 2,048 records / 8 MiB.
- Latency (SPEC §20.2): capture→committed p95 ≤ 100 ms; committed→visible DOM p95 ≤ 100 ms; native event→visible p95 ≤ 250 ms; route p95 ≤ 750 ms, hard 2 s.
- No `useEffect` in React components. No new runtime dependencies without need.
- Qualification-only controls stay behind `--features qualification` or the qualification build flag.
- Public evidence is redacted (no prompt bodies, transcripts, home paths, settings backups).

## Review focus

1. A delayed `turn.complete` from session A arriving after the mod observed session B must stay A's (frozen entry context) — T4 test `delayed_result_keeps_entry_session`.
2. A reload followed by a lower-tier `turn.complete` must not create a native outcome until discovery proves the same ProcessKey runs that Session; arriving in either order must converge — T2 permutation family `observer-reload`.
3. Uninstall after the owner edited one of our hook entries must leave their edit and report a conflict, not delete it — T7 test `uninstall_keeps_owner_edit_as_conflict`.
4. A companion that dies mid-batch (partial commit) then a retry must not duplicate observations and must answer exactly one result per record — T5 test `partial_commit_then_retry_is_idempotent`.
5. A route requested for a Session whose Terminal tab was closed and whose TTY was reused must refuse (`NO_MATCHING_TAB`/`BINDING_STALE`), never focus the new tab — T10 native case `stale_tty_reuse` and existing surfaces unit tests.

## File map

| Area | Files | Responsibility |
| --- | --- | --- |
| Profiles | `crates/provider-claude/src/profiles.rs`, `docs/compatibility/claude-observer-2.1.295.json`, `docs/decisions/D-0009-claude-2.1.295-observer-requalification.md` | Qualified observer versions and capabilities |
| Engine | `crates/contracts/src/canonical/{fact,records}.rs`, `crates/state-engine/src/{reduce,semantic,lib,engine}.rs`, `crates/journal/src/{canonical,materialize}.rs`, `docs/decisions/D-0010-m2-evidence-sets.md` | Evidence sets, lower-tier gating, reducer 3 upgrade |
| Engine tests | `tests/synthetic/src/{scenarios,builder}.rs`, `tests/synthetic/src/bin/m1/permutations.rs`, `fixtures/m2/reducer-2-store/`, `tests/synthetic/tests/reducer_upgrade_v3.rs` | Adversarial permutations, upgrade fixtures |
| Observer adapter | `crates/provider-claude/src/observer.rs`, `apps/agent-macos/core/src/adapters.rs` | Mod record → facts with provenance |
| Helper | `crates/relay/src/bin/threadspace-hook.rs`, `crates/relay/src/modbatch.rs`, `crates/relay/src/capture.rs`, `crates/contracts/src/canonical/capture.rs` | Mod-batch transport, ancestry, receipts |
| Mod | `packages/provider-mod/**` | Budget argv, epoch predecessor, tests |
| Discovery | `crates/provider-claude/src/{inventory,discovery}.rs`, `apps/agent-macos/core/src/discovery.rs`, `crates/journal/src/identity.rs` | Ordered inventory points, wait mapping, background rows |
| Installer | `crates/provider-claude/src/setup/{mod,ordered_json,settings,owned}.rs`, `apps/desktop/src-tauri/src/integration.rs`, `apps/agent-macos/build.sh` | Reversible setup, bundled helper |
| UI | `apps/desktop/src/ui/*`, `apps/desktop/src/bridge/client.ts`, `crates/contracts/src/projection.rs`, `crates/journal/src/projection.rs`, `apps/desktop/src/qualification/m2.ts` | Worker, coverage, attention, Return refusals |
| Native harness | `tests/native/harness/src/bin/m2/*.rs`, `tests/native/harness/src/{claude,integration}.rs` | Vertical slice, routes, faults, cycles, latency |
| Evidence | `evidence/M2/**`, manifest generator `tests/native/harness/src/bin/m2/manifest.rs` | Machine-readable acceptance record |

---

### Task 1: Observer profiles (S1, S2)

**Files:** Modify `crates/provider-claude/src/profiles.rs`; create `docs/compatibility/claude-observer-2.1.295.json`, `docs/decisions/D-0009-claude-2.1.295-observer-requalification.md`, `evidence/M2/provider/claude/declarations.json`.

**Interfaces — Produces:**
```rust
pub struct ObserverProfile { pub version: &'static str, pub profile_ref: &'static str }
pub const QUALIFIED_OBSERVER_VERSIONS: &[&str] = &["2.1.291", "2.1.295"];
/// `Some` only for a qualified version; the version is the kernel-read executable's.
pub fn observer_profile(version: &str) -> Option<ObserverProfile>;
/// "~/.local/share/claude/versions/2.1.295" → Some("2.1.295").
pub fn version_from_executable(path: &str) -> Option<String>;
```

- [ ] Write tests: `observer_profile("2.1.295").is_some()`, `observer_profile("2.1.292").is_none()`, `version_from_executable` on a versions path, on `ClaudeCode.app/.../claude` (None), on a `..` path (None).
- [ ] Implement; `cargo test -p threadspace-provider-claude profiles`.
- [ ] Write the 2.1.295 profile JSON with per-capability `basis` / `classes` / `refs`; native facets marked `PENDING_NATIVE` until Task 9 fills them.
- [ ] Commit `Qualify the observer profile table for Claude Code 2.1.295`.

### Task 2: Reducer 3 — evidence sets and lower-tier gating (S5, S7, S8)

**Files:** `crates/contracts/src/canonical/{fact,records}.rs`; `crates/state-engine/src/{reduce,semantic,lib}.rs`; `crates/journal/src/canonical.rs`; schemas/TS regenerated; D-0010.

**Interfaces — Produces:**
```rust
// contracts
pub enum EvidenceClass { /* existing */ HostRead }
pub struct AttachObservation { pub observation_id: String, pub point: Option<CausalPoint>,
    pub mode: ExecutionMode, pub presence: AttachedPresence,
    pub native_runtime_id: Option<String>, pub controlling_device: Option<u32> }
pub struct InputVerdict { pub observation_id: String, pub point: Option<CausalPoint>,
    pub accepted: Option<AcceptanceProof>, pub rejected: Option<String>, pub provider_process: Option<String> }
pub struct LinkObservation { pub observation_id: String, pub point: Option<CausalPoint>, pub link: ObservationState }
pub struct PendingOutcome { pub observation_id: String, pub outcome: TurnOutcome, pub reason: Option<String>, pub provider_process: String }
// records: ExecutionRecord.attachments: BTreeSet<AttachObservation>  (mode/attached/runtime/device stay as derived fields)
//          InputRecord.verdicts: BTreeSet<InputVerdict>               (acceptances/rejections derived)
//          SessionRecord.links: BTreeSet<LinkObservation>             (link derived)
//          TurnRecord.pending_outcomes: BTreeSet<PendingOutcome>
// state-engine
pub const REDUCER_VERSION: u32 = 3;
fn latest<T>(items, point_of) -> Vec<&T>   // causal maxima via causal::compare
fn derive_attachment(execution) / derive_input(input) / derive_link(session) / derive_frontier(session)
fn corroborated(state, session_id, provider_process) -> bool   // process key of an execution of the session, Kernel provenance
```

- [ ] Failing scenarios first (tests/synthetic): `attach_reorder_converges`, `attach_conflict_conservative`, `followup_accept_then_reject` (both orders, incomparable), `observer_link_reorder`, `observer_reload_lower_tier` (outcome before/after discovery corroboration), `duplicates_of_each`. Families: new `evidence-sets` in `REQUIRED_FAMILIES`.
- [ ] Run `cargo test -p threadspace-synthetic --test scenarios` → the new ones FAIL (semantic hashes diverge across orders).
- [ ] Implement sets and derivations in dependency order inside `Tx::derive` (execution attachment before presence; inputs before frontier; frontier recomputed from all inputs of the session; link after inputs; pending outcomes consulted in `derive_turn`); `rederive` touches inputs, frontiers and links.
- [ ] Upgrade: `upgrade_json` accepts versions 1 and 2; `load_engine` for `version < 3` starts from the earliest checkpoint, replays every later entry as catch-up, `read_as_recorded`, `keep_committed`, `Engine::upgrade`, persists one `REDUCER_UPGRADE` checkpoint.
- [ ] Fixture `fixtures/m2/reducer-2-store/journal.sqlite3` emitted at af9b285 (emitter copied from `fixtures/m1/reducer-1-store/emit_reducer1_store.rs` pattern); tests: upgrade from reducer-1 fixture and reducer-2 fixture, checkpoint at 0/middle/end → identical state hash; dispositions PENDING stays PENDING, created HELD, upgrade submits nothing; restart no second upgrade.
- [ ] Permutations: `threadspace-m1 permutations 20000` passes with the new family; record counts.
- [ ] `cargo test --workspace` (with `--features qualification` where required), Clippy clean; schema freshness (`THREADSPACE_CHECK_SCHEMAS=1`), regenerate TS.
- [ ] Commit in three steps: sets+derivation; lower-tier gating; reducer-3 upgrade + fixtures.

### Task 3: Inventory points and wait mapping

**Files:** `crates/provider-claude/src/{inventory,discovery}.rs`, `apps/agent-macos/core/src/discovery.rs`, `crates/journal/src/identity.rs`.

**Interfaces — Produces:**
```rust
pub struct InventoryPoint { pub epoch: String /* core generation */, pub sequence: u64 /* pass counter */ }
pub enum InventoryWait { None, Waiting { category: WaitCategory, subtype: String } }
pub fn interpret_status(row: &InventoryRow) -> InventoryWait   // "waiting" + "input needed" → Input; approval/permission → Approval; background "blocked" → JobBlocked
```

- [ ] Tests: `interpret_status` table; apply_discovery emits `WaitStateObserved{Positive}` session-scoped (no turn) on transition into waiting and `Cleared` on transition out; `ProviderSnapshotObserved.causal` = `claude.inventory`/core generation/`inventory`/pass; absent rows never end an execution; parent turn completed + session wait → turn stays COMPLETED (scenario `parent_completed_child_waiting`).
- [ ] Background rows (`kind != interactive`): `state` kept in `SnapshotRow.state` (new optional field), `blocked` → JobBlocked wait, others display only.
- [ ] Commit.

### Task 4: Observer adapter

**Files:** create `crates/provider-claude/src/observer.rs`; modify `crates/provider-claude/src/lib.rs`, `apps/agent-macos/core/src/adapters.rs`.

**Interfaces — Consumes:** Task 1 `observer_profile`; Task 2 `EvidenceClass::HostRead`. **Produces:** `pub fn normalize(envelope: &ObservationEnvelope) -> Normalized` routed for `adapterId == "threadspace-observer"`.

Mapping (entry/result phases; `authoritative = engineDispatch && dispatchOrigin == {engine, core} && profile qualified && sessionIdSource != "session.id"`; result-phase lifecycle also needs `core.coreSettled`):

| Record | Facts |
| --- | --- |
| bootstrap (session.start result) | `ObservationLinkChanged{CURRENT}` (point: epoch/entry sequence, `predecessorEpoch` as native predecessor key) |
| classic.SessionStart result | `SessionIdentified{start_source}` |
| prompt.submit entry | `InputSubmitted{origin}` (composer→HUMAN_COMPOSER, bridge→HUMAN_BRIDGE, sdk→SDK, task-notification→TASK_NOTIFICATION, scheduled-trigger→SCHEDULED, plugin→PLUGIN, else UNCLASSIFIED; `submission` when `activeTurnIdAtSubmission` is set) |
| prompt.submit result | `InputAccepted{proof}` or `InputRejected{"DROPPED"}` |
| turn.start result | `TurnStarted` |
| turn.step result | `TurnStepObserved`; `abandoned` phase → nothing native |
| turn.complete result | authoritative: `TurnOutcomeObserved{answer→COMPLETED, aborted→INTERRUPTED, error→FAILED, refusal→REFUSED}`; otherwise the same fact with `HostRead` provenance (pending, S7) or nothing when nonengine |
| tool.call entry/result | `ActivityStarted` / `ActivityFinished{SUCCESS/FAILURE}` |
| tool.check result | `PermissionCheckObserved` |
| agent.spawn result, coreSettled, agentId | `ActorIdentified{SUBORDINATE}` + `ActorRelationObserved{IMMEDIATE_PARENT}` |
| session.attach / detach result | `ExecutionAttached{mode from surface, LIVE/DETACHED}` |
| session.end result (exit reasons) | `ObservationLinkChanged{DISCONNECTED}` |
| provider-error | `ObservationGapDetected{"observer", ...}` (no outcome) |

- [ ] Tests (one per row) plus: `forged_spawn_creates_no_actor`, `nonengine_turn_complete_creates_no_outcome`, `delayed_result_keeps_entry_session`, `incompatible_version_is_limited`, `sanitized_payload_retains_no_text`.
- [ ] Commit.

### Task 5: `threadspace-hook mod-batch` transport

**Files:** create `crates/relay/src/modbatch.rs`; modify `crates/relay/src/bin/threadspace-hook.rs`, `crates/relay/src/capture.rs` (ancestry returning the Claude ProcessKey), `crates/contracts/src/canonical/capture.rs` (`results`).

**Interfaces — Produces:**
```rust
pub struct ModBatchRequest { pub receipt_version: u32, pub kind: String, pub source_epoch: String, pub dropped_records: u64, pub records: Vec<serde_json::Value> }
pub fn envelopes(request: &ModBatchRequest, context: &HookContext) -> Vec<(String, Result<ObservationEnvelope, &'static str>)>
pub fn run(stdin: &[u8], argv: &[String], deadline: Instant) -> ModBatchReceipt   // exactly one result per submitted record
```

- [ ] Tests: valid batch → all COMMITTED (fake companion socket); companion absent → LOCAL_SPOOLED; spool unavailable → NOT_ACCEPTED; malformed record → NOT_ACCEPTED with reason, others proceed; duplicate UUID in one batch → one result; `partial_commit_then_retry_is_idempotent`; receipt ≤ 16 KiB at 128 records; `--budget-ms 80` respected; dropped-records marker has a stable observation id.
- [ ] Qualification-only fault (`THREADSPACE_QUALIFY_MOD_BATCH_FAULT=partial|malformed|exit1|slow`, compiled only with `--features qualification`).
- [ ] Bundle: `apps/agent-macos/build.sh` builds `threadspace-hook` and copies it into `ThreadspaceAgent.app/Contents/MacOS/` before signing.
- [ ] Commit.

### Task 6 *(parallel)*: Observer mod updates

**Files:** `packages/provider-mod/hooks/{register,delivery}.ts`, `packages/provider-mod/types/index.d.ts`, `.claude-plugin/plugin.json`, `packages/provider-mod/tests/*`.

- [ ] `delivery.ts` appends `["--budget-ms", String(timeoutMs - 20)]` to argv; kit helper expects it.
- [ ] Epoch predecessor: `$.state` value `threadspace-observer.epoch` (contract in `types/index.d.ts`); bootstrap record carries `predecessorEpoch`.
- [ ] Tests updated for `results` receipts; new tests `budget_argv_appended`, `reload_records_predecessor_epoch`.
- [ ] `claude plugin validate` and `claude plugin test` on a disposable copy under 2.1.295: all pass.
- [ ] Commit.

### Task 7 *(parallel)*: Reversible integration setup

**Files:** create `crates/provider-claude/src/setup/{mod,ordered_json,settings,owned}.rs`, `apps/desktop/src-tauri/src/integration.rs`; modify `apps/desktop/src-tauri/src/launch.rs` (`--integration plan|install|uninstall|status --config-dir <dir> --scope user|session`).

**Interfaces — Produces:**
```rust
pub enum Scope { User, Session }
pub struct Target { pub config_dir: PathBuf, pub owned_dir: PathBuf, pub agent_identifier: String, pub helper_source: PathBuf, pub mod_source: PathBuf, pub scope: Scope }
pub struct Plan { pub files: Vec<FileChange>, pub owned_entries: Vec<OwnedEntry>, pub rollback: Vec<String> }
pub fn plan(target: &Target) -> Result<Plan, SetupError>;
pub fn install(target: &Target) -> Result<InstallRecord, SetupError>;   // idempotent
pub fn uninstall(target: &Target) -> Result<UninstallReport, SetupError>; // removes only still-matching owned entries; conflicts reported
pub fn status(target: &Target) -> Result<IntegrationState, SetupError>;
```

- [ ] Ordered JSON round-trip tests (key order, unicode, numbers as written).
- [ ] Tests: install into empty / existing complex settings (foreign hooks, matchers, MCP servers, env); twice → one owned set; ten install/reinstall/remove cycles → original bytes restored when untouched; `uninstall_keeps_owner_edit_as_conflict`; hash changed between read and write → refused; backups 0600 under owned dir; atomic same-dir temp + rename; no `WorktreeCreate`/`WorktreeRemove`; existing `CLAUDE_CODE_PLUGIN_DIRS` preserved and appended; session scope writes only owned files.
- [ ] Owned mod copy at `<owned>/observer/<sha>/` with `captureArgv` default `[<owned>/bin/threadspace-hook, "mod-batch", "--agent", <id>]`; helper staged by atomic replace.
- [ ] Commit.

### Task 8 *(parallel after T2 contracts land)*: Coverage projection and UI

**Files:** `crates/contracts/src/projection.rs` (`SessionView.coverage`), `crates/journal/src/projection.rs`, `apps/desktop/src/ui/{FleetPanel,Inspector,AttentionPanel,SceneView,Toolbar}.tsx`, `apps/desktop/src/bridge/client.ts`, `apps/desktop/src/ui/refusal.ts`, `apps/desktop/src/qualification/m2.ts`.

**Interfaces — Produces:** `SessionCoverage { observer: "NATIVE" | "LOWER_TIER" | "LIMITED" | "NONE", observerVersion, link, inventory: "CURRENT" | "STALE" | "NONE", hooks: boolean, conflicts: string[] }`; `refusalText(code): string`; qualification commands `m2-fleet`, `m2-press-return`, `m2-press-mark-handled`.

- [ ] Vitest: `sceneModel` keeps a worker for a completed session; attention lists exclude resolved; `refusalText` covers every SPEC §13.2 code; Return disabled for fixtures.
- [ ] No `useEffect`.
- [ ] Commit.

### Task 9: Native harness — integration, vertical slice, routes, faults, latency

**Files:** create `tests/native/harness/src/{claude,integration}.rs`, `tests/native/harness/src/bin/m2/{main,cycles,vertical,routes,faults,latency,manifest}.rs`; update `tests/native/README.md`.

- [ ] `m2 cycles 10` — disposable config dirs from fixtures; invariants per cycle.
- [ ] `m2 vertical 10` — disposable Terminal (`terminal::Tab::open`), activation env, `claude --settings … --model haiku`, trust prompt, tool-using prompt, completion, worker present, UI Return (readback), follow-up, same Session/worker, `/exit`, historical state.
- [ ] `m2 routes 30` — exact Returns with independent readback (reuse `threadspace-qualify route-loop` checks); negative cases reorder/move/close/stale TTY.
- [ ] `m2 faults all` — stop continuation, interrupt, failed outcome (or recorded BLOCKED), blocked submission, provider exception, relay unavailable, incompatible profile (2.1.292 binary), mod reload + restoration, parent-completed/child-waiting, delayed submission, generator cancellation, forged spawn/nonengine lifecycle (test-only forger plugin), partial receipt + retry.
- [ ] `m2 latency` — capture/commit/view timestamps.
- [ ] Commit per runner.

### Task 10: Evidence, documents, close-out

- [ ] `evidence/M2/manifest.json` (base SHA, build identities, profile, settings diffs, traces, outcomes, routes, Mark handled, relay faults, tests/seeds/counts, limitations).
- [ ] SPEC/MILESTONES current-design edits; D-0009, D-0010 final.
- [ ] Privacy scan (redaction literals, home paths, emails, credentials) and document consistency.
- [ ] Commit, push `m2`, verify `m2 == origin/m2`, clean tree. Stop for independent review.
