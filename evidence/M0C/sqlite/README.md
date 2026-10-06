# M0C SQLite crash/receipt and backup/restore qualification (G10, M0C portion)

**Result: PASS.** Every case run in the primary matrix and in the supplementary soak passed:
no ACKed record was lost, no duplicate row or duplicate admission occurred, and every reopen
found the qualified engine with WAL and `synchronous=FULL`.

- Journal primitives: commit `1e38b20` (`crates/journal`).
- Runner: commit `c62878e` (`tests/native/journal-crash`), built in the debug profile from that
  commit's tree and run as recorded below.
- Durability domain: **ordinary process crash only.** `fullfsync` is off. Power-loss durability
  is **not** qualified here and remains with M13, unchanged.

## What the runner does

`threadspace-journal-crash run` re-executes itself as worker processes. A worker is the real single
writer: it holds `WriterLock`, opens `Journal`, admits deterministic fixture records through
`Journal::admit_observation`, and prints each receipt as a JSON line only after the journal
returned it. That printed line is the ACK. The worker dies at a journal crash point, or the
parent `SIGKILL`s it right after reading an ACK. The parent then acquires the lock the dead
worker held, reopens the journal, checks the engine and connection policy, and asserts.

Each store lives in its own directory under a per-run root in `/private/tmp/claude-501/`, which
the runner creates and removes at the end (`--keep` retains it). After both runs, no runner
directory remained there.

## Cases and what each proves

| Case | Primary / soak runs | What it proves |
| --- | --- | --- |
| `before-transaction` | 25 / 250 | Crash point before the admission transaction begins. Exactly `k-1` ACKs; record `k` absent after reopen; every ACKed record present at its ACKed cursor. |
| `in-transaction` | 25 / 250 | Crash point after the INSERT ran inside the IMMEDIATE transaction, before COMMIT. Record `k` absent; every ACKed record present. For these small records, the WAL held 0 uncommitted frames at the crash (`walAtCrash`). This proves that a writer killed holding an open write transaction leaves nothing visible and does not block recovery. |
| `in-transaction-spill` | 5 / 10 | The same point with a 3 MiB payload, so SQLite spills the open transaction to the WAL before COMMIT. In every run the WAL held 339 frames of its current generation with no commit frame after them when the writer died. After reopen the record is absent, and re-delivery commits it once. |
| `after-commit-before-receipt` | 25 / 250 | Crash point after COMMIT returned, before the receipt reached the caller. Record `k` present although it was never ACKed; its retry returns `ALREADY_COMMITTED` at its committed cursor. |
| `after-receipt` | 25 / 250 | The parent `SIGKILL`s the worker as soon as it reads ACK `k` (a helper crash after ACK). Every ACKed record survives; records never ACKed may or may not exist. |
| all four above | | After restart, every attempted record is re-delivered twice. ACKed records return `ALREADY_COMMITTED` with the original cursor on both passes; the table then holds exactly one row per attempted record. Records `k+1..n` are never present when the crash came first. |
| `wal-replay` | 5 / 10 | From a checkpointed baseline, a worker commits 40 records and is killed with 506,792 WAL bytes on disk. A copy of the main file alone holds 0 of them. Reopen replays all 40. `checkpoint_truncate` returns `busy=false` and leaves a 0-byte `-wal`, and a copy of the main file alone then holds all 40 at the same cursors. |
| `writer-lock` | 5 / 10 | While a worker holds the lock, a second in-process `WriterLock::acquire` gets `LockError::Held`, and a second writer process exits 75 (`lock-held`). `writer.lock` names the holder's PID. After the holder is `SIGKILL`ed the parent acquires the lock and reopens. A writer process is then refused while the parent holds the lock. |
| `backup-concurrent` | 5 / 10 (8 backups each) | `backup_store_into` runs through a separate read-only connection while a worker commits continuously. Every backup verifies, and re-verifies with the same sha256. Every backup contains every record ACKed before it began. Each holds exactly the committed rows through its cursor (no gap, no extra row). Commits completed during every one of the 40 / 80 backups. |
| `restore` | 5 / 10 | The parent commits 20 records and calls `Journal::backup_into`; a worker commits 15 more and is killed (`-wal` and `-shm` present). `restore_backup` installs a file whose sha256 equals the backup's. It preserves `journal.sqlite3`, `-wal` and `-shm` byte-identical in `recovery-original-<ms>/` and leaves no `-wal` or `-shm` beside the restored database. The reopened store holds exactly the 20 backup rows, with the same store generation and cursor. The preserved original, opened with its WAL, still holds all 35 ACKed records. |
| `restore-rejected` | 5 / 10 (7 variants each) | Each variant is rejected: an empty file, a non-SQLite file, a file truncated mid-page, a file missing its last page (interrupted copy), corrupt b-tree page headers, one corrupt page caught by `integrity_check`, and a byte copy of the live main file while the WAL holds commits. The original `journal.sqlite3`, `-wal` and `-shm` keep the same sha256. The store directory listing is unchanged (no recovery directory, no staged file), and the original then reopens with every ACKed record. |
| `restore-without-lock` | 5 / 10 | While a worker holds the store's lock, the parent cannot acquire it, so it cannot call `restore_backup`. A lock for another directory gets `BackupError::LockNotHeld`. The original's hashes are unchanged either way. |
| engine | every reopen | `sqlite_version()` 3.53.4, source ID `2026-07-24 19:02:57 bf7c7f30…09459bcc`, `journal_mode=wal`, `synchronous=2`, `foreign_keys=1`, `fullfsync=0`. This held on 210 reopens in the primary run and 1,220 in the soak. |

The unit tests in `crates/journal/tests/durability.rs` cover the same primitives in-process. They
also cover rejection cases the runner cannot build without a direct SQLite connection: a future
schema migration row, a backup record naming another engine, a missing backup record, and a
backup record whose cursor disagrees with the rows. The crash-point test re-executes the test
binary as a child for each point, armed both by setter and by environment.

## Results

| Run | Seed | Crash runs | ACKed | Lost ACKed | Duplicate rows | Duplicate admissions | Unacked present | Fixed runs | Reopen checks |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| primary (`summary.json`) | 1831887632 | 100 (25 per point) | 1,458 | **0** | **0** | **0** | 25 | 35, all pass | 210 |
| soak (`soak/summary.json`) | 20261006 | 1,000 (250 per point) | 15,094 | **0** | **0** | **0** | 251 | 70, all pass | 1,220 |

"Unacked present" counts committed records that were never ACKed. The deterministic
`after-commit-before-receipt` point produces one per run. The soak's extra one is an
`after-receipt` run where the `SIGKILL` landed between a commit and its ACK. In the primary run,
every `after-receipt` kill landed before the next commit, so that window is covered
deterministically by `after-commit-before-receipt` rather than by kill timing.

The fixed cases ACKed a further 17,895 (primary) and 37,106 (soak) records, with none lost.
Of these, `backup-concurrent` ACKed 17,305 and 35,926.

## Journal API added (`crates/journal`, additive)

~~~rust
pub struct ObservationAdmission<'a> { observation_id, source_id, source_epoch, source_sequence: Option<&str>, native_event, captured_wall_ms: i64, payload: &serde_json::Value }
pub struct AdmissionReceipt { pub observation_id: String, pub status: ReceiptStatus, pub cursor: i64 }
impl Journal { pub fn admit_observation(&mut self, observation: &ObservationAdmission<'_>, now_ms: i64) -> Result<AdmissionReceipt, JournalError> }

impl Journal { pub fn backup_into(&self, dir: &Path, now_ms: i64) -> Result<BackupInfo, BackupError> }
pub fn backup_store_into(db_path: &Path, dir: &Path, now_ms: i64) -> Result<BackupInfo, BackupError>
pub fn verify_backup(path: &Path) -> Result<BackupInfo, BackupError>
pub fn restore_backup(lock: &WriterLock, db_path: &Path, backup: &Path, now_ms: i64) -> Result<RestoreOutcome, BackupError>
impl Journal { pub fn checkpoint_truncate(&mut self) -> Result<WalCheckpoint, JournalError> }

// qualification feature only
pub enum CrashPoint { BeforeTransaction, InTransaction, AfterCommitBeforeReceipt }
pub struct CrashPlan { pub point: CrashPoint, pub at_admission: u64 }
impl CrashPlan { pub fn from_env() -> Result<Option<Self>, JournalError> }
impl Journal { pub fn set_crash_plan(&mut self, plan: Option<CrashPlan>); pub fn admitted_observations(&self, source_id: &str) -> Result<Vec<(String, i64)>, JournalError> }
pub fn file_sha256(path: &Path) -> io::Result<String>
pub const CRASH_POINT_ENV = "THREADSPACE_QUALIFY_CRASH_POINT"; pub const CRASH_AT_ENV = "THREADSPACE_QUALIFY_CRASH_AT";
~~~

- **Admission** canonicalises the UUID (lowercase, hyphenated) and uses one IMMEDIATE transaction.
  A duplicate with identical source, epoch, sequence, event, capture time and payload returns
  `ALREADY_COMMITTED` and the original cursor, and writes nothing. Reusing a UUID for different
  content is `JournalError::Conflict`. The receipt is returned only after COMMIT.
- **Crash points** exist only with the `qualification` feature. A plan is armed by
  `set_crash_plan` or by the two environment variables, which `Journal::open` reads under the
  feature. The crash sends `SIGKILL` to the process and parks the crashing thread in `pause(2)`.
  Like `abort()` this runs no unwinding, destructors or `atexit` handlers, but it produces no
  macOS crash report per death. `SIGKILL` replaced `SIGABRT` after attempt 01.
- **Backup**: the SQLite online backup API copies one read-transaction snapshot into a temp file
  in the target directory, never a copy of the live main file. The copy is switched to
  `journal_mode=DELETE`, a self-contained file. It gets a backup record in its own `store_meta`
  (`backup_cursor`, `backup_schema_version`, `backup_sqlite_version`, `backup_sqlite_source_id`,
  `backup_created_at_ms`). It is then verified, fsynced and renamed to
  `backup-<cursor>-<now_ms>.sqlite3`, and the directory is fsynced.
- **Verification** checks, in order: the header (magic, page size, version bytes 18/19 = 1,
  in-header page count × page size = file length); that it opens read-only under the linked
  3.53.4 engine; `integrity_check`; this binary's exact migrations and checksums; the store
  generation; that the backup record was written by 3.53.4 with that source ID; and that the
  recorded cursor equals `MAX(ingest_seq)`.
- **Restore** requires the caller's `WriterLock` to be the store directory's lock, and every
  `Journal` on the path closed. It copies the backup beside the database and verifies that
  staged copy, so any failure up to this point leaves the original untouched. It then
  hard-links the original database and its `-wal`, `-shm` and `-journal` into
  `recovery-original-<now_ms>/` and fsyncs that directory. It removes the side files (SQLite
  would replay a stale `-wal` onto the restored database) and renames the verified copy over
  the database path. The database path is never absent.

## Commands

All commands were run in the worktree at `c62878e`, debug profile. The checkout root is shown as
`<repo>` in the logs.

~~~sh
cargo fmt -p threadspace-journal -p threadspace-journal-crash -- --check        # logs/fmt.log (cargo fmt --all also clean)
cargo clippy -p threadspace-journal -p threadspace-journal-crash --all-targets \
  --features threadspace-journal/qualification -- -D warnings                  # logs/clippy.log
cargo test -p threadspace-journal --features qualification                      # logs/test-qualification.log: 27 passed
cargo test -p threadspace-journal                                               # logs/test-default.log: 23 passed
cargo check -p threadspace-agent --features qualification && cargo check -p threadspace-agent   # logs/agent-check.log
cargo run -q -p threadspace-journal-crash -- run --out evidence/M0C/sqlite      # results.jsonl, summary.json, logs/runner.*
cargo run -q -p threadspace-journal-crash -- run --out evidence/M0C/sqlite/soak \
  --runs 250 --fixed-runs 10 --seed 20261006                                    # soak/
~~~

`logs/crash-points-absence.log` counts the crash-point environment name in every journal test
executable. It appears 0 times in each non-qualification executable and 1–2 times in each
qualification integration-test executable. The library's own unit-test harness calls no
journal code and holds it 0 times in both builds. `logs/sync-all-disassembly.log` shows std
`File::sync_all` on this target calling `fcntl(fd, 0x33)`, which is `F_FULLFSYNC`.

## Environment

- macOS `sw_vers`: product 27.2, build 26B5091g; `arm64`; Darwin kernel 27.2.0 (from `summary.json`).
- SQLite 3.53.4 linked through the patched `libsqlite3-sys` (`docs/decisions/D-0001`), source ID
  `2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc`.
  The compile options are listed in `summary.json` (`engine.sample.compileOptions`).
- Stores are on the local APFS volume under `/private/tmp/claude-501/`.

## Durability domain and limits

- **Process crash, not power loss.** Every crash here is process death (`SIGKILL`). The kernel,
  the page cache and the filesystem keep running. Journal commits use WAL with
  `synchronous=FULL` (`fsync`) and `fullfsync` **off**. Power-loss, OS-crash and storage-failure
  durability are **not qualified** (M13), unchanged.
- Backup files and their directories are flushed with `File::sync_all`, which issues
  `F_FULLFSYNC` here. That makes publication of a backup flushed, but it is not a power-loss
  qualification either.
- `integrity_check` detects structural corruption. A payload bit flip inside a record is not
  detectable without page checksums. `BackupInfo.sha256` lets a caller that recorded it at
  backup time detect any byte change.
- The restore steps are ordered so that a crash never leaves a stale WAL beside the restored
  database. A crash between removing the original side files and the final rename would leave
  the original main file without its WAL: an older committed state, not corruption. The
  complete original remains in `recovery-original-<ms>/`. Crash injection inside restore is
  not part of this qualification.
- Restore cannot detect an open `Journal` on the same path in the caller's own process. The
  writer lock excludes other processes.
- A backup holds one read transaction for its copy. A whole `backup_store_into` call, including
  verification and flushes, took 11–80 ms for the ≤3,300-row stores in the primary run. A long
  reader delays WAL checkpointing (SPEC §9.1).

## Files

- `results.jsonl`: one line per case run (primary).
- `summary.json`: totals per case and for the crash matrix, engine sample, environment and
  violations (primary).
- `soak/`: the same for the soak run.
- `logs/`: raw output of every command above.
- `attempts/`: failed attempts, kept rather than deleted.
