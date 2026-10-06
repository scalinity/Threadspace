# D-0005 — Claude Code 2.1.291 observer-mod semantics

**Status:** Accepted for M0C qualification; open for owner/reviewer confirmation.
**Affects:** SPEC §5.2 (mod dispatch provenance), §7.3 (human-follow-up auto-resolution), §11.4 (observer mod), §21.3 (fixture families); MILESTONES M0C "Provider semantics" and M2.

## Evidence

`evidence/M0C/provider/claude/` (README, `test-output.txt`, `validate-output.*`, `mutation-check.txt`, `native/`), produced by `claude plugin test` and `claude plugin validate` against the installed Claude Code **2.1.291** and three disposable `claude -p --model haiku` sessions. Generated declarations: sha256 `03815831f5ceaffc9557ebcd75fe8c3bf36369a404a3153865a3e920cb818d61` (20,755 lines, first line `// Written by Claude Code 2.1.291.`), identified by hash and excerpted, not committed. Capability profile: `docs/compatibility/claude-observer-2.1.291.json`.

1. **Plugins cannot raise lifecycle events through `$`.** In 2.1.291 the plugin-facing interface exposes `turn: { abort }` and a `session` noun without `start`/`end`; calling a lifecycle raise from a plugin fails at runtime. `EventCalls` belongs to the engine and the test kit only. Plugin-caused lifecycle remains possible indirectly (`$.turn.abort`, `$.agent.spawn` under the caller's origin).
2. **After a hot reload the session identity is lower-tier until the next engine-stamped session event.** `classic.SessionStart` does not re-fire on reload, so a reloaded module learns the session only through the interceptable `$.session.id()`.
3. **Loading a mod folder writes into it.** Each load writes `.claude-plugin/types/` and a root `tsconfig.json` into a `--plugin-dir` folder.
4. **No positive original-order witness for prompt submission.** `PromptSubmitInput` carries `text`, `attachments`, `context`, `turnId` (the turn running at submission, absent when idle), `wait` and a host-stamped `origin`; no submission sequence, timestamp or predecessor relation reaches the API.
5. **Accepted-input provenance is present.** Host-stamped `next.origin` (`{engine, core}` for engine dispatch), the original `PromptSubmitInput.origin` that no hook may set, and `next.trace` ending with a settled core entry.

## Decision

- `acceptedInputProvenance = true` for 2.1.291; `automaticHumanFollowupResolution = false` (NOT_SUPPORTED). Explicit "Mark handled" remains the resolution path; native outcomes, identity and actors that qualify are unaffected.
- Lifecycle authority still requires host-stamped engine origin plus core settlement or independent native corroboration (SPEC §5.2 unchanged in effect). The "plugin-raised `turn.complete`" fixture becomes a defence-in-depth case rather than a reachable 2.1.291 path; it stays in the synthetic suite because later builds may reopen it.
- After a reload the observer marks session-scoped context as lower-tier until an engine-stamped session event arrives, and never attaches a delayed result to a reload-learned session.
- The pinned mod is installed from a Threadspace-owned copy, never from the repository folder, because loading writes into the folder.

## Consequences

- SPEC §11.4's `EventCalls` paragraph and §7.3's witness discussion need the wording above once accepted; M2 installs the mod as described.
- A later Claude build that adds an original-submission witness, or restores plugin lifecycle raises, requires a new profile and requalification.
