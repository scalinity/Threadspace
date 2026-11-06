# Pre-M3 journal compatibility fixture

`journal.sqlite3` and `state.json` were generated from the unmodified contracts,
state-engine and journal sources at `70eb355`, using `generator.rs` as a temporary
journal example. Only the archived workspace member list was narrowed for the run.
The baseline source archive was inside the M3 worktree's ignored artifacts folder;
no main-checkout source or build output was used.

The generator opens an empty journal without demo data, applies one inventory
activation with a terminal binding, verifies genesis replay and projections, and
writes a checkpoint with origin `PRE_M3_70eb355`. It checkpoints SQLite's WAL before
closing. The fixture contains no native session or user data.

`pre_m3_journal_checkpoint_and_genesis_preserve_state` loads copies with and without
the final checkpoint, compares the complete state against the baseline JSON (new
optional maps default empty), checks genesis replay and projections, then writes a
new checkpoint and restarts. The companion native activation tests cover adding a
new start to legacy inventory evidence.
