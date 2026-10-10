# F1A — immutable observer work ownership

This is focused remediation evidence, produced on Linux from a worktree based on rejected candidate `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`. It does not mark M2 or D-0009 accepted. The source content hashes in `execution-context.json` and `observer-witnesses.json` bind the executed code; final candidate/source reconciliation belongs to the enclosing remediation manifest.

## Executed evidence

| Record | What actually ran | Result |
|---|---|---|
| `observer-witnesses.json`, `portable-test-output.txt` | `packages/provider-mod/tests/ownership.node.mjs` imports the actual `hooks/register.ts` and `hooks/ownership.ts` under Node 26.11.1. Host origins/traces, receipts and timers are in-memory fixtures. | 14 cases passed. Each applicable case retains the actual emitted observation records. |
| `negative-control-old-candidate.json`, `.txt` | The same F1A-1 assertion runs against exact `register.ts` and `delivery.ts` obtained with `git show` from the rejected candidate, in a disposable directory. | Expected exit 1: the delayed A completion is stamped as Session B. The assertion fails specifically on that wrong Session, before checking new fields. |
| `kit-original-expectations.txt` | Claude Code 2.1.295 Linux `plugin test` on the first repaired observer copy with the old provenance expectations. | 35 passed, 3 failed. The three mismatches are described below; this attempt is retained. |
| `kit-repaired.txt` | Claude Code 2.1.295 Linux `plugin test` on a fresh, disposable copy of the final observer sources and focused expectation changes. | 38 passed, 0 failed; process exit 0. |
| `kit-validate.txt` | Claude Code 2.1.295 Linux `plugin validate` on that same disposable copy. | Validation passed; process exit 0. |
| `execution-context.json` | Runtime versions, actual executable hash and hashes of the qualified copy's files. | Source/execution identity, not an acceptance label. |

The final records were regenerated after the F4 optional-census deadline guard was repaired. `final-portable-execution.json` records the final 14-case execution; `final-kit-execution.json` records the final isolated 38-test and validation commands, times, and source hashes before and after execution. All qualified-copy files matched the worktree and remained unchanged during the final kit run. Earlier passing source identities and outputs remain in `pre-final-f4/` and `pre-missing-timer-repair/`; they are historical attempts, not additional native qualification. The separate F4 independent audit retains the failed missing-timer witness and its passing repair controls.

All Claude commands used an explicit disposable `CLAUDE_CONFIG_DIR`. No owner Claude settings, production application, live store, Terminal window, or unrelated process was touched. The test-kit bottom handlers stand for the engine; positive engine/core provenance in the direct Node suite is an explicit fixture. Native macOS provider execution and the revised independent-inventory helper path were **NOT RUN** by this F1A Linux suite.

The F1B evidence runs these emitted records through the production mod-batch parser, observer adapter, canonical facts, SQLite admission, public state and checkpoint/restart checks. It is the separate evidence source for the full durable-state assertions. This directory alone does not claim SQLite or native qualification.

## Ownership behavior under test

The original owner is `{source epoch, Session, observer generation}` plus native Turn and actor lookup scope. Namespace and actual process/executable proof are attached independently by the native helper. A principal engine/core-settled Turn start retains its entry scope. An actual engine/core-settled spawn retains its original Session for the returned actor; child steps can then establish actor-scoped Turn ownership. Results always retain their immutable entry context.

Native Turn IDs are not globally unique. If two Sessions/generations claim the same Turn/actor lookup key, subsequent unresolved callbacks retain `AMBIGUOUS` ownership and no Session. Different actors may use the same native Turn ID independently. A first-seen late native event remains unowned; it also leaves an uncertainty marker rather than allowing a later start to silently overwrite its past. Native lookup identifiers are never truncated into an ownership key.

The maps retain at most 4,096 Turns, 4,096 actors and 8,192 occurrences per source epoch. They never evict an old key. New keys at saturation remain unowned; collisions with retained keys stay ambiguous. A new observer load starts a separate ledger, so a first-seen old callback cannot inherit a previous module's identity.

Root tool/spawn callbacks contain no native Turn field. In the uninterrupted initial engine-identified interval, their Session is known. After any Session transition, first-seen root occurrences require retained actor/occurrence proof and otherwise stay `UNKNOWN`. This is a conservative scope limit, not attribution by latest active Turn or current Session. Existing known child/occurrence ownership remains usable after a switch. `currentSessionId`, `currentSessionIdSource` and `currentSessionGeneration` are retained solely as diagnostic entry metadata. Unowned records use numeric `sessionGeneration: 0` for the existing transport schema and omit `ownershipGeneration` and `sessionId`.

## Focused cases

| Case | Discriminating assertion |
|---|---|
| F1A-1 | A starts a Turn; B becomes current; completion enters late. It remains A/generation 1 while current metadata says B/generation 3. Never a B outcome. |
| F1A-2 | Completion enters A, waits under `next(e)`, and returns after B starts. The exact result object and original A entry context remain unchanged. |
| F1A-3 | A→B→A, delayed principal and child callbacks, and the same child Turn ID under different actors. Original generations 1 and 3 stay distinct from current A generation 5. |
| F1A-4 | A and B use the same principal native Turn ID; A later returns. Later outcomes have no Session and remain ambiguous. |
| F1A-5 | An original tool call enters under A and its nested spawn returns after B is current and the parent completes. The actual returned child's subsequent callbacks still belong to A. |
| F1A-6 | An unknown late outcome, later reuse of that ID and a first-seen root spawn after a transition cannot invent a current owner. |
| F1A-7 | A real actor ID is reused in another generation. Its old retained Turn cannot override the resulting actor ambiguity. |
| F1A-8 | Plugin dispatch, a downstream shortcut and a fabricated classic event cannot establish native Turn/actor ownership or change the current native Session. |
| F1A-9 | Small test limits exercise the same bounded map implementation: no eviction, no loss of collisions, no cross-epoch lookup and no overlong-key truncation. |
| F1A-10 | Exact provider event/result identity, exceptions, streamed chunk identity/order, generator cancellation, capture getter failure and helper unavailability remain fail-open. |
| F1A-11 | Two actual overlong native Turn IDs sharing a 256-character prefix remain unowned throughout capture. |
| F1B-bridge-1 | A detached proof seals an unchanged host-read generation. A pre-seal Turn keeps no token; a post-seal Turn retains the token but still reports `HOST_READ`. The receipt alone grants no native authority. |
| F1B-bridge-2 | A→B→A while a proof response waits fails the exact generation check; no seal is emitted. |
| F1B-bridge-3 | Wrong epoch, generation or Session, NOT_ACCEPTED, nonzero exit, malformed JSON and helper failure all leave ownership unsealed. |

## Why the first host-kit attempt failed

The old delayed-result fixture supplied an arbitrary `agent-7` without any qualified actor-ownership evidence. The fixed observer correctly left it unowned. The preserved positive control now uses the known initial principal interval and still proves A ownership across a delayed return. Qualified original child ownership is exercised by F1A-3 and F1A-5.

The host-read-interception test and the logical-Session-change test expected a new root tool occurrence to inherit a newer current Session. Those expectations expressed the behavior being removed. The revised assertions still verify each native current-Session transition and its generation, while separately requiring unproved root work to stay `UNKNOWN`. The failed original attempt was not discarded or represented as a passing native run.

## Reproduction

From the repository root, with the pinned Node version available:

```sh
node packages/provider-mod/tests/ownership.node.mjs --output /absolute/disposable/observer-witnesses.json
```

For the negative control, place the exact rejected candidate's `packages/provider-mod/hooks/register.ts` and `delivery.ts` together in a disposable directory, then run:

```sh
node packages/provider-mod/tests/ownership.node.mjs --module /absolute/disposable/register.ts --case F1A-1 --output /absolute/disposable/negative-control.json
```

Exit 1 is required for that old-source negative control. Run `claude plugin test` and `claude plugin validate` only on a disposable mod copy with an explicit disposable Claude configuration. The host may generate files in the loaded mod directory.

## Remaining qualification

The revised observer and independent native proof helper require focused source-matched development-channel macOS evidence for the affected ownership/reload paths. A retained old `RESTORED` label cannot qualify the new proof predicate. The existing ten-cycle vertical and thirty ordinary Return evidence remain baseline evidence for unchanged paths; this directory does not rerun or replace them. D-0009 and D-0010 remain PROPOSED pending independent adjudication.
