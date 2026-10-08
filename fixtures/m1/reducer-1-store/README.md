# Reducer-1 store

`journal.sqlite3` is a synthetic store written by M1 candidate `f7e9a6ce2ce034e04abff03bf8a358dc98a98e01` (reducer version 1). It holds four wait owner-coverage histories and a reducer-1 checkpoint (`origin = 'FIXTURE'`, through cursor 30) taken after them. SHA-256 `26ad5fdfcd5c9b33d88b09e7eaeca066e17e68c6c73a14db0e974b51a579454f`.

The four histories are `wait-owner-partial-coverage`, `wait-owner-merge-keeps-coverage`, `wait-owner-partial-actions` and `wait-owner-unordered-coverage`. Each is built with exactly the builder calls of the current catalog, so both reducers receive the same envelopes and owner commands.

## How it was made

[`emit_reducer1_store.rs`](emit_reducer1_store.rs) was copied into a detached checkout of `f7e9a6c` as `tests/synthetic/tests/emit_reducer1_store.rs` and run there:

```text
REDUCER1_OUT=<abs path>/journal.sqlite3 \
  cargo test -p threadspace-synthetic --test emit_reducer1_store -- --nocapture
```

The emitter admits each history in its reference order through the SQLite journal (seeded allocator 31), checkpoints, and exports the store with `VACUUM INTO`. It is not compiled in this workspace.

## What reducer 1 derived

[`reducer-1-derivation.json`](reducer-1-derivation.json) is the emitter's output: how reducer 1 left each wait item. Every partially covered item was taken as handled.

| Session | Wait item | Reducer 1 |
| --- | --- | --- |
| `sess-partial` | P2 resolved by the owner; Q1 never handled | owner-resolved, not notifiable |
| `sess-merge` | P10 resolved by the owner; Q2 merged in later | owner-resolved, not notifiable |
| `sess-unordered-wait` | P3 and U1 resolved by the owner; U2 never seen | owner-resolved, not notifiable |
| `sess-partial-actions` t1 | P2 acknowledged; Q1 never handled | acknowledged, not notifiable |
| `sess-partial-actions` t2 | P12 snoozed; Q11 never handled | snoozed |

`tests/synthetic/tests/reducer_upgrade.rs` opens a copy of this store under the current reducer. It checks two things:

- the checkpoint is upgraded rather than reinterpreted;
- the upgraded state is exactly the one the current reducer reaches when it admits the same histories itself.
