# M1 remediation of the third independent review

The third independent review of candidate `85d188e221c24a4e91a533d0ab743fa0e1184c80` accepted:
- every remediation group of the first two reviews;
- current-reducer wait owner coverage and semantic equality;
- the WAL-without-`-shm` refusal;
- D-0008/C-04, C-12 and C-13.

It found one remaining blocker: a reducer-1 → reducer-2 upgrade could create PENDING notification intents when reducer 1's newest checkpoint was followed by journal entries.

| | Commit |
| --- | --- |
| Base | `85d188e221c24a4e91a533d0ab743fa0e1184c80` |
| Repair | `6ed18f0` (`crates/journal/src/canonical.rs`, `crates/journal/src/materialize.rs`, `crates/state-engine/src/engine.rs`) |
| Tests and reducer-1 stores | `2e42561` (`tests/synthetic/tests/checkpoint_tail_upgrade.rs`, `fixtures/m1/reducer-1-store/`) |
| Evidence in this directory | run from a clean `2e42561` source tree |

The review's text was not available in this environment. The defect, the reproducer and the expected outcome are taken from its Section 4 as quoted in the remediation request. The reproducer was then executed here, and it reproduces on `85d188e`.

## The defect

`load_engine` restored the newest checkpoint and replayed every later journal entry with the delivery recorded when it was admitted. Only after that did it call `Engine::upgrade`, which re-derives every record with catch-up delivery.

Most entries in a reducer-1 journal were admitted live. Replaying one under reducer 2 is a live reduction, so attention that reducer 2 first derives during the replay creates a PENDING intent, or re-arms a suppressed one as PENDING. The final catch-up re-derivation does not touch an intent that is already PENDING, so the upgrade turned historical attention into fresh live banner work. Whether that happened depended only on where reducer 1's newest checkpoint sat:

| Store (same committed history) | Upgrade on `85d188e` |
| --- | --- |
| Reducer-1 checkpoint at cursor 30, no suffix | 1 PENDING / 4 HELD / 1 SUPPRESSED |
| Reducer-1 checkpoint at cursor 0, 30-entry suffix | 5 PENDING / 0 HELD / 1 SUPPRESSED |
| Reducer-1 checkpoint at cursor 6, 24-entry suffix | 4 PENDING / 1 HELD / 1 SUPPRESSED |

The semantic hash is `72b804a6…` in all three. Semantic equality compares the outbox by which intents are eligible (D-0007 §3), and PENDING and HELD are both eligible. That is why `reducer_upgrade`, which compared semantic hashes and counted HELD rows only on the cursor-30 store, could not see the defect. The new test asserts every request's state by its ID.

## Repair (`6ed18f0`)

The repair works at the replay and load boundary. A checkpoint from the current reducer loads exactly as before: its suffix replays with the recorded delivery, so exact replay is unchanged. For a checkpoint from an earlier reducer, `load_engine` now runs four steps in order.

1. **Recovery context.** Each suffix entry is reduced with `Engine::recover`:
   - a live entry is reduced as catch-up, so eligibility it yields is held, never fresh live work;
   - a catch-up or bootstrap entry is reduced as recorded.

   The stored entry, its payload and its recorded delivery are not changed; recovery reduces an in-memory copy with a different effective delivery.
2. **Decisions read as recorded.** Owner decisions recorded by that replay are read the way the earlier reducer recorded them, just as `upgrade_json` reads the decisions already in its checkpoint (`read_as_recorded`). Reducer 1 kept only a flag for positives without a causal point, and the flag reads as 1. See below.
3. **Rebase onto the committed rows** (`keep_committed`). An earlier reducer committed its materialized rows in the same transaction as each entry, so they hold its state through the journal's last entry:
   - **An intent with a `notification_outbox` row** takes that row's record: state, detail, times and revision. A committed PENDING intent stays PENDING, and a submitted, suppressed or failed intent keeps its history. Delivery outcomes are journaled facts, so recovery rebuilds them as well.
   - **An intent with no row** was first derived by the current reducer. It is dropped, and step 4 creates it as the upgrade's own: HELD, at the upgrade cursor.
   - **An attention item** keeps its committed revision unless its materialized row now differs from the committed one. In that case the upgrade changed it, and it takes the upgrade cursor as its revision, as it would when upgrading from a checkpoint taken at the journal's end.
4. **Re-derivation.** `Engine::upgrade` re-derives every record with catch-up delivery, as before:
   - a suppressed-before-submission intent whose item is eligible again re-arms HELD;
   - an eligible item with no intent gets a HELD one;
   - an ineligible item's PENDING or HELD intent is suppressed.

The result is materialized and checkpointed at reducer 2 in one transaction (`persist_upgrade`, unchanged).

**Why placement no longer matters.** After step 3, the state equals what upgrading from a reducer-1 checkpoint at the journal's end would start from, in every field step 4 reads or writes. The upgrade's outcome is therefore the same wherever the old checkpoint sits, however long the suffix, and whether an intent first appears during the replay or the re-derivation. It is also independent of when the upgrade runs, because the reducer reads no clock.

## A second placement dependence: owner decisions

Reducer 1 recorded `unordered: bool` on a wait owner decision, meaning only whether its episode held positives without a causal point. Reducer 2 records how many. A reducer-1 checkpoint's decisions are converted with `true` read as 1, the fewest the decision can have covered. A decision replayed from the suffix, however, was recorded by reducer 2 with its real count. A Resolve over P3, U1 and U2 therefore upgraded in two ways:
- **Inside the checkpoint:** read as covering one of the two unordered positives. The item stays actionable and its intent is HELD.
- **Replayed after the checkpoint:** read as covering both. The item is resolved and its intent stays SUPPRESSED.

That is again an outcome decided by checkpoint placement. Step 2 reads the replayed decision as reducer 1 recorded it, so both placements give the checkpoint's reading. The reading is the conservative one, and Group 2's rule applies: evidence a decision may not have covered keeps the item actionable. The cost is that an item whose owner covered two or more such positives in one reducer-1 decision is shown again after the upgrade.

Two things are unchanged:
- the Group 2 owner-coverage logic itself (`wait.rs`, `reduce.rs`);
- every decision record apart from that count.

No decision is created, and no decision's coverage is widened.

## The reducer-1 stores

Each store was written by candidate `f7e9a6c` with [`emit_reducer1_store.rs`](../../../fixtures/m1/reducer-1-store/README.md). With neither new variable set, the emitter reproduces the accepted fixture byte for byte (`26ad5fdf…`).

| Store | SHA-256 | Checkpoints (reducer 1) |
| --- | --- | --- |
| `journal.sqlite3` (accepted fixture, unchanged) | `26ad5fdfcd5c9b33d88b09e7eaeca066e17e68c6c73a14db0e974b51a579454f` | EMPTY @ 0, FIXTURE @ 30 |
| `journal-checkpoint-6.sqlite3` | `3bea2963fc57acb0e746a2b4f5f43873bf88b3c13f0a29d90aa8a918d83e7d9e` | EMPTY @ 0, FIXTURE @ 6, FIXTURE @ 30 |
| `journal-unordered-pair.sqlite3` | `bc6c53289695fa8f2906380f379ff9752bfd83d13c5407589abc595993c1b0ff` | EMPTY @ 0, FIXTURE @ 6 |

The test makes disposable variants with `VACUUM INTO`. The committed files are opened with `immutable=1` and never written.

| Variant | Made from | Newest checkpoint | Suffix replayed |
| --- | --- | --- | ---: |
| A | `journal.sqlite3` as committed | reducer 1 @ 30 | 0 |
| B | `journal.sqlite3` without its cursor-30 checkpoint | reducer 1 @ 0 | 30 |
| C | `journal-checkpoint-6.sqlite3` without its cursor-30 checkpoint | reducer 1 @ 6 | 24 |
| X | `journal-unordered-pair.sqlite3` as committed | reducer 1 @ 6 | 0 |
| Y | `journal-unordered-pair.sqlite3` without its cursor-6 checkpoint | reducer 1 @ 0 | 6 |

**Same history, different checkpoints.** The test hashes every row of every table except `projection_checkpoints` and its `sqlite_sequence` counter. A, B and C all hash to `400aab1b1d218a04d5fa03886afcad7df38b3b4fc9106ca9401e7e3dbbaa102b`. X and Y match each other. In each case the only difference is checkpoint placement.

## Results

From [`checkpoint-tail.json`](checkpoint-tail.json) and [`unordered-pair.json`](unordered-pair.json):

| Variant | PENDING | HELD | SUPPRESSED | State SHA-256 | Tables SHA-256 | Semantic SHA-256 | Differences from A |
| --- | ---: | ---: | ---: | --- | --- | --- | ---: |
| A | 1 | 4 | 1 | `4b5745d9…` | `3402b130…` | `72b804a6…` | — |
| B | 1 | 4 | 1 | `4b5745d9…` | `3402b130…` | `72b804a6…` | 0 |
| C | 1 | 4 | 1 | `4b5745d9…` | `3402b130…` | `72b804a6…` | 0 |

"Differences from A" counts the JSON paths at which the full canonical state differs, including revisions, cursors and times. Variant A's upgraded state is the same as on `85d188e` (`4b5745d9…`), so the outcome the review accepted for the cursor-30 store is preserved.

The unordered pair also agrees: X and Y both read the decision as covering 1 positive, both leave 1 HELD intent, and both reach state `38dc49df…` and tables `c590ac80…`, with 0 differences.

For each of A, B and C:
- the attention items number 6 before and after the upgrade;
- the owner commands number 5 before and after;
- the materialized tables equal the state (`projection_differences` is empty);
- checkpoint plus replay reproduce the state hash.

## Request-level disposition

Identical in A, B and C:

| Request ID | Attention ID | Wait scope | Reducer 1 committed | After upgrade |
| --- | --- | --- | --- | --- |
| `62917481-141c-8b01-9f08-8b9039b114d2` | `9c14d766-ad73-835e-ae27-ff21b71c0222` | `sess-partial-actions` t2, episode 0 (P12 snoozed) | PENDING | **PENDING**: the original live intent |
| `4d9804b6-303f-81f5-b80d-c997326b6dc8` | `663fb032-fef5-812b-9104-b15bd1135349` | `sess-partial` t1, episode 0 (Q1 uncovered) | SUPPRESSED | **HELD**: re-armed |
| `60b47679-b35e-8713-8a93-887e332d3f23` | `38cc3ee8-5b0f-8db0-b9f9-5009640eba9c` | `sess-partial-actions` t1, episode 0 (Q1 uncovered) | SUPPRESSED | **HELD**: re-armed |
| `c225bdd2-c3d5-8948-8fb9-777919d8656d` | `7b5efbc8-620e-8af7-b38a-5933656372eb` | `sess-unordered-wait` t1, episode 0 (U2 uncovered) | SUPPRESSED | **HELD**: re-armed |
| `e976fef2-0ab9-86de-86e5-5eb61c2cadc1` | `3e47b2b0-a74f-827a-97d8-ce9cae31767e` | `sess-merge` t1, episode 1 (P10 and the uncovered Q2) | none | **HELD**: created by the upgrade |
| `d2f458a7-c106-8216-938e-665fa5b6d380` | `d3561f2e-346b-8bf7-a15f-48d9d2563591` | `sess-merge` t1, superseded episode 0 (C5 moved P10 into episode 1) | SUPPRESSED | **SUPPRESSED**: "ineligible before submission" |

The test asserts every row:
- the request ID;
- its attention ID and wait scope;
- reducer 1's committed state;
- the upgraded state, both in `notification_outbox` and in the canonical state.

There are exactly six requests and no duplicates.

## Negative controls

Each control is a patch against `2e42561`. The test was run with the patch applied, and the source was then restored. Patches, results and the test's JSON output are in [`negative-controls/`](negative-controls/).

| Control | Patch | A | B | C | Unordered pair | Test result |
| --- | --- | --- | --- | --- | --- | --- |
| The previous candidate's load path | [`before-fix.patch`](negative-controls/before-fix.patch) (the three files as at `85d188e`) | 1 / 4 / 1 | **5 / 0 / 1** | **4 / 1 / 1** | X HELD, **Y SUPPRESSED** (decision count 2) | both tests fail; first at `4d9804b6` in B: PENDING, expected HELD |
| Catch-up replay only (the naive fix) | [`catch-up-replay-only.patch`](negative-controls/catch-up-replay-only.patch) | 1 / 4 / 1 | **0 / 5 / 1** | **0 / 5 / 1** | X HELD, **Y SUPPRESSED** | both fail; first at `62917481` in B: HELD, expected PENDING (the committed live intent is downgraded) |
| Committed dispositions without the rebase | [`dispositions-without-rebase.patch`](negative-controls/dispositions-without-rebase.patch) | 1 / 4 / 1 | 1 / 4 / 1 | 1 / 4 / 1 | agree | the placement test fails: B differs from A at 8 paths and C at 7 (four attention revisions; the new intent's creation cursor, times and revision) |
| Replayed decisions not read as recorded | [`decisions-not-read-as-recorded.patch`](negative-controls/decisions-not-read-as-recorded.patch) | 1 / 4 / 1 | 1 / 4 / 1 | 1 / 4 / 1 | X HELD, **Y SUPPRESSED** (count 2) | the unordered-pair test fails |

Counts are PENDING / HELD / SUPPRESSED. The third control has the right counts in every variant and still fails: that is why the test compares whole states, and why the request-level assertions name each request.

## Reducer version, atomicity and restart

**Reducer version.** A reducer-1 checkpoint becomes reducer 2, `REDUCER_VERSION` is unchanged at 2, and `upgrade_json` still converts reducer 1's checkpoint representation.

**Atomicity.** `persist_upgrade` writes every materialized row and the REDUCER_UPGRADE checkpoint in one immediate transaction. If the upgrade fails before commit, the store keeps reducer 1's checkpoint and rows, and the next open upgrades from them again. Recovery and rebasing happen in memory inside `Journal::open`, before that transaction, on the one writer connection. There is no second writer.

**Upgrade checkpoint.** Each variant ends with exactly one `REDUCER_UPGRADE` checkpoint: reducer 2, through cursor 30, the newest checkpoint, with state SHA-256 `4b5745d927bae69e46dfa414af0d4a04ffda9875b1c5f855210b5b178431654a`. C's cursor-0 checkpoint was pruned by the normal keep-three rule.

**Restart.** A restart reads the upgrade checkpoint, reproduces the state hash, and writes no further checkpoint.

## Live work after the upgrade

After the restart, each variant admits a new session's live wait through `admit_batch` with live delivery.
- **The new intent:** exactly one is created (`78ff7f31-3ee7-88e5-9b7e-d519939fe1fc`), it is PENDING, and it is the only intent handed out as a notification.
- **The upgrade's intents:** the four HELD intents and the original PENDING one are unchanged.
- **Opening the store:** hands out no notifications, and the upgrade's HELD intents are never submitted as live work. Their OS delivery remains M5's.

A second restart replays those live entries after the upgrade checkpoint with their recorded delivery, under the same reducer. It reproduces state `05f884d1…`, identically in A, B and C.

## Requalification

Commands and results are in [`test-runs.txt`](test-runs.txt) and [`focused-tests.txt`](focused-tests.txt). All of them ran on `2e42561`.

**Reran:**

| Check | Result |
| --- | --- |
| `cargo test --workspace --features threadspace-journal/qualification,threadspace-agent/qualification` | 53 suites, 436 passed, 0 failed |
| Workspace clippy | Clean |
| `THREADSPACE_CHECK_SCHEMAS=1` schema check | 3 passed |
| `npx vitest run` | 43 passed |

The workspace suites include:
- `checkpoint_tail_upgrade` (2);
- `reducer_upgrade` (1);
- `wait_owner_coverage` (8: every Group 2 witness through checkpoint, restart and genesis replay, each in 8 more valid orders);
- journal `future_store` (18) and `journal`;
- the state-engine tests;
- the synthetic catalog, SQLite and permutation tests.

**`threadspace-m1` rerun on this source:**
- `fixtures`, `contracts`, `replay` and `crash` reproduced their committed files byte for byte. Replay: 40 scenarios. Crash: 100 injections, 0 acknowledged records lost, 0 duplicate facts.
- `migration` reproduced every field except the per-store file SHA-256 values. Those differ from run to run, because each run creates fresh stores with new identifiers. Within each run, every refusal store's before and after hashes are equal.
- All 8 refusals still refuse, with every file unchanged. The regenerated `migration/summary.json` is committed. The previous run, which the second remediation's record cites, is kept with the previous manifest in [`../history/85d188e/`](../history/85d188e/README.md).

**Not rerun:**
- **Permutations.** The 20,400 permutations and 2,040 SQLite checks are retained from `05a9a2e`. The reducer (`reduce.rs`, `wait.rs`) and admission are unchanged. Every permutation store is written by reducer 2, so its load takes the unchanged same-reducer branch.
- **Native qualification.** C-04's 60 recoveries, G12, VoiceOver, sustained graphics, M0B routing and the native Terminal restart are retained, as the remediation request directs.
- **Capture and sanitization.** Retained from `e52c281`, as before. Those workloads never open a reducer-1 store.

## The owner's machine

The installed application is still the reducer-1 build recorded as `5944c81`. A read-only check of the bundle found it unmodified since 2026-10-07 and found no `REDUCER_UPGRADE` code in its executable. The live store, schema 3, was not opened, read or copied. Every test used disposable copies of the committed fixtures in temporary directories. No reducer-2 build was installed or launched.

Installing reducer 2 belongs to the accepted close-out, which needs two things: a verified pre-upgrade backup and a controlled first launch.
