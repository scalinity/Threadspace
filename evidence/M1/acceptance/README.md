# M1 final acceptance — resolution timestamp

**Verdict: ACCEPT M1.** The final independent review accepts candidate `01a215ddb55c3214a7d669b040f84886ede0be58`. The sole remaining blocker—the durable resolution timestamp depending on reducer-1 checkpoint placement—is closed. M2 is authorized by the milestone gate and remains unstarted.

This review preserves all previously accepted M1 findings. It does not merge M1, install reducer 2, access the owner's live store, or begin M2. The acceptance commit changes documentation and evidence only.

Machine-readable records: [review.json](review.json), [execution.json](execution.json), and [validation.json](validation.json). New execution output is in [portable-execution.txt](portable-execution.txt); the disposable build adaptations are in [portable-scaffolds.txt](portable-scaffolds.txt).

## Exact identities

| Role | Commit |
| --- | --- |
| Accepted main, independently refreshed | `cd9e37645adf7e6b5f74ab7f0baa5197d8e08b54` |
| Previous rejected candidate | `579df5b7902396f706095a28b455f9189513daa5` |
| Production repair and decoder unit test | `7fe5539c97a47e0d223d30b43550bdc51570c366` |
| Focused tests, fixture, and retained clean qualification | `80df8b09c04c13c7b527d8913eb890858da3a606` |
| Decision and invariant documentation | `0ef472c48616cd3692b0f83269249f1e31301a5b` |
| Evidence and manifest composition | `787a3d42ec053a4d0d47555382458c01e61de0ad` |
| Reviewed final candidate | `01a215ddb55c3214a7d669b040f84886ede0be58` |

The five commits after the rejected candidate form a linear chain in the order shown. Main is the candidate's merge base; the reviewed candidate was 90 commits ahead of it. Both branch identities were checked through GitHub and a fresh `git fetch origin --prune`. The published repository had only `main` and `m1`, no `evidence/M2`, and documented M2 as unstarted. No claim is made about unobserved work on another machine.

The only production-file difference from the rejected candidate is `crates/journal/src/canonical.rs`. Its Git blob is `48b249377fe8bafac8ee00fafa469edafa772ccc` at the repair, qualification, and final candidate. All application sources, dependency locks, and the toolchain are unchanged after qualification. The later Rust change is the evidence composer, `tests/synthetic/src/bin/m1/manifest.rs`; it is not application code. The focused tests and fixture emitter also match their qualification-source versions.

The manifest's retained `sourceCommit = 787a3d4…` identifies composition source. It does not relocate the recorded test execution from `80df8b0`. The immutable [candidate manifest](../history/01a215d/manifest.json) preserves the pre-acceptance bytes. The metadata commit containing this record is the accepted descendant; its exact SHA is reported by the final review and is discoverable in this file's Git history.

## Timestamp repair: PASS

In [`canonical.rs`](../../../crates/journal/src/canonical.rs), `load_engine` recovers the historical suffix, converts recovered decisions as reducer 1 recorded them, and invokes `keep_committed` before `Engine::upgrade`.

For each attention item with a committed row, the new restoration at lines 616–618 reads the committed SQL value before the reconstructed-row comparison at lines 619–621. The decoder distinguishes an integer, a legitimate NULL, and an invalid/missing column:

| SQL input | `nullable_integer` result | Restored field or outcome |
| --- | --- | --- |
| Integer `t`, including zero | `Some(Some(t))` | `Some(t)` |
| NULL | `Some(None)` | `None` |
| TEXT, REAL, BLOB, missing column | `None` | `JournalError::Invalid` |

The initial committed revision assignment precedes timestamp restoration; the following row comparison reconciles the revision using the restored timestamp. Final rederivation and atomic persistence follow both operations. An item without a committed row skips restoration and receives no invented historical timestamp.

The entire state-engine implementation is unchanged by this repair. [`derive_attention_resolution`](../../../crates/state-engine/src/reduce.rs) clears the timestamp and reason when resolution causes are empty, and uses `get_or_insert` when a cause exists. Thus current evidence still decides whether an item is resolved. The patch changes neither resolution causes nor owner coverage nor notification disposition.

## Genuine six-entry fixture and complete-state regression: PASS

The [historical emitter](../../../fixtures/m1/reducer-1-store/emit_reducer1_store.rs) was independently run under actual reducer-1 producer `f7e9a6ce2ce034e04abff03bf8a358dc98a98e01`, using the emitter committed at `80df8b0`. It admitted the six real entries, with seeded allocator 31, and checkpointed after entries 4 and 6. The independently regenerated 299,008-byte fixture exactly matches the committed fixture:

`92d8666a16e5f478f74c4d1bf5da3ae2ecec456c841e85d71030d543ceb6bc47`

Direct SQLite inspection confirmed session start, turn start, P3, owner Resolve(P3), P4, and C5 at wall times `1791000000010` through `1791000000060` in ten-millisecond increments. There are six committed canonical entries and six facts. `cmd-resolve-p3` covers only P3, at time `1791000000040`; reducer 1's materialized attention row has that timestamp, revision 6, and reason `handled P3`. Its original request `4d9804b6-303f-81f5-b80d-c997326b6dc8` is SUPPRESSED. Every original checkpoint's state checksum was independently recomputed.

The regression uses disposable copies. It deletes only the selected reducer-1 checkpoints and proves equality of every other table's rows, excluding the checkpoint sequence counter. Each variant opens through `SqliteRunner::open` and real production `Journal::open_with`.

| Variant | Latest old checkpoint | Suffix | Canonical timestamp | SQL timestamp | Differing canonical paths |
| --- | ---: | ---: | ---: | ---: | ---: |
| A | 6 | 0 | `1791000000040` | `1791000000040` | 0 |
| B | 0 | 6 | `1791000000040` | `1791000000040` | 0 |
| C | 4 | 2 | `1791000000040` | `1791000000040` | 0 |

All three independently reproduce:

- State/checkpoint SHA-256: `bc0267a00066420911ea43b05ed0371f28ce9c3758400451dd1ce6e5da225c88`.
- Projection/table SHA-256: `223353b4c02f1688cbfad04d5db64941787e41d6ab2002c3e875fff0eab7e008`.
- Semantic SHA-256: `82e25cf752585b29a36b29735e559decffefa2e2f68295b89d8f7cd38937673e`.
- Journal SHA-256: `9a167320e9ade221d808412062f35ab7f5805cc2da8a33ab17fa40733a1a649c`.

The [test](../../../tests/synthetic/tests/checkpoint_tail_upgrade.rs) compares complete canonical JSON, separately asserts the exact state and SQL timestamps, compares state/projection/table digests, and requires zero projection differences. It preserves the single owner command, single P3-only owner decision, original SUPPRESSED request, attention revision 6, created cursor 3, and through-cursor 6. Every variant writes exactly one newest reducer-2 `REDUCER_UPGRADE` checkpoint; reopening reproduces the state and timestamp without another upgrade.

The freshly emitted `resolved-at.json` is byte-identical to the [committed report](../remediation-4/resolved-at.json). This conclusion does not rely on semantic equality alone.

## Four independent negative controls: PASS

Each committed [control patch](../remediation-4/negative-controls/) was applied in an isolated checkout. The focused tests remained byte-identical. The actual assertion output, production patch identity, and restored source were checked.

| Control | Synthetic results | Decoder results | Relevant discriminator |
| --- | --- | --- | --- |
| A: remove only timestamp restoration | 3 pass / 2 fail; exit 101 | 1 pass; exit 0 | B and C become time 60; exact timestamp assertion fails. Malformed-store refusal also fails. |
| B: overwrite committed non-null timestamps after rederivation | 4 pass / 1 fail; exit 101 | 1 pass; exit 0 | Three genuinely unresolved items retain historical times; reopened-item assertion fails. |
| C: decode NULL as zero | 5 pass; exit 0 | 1 fail; exit 101 | Decoder returns `Some(Some(0))` instead of `Some(None)`. |
| D: decode unexpected types as NULL | 4 pass / 1 fail; exit 101 | 1 fail; exit 101 | Malformed-store open succeeds and the decoder converts invalid data into a legitimate absence. |
| Restored candidate | 5 pass; exit 0 | 1 pass; exit 0 | Expected positive behavior returns. |

Control A reproduces exactly A = `1791000000040`, B = C = `1791000000060`. The sole differing canonical path is `/attention/663fb032-fef5-812b-9104-b15bd1135349/resolvedAtMs`. B and C have state SHA-256 `32237eb2cfc47a017c82dc771b37866b98d74952f07c82a9196fd65fd4f18464` and table SHA-256 `bd1782a6156bde6d08e0a48e2e914ad5f05f463626378cc29beedb110e1fa98d`; their semantic hash still matches A. This is the principal discriminator.

Control C's end-to-end limitation is acknowledged and independently reproduced: final derivation clears the wrong value in the tested unresolved histories, so the direct decoder unit test is essential. Every patch was removed, every tracked Rust source was rechecked, and the restored candidate passed. All eighteen JSON reports across candidate, controls, and restored candidate match their corresponding committed reports byte for byte.

## Unresolved items and earlier migration: PASS

For old checkpoint placements 30, 0, and 6, the passing reopened-item test asserts exact maps in both canonical state and SQL:

| Attention ID prefix | Old timestamp | Upgraded timestamp |
| --- | ---: | ---: |
| `38cc3ee8` | NULL | NULL |
| `3e47b2b0` | `1791000000050` | NULL |
| `663fb032` | `1791000000040` | NULL |
| `7b5efbc8` | `1791000000050` | NULL |
| `9c14d766` | NULL | NULL |
| `d3561f2e` | `1791000000070` | `1791000000070` |

A timestamp is present exactly when resolution causes are present. The three reopened items therefore lose their historical values, while the superseded episode that remains resolved keeps its valid value.

The earlier migration retains **1 PENDING / 4 HELD / 1 SUPPRESSED** and state hash `4b5745d927bae69e46dfa414af0d4a04ffda9875b1c5f855210b5b178431654a`. The exact six final request-to-attention mappings are recorded in [review.json](review.json) and the unchanged [checkpoint-tail report](../remediation-4/checkpoint-tail.json). Five requests existed in reducer 1; `e976fef2…` is the sixth expected request, newly derived HELD for an existing attention item. All five original relationships and the sixth derived relationship match accepted remediation-3 evidence.

The new checkpoint-tail and unordered-pair reports are byte-identical to both remediation 4 and remediation 3. X and Y have identical complete states, `unordered = 1`, and one HELD intent. Every older migration variant writes one upgrade checkpoint at cursor 30 and restarts without another. Subsequent LIVE work emits only its own new PENDING intent; historical HELD work remains HELD, and a further restart replays the new suffix deterministically.

## Transaction boundary and execution limits

The rebase is in memory. `Journal::open_with` propagates a decoding error from `load_engine` before calling `persist_upgrade` or returning a usable journal. `persist_upgrade` uses the unchanged IMMEDIATE transaction; `initial_checkpoint` writes every materialized row and the upgrade checkpoint through that transaction, then commits once.

The passing malformed-store test deliberately removes STRICT only in disposable copies, writes actual TEXT, REAL, and BLOB values, verifies their SQL types, and requires the production decoder to refuse. It checks that every non-checkpoint row and the reducer-1 checkpoint records survive unchanged. This is valid conservative-refusal coverage; it does not claim such values are ordinarily insertable through the normal STRICT schema. Missing-column refusal is tested directly by the decoder unit test.

A crash inside the upgrade transaction was not injected. Logical precommit preservation, successful durability, and deterministic restart were executed. Physical database/WAL byte identity after refusal and power-loss durability are not claimed. The unchanged accepted SQLite transaction evidence and source structure are sufficient for this memory-only field restoration; no transaction mechanics changed.

## Qualification sufficiency and retained scope

New independent checks used Rust 1.99.0 on `x86_64-unknown-linux-gnu`, separate disposable checkouts, and fresh target directories. The Cargo-only adaptations removed unused native agent/relay edges and the synthetic CLI target; no Rust implementation or focused test changed except each deliberate control mutation. No dependency version changed. The original graph's macOS default-target setup failure is retained in the transcript, followed by the explicit Linux-target success. These are portable domain checks, not native macOS application qualification or an unchanged full-workspace build.

The independently executed positive tests passed 5 checkpoint-tail cases, 1 reducer-upgrade case, and 1 decoder unit; the genuine historical emitter also passed. The [implementation-run record](../remediation-4/test-runs.txt), audited at clean source `80df8b0`, records 53 workspace suites totaling 440 passed / 0 failed, clean Clippy, 3 schema checks, and 43 Vitest tests. That broad record is a summarized execution record, not complete raw workspace stdout and not a broad rerun by this reviewer.

The prior 20,400 permutations, fixture/contracts/replay/crash evidence, capture/sanitization, native C-04 on `73636ec2482313f3507f32b378fb64cd3ef082de`, and M0B identity/Return on `5944c81228f163e2583a8418369450ba5f1a0829` remain credited within their accepted scopes. Their affected product code is unchanged. D-0007 is ACCEPTED with its limitations; D-0008 remains ACCEPTED and version-scoped. All sixteen manifest limitations and all M2/M5/M6/M13/M15 obligations remain intact.

The public-evidence review includes a bounded pattern scan of the forty changed evidence/fixture files in the candidate delta and a separate scan of this acceptance update. No matching credential or personal-home-path pattern was found. This is a bounded check, not exhaustive secret detection; it is distinct from the retained product sanitization tests.

## Final gate and next step

**M1 is ACCEPTED. M2 is authorized and unstarted.**

**SAFE TO FAST-FORWARD M1 INTO MAIN**

In the next integration task, fast-forward the exact final accepted M1 commit into `main`, verify the remote identities and retire completed branches/worktrees. Then perform a separately controlled reducer-2 installation with a verified pre-upgrade backup and post-upgrade checks before starting M2. None of those operations is performed by this review.

The unchanged manifest composer produces candidate evidence. Regenerating it cannot grant or carry forward independent acceptance; this record applies to the frozen reviewed candidate and its qualified implementation.
