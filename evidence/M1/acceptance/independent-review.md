# M1 supplemental independent timestamp acceptance

**ACCEPT M1 — 2026-10-08.** Candidate `01a215ddb55c3214a7d669b040f84886ede0be58` closes the sole remaining M1 blocker: checkpoint-placement-dependent restoration of an attention item's committed resolution timestamp. D-0007 is ACCEPTED with its existing qualified limitations. M2 is authorized but unstarted; the integration and controlled-installation steps below precede its execution.

This is the final timestamp-only closure review. Previously accepted M1 groups remain accepted. No application implementation, test source, dependency pin, native qualification rule or live store was changed by this acceptance commit. The independent portable runs used disposable worktrees; their explicitly recorded Cargo scaffolds were not applied to `m1`.

This supplement records this review’s own portable executions and two historical-producer reproductions. During publication, `origin/m1` advanced to acceptance commit `7d84e4b3327d5d22ffb0428bd8d944a2ff18af4f`. That commit and all of its records are preserved. Its [execution.json](execution.json) describes separate executions, including one historical reproduction; the two-run claim here belongs specifically to [historical-reproduction.json](historical-reproduction.json). This descendant adds the separate review records and corrects the stale current-status paragraph in SPEC §18.5 without changing its qualified limits.

## 1. Frozen source and qualification identities

| Role | Exact commit |
| --- | --- |
| Reviewed candidate | `01a215ddb55c3214a7d669b040f84886ede0be58` |
| Previous rejected candidate | `579df5b7902396f706095a28b455f9189513daa5` |
| Accepted, still-unmerged main | `cd9e37645adf7e6b5f74ab7f0baa5197d8e08b54` |
| Production repair | `7fe5539c97a47e0d223d30b43550bdc51570c366` |
| Original focused/workspace qualification and fixture source | `80df8b09c04c13c7b527d8913eb890858da3a606` |
| D-0007/invariant documentation | `0ef472c48616cd3692b0f83269249f1e31301a5b` |
| Evidence and manifest-assembly source | `787a3d42ec053a4d0d47555382458c01e61de0ad` |
| Historical reducer-1 producer | `f7e9a6ce2ce034e04abff03bf8a358dc98a98e01` |

Origin was fetched, and both refs were independently checked through GitHub. The five remediation commits form the stated linear chain; accepted main is an ancestor of the reviewed candidate (0 behind, 90 ahead). Remote branches at review were `main` and `m1`, with M2 explicitly unstarted in the repository. This establishes published repository state, not unseen work elsewhere on the owner's machine.

The production diff against the rejected candidate is limited to `crates/journal/src/canonical.rs`. Application source, dependency/toolchain pins, focused test assertions and historical fixture inputs are unchanged between qualification `80df8b0` and candidate `01a215d`. A later change to the evidence-composition binary `tests/synthetic/src/bin/m1/manifest.rs` assembles the new report; it does not change the qualified application or focused tests.

The manifest's original `sourceCommit = 787a3d4...` is preserved as assembly provenance. Its `acceptance` object and final verdict are an explicit independent-review overlay. The original generated candidate manifest is archived byte-for-byte in [history/01a215d](../history/01a215d/README.md). The acceptance commit is the Git commit containing this record; its exact SHA is reported after commit/push rather than self-referenced here.

## 2. Timestamp repair — PASS

In [canonical.rs](../../../crates/journal/src/canonical.rs), `keep_committed` first assigns the committed revision, then restores the matching row's nullable `resolved_at_ms`, then compares the complete rebuilt row and reconciles its revision. `load_engine` performs this rebase after suffix recovery and historical owner-decision conversion, before `Engine::upgrade` and persistence.

`nullable_integer` distinguishes three outcomes: SQL INTEGER becomes `Some(value)`; SQL NULL becomes `None`; a missing column or TEXT/REAL/BLOB is invalid. Its outer option distinguishes an invalid decode from a valid nullable value, and the production caller converts an invalid decode to `JournalError::Invalid` naming the attention row. The unit test covers the historical timestamp, integer zero, NULL, all three malformed types and a missing named column.

The unchanged `derive_attention_resolution` remains authoritative: empty resolution causes clear the timestamp and reason; nonempty causes preserve an existing timestamp with `get_or_insert`. Thus a still-resolved item retains its committed time, a reopened item loses it, and an item without a committed row never receives an invented committed time from this repair. Resolution causes, owner coverage and notification disposition are still derived from current evidence. `derive_attention_resolution`, `Engine::upgrade`, materialization and transaction mechanics were not changed to conceal the defect.

## 3. Genuine six-entry regression — PASS

The committed fixture has SHA-256:

`92d8666a16e5f478f74c4d1bf5da3ae2ecec456c841e85d71030d543ceb6bc47`

Independent read-only SQLite inspection confirmed six observations/facts at the specified times, P3 at sequence 3, owner `cmd-resolve-p3` at cursor 4/time `1791000000040`, uncovered P4 at sequence 4, and comparable C5 at sequence 5. The original attention row is `663fb032-fef5-812b-9104-b15bd1135349`, revision 6, resolved at time 40. Its original request `4d9804b6-303f-81f5-b80d-c997326b6dc8` is SUPPRESSED. Reducer-1 checkpoints at 0, 4 and 6 have valid checksums.

The historical producer was independently executed twice with the candidate emitter, seeded allocator 31 and a checkpoint after step 4. Both produced exactly the committed database bytes and identical complete SQL tables. The differing bytes of the earlier reviewer's unavailable fixture therefore do not undermine the genuine source-level counterexample. See [historical-reproduction.json](historical-reproduction.json) and [captured execution](historical-qualification.txt).

The focused tests use `Journal::open_with` through the real SQLite runner. Their A/B/C inputs differ only in checkpoint placement; complete non-checkpoint history equality is asserted before upgrade.

| Variant | Latest old checkpoint | Suffix | Canonical `resolvedAtMs` | SQL `resolved_at_ms` | Attention revision | Differing canonical paths |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 6 | 0 | 1791000000040 | 1791000000040 | 6 | 0 |
| B | 0 | 6 | 1791000000040 | 1791000000040 | 6 | 0 |
| C | 4 | 2 | 1791000000040 | 1791000000040 | 6 | 0 |

All three independently reproduced the same complete-state/checkpoint and materialized-table digests:

| Digest | SHA-256 |
| --- | --- |
| State and checkpoint | `bc0267a00066420911ea43b05ed0371f28ce9c3758400451dd1ce6e5da225c88` |
| Projection and actual materialized tables | `223353b4c02f1688cbfad04d5db64941787e41d6ab2002c3e875fff0eab7e008` |
| Semantic | `82e25cf752585b29a36b29735e559decffefa2e2f68295b89d8f7cd38937673e` |
| Journal | `9a167320e9ade221d808412062f35ab7f5805cc2da8a33ab17fa40733a1a649c` |

Assertions cover complete canonical JSON, exact state and SQL timestamps, actual-table versus expected-projection equality, revisions/cursors, one owner decision covering P3, one preserved owner command, the original outbox row, exactly one newest REDUCER_UPGRADE checkpoint through cursor 6, and close/reopen with no repeated upgrade. The created cursor remains 3. This evidence exceeds semantic-hash equality.

The independently generated [resolved-at report](../remediation-4/resolved-at.json) was byte-identical to the retained report (file SHA-256 `946db64facb6ac6fdbcfed1ff3f856e40075aa2ba53185806fbbf774ceca4b64`). Separately, the actual cursor-6 checkpoint's representation conversion reproduces the reported expected state hash, and direct reads of its 16 materialized tables reproduce the reported table hash; those static hash checks are labeled separately in [verification.json](verification.json).

## 4. Negative controls — PASS: all four discriminate

Each committed control patch was applied only to `canonical.rs` in a disposable worktree. The Rust tests and library modules stayed byte-identical. Patch preimage/postimage Git blob hashes were checked. The fresh runs reproduced every corresponding retained JSON report byte-for-byte; failures are actual relevant assertions, not wrapper exit labels.

| Control | Actual result | Synthetic / decoder exit |
| --- | --- | --- |
| A: remove only timestamp restoration | A keeps time 40; B/C become time 60 in state and SQLite. The exact-time test fails at line 513; malformed-store refusal also fails. | 101 / 0 |
| B: overwrite with committed non-null time after rederivation | Reopened items incorrectly retain 50/40/50. The reopened-item map assertion fails at line 585. | 101 / 0 |
| C: decode NULL as zero | All five synthetic tests pass because final rederivation clears this value; the direct decoder assertion fails at line 1236. This limitation is explicit. | 0 / 101 |
| D: decode an unexpected type as NULL | Malformed-store refusal fails at line 626, and the decoder assertion fails at line 1243. | 101 / 101 |

Under A, B/C differ from A only at `/attention/663fb032-fef5-812b-9104-b15bd1135349/resolvedAtMs`. Their state hash is `32237eb2cfc47a017c82dc771b37866b98d74952f07c82a9196fd65fd4f18464` and table hash `bd1782a6156bde6d08e0a48e2e914ad5f05f463626378cc29beedb110e1fa98d`; their semantic hash remains unchanged. This is the principal discriminator for the original defect.

After restoration, the unchanged candidate again passed all 5 checkpoint-tail, 1 reducer-upgrade and 1 decoder tests. All 197 tracked Rust files were verified restored. See [portable-results.json](portable-results.json), [complete captured output](portable-qualification.txt) and the original [control patches and reports](../remediation-4/negative-controls/).

## 5. Unresolved items and nullable refusal — PASS

The reopened regression asserts this complete map in both canonical state and SQLite at old checkpoint positions 30/0/6:

| Attention ID | Reducer-1 timestamp | Upgraded timestamp |
| --- | ---: | ---: |
| `38cc3ee8-5b0f-8db0-b9f9-5009640eba9c` | NULL | NULL |
| `3e47b2b0-a74f-827a-97d8-ce9cae31767e` | 1791000000050 | NULL |
| `663fb032-fef5-812b-9104-b15bd1135349` | 1791000000040 | NULL |
| `7b5efbc8-620e-8af7-b38a-5933656372eb` | 1791000000050 | NULL |
| `9c14d766-ad73-835e-ae27-ff21b71c0222` | NULL | NULL |
| `d3561f2e-346b-8bf7-a15f-48d9d2563591` | 1791000000070 | 1791000000070 |

It also asserts that every item has a timestamp exactly when it has resolution causes. Preserving every old non-null value would fail this test, as control B demonstrates.

The malformed-store test deliberately removes STRICT from a disposable attention table so that TEXT, REAL and BLOB values can reach the production decoder. It verifies each actual storage type, calls `Journal::open_with`, requires the named `JournalError::Invalid`, and then verifies unchanged old checkpoints and every non-checkpoint row. This is a valid conservative-decoder negative test; it does not claim normal STRICT insertion accepts those values.

## 6. Previously accepted migration — preserved

The existing checkpoint positions 30/0/6 retain **1 PENDING / 4 HELD / 1 SUPPRESSED**, state hash `4b5745d927bae69e46dfa414af0d4a04ffda9875b1c5f855210b5b178431654a` and table hash `3402b1305815e9d52c0f50e0ed6fb795d2e9c6546b23e7c672895ae109e65349`.

| Request ID | Attention ID | Upgraded disposition |
| --- | --- | --- |
| `62917481-141c-8b01-9f08-8b9039b114d2` | `9c14d766-ad73-835e-ae27-ff21b71c0222` | PENDING |
| `4d9804b6-303f-81f5-b80d-c997326b6dc8` | `663fb032-fef5-812b-9104-b15bd1135349` | HELD |
| `60b47679-b35e-8713-8a93-887e332d3f23` | `38cc3ee8-5b0f-8db0-b9f9-5009640eba9c` | HELD |
| `c225bdd2-c3d5-8948-8fb9-777919d8656d` | `7b5efbc8-620e-8af7-b38a-5933656372eb` | HELD |
| `d2f458a7-c106-8216-938e-665fa5b6d380` | `d3561f2e-346b-8bf7-a15f-48d9d2563591` | SUPPRESSED |
| `e976fef2-0ab9-86de-86e5-5eb61c2cadc1` | `3e47b2b0-a74f-827a-97d8-ce9cae31767e` | HELD, consistently derived by the upgrade |

Five requests existed in reducer 1; the sixth is the same previously accepted upgrade-derived request. All six identities and relationships remain exact. Unordered X/Y both conservatively read the old true flag as count 1, retain one HELD request and produce zero differing state paths. Each existing-history variant has exactly one upgrade checkpoint, stable close/reopen and preserved live replay. A new LIVE wait creates and hands out only its own PENDING request `78ff7f31-3ee7-88e5-9b7e-d519939fe1fc`; historical HELD work stays HELD.

Remediation-4 `checkpoint-tail.json` and `unordered-pair.json` are byte-identical to remediation-3. Independent fresh execution also reproduced those files exactly. Their report SHA-256 values are respectively `4be7d7843c92361ffd9197fd895d4160fc111e43d2528b14b9a2b6712587c549` and `a2c599ff63ad699895ceedb26096e8c7ce9f991645d9b879e651d1c91c1bd994`.

## 7. Transaction boundary — sufficient for this change

The timestamp rebase is in memory. `load_engine` propagates decode failure before `persist_upgrade`; a usable Journal is returned only after successful persistence. `persist_upgrade` opens one IMMEDIATE transaction, materializes all rows and writes the REDUCER_UPGRADE checkpoint through that same transaction, then commits. Successful upgrade and deterministic close/reopen were executed; malformed pre-persistence refusal leaves old rows/checkpoints unchanged.

No separate crash was injected inside the upgrade transaction. The earlier accepted admission crash campaign is retained, not relabeled as that missing injection. The unchanged transaction structure and accepted SQLite evidence are sufficient for this nullable-value restoration; the patch changes no transaction mechanics. Close/reopen in these focused tests means dropping and reopening the production Journal on the disposable store, not relaunching a native app or claiming an OS-process crash.

## 8. Evidence and execution classification

**Independently executed in this review:** origin/ref/ancestry and source comparisons; fixture and checkpoint checksums; SQLite row inspection and independent hash calculations; report/patch identity comparisons; a direct decoder test on the unchanged candidate; portable focused tests, all four controls and restored PASS; two historical-producer executions yielding byte-identical fixtures; scoped privacy and document/evidence consistency checks.

The original synthetic package links macOS adapters that cannot compile on Linux. Only in disposable worktrees, the [portable scaffold](portable-scaffold.patch) and [historical scaffold](historical-scaffold.patch) omit two unused direct dependencies and disable the unrelated CLI binary. Their lock diffs remove only those dependency edges; no version/checksum changes. Production Rust, test assertions, synthetic library modules and SQLite implementation remain unchanged, apart from each deliberately applied control. Runs use Rust 1.99.0 and the Linux target. These are supplemental portable executions; they do not claim native macOS application qualification. All 18 generated reports across the six stages matched their respective retained reports exactly.

**Retained implementation-run qualification at clean `80df8b0`:** 5 checkpoint-tail tests, 1 reducer-upgrade test, 1 decoder test, 53 workspace suites/440 passed/0 failed, clean Clippy, 3 schema-freshness tests and 43 Vitest tests. The [workspace ledger](../remediation-4/test-runs.txt) is a normalized results record, while the [focused record](../remediation-4/focused-tests.txt) names the executed assertions. Synthetic fixtures/contracts/replay/crash reproduction is retained as recorded. The 20,400 reducer permutations, capture/sanitization, native recovery and M0B evidence remain credited because their affected product code is unchanged.

Retained native build identities remain separate: C-04 source `73636ec2482313f3507f32b378fb64cd3ef082de`; M0B/Return source `5944c81228f163e2583a8418369450ba5f1a0829`. Installer/runner identity records agree and are unchanged. No native macOS rerun, reducer-2 installation or access to the owner's live store occurred here.

All 44 candidate-delta files, including SQLite bytes, were scanned for personal absolute paths, email addresses and common secret patterns with no matches. The new acceptance evidence was also checked; review-workspace path prefixes in captured output are normalized explicitly. This is a scoped inspection, not a proof that regexes detect every possible secret. Historical evidence was not overwritten.

## 9. Final M1 gate and next steps

**M1 is ACCEPTED. M2 is authorized and remains unstarted.** D-0007 is ACCEPTED; D-0008 remains ACCEPTED and version-scoped. C-04/C-12/C-13 and every previously accepted group retain their established scope.

Preserved obligations include M2's three evidence-set conversions, observer/inventory and native Mark handled qualification; M5's native outbox delivery and selection/readback-race investigation; M6's live Codex ancestry/handshake qualification; M13's sustained-resource, retention and stress obligations; and M15's packaging/update, Terminal restart and external-display facets. D-0008 still requires containment removal and native recovery requalification before a Tauri/tao/Wry update.

**Merge recommendation:** `SAFE TO FAST-FORWARD M1 INTO MAIN`.

The next controlled work is to fast-forward the final accepted M1 commit into main, verify it, remove completed branches/worktrees after verification, then perform a separately controlled reducer-2 installation with a verified pre-upgrade backup and post-upgrade checks before starting M2. This review performs none of those merge, installation or M2 actions.
