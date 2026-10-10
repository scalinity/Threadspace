# F1B — independent observer ownership proof and explicit state upgrade

**Portable campaign: PASS. Native qualification: INCOMPLETE. D-0010 remains PROPOSED.**

This directory records implementation and portable qualification of the F1B remediation against rejected candidate `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`. It does not grant M2 acceptance. No macOS application, Claude session, Terminal tab or owner store was exercised here.

## Repaired authority rule

Historical attachment of Session A to ProcessKey P no longer restores an observer that currently runs in Session B at the same P. The helper's new `observer-proof` mode independently checks its actual provider parent's ProcessKey incarnation and executable identity against two fresh native inventory reads bracketing kernel samples. It generates a fresh token only after that join succeeds and separately commits or durably spools the native proof before returning the token. A middleware-fabricated subprocess result cannot create that proof.

The observer can seal the returned token only while its immutable source epoch, Session and ownership generation are unchanged. A genuine qualified core Turn start must capture the sealed scope. A later outcome must retain that original scope. The reducer joins the separate proof to the exact Session, process incarnation, executable, epoch, generation and token, and requires **seal before original Turn start before outcome**. The proof may arrive before or after the observer records. Pre-seal outcomes stay pending; a new A interval cannot authorize an unproven old A interval. A genuinely proven original A Turn can still finish after A → B → A using its own retained scope.

Product paths are `crates/provider-claude/src/ownership.rs`, `ownership_record.rs`, `observer.rs`, `crates/relay/src/bin/threadspace-hook.rs`, `crates/state-engine/src/reduce.rs`, and `crates/journal/src/canonical.rs`. The F1A ownership ledger and actual TypeScript witnesses live in the neighboring `f1a/` directory.

## Explicit version transition

The persisted representation and authority rule changed, so this proposal uses **reducer 4, canonical fact payload version 2 and journal admission payload version 2**. The existing durable `observations.payload_version` column records the admission version for every canonical observation, including a zero-fact metadata observation. `JournalEntry.payloadVersion` exposes it; earlier serialized headers default to 1, and unsupported stored header versions refuse recovery. No SQL schema change is needed. This does not reinterpret an existing reducer-3 development checkpoint as if it had always used the repaired rule.

The old checkpoint and suffix first recover the historical decisions. The upgrade reads retained canonical outcome facts, withdraws unsupported host-read outcome authority, preserves direct native facts and all committed owner-command facts and receipts, and re-derives using catch-up delivery. Previously presented output attention remains auditable with an explicit `EVIDENCE_UNAVAILABLE` cause; any owner cause remains alongside it. Unsent intent for unsupported output is suppressed. Unaffected native dispositions remain unchanged. Missing retained outcome evidence refuses the upgrade.

Versioned genesis replay performs the same transition for every version-1 prefix, including native-only history. The first version-2 **observation header**, even when it carries no facts, permanently selects the repaired authority rule. Later delivery of a version-1 entry cannot turn legacy authority back on. Recovery from an older valid checkpoint applies that transition before the same current suffix. Process and execution revision assignment at that transition agrees with the existing materialization rules. Committed attention notification state is preserved with its matching committed outbox disposition before re-derivation. The full state hash, projection, tables and checkpoint hash must agree; the oracle was not weakened to hide the revision or notification-state mismatches found during review.

## Executed portable checks

| Check | Executed result | Primary record |
|---|---:|---|
| Contracts, state engine and journal Rust tests | 306 passed, 13 nonempty suites | `portable-core.log` and `.json` |
| F1B tests within that run | 13 focused tests; 23 additional tests from the actual included provider/relay modules | `crates/journal/tests/observer_ownership.rs`; `portable-core.log` |
| Existing reducer-1/2 upgrade, checkpoint-tail, scenario and SQLite regressions | 21 passed, 5 suites | `retained-migration-and-scenarios.log` and `.json` |
| Original permutation generator | 24,400 permutations, zero failures | `permutations/summary.json` |
| Evidence-set family within that campaign | 4,000 permutations, zero failures | Same summary, `evidence-sets` family |
| Stepwise SQLite cross-check campaigns | 2,440, zero failures | Same summary, `sqliteCrossChecks` |
| Fresh proof/seal/start/outcome delivery orders | All 24 orders, with stable-UUID retries | Focused test and `records/` |
| Actual F1A TypeScript captures through production mod-batch normalizer, observer adapter and SQLite | 14 witness cases; unknown ownership retained without invented Session; forged receipt token alone remains pending | Focused test and `records/` |
| Final F1A capture regeneration after the F4 watchdog guard | The one consuming adapter/SQLite/restart test rerun: passed, all 14 cases; 15 retained execution states | `final-f1a-adapter.log`, `.json`, `final-f1a-records/` |
| Genuine reducer-3 checkpoint placements | Both fixtures at all three positions; exact state equals genesis and checkpoint; zero-fact tails, restart, first current fact and mixed-suffix fallback checked | `records/*migration-checkpoint-*.json`, metadata and mixed-suffix records |

The core count includes generated contract export tests and the qualification timing tests, with the complete included relay latency source chain fingerprinted. It is not that many distinct ownership scenarios. F4 also records its separate timing negative controls and all-imports Clippy qualification. No native producer test execution is included in these portable counts.

The final four-phase campaign retained 92 execution-state records: 56 admission outcomes, six original-checkpoint upgrades, 12 zero-fact metadata tails, 12 recoveries from old checkpoints after those tails, and six mixed suffixes containing a later notification disposition and owner command. Every migration state equals its complete genesis result; the initial six also retain equal checkpoint/state and projection/materialized-table digests.

After that campaign completed, F4 corrected its qualification-only missing-watchdog-handle behavior and regenerated the F1A fixture. The original Rust sources, including the complete relay latency include chain, did not change. `final-f1a-source-delta.json` records that the fixture was the sole changed file among 130 core-scope fingerprints. Only the test consuming that fixture was rerun against final SHA-256 `ea64a907ea469615fb0f356a8d63e88c6b4d0eef1f0f878550b906b61b06ff21`. Its before/after sources matched. The original 306-test run and hashes remain unchanged, and the 110-source synthetic scope still matches its 24,400-permutation run. The supplemental test is a repeated qualification of the same 14 cases, not an additional ownership scenario count.

The focused ownership tests also cover stale A/P with current B/P in both arrival orders; wrong process birth, executable, epoch, generation and token; A → B → A; pre-seal and old-generation claims; mixed direct-native and host-read outcomes; checkpoint/restart; semantic-oracle deletion controls; and refusal when an old checkpoint's pending outcome lacks its retained canonical fact. Both genuine old stores additionally receive two actual metadata-only adapter records, followed by a native fact, a production-admitted notification disposition and an actual owner Resolve command. Restoring each old valid checkpoint must recover that mixed suffix with exact timestamps, revisions, attention, outbox and original owner retry receipts. These are controlled fixture notification records; no OS notification is sent. The actual TypeScript proof-response fixture is only a correlation record until an independently admitted proof arrives.

## Genuine old stores and discriminating failures

The two fixture databases were generated by the actual rejected source, not by relabeling a fresh reducer-4 store. `old-source-provenance.json` verifies 229 Rust/manifests/lockfile paths against that commit's Git objects. The only additions to the rejected checkout were the preserved fixture generators.

| Fixture | Old contents | Old checkpoint cursors | Main database SHA-256 |
|---|---|---|---|
| `reducer-3-store/journal.sqlite3` | 11 observations, 11 facts; historical A/P, current B/P; falsely handled and unhandled outcomes; durable Mark handled; genuine native control | 0, 2, 11 | `79e8f48dc358dec013ec1dde3ed18f4dd639f8f5d0ba60e614eef16a021cf559` |
| `reducer-3-native-only-store/journal.sqlite3` | 7 observations, 8 facts; native-only handled and unhandled history; no host-read lifecycle fact | 0, 2, 7 | `35fadca05ea258d0a50e7bb44078e0609d2c8550a8c9b4bcce57bfa181310a24` |

Both database main files are self-contained. After fixture handles closed, immutable SQLite integrity checks returned `ok`, WAL sizes were verified as zero, and only empty WAL and ephemeral SHM sidecars were removed. The database hashes stayed unchanged. No required WAL was discarded.

The stale-history fixture upgrades to state/checkpoint hash `5d1457917ddba0e7951b3e514b69eebf72250a7dd783ad0729cd17552978d37d` at every old checkpoint. The native-only fixture upgrades to `ef691f15947db3764930e683a25c795617209115986458fafb59198874fb0968` at every old checkpoint. Each retained migration record contains the complete upgraded state, complete genesis result and production journal digest, including projection and materialized-table hashes.

The failures are preserved rather than overwritten:

1. `old-code-negative-control.log` runs the repaired invariant against the actual rejected implementation and fails because historical same-ProcessKey evidence falsely grants `RESTORED` and completion.
2. `migration-negative-control.log` deliberately relabels a copy of a genuine reducer-3 checkpoint as reducer 4 without the explicit retained-fact upgrade. It fails at the intended `RESTORED` versus `LOWER_TIER` assertion. This is an expected failure, not a passing ordinary test run.
3. `independent-review/migration-mismatch.log` preserves an independently found Process/Execution revision mismatch in an early remediation. Its exact whole-worktree source snapshot was not captured, which the independent report explicitly states. The repaired independent run captured exact before/after source hashes.
4. `runs/native-only-prefix-before-fix/` preserves the additional native-only fixture failure with source hashes. It exposed a transition trigger that handled only host-read prefixes. The repaired trigger handles every old journal prefix; the final six migration records and full campaign use that correction.
5. `runs/before-native-only-prefix-fix/` preserves an earlier passing campaign that lacked the native-only branch. Its migration completeness is superseded by this directory's final campaign.
6. `runs/metadata-boundary-before-fix/` preserves the actual zero-fact-tail counterexample and full states. A fact-only marker relocated a transition already committed at cursor 11 when metadata advanced the endpoint to 12. The initial fixture attempt reused an old observation UUID and was safely refused; its diagnostic is retained separately and is not counted as the boundary witness.
7. `runs/mixed-checkpoint-boundary-before-fix/` preserves the older-checkpoint recovery failure after only genesis had been corrected. `runs/mixed-checkpoint-rebase-attempt/` isolates the remaining `NOT_REQUESTED` versus `PENDING` attention display mismatch from rebasing final outbox dispositions without their corresponding committed attention column.
8. `runs/before-journal-header-boundary-fix/` preserves the earlier 288-test passing campaign that lacked metadata-tail and mixed-suffix coverage. Its original source hashes and results are unchanged; it is not the final migration qualification.

## Reproducibility and attribution

Run `python3 tests/native/tools/f1b-portable.py --out <new-disposable-output>` with the pinned Rust 1.99.0 toolchain and dependencies available. Use a new output directory to retain prior attempts. The runner captures commands, output hashes and before/after hashes of its explicitly enumerated source files, vendored SQLite sources and old-store fixtures. `--phase synthetic` and `--phase core` may run separately into the same output directory. A partial campaign never reports full PASS: all four phase records must exist, pass, and match the current sources in their recorded scope. The synthetic phase does not consume the F1A captures or the relay latency test modules; the core phase fingerprints their complete include chain. The native helper producer is source-reviewed and separately cross-compiled; it is not executed by this Linux runner.

The synthetic manifest pulls unused native dependencies into a normal Linux package build. The acquired temporary wrapper uses the exact original synthetic library, regression tests, permutation generator and oracle files. It changes no test assertions. Its 73 selected package version/source/checksum identities match the repository lockfile. The wrapper manifest, lockfile, entry point and verification result are retained. The one intentional scenario expectation change requires the former historical-attachment `host-read-outcome` case to stay pending and lower-tier; the new positive ownership-proof path has its own production-adapter tests.

An optional Clippy invocation overlapped an unfinished F4 source split. Its missing-file diagnostic is attributed under `runs/interim-f4-edit/` and is not a source-qualified result. The subsequent final journal/Clippy check must include all F4 `#[path]` source files, and is recorded by the root/F4 campaign. This does not replace any F1B assertion or reclassify a failed authority test.

`summary.json` provides the cumulative source qualification, counts and migration digests. `tested-source-hashes.json` preserves each execution scope and the final fixture delta; it does not retroactively alter a historical run. `artifact-hashes.json` inventories the retained F1B evidence. `portable-summary.json` is the runner's unedited four-phase result, captured before the final F1A regeneration; `final-f1a-adapter.json` qualifies the sole later-changed input. `D-0010` remains proposed, and the former native 819 ms `RESTORED` result cannot qualify this new producer. A source-matched macOS unchanged-ID reload, independently journaled proof and newly started post-seal Turn are still mandatory before acceptance.
