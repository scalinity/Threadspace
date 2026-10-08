# M1 remediation of the second independent review

The second independent review of candidate `f7e9a6ce2ce034e04abff03bf8a358dc98a98e01` accepted these first-review groups: 1, public session state; 3, a fresh proof after A → B → A; 4, migrated M0 command retries; 6, cross-process spool capacity; 7, the D-0008 safeguards with the C-04 retirement oracle; and 8, the M2 conversion contract. It also accepted C-12 and C-13. It required remediation of two groups:

- **Group 2:** a wait owner decision applied to evidence it never covered, and semantic equality could not tell decisions on different evidence apart.
- **Group 5:** refusing a future store whose `-wal` had no `-shm` created a persistent `-shm`.

This record gives each group's counterexample, fix, tests, negative controls and evidence. The candidate's evidence before this remediation is kept unchanged in [`../history/f7e9a6c/`](../history/f7e9a6c/README.md).

**Witness provenance.** The review's text and its supplemental Rust witnesses were not available in this environment, and the reviewer reported those witnesses as uncompiled and unexecuted. The witnesses below are source-level cases built from the review's stated sequences. They were run in two places:
- **The fixed implementation:** every layer named below.
- **The old candidate `f7e9a6c`:** the defects reproduce there (see [`fixtures/m1/reducer-1-store`](../../../fixtures/m1/reducer-1-store/README.md)).

## Group 2 — a wait owner decision applies only to the evidence it covered

**Defect.** An owner's Acknowledge, Resolve or Snooze on a wait item is kept on the wait scope with the positives of the episode the owner acted on. Each episode's item took its owner state from every decision that *intersected* the episode's positives. An episode holds every positive placed in it, though: positives a decision covered, positives a clear has since ended, and positives that arrived later. So a decision on P governed an episode that also held a new Q the owner never handled, and Q was suppressed. Reducer 1 run on the witnesses shows this ([`reducer-1-derivation.json`](../../../fixtures/m1/reducer-1-store/reducer-1-derivation.json)):
- **P2, Resolve, C3, Q1 (witness A):** the item is owner-resolved and Q1's intent is suppressed.
- **P10, Resolve, C5, Q2, D1 (witness B):** the merged item is owner-resolved.
- **A later positive with no causal point:** absorbed by the decision.
- **Acknowledge and Snooze:** carried onto the incomparable Q.

**Fix** (`04a3f3c`, `crates/state-engine/src/wait.rs`). An episode now distinguishes two kinds of evidence:
- **Historical positives:** every witness placed in it.
- **Active evidence:** positives no comparable clear ended, whether open or uncertain, plus its positives without a causal point (never ended).

Owner state is decided per action, from that action's decisions:
- **While the episode holds active evidence:** the item takes the action only if the decisions together cover every active witness, and each decision covering an active witness then applies. Evidence the owner never handled keeps the item actionable: a new wait after a clear, a positive that joins the episode after the decision, or one a late clear merges into a decided episode. A repartition therefore never moves a decision onto evidence it did not cover. A late, earlier clear that moves P10 into the next episode carries P10's decision with it.
- **Once all of an episode's evidence has ended natively:** every decision made on its evidence stays on its item as history. This is the behaviour the first remediation accepted.
- **The decision record itself:** never changes when the partition changes, and no decision is manufactured.
- **Times:** an Acknowledge takes effect when its last active witness was first covered. A Snooze lasts until the earliest active witness's snooze ends.
- **Positives without a causal point:** these have no identity, and their count only grows, so a decision now records how many it covered (`unordered: u32`, previously a flag) and covers the first that many.

**Scope.** The turn is still part of the wait key; a turnless (inventory) wait stays session-scoped. `wait-owner-scopes` shows that a decision on t1's wait, a session-scoped Acknowledge and t2's untouched wait each stay on their own scope.

**Checkpoint versioning** (`eadc73c`). `REDUCER_VERSION` is now 2. A checkpoint from reducer 1 is upgraded when the store opens:
1. Its JSON is read through reducer 1's representation. A covered-unordered flag `true` becomes 1, the fewest it can have covered, so a later such positive is never taken as handled.
2. Later entries are replayed.
3. `Engine::upgrade` re-derives every record, wait scopes included, with catch-up delivery, so an item that is eligible again re-arms its suppressed intent as HELD.
4. Every materialized row and a `REDUCER_UPGRADE` checkpoint are written in one transaction.

A reducer-1 checkpoint does not deserialize as reducer 2's state, so it is never read silently. [`reducer_upgrade`](../../../tests/synthetic/tests/reducer_upgrade.rs) opens the real reducer-1 store and checks four things:
- the upgraded state's semantic hash equals the one reducer 2 reaches admitting the same histories;
- the four intents reducer 1 suppressed are HELD, and the tables equal the state;
- checkpoint plus replay reproduce the state;
- a restart upgrades nothing again.

The owner's live store was not opened by a reducer-2 build.

**Monitor.** The synthetic invariant monitor (`tests/synthetic/src/runner.rs`) restates the rule without the reducer's helper. After every delivered step, every item must show each action exactly when that action's decisions cover its active evidence, or, once ended, any of its evidence. Every permutation in the campaign is checked against it.

### Semantic equality

**Defect.** The projection named a decision by the episodes it governed (`governs: [0]`). Witness C, P2 → Resolve → Q10 against Q10 → Resolve → P2, then normalized two different decisions identically. Under the oracle-only negative control below, the two histories hash to the same value (`8c4d84d6…`).

**Fix** (`de3360a`, `crates/state-engine/src/semantic.rs`). Each decision is compared by three things:
- the native causal points it covered: source, epoch, order domain, sequence, native key and predecessors, which are stable native identities rather than allocated UUIDs;
- the count of covered positives without a causal point;
- the episodes whose items it applies to now.

Command time, allocated IDs, cursors and other administrative fields stay excluded. A decision covering P, one covering Q, one covering both and one with no active covered evidence are therefore all distinct. Equal coverage in another admissible order converges (witness D), and different coverage stays distinguishable (witness C), because those are different histories.

### Witness tests

All are in [`tests/synthetic/tests/wait_owner_coverage.rs`](../../../tests/synthetic/tests/wait_owner_coverage.rs) and the catalog (`tests/synthetic/src/scenarios.rs`). Each `admit` runs the order through every layer:
- the pure reducer and the SQLite journal (equal semantics);
- the materialized tables (equal to the state) and the materialized outbox (equal to canonical eligibility);
- the public snapshot (its open-attention view holds exactly the unresolved items, with canonical acknowledgement);
- a checkpoint, a restart and replay from genesis (equal state).

Eight further valid orders per scenario converge on the same semantic hash.

| Witness | Case | Result |
| --- | --- | --- |
| A | P2(A) → Resolve → C3(A) → Q1(B) | Pass. The decision covers P2 alone; C3 ends P2; Q1 stays active (uncertain); the item is open and its intent eligible; it is in the public open-attention view; the turn is WAITING |
| B | P10(A) → Resolve → C5(A) → Q2(B) → D1(B) | Pass. Before D1, P10's item is owner-resolved and Q2's is open and eligible. After D1 merges them, the one item is open and eligible, the decision still covers P10 alone, there is still one command, and the patch built for D1 carries the item as needing attention |
| C | P2 → Resolve → Q10 versus Q10 → Resolve → P2, then C5 | Pass. Same positives, command ID and action; the covered witnesses (`2` / `10`) and the semantic hashes differ before C5. After C5, P's history has Q10 open and Q's has nothing open |
| D | P2, Q10 in either order → Resolve → C5 | Pass. One semantic hash; nothing open; it differs from both C histories |
| E | `wait-new-after-resolution` (P10 → Resolve → C12 → P15) | Pass. P10's item keeps its owner resolution and native clear; P15's item is not owner-resolved and is the one eligible |
| F | `wait-owner-scopes` | Pass. t1's Resolve, the session-scoped Acknowledge and t2's untouched wait each stay on their own scope; only t2 is notifiable |
| G | `wait-owner-partial-actions` (Acknowledge on t1, Snooze on t2, each followed by C and an incomparable Q) | Pass. Before Q, each action applies; after Q, neither does. Acting again on the item now showing P and Q covers both: t1 is acknowledged and t2 is snoozed until its time |
| — | `wait-owner-unordered-coverage` | Pass. The decision records one covered positive without a causal point; the second keeps the item open |
| — | Oracle | Pass. Swapping the covered witnesses swaps the hash; changing the covered unordered count changes it |

The preserved scenarios all still meet their expectations in reference order and converge in every seeded and stepwise SQLite permutation: `wait-reappears`, `wait-uncertain-reopens`, `wait-owner-resolution-kept`, `wait-new-after-resolution`, `wait-turn-ownership`, `wait-session-scoped`, `delayed-clear-wait`, `delayed-positive-wait` and `wait-generations`. The first review's `wait_counterexamples` (5 tests) and `public_read_model` (6 tests) pass.

### Negative controls

Each control is a patch in [`negative-controls/`](negative-controls/), applied to the candidate, run, recorded and reverted.

| Control | Result |
| --- | --- |
| [`any-intersection-rule.patch`](negative-controls/any-intersection-rule.patch): reducer 1's rule restored | 7 of 8 `wait_owner_coverage` tests fail. Only F passes, since it involves no partial coverage. `reducer_upgrade` fails. Reference-order expectations fail for 6 owner-coverage scenarios (all but `wait-owner-covers-both` and `wait-owner-scopes`), with monitor violations "Resolve/Acknowledge shown=true, covered=false". Seeded permutations report 637 failures, including all 20 `wait-new-after-resolution` seeds, whose orders deliver P15 before C12. The SQLite catalog and permutation tests fail. ([results](negative-controls/any-intersection-rule.results.txt)) |
| [`oracle-without-covered-witnesses.patch`](negative-controls/oracle-without-covered-witnesses.patch): `covers` and `coversUnordered` removed from the projection | Exactly the two oracle tests fail. Witness C's two histories hash identically (`8c4d84d6…`), and so do a decision and the same decision with a different unordered count. Every behavioural test passes, so the control isolates the oracle. ([results](negative-controls/oracle-without-covered-witnesses.results.txt)) |

## Group 5 — a WAL store without `-shm` is refused without creating one

**Defect.** The preflight added `?readonly_shm=1` only when both `-wal` and `-shm` existed. With a `-wal` and no `-shm` it opened `SQLITE_OPEN_READ_ONLY` with no URI option. SQLite needs a WAL index to read the WAL, so it created a 32,768-byte `-shm` that persisted after the refusal.

**Options measured on vendored SQLite 3.53.4.** These were measured with throwaway tests in the Group 5 worktree, which were not kept:

| Option | Result | Verdict |
| --- | --- | --- |
| (a) `readonly_shm=1` with no `-shm` | `SQLITE_CANTOPEN` | Creates nothing, but sees nothing and would block a supported store |
| (b) Read-only with `PRAGMA locking_mode=EXCLUSIVE` | The first read fails with `SQLITE_IOERR_LOCK` | Rejected for the same reason |
| (c) Private inspection copy | Sees the WAL-only state and leaves the store directory unchanged | Chosen |

**Fix** (`8c516b1`, `crates/journal/src/lib.rs`, `InspectionCopy`). Only the WAL-without-`-shm` branch changed:
1. The main file and `-wal` are copied into a new 0700 directory under the temporary directory. The caller holds the store's `WriterLock`, so no writer runs and the two copies form one committed state; on APFS `std::fs::copy` clones them.
2. The copy is opened read-only with `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE`, so the `-shm` SQLite needs lands only there.
3. The schema and reducer checkpoint are read from the copy.
4. The directory is removed on every path, after the connection closes.

A supported store then opens through the ordinary writable path. `immutable` is never used while a `-wal` exists. The other branches are unchanged:
- `-wal` beside a `-shm`: `readonly_shm`;
- no sidecar: `immutable`;
- hot rollback journal: opened writable.

**Tests** (`crates/journal/tests/future_store.rs`, 18 tests, all pass):

| Case | What the test shows |
| --- | --- |
| A: schema 99 only in a `-wal` with no `-shm` | Refused as `SchemaTooNew { found: 99 }`; main and WAL byte-identical; `-shm` still absent; same directory listing |
| B: reducer `REDUCER_VERSION + 1` only in such a `-wal` | Refused the same way, unchanged |
| C, D, G: existing WAL+`-shm`, rollback and hot-journal refusals | Pass unchanged |
| E: a supported store whose last rows exist only in a `-wal` with no `-shm` | Opens, and the WAL-only row reads back |
| F: supported stores and the schema-2 upgrade | Pass (`supported_stores_still_open_after_the_preflight`, `migrated_commands`, `migrated_outbox`, the migration runner) |
| H: unreadable `-wal`, unreadable main file, inspection copy that cannot be created | Each returns an error; the store directory is unchanged |
| H: garbage `-wal`, and garbage `-shm` beside a future `-wal` | Both refused `SchemaTooNew`; the store directory is unchanged |
| The copy is removed after a refusal | The private `TMPDIR` is left empty (child-process test) |

**Negative controls.**
- **Old open, reproduced:** `a_plain_read_only_open_creates_a_shm_beside_a_wal_without_one` opens exactly as before the fix and asserts a new 32,768-byte `-shm` beside unchanged main and WAL files.
- **Unfixed preflight restored:** with [`preflight-before-fix.patch`](negative-controls/preflight-before-fix.patch) applied, 5 of the 18 tests fail: A, B, the garbage `-wal`, the copy failure and the copy removal ([results](negative-controls/preflight-before-fix.results.txt)).

**File-by-file evidence.** In [`../migration/summary.json`](../migration/summary.json), every refusal store (rollback, clean WAL, WAL with sidecars, WAL without `-shm`, each for the schema and the reducer) records:
- the main header fields and bytes 18–19;
- the versions in the committed state and in the main file alone;
- the presence, size and SHA-256 of the main file, `-wal`, `-shm` and `-journal`, and every other file before and after;
- the returned error, the SQLite version, and the open flags, URI query and target the preflight used.

All 8 are `refused: true, unchanged: true`. For the WAL-without-`-shm` schema-99 store, committed schema 99 is read where the main file alone says 3. Main `ae803b40…` (286,720 bytes) and WAL `46aa8140…` (4,152 bytes) are identical before and after, and no `-shm` or journal exists before or after. The hashes differ between runs because each run creates a fresh store.

**Observation.** A refused newer *reducer* checkpoint returns `SchemaTooNew { found: <reducer version> }`, whose message reads "store schema 3 is newer than supported 3". The typed refusal is correct and is the same variant M1 uses for both kinds of refusal; only the message names the wrong kind of version. It was left unchanged.

## Regenerated qualification

Regenerated on source `05a9a2eaaab72c0ed08ea2ac9e5cfb5f31bd9f9a`. Production and test code are identical through the candidate commit.

| Area | Result |
| --- | --- |
| Fixtures | 40 scenarios. The 32 existing fixtures are byte-identical; the 8 new ones are added |
| Replay | 40 scenarios × 3 repeats; restart and genesis replay identical. Journal and materialized-table hashes are unchanged for all 32 existing scenarios: the new rule changes none of their visible state. State and checkpoint hashes change for every scenario only because the state records `reducerVersion` 2. Semantic hashes change only for `wait-owner-resolution-kept` and `wait-new-after-resolution`, the two with wait owner decisions, which now carry their covered witnesses |
| Permutations | 20,400 (20,000 over the required families), 0 failures, 2,040 stepwise SQLite cross-checks. The wait-events family grew from 10 to 18 scenarios, so the campaign doubled to keep each existing wait scenario at no fewer seeds than before. The owner-coverage scenarios are appended to the catalog, so every earlier scenario keeps its seeds. The 17 preserved failing seeds still pass |
| Crash | 100 injections at five admission positions, 0 acknowledged lost, 0 duplicate facts. Rerun because both repairs touch `Journal::open`, which every recovery runs; its summary reproduced byte for byte |
| Migration | Schema-2 upgrade deterministic and preserving; all 8 future-store refusal shapes unchanged; reducer version 2 |
| Contracts | Reducer version 2; regenerated `WaitOwnerDecision` bindings and `CanonicalState` schema fresh against Rust; every instance validates |

**Fresh execution in this remediation, at `05a9a2e`:**

| Run | Result |
| --- | --- |
| `cargo test --workspace --features threadspace-journal/qualification,threadspace-agent/qualification` | 434 passed, 0 failed, 52 suites |
| `npx vitest run` (repository root) | 43 passed, 3 files |
| `THREADSPACE_CHECK_SCHEMAS=1 cargo test -p threadspace-contracts --test schemas` | 3 passed |
| `cargo clippy --workspace --all-targets` with the same features and `-D warnings` | Clean on the final code |

### Native UI

The public attention contract (`AttentionView`, the snapshot and the patch) is unchanged in shape. Which items are open changed, and that is what the UI hydrates and patches from. That was exercised through the real journal: the snapshot in every witness, and the patch for witness B's late D1. The UI's own tests (vitest) pass on the regenerated bindings; no UI code reads the changed wait types.

The installed app was not rebuilt or reinstalled. A reducer-2 build would upgrade the owner's live store on its next open, and the live store must not be migrated for this remediation.

## Retained accepted evidence

- **Capture and sanitization:** retained from `e52c281`. The hook, relay, spool and companion code are unchanged since. The journal changes are wait reduction (no hook event produces a wait fact), the WAL-without-`-shm` preflight branch and the reducer-upgrade load path, none of which those workloads reach.
- **Groups 1, 3, 4, 6, 7 and 8:** unchanged and still passing.
- **D-0008 / C-04:** native evidence from build `73636ec`, with the guard and oracle untouched.
- **C-12 and C-13:** unchanged.
- **M0B identity and Return:** build `5944c81`, unchanged.

## Evidence wording corrections

These correct the narrative only; no raw run file was changed. The original first-remediation text is in [`../history/f7e9a6c/remediation-README.md`](../history/f7e9a6c/remediation-README.md).

1. **Permutation generator.** Recomputed from the archived 10,400-seed campaign by `tests/synthetic/tests/generator_history.rs` ([`generator-orders.json`](generator-orders.json)).
   - 37 orders changed, all in `wait-new-after-resolution`.
   - 28 of them were invalid before the fix: 15 put C12's duplicate before a step it must follow, 10 put P15's, and 3 both, so the causes overlap.
   - 9 valid orders changed too.
   - The current generator makes no invalid order.
   - The first record and commit `ec8ef5b` said only P15 was involved and that every other order was unchanged; that was wrong.
2. **Terminal `-600`.**
   - 9 of the 10 race routes over the two runs on build `5944c81` were refused conservatively as READBACK_FAILED, and 1 was focused exactly; 0 wrong targets.
   - Ordinary exact Return (`baseline-exact`) passed in both runs.
   - The runs record each route's focus timing but not when the competing selection happened, so the race setup and its overlap are not fully attested.
   - M5 owns the investigation. Diagnosing it does not require running an old binary against the owner's live store.
3. **Fullscreen.** The G08 `fullscreen-space-then-return` case in both runs recorded null `enteredFullscreen` and `exitFullscreen` witnesses. It shows exact Return after that step and is not a fresh fullscreen-transition qualification; the accepted M0C H-10 evidence is authoritative.
4. **Hot rollback journal.** The preflight opens such a store writable, so SQLite rolls the journal back, restoring the last committed bytes. That is an accepted, narrowly scoped exception, not a necessity: refusing such a store from a private copy, as is now done for a WAL without `-shm`, would also be possible.
5. **Test execution provenance.**
   - The first remediation's handoff reported "413 Rust tests and 43 TypeScript tests". Those figures appear in no recorded evidence file and are not evidence.
   - Every run listed under "Fresh execution" above was executed in this remediation at the commit shown.
   - Retained areas keep their own source commits, listed in the manifest under `remediation.second.evidenceSources`.
   - The reviewer's supplemental witnesses were not executed here; equivalent cases were (see "Witness provenance" above).

D-0007 stays PROPOSED. §3, §4 and §6 now state the corrected wait model, the semantic comparison, the preflight and the checkpoint upgrade.
