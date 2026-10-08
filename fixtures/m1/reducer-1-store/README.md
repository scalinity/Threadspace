# Reducer-1 store

`journal.sqlite3` is a synthetic store written by M1 candidate `f7e9a6ce2ce034e04abff03bf8a358dc98a98e01` (reducer version 1). It holds four wait owner-coverage histories and a reducer-1 checkpoint (`origin = 'FIXTURE'`, through cursor 30) taken after them. SHA-256 `26ad5fdfcd5c9b33d88b09e7eaeca066e17e68c6c73a14db0e974b51a579454f`.

The four histories are `wait-owner-partial-coverage`, `wait-owner-merge-keeps-coverage`, `wait-owner-partial-actions` and `wait-owner-unordered-coverage`. Each is built with exactly the builder calls of the current catalog, so both reducers receive the same envelopes and owner commands.

Two more stores come from the same emitter and commit:

| Store | Holds | SHA-256 |
| --- | --- | --- |
| `journal-checkpoint-6.sqlite3` | The same history as `journal.sqlite3`, with one more reducer-1 checkpoint, through cursor 6 (after the first history) | `3bea2963fc57acb0e746a2b4f5f43873bf88b3c13f0a29d90aa8a918d83e7d9e` |
| `journal-unordered-pair.sqlite3` | `wait-owner-unordered-pair`: one Resolve over P3 and two positives without a causal point (U1, U2), checkpointed through cursor 6 | `bc6c53289695fa8f2906380f379ff9752bfd83d13c5407589abc595993c1b0ff` |

## How they were made

[`emit_reducer1_store.rs`](emit_reducer1_store.rs) was copied into a detached checkout of `f7e9a6c` as `tests/synthetic/tests/emit_reducer1_store.rs` and run there:

```text
REDUCER1_OUT=<abs path>/journal.sqlite3 \
  cargo test -p threadspace-synthetic --test emit_reducer1_store -- --nocapture
REDUCER1_CHECKPOINT_AFTER=1 REDUCER1_OUT=<abs path>/journal-checkpoint-6.sqlite3 \
  cargo test -p threadspace-synthetic --test emit_reducer1_store -- --nocapture
REDUCER1_HISTORIES=unordered-pair REDUCER1_OUT=<abs path>/journal-unordered-pair.sqlite3 \
  cargo test -p threadspace-synthetic --test emit_reducer1_store -- --nocapture
```

The emitter admits each history in its reference order through the SQLite journal (seeded allocator 31), checkpoints, and exports the store with `VACUUM INTO`. It is not compiled in this workspace. Run without either variable, the emitter as it stands reproduces `journal.sqlite3` byte for byte.

## What reducer 1 derived

[`reducer-1-derivation.json`](reducer-1-derivation.json) is the emitter's output: how reducer 1 left each wait item. Every partially covered item was taken as handled.

| Session | Wait item | Reducer 1 |
| --- | --- | --- |
| `sess-partial` | P2 resolved by the owner; Q1 never handled | owner-resolved, not notifiable |
| `sess-merge` | P10 resolved by the owner; Q2 merged in later | owner-resolved, not notifiable |
| `sess-unordered-wait` | P3 and U1 resolved by the owner; U2 never seen | owner-resolved, not notifiable |
| `sess-partial-actions` t1 | P2 acknowledged; Q1 never handled | acknowledged, not notifiable |
| `sess-partial-actions` t2 | P12 snoozed; Q11 never handled | snoozed |

[`reducer-1-unordered-pair-derivation.json`](reducer-1-unordered-pair-derivation.json): reducer 1 left the `sess-unordered-pair` item owner-resolved, recording only that the decision covered positives without a causal point (a flag, not how many).

## Tests that open them

Each test opens a disposable copy under the current reducer:

- `tests/synthetic/tests/reducer_upgrade.rs`: the checkpoint is upgraded rather than reinterpreted, and the upgraded state is exactly the one the current reducer reaches when it admits the same histories itself;
- `tests/synthetic/tests/checkpoint_tail_upgrade.rs`: the upgrade is the same wherever reducer 1's newest checkpoint sits. Its variants:
  - `journal.sqlite3` as committed, and without its cursor-30 checkpoint;
  - `journal-checkpoint-6.sqlite3` without its cursor-30 checkpoint;
  - `journal-unordered-pair.sqlite3` with and without its cursor-6 checkpoint.
