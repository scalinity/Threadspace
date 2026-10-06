# D-0005 — Claude Code 2.1.291 observer-mod semantics

**Status:** ACCEPTED by independent review on 2026-10-06 of candidate `6f903c273eedc135123e3a7968d3c00887e8cc70`, with the evidence limits below. This accepts the qualified capability contract, not M0C as a whole.
**Affects:** SPEC §5.2 (mod dispatch provenance), §7.3 (human-follow-up auto-resolution), §11.4 (observer mod), §21.3 (fixture families); MILESTONES M0C "Provider semantics" and M2.

## Evidence

`evidence/M0C/provider/claude/` (README, `test-output.txt`, `validate-output.*`, `mutation-check.txt`, `native/`), produced by `claude plugin test` and `claude plugin validate` against the installed Claude Code **2.1.291** and three disposable `claude -p --model haiku` sessions. Generated declarations: sha256 `03815831f5ceaffc9557ebcd75fe8c3bf36369a404a3153865a3e920cb818d61` (20,755 lines, first line `// Written by Claude Code 2.1.291.`), identified by hash and excerpted, not committed. Capability profile: `docs/compatibility/claude-observer-2.1.291.json`.

1. **Plugins cannot raise lifecycle events through `$`.** In 2.1.291 the plugin-facing interface exposes `turn: { abort }` and a `session` noun without `start`/`end`; calling a lifecycle raise from a plugin fails at runtime. `EventCalls` belongs to the engine and the test kit only. Plugin-caused lifecycle remains possible indirectly (`$.turn.abort`, `$.agent.spawn` under the caller's origin).
2. **After a hot reload the session identity is lower-tier until qualified native proof restores authority.** `classic.SessionStart` does not re-fire on reload, so a reloaded module learns the session only through the interceptable `$.session.id()`. The M0C observer's unchanged-ID early return does not promote that source on a later same-ID event; such promotion is not claimed as qualified.
3. **Loading a mod folder writes into it.** Each load writes `.claude-plugin/types/` and a root `tsconfig.json` into a `--plugin-dir` folder.
4. **No positive original-order witness for prompt submission.** `PromptSubmitInput` carries `text`, `attachments`, `context`, `turnId` (the turn running at submission, absent when idle), `wait` and a host-stamped `origin`; no submission sequence, timestamp or predecessor relation reaches the API.
5. **Accepted-input provenance is present.** Host-stamped `next.origin` (`{engine, core}` for engine dispatch), the original `PromptSubmitInput.origin` that no hook may set, and `next.trace` ending with a settled core entry.

## Decision

- `acceptedInputProvenance = true` for 2.1.291; `automaticHumanFollowupResolution = false` (NOT_SUPPORTED). Explicit "Mark handled" remains the required resolution path: M0C qualifies this policy, M1 implements its durable command and M2 qualifies the native control. Native outcomes, identity and actors that qualify are unaffected.
- Lifecycle authority still requires host-stamped engine origin plus core settlement or independent native corroboration (SPEC §5.2 unchanged in effect). The "plugin-raised `turn.complete`" fixture becomes a defence-in-depth case rather than a reachable 2.1.291 path; it stays in the synthetic suite because later builds may reopen it.
- After reload, session-scoped context remains lower-tier until separate qualified event/inventory proof restores authority. M2 must qualify restoration for an unchanged native ID; no delayed result is reattributed from its immutable callback-entry ownership.
- The pinned mod is installed from a Threadspace-owned copy, never from the repository folder, because loading writes into the folder.

## Consequences

- SPEC §5.2/§7.3/§11.4/§21.3 and MILESTONES M0C/M1/M2 now carry this contract; M2 installs the mod as described.
- Native runs establish SDK/task-notification prompt origins and successful main/child outcomes. Composer/bridge stamping is installed-declaration and engine-kit evidence; interactive human input, additional outcome reasons and Stop continuation remain M2 qualification. The reload run discarded 13 buffered records, none acknowledged. It used the documented stand-in helper and is not a native Threadspace journal-durability test.
- A later Claude build that adds an original-submission witness, or restores plugin lifecycle raises, requires a new profile and requalification.
