# M1 evidence — Journal, contracts and deterministic synthetic harness

M1 makes the canonical engine the store's only write path. Every observation, reconciler pass, owner command and record the companion writes goes through one single-writer admission transaction: validate and dedup the stable observation UUID, journal the observation with the adapter's allowlisted payload, resolve native keys to canonical IDs (recording new assignments), journal the resolved facts, reduce them with the pure engine, upsert every changed projection row, attention item, outbox intent and the applied cursor, then commit. Receipts and side effects exist only after commit. The design decisions a reviewer should see are in [D-0007](../../docs/decisions/D-0007-m1-canonical-engine.md); each invariant's rule and checks are in the [reducer invariant catalog](reducer-invariants.md). Every figure below is in [`manifest.json`](manifest.json), which is generated from the areas' own summaries (`threadspace-m1 manifest`).

## Where the code is

| Concern | Code |
| --- | --- |
| Canonical contracts (records, facts, envelope, commands, capture protocol) | `crates/contracts/src/canonical/`; generated TypeScript in `apps/desktop/src/contracts/generated/`, JSON Schemas in `crates/contracts/schemas/` |
| Pure reducer, identity resolution, causality, semantic projection, hashing | `crates/state-engine/` (`tests/purity.rs` checks the reducer reads no ambient state) |
| Admission transaction, migration 3, materialization, checkpoints, M0 baseline | `crates/journal/src/{canonical,materialize,baseline,schema}.rs` |
| Capture executable, spool, event-socket client | `crates/relay/src/{capture,spool,events}.rs`, `crates/relay/src/bin/threadspace-hook.rs` |
| Companion event socket, spool drainer, adapter registry | `apps/agent-macos/core/src/{events,spool_drain,adapters}.rs` |
| Claude conventional-hook normalization (classic-limited) | `crates/provider-claude/src/hooks.rs` |
| Synthetic provider, scenarios, runners, evidence runner | `crates/state-engine/src/synthetic.rs`, `tests/synthetic/` |

## Areas

| Area | What it shows | Summary |
| --- | --- | --- |
| Contracts | Version catalog, migration and generated-artifact digests, JSON Schema freshness, and validation of every fixture envelope, normalized draft, journal entry, canonical state and owner command against the generated schemas | [`contracts/summary.json`](contracts/summary.json) |
| Fixtures | The scenario catalog with per-fixture digests; fixtures exported to `fixtures/m1/*.jsonl` | [`fixtures/catalog.json`](fixtures/catalog.json) |
| Replay | Per scenario through the real SQLite journal: journal, checkpoint, state, projection, table and semantic digests; repeated replay, restart and replay from genesis identical; journal exports replayable with `threadspace-m1 verify` | [`replay/summary.json`](replay/summary.json) |
| Permutations | 20,000 seeded valid partial-order permutations over the eight required families (plus duplicates and robustness), duplicate redeliveries injected into half, every 10th also through SQLite with tables checked against state after every step; the invariant monitor checks every step, including that a wait item shows an owner action exactly when that action covers its active evidence | [`permutations/summary.json`](permutations/summary.json), [`permutations/failing-seeds.json`](permutations/failing-seeds.json) |
| Crash | 100 SIGKILL injections: five admission positions × twenty admissions; acknowledged records, store consistency, convergence after retry, duplicate facts, owner-command retry stability | [`crash/summary.json`](crash/summary.json) |
| Capture | The release `threadspace-hook` spawned per capture against the real writer and event socket (fixture companion): wall-time percentiles, every fail-open path, saturation measured in real on-disk bytes; cross-process quota races in relay `spool_capacity` | [`capture/summary.json`](capture/summary.json), [`capture/saturation.json`](capture/saturation.json) |
| Sanitization | Planted bodies, paths and messages in every Claude hook event, through the journal path and the spool path; stored-row snapshots; every store and spool file scanned | [`sanitization/summary.json`](sanitization/summary.json), [`sanitization/snapshots.json`](sanitization/snapshots.json) |
| Migration | The schema-2 store written by the accepted M0C journal (`fixtures/m1/m0-store-v2`) upgrades deterministically, preserves identities, bindings, commands and routes, keeps M0B routing, and a newer schema or reducer checkpoint is refused with every store file, sidecar and header byte unchanged and no file created (rollback, clean WAL, WAL with sidecars, WAL without `-shm`), each store recorded file by file; a store checkpointed by reducer 1 upgrades to the current reducer's state (`reducer_upgrade`, `fixtures/m1/reducer-1-store`), and to the same state and committed notification dispositions wherever reducer 1's newest checkpoint sits (`checkpoint_tail_upgrade`) | [`migration/summary.json`](migration/summary.json) |
| C-12, C-13 | Renderer asset outcome accounting; companion log framing under SIGKILL | [`c12/README.md`](c12/README.md), [`c13/README.md`](c13/README.md) |
| C-04, M0B | Native. C-04 on build `73636ec`: view recovery over 60 recoveries under D-0008 (office shells 1 → 1, all 63 retired native objects released by the hard oracle, no sustained footprint growth, bounds, intents and incarnation rejection preserved). M0B on build `5944c81`: G08 Terminal identity and Return with real Claude sessions (0 wrong targets; exact Return passes; 9 of 10 selection-race routes refused conservatively, an open observation owned by M5; the fullscreen case's transition witnesses are null, so M0C H-10 stays the fullscreen evidence) | [`native.json`](native.json), [`c04/NOTE.md`](c04/NOTE.md) |
| Remediation | The first independent review's eight groups (counterexamples, fixes, tests, negative controls; D-0008 guard controls; retired-native verdicts for every retained run) the second review's two (wait owner coverage and semantic equality; a WAL store without `-shm`), and the third review's one (a reducer-1 upgrade over a journal suffix), each candidate's evidence kept unchanged | [`remediation/README.md`](remediation/README.md), [`remediation-2/README.md`](remediation-2/README.md), [`remediation-3/README.md`](remediation-3/README.md), [`history/d93b0fb/`](history/d93b0fb/README.md), [`history/f7e9a6c/`](history/f7e9a6c/README.md), [`history/85d188e/`](history/85d188e/README.md) |

## Found and fixed during M1

- The first permutation run (at `ce1a94f`) failed 17 of 10,400: semantics converged, but on the SQLite path a binding admitted before its execution's attach kept a projection row without the process the attach later named. A binding's row is now rewritten when its execution or surface changes; SQLite cross-checks now compare tables with state after every step. The 17 seeds, reproduced deterministically, are preserved in `fixtures/m1/regression-seeds.json` with a test that fails without the fix.
- Upgrading the real M0 store exposed that the M0A fixture linked its process only through `execution_processes`; the baseline now reads that link.
- A read-only pre-review of the engine traced defects no test reached. Each is fixed with a test that fails without the fix:
  - a late, earlier clear left a wait item open forever (`delayed-clear-wait`);
  - a session's turn state followed arrival order, unseen because semantic equality left it out (`latest-turn-outcome`, which also exposed `outcomes`);
  - `turns` uniqueness ignored the actor, so a subagent reusing a native turn ID failed admission (`shared-native-turn-id`), and one refused record stalled the spool drain (companion `spool_drain`);
  - `mod-batch` never printed a receipt under the stdout buffer size (relay `mod_batch`);
  - migrated M0 intents were invisible to the reducer (`migrated_outbox`);
  - spool loss was deleted before it was recorded (companion `spool_loss`);
  - a live batch reported suppressed intents, a no-op command reported a revision the item never had, uppercase observation IDs missed their receipts, and an unordered image revived a lost binding (SQLite tests, `replaced-then-unordered-image`).

- The first independent review found eight groups of remaining defects (public session state, the wait model, fresh proofs after A → B → A, migrated M0 command retries, future-schema refusal writes, cross-process spool capacity, D-0008 safeguards and the M2 conversion contract). Their fixes, tests and negative controls are in [`remediation/README.md`](remediation/README.md).
- The second independent review found that a wait owner decision applied to evidence it never covered, that semantic equality compared decisions by episode alone, and that refusing a WAL store without `-shm` created one. Their fixes, tests and negative controls are in [`remediation-2/README.md`](remediation-2/README.md); the contracts, fixtures, replay, permutation, crash and migration evidence above was regenerated on that source, and capture and sanitization are retained from `e52c281` (see the manifest's `remediation.second.evidenceSources`).
- The third independent review found that a reducer-1 store whose newest checkpoint is followed by journal entries upgraded with fresh PENDING intents for attention reducer 2 first derived while replaying them, so the upgrade depended on where the checkpoint sat. The fix, tests and negative controls are in [`remediation-3/README.md`](remediation-3/README.md); the migration evidence above was rerun on that source, and the fixtures, contracts, replay and crash runs reproduced their committed files byte for byte (see the manifest's `remediation.third.evidenceSources`).

## Reproduce

From the repository root:

```sh
cargo test --workspace --features threadspace-journal/qualification,threadspace-agent/qualification
npx vitest run
cargo build --release -p threadspace-synthetic --bin threadspace-m1 -p threadspace-relay --bin threadspace-hook
M=target/aarch64-apple-darwin/release/threadspace-m1; H=target/aarch64-apple-darwin/release/threadspace-hook
$M fixtures && $M contracts && $M replay && $M permutations 20000 && $M crash && $M migration
$M capture $H 1000 && $M sanitization $H && $M manifest
tests/native/d0008-guard.sh evidence/M1/remediation/d0008-guard
target/aarch64-apple-darwin/debug/threadspace-m0c c04-verdict prod evidence/M1/view-recovery/20261008T015231Z-prod
```

Synthetic areas are deterministic in their seeds; the capture timings and native areas are measurements of this machine.
