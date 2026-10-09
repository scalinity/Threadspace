# D-0009 — Claude Code 2.1.295 observer requalification

**Status:** PROPOSED for M2 independent review.
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
   - **Reload:** saving the owned mod copy reloads the module. The session's tier goes from `NATIVE` to `RESTORED` 819 ms later, its native ID unchanged, and turns complete after the reload.
   - **Forger:** a test-only plugin beside the observer submits a prompt and answers spawns with a fabricated agent ID. It creates no forged turn and no actor.
   - **Delayed submission:** a prompt submitted during a running turn keeps that turn as its active-at-submission identity.
   - **Child wait:** a background child's permission wait stays session-scoped beside its completed parent turn.

## Decision

- **Claude Code 2.1.295 is a qualified observer build with D-0005's capability profile unchanged:** `acceptedInputProvenance = true` and `automaticHumanFollowupResolution = false` (NOT_SUPPORTED), because `PromptSubmitInput` still carries no original-submission order witness. Explicit Mark handled remains the resolution path; M2 qualifies it natively against its durable command. Lifecycle authority, the reload rule and the owned-copy rule stand as D-0005 states them.
- **The qualified builds are exactly 2.1.291 and 2.1.295** (`QUALIFIED_OBSERVER_VERSIONS`). Every other build, including 2.1.292–2.1.294, runs lower-tier until requalified.
- The additive `workflow` spawn field and `ToolRequestMeta` stay unread. Using either needs its own qualification.

## Consequences

- `docs/compatibility/claude-observer-2.1.295.json` records the profile, its declaration identities and the M2 evidence hashes; SPEC §5.2, §7.3 and §11.4 name both builds.
- A build that changes any of the 92 compared declarations, adds an original-submission witness or restores plugin lifecycle raises needs a new requalification. So does a newer launcher target.
