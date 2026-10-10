# D-0009 — Claude Code 2.1.295 observer requalification

**Status:** PROPOSED — pending independent re-review.
**Affects:** D-0005 (extends its contract to a second build), SPEC §5.2, §7.3, §11.4; `crates/provider-claude/src/profiles.rs`; `docs/compatibility/claude-observer-2.1.295.json`; MILESTONES M2 prerequisites.

## Context

D-0005 qualifies the observer mod's semantics on Claude Code **2.1.291** only. The machine M2 runs on has 2.1.292, 2.1.293, 2.1.294 and 2.1.295 installed, and its `claude` launcher resolves to 2.1.295. D-0005's evidence does not carry over to another build by assumption. M2 therefore requalifies the paths it exercises on 2.1.295 and leaves every other build unqualified.

## Evidence

`evidence/M2/provider/claude/` holds the installed-declaration comparison, `claude plugin validate` and `claude plugin test` output. Native evidence comes from the M2 runs in `evidence/M2/vertical/`, `routes/` and `faults/`.

1. **Declarations.** The 2.1.295 plugin-authoring declarations have sha256 `e2e13b2ceb80ff3cde49d8427ebe19c782a24caa817c7b20c56f91925a06f9cb` (21,463 lines, first line `// Written by Claude Code 2.1.295.`). They are compared, with comments stripped, against D-0005's 2.1.291 file (`03815831…`, 20,755 lines) for the 97 named declarations the observer depends on. 92 are identical. Five change, all additively:
   - `AgentSpawnArgs` and `AgentSpawnInput` gain an optional `workflow { runId, agentIndex }`;
   - `ToolCallEnvelope` and `ToolCallInput` gain `ToolRequestMeta`;
   - `MockSession` is a new test-kit utility.

   Among the unchanged names are `EventCalls`, `Origin`, `PromptSubmitInput`, `PromptOrigin`, `TurnCompleteInput` and every session and turn input and result. The observer reads none of the added fields (`declarations.json`).
2. **Engine test kit and validate.** Under 2.1.295, `claude plugin validate` passes and `claude plugin test` runs 38 of 38, on a disposable copy of `packages/provider-mod`. The kit includes the provenance, reload, spawn, generator and delivery cases D-0005 relies on.
3. **Loading still writes into the mod folder.** A session loading the Threadspace-owned copy wrote `.claude-plugin/types/` (`claude-code/index.d.ts`, sha256 `b45a8c7d21abd38a5753080526e423ea73d1061c4c3423755e789deb45e0f60d`, 15,982 lines, plus `claude-code-mcp/`, `claude-code-tools/` and `tsconfig.json`) into it.
4. **Interactive human input, natively.** The vertical slice types every prompt into a Claude session started by hand in Terminal. In ten cycles, all 20 prompt submissions report original origin `composer` with host-stamped `engine`/`core` dispatch, and all 20 `turn.complete` results report reason `answer` under the same dispatch, in both full runs (`native-origins.json`). D-0005 had this only as declaration and test-kit evidence.
5. **Version gating.** The profile is chosen from the provider executable the kernel reports (`…/claude/versions/<version>`). A 2.1.292 session runs lower-tier and produces no lifecycle outcome (`faults` case `incompatible-profile`).
6. **Native outcome and provenance cases** (`evidence/M2/faults/`):
   - **Stop continuation:** a Stop hook that blocks once keeps one native turn, `COMPLETED`, with one completion item.
   - **Interrupt:** Ctrl+C during a running turn gives `INTERRUPTED`.
   - **Failure:** an unreachable API gives `FAILED` with an `ERROR` item.
   - **Blocked prompt:** a blocking `UserPromptSubmit` hook is recorded as a rejected input that starts no turn.
   - **Reload (rejected proof predicate):** saving the owned mod copy reloads the module. The old candidate displayed `RESTORED` 819 ms later, its native ID unchanged, and applied outcomes after reload. Independent review established that historical Session/process attachment could satisfy the old predicate. This retained run proves the observed old behavior; it does not qualify the corrected ownership proof described below.
   - **Forger:** a test-only plugin beside the observer submits a prompt; the retained native run creates no forged turn or actor. It does not establish that the fabricated-spawn handler was exercised. The explicit shortcut-spawn provenance check is test-kit evidence.
   - **Delayed submission:** a prompt submitted during a running turn keeps that turn as its active-at-submission identity.
   - **Child wait:** a background child's permission wait stays session-scoped beside its completed parent turn.

## Decision

- **Claude Code 2.1.295 is a qualified observer build with D-0005's capability profile unchanged:** `acceptedInputProvenance = true` and `automaticHumanFollowupResolution = false` (NOT_SUPPORTED), because `PromptSubmitInput` still carries no original-submission order witness. Explicit Mark handled remains the resolution path; M2 qualifies it natively against its durable command. Lifecycle authority, the reload rule and the owned-copy rule stand as D-0005 states them.
- **The qualified builds are exactly 2.1.291 and 2.1.295** (`QUALIFIED_OBSERVER_VERSIONS`). Every other build, including 2.1.292–2.1.294, runs lower-tier until requalified.
- The additive `workflow` spawn field and `ToolRequestMeta` stay unread. Using either needs its own qualification.

## Consequences

- `docs/compatibility/claude-observer-2.1.295.json` records the profile, its declaration identities and the M2 evidence hashes; SPEC §5.2, §7.3 and §11.4 name both builds.
- A build that changes any of the 92 compared declarations, adds an original-submission witness or restores plugin lifecycle raises needs a new requalification. So does a newer launcher target.

## F1 remediation proposal — original ownership and reload proof

**This decision remains PROPOSED.** The first independent M2 review rejected delayed-callback ownership and the historical reload-corroboration predicate. The remediation implements the narrower rules below and retains its portable evidence at `evidence/M2/remediation-1/f1a/`. Native qualification of the revised paths requires a newly source-matched development build; no such macOS execution is claimed by the Linux remediation records.

### Original callback ownership

`packages/provider-mod/hooks/ownership.ts` retains immutable original Session and observer-generation scopes for one source epoch. The native helper supplies the provider namespace and actual process identity; the module never shares its ledger across loads or provider processes.

- A principal Turn is retained after its original `turn.start` reaches and settles in engine/core. A later `turn.step` or `turn.complete` resolves that retained Turn, including after A→B or A→B→A. The callback's result keeps the context frozen at its own entry.
- Child ownership is established from an actual engine/core-settled spawn or an engine/core-settled classic actor identity. The actor's first qualified step can establish its own Turn. Tool occurrence ownership is frozen before `next(e)` can invoke a nested spawn and retained for delayed callbacks.
- Turn lookup includes actor identity. Reusing the same native Turn or actor key for another Session/generation makes the key ambiguous. No global native Turn-ID uniqueness is assumed; no newest/current-owner selection resolves a collision.
- Unknown native ownership is retained without a Session. The required legacy `sessionGeneration` field uses zero, `ownershipGeneration` is absent, and separately named `currentSessionId`/`currentSessionGeneration` fields remain diagnostic metadata. Unknown native creation claims leave bounded uncertainty markers, so a later reuse cannot erase the uncertainty. Overlong identities cannot acquire ownership through truncation.
- The retained maps have fixed limits and never evict. At a limit, new work remains unowned. Root tool/spawn events with no Turn field may use only the uninterrupted initial engine-identified Session interval; after a Session transition they require retained actor/occurrence evidence. A host-read bootstrap alone never establishes subordinate ownership.

The portable actual-module suite covers delayed entry, delayed return, A→B→A generations, native ID reuse, delayed child work, first-seen late events, forged/shortcut authority, bounded retention and fail-open generator/exception behavior. The identical delayed-entry assertion fails against the exact rejected candidate because its outcome acquires Session B. The 2.1.295 Linux host kit passes 38/38 after three old current-Session attribution expectations were corrected; the original 35/38 attempt remains retained with its specific failures explained.

### Scoped native proof after reload

The observer starts a detached, bounded `observer-proof` helper request for a host-read scope. Its request names the source epoch, observed Session generation and claimed Session. The helper independently verifies the actual process incarnation, executable and live inventory ownership and durably records/spools that native proof before returning a new token. A response from middleware-interceptable `$.process.run` is insufficient on its own.

The observer accepts only a typed, successful response with matching scope fields and seals it only if its epoch, Session and generation stayed unchanged throughout the request. The seal is explicitly observer-origin metadata, never an engine dispatch. Only subsequently captured qualified Turn starts can retain the sealed token. A pre-seal Turn is not rewritten after the proof completes.

The native adapter/reducer must join the independently durable proof, matching observer seal, original process/executable/Session scope and applicable Turn evidence. Historical attachments, an unmatched token, a stale generation, a wrong process or an ambiguous proof leave the outcome pending. D-0010 describes the revised canonical proof representation and reducer-version transition.

The F1 portable records include a valid observer-side handshake, pre-seal versus post-seal scopes, A→B→A during a delayed proof and malformed/mismatched/failing replies. The matching native evidence and canonical-state checks are separately retained by the F1B campaign. These tests establish the implemented handshake and conservative state behavior; they do not substitute for a native unchanged-Session reload run on the revised build.

The provider capability contract stays `acceptedInputProvenance = true`, `automaticHumanFollowupResolution = false`. The tested helper/proof path adds no provider-control output, and no input acceptance or automatic follow-up authority is inferred from a proof token.
