# M1 C-13 — companion log framing

**Result: CLOSED.** Each companion log record and its newline go to the file in one write. A
torn record left by a killed predecessor is ended on its own line when the next process opens
the log. The harness reader parses one record per complete line and counts the lines it skips.
In 720 SIGKILLed writers across three runs, no physical line held more than one record, and every
record written after a kill parsed. Diagnostics only: product correctness does not depend on this
log.

## Defect

`evidence/M0C/failure-ledger.md`, row C-13: a companion log record and its newline are written
separately, so a companion killed between them leaves the next process's first record on the
same physical line. In the accepted `apps/agent-macos/core/src/log.rs` (`cd9e376`), `event()`
called `writeln!(file, "{text}")`. `std::fs::File` is unbuffered, and `write_fmt` issues one
`write` for the record and another for the newline.

## Repair

- `apps/agent-macos/core/src/log.rs`
  - `event()`: the record text gets its `\n` appended in the same `String`, and the whole buffer
    goes out with one `write_all` on the append-mode file. One `write` of a small buffer narrows the
    window for a torn record to an interrupted or partial write. It does not make the log
    crash-atomic storage, and the module doc says so. The stderr fallback, used before `init`, is
    unchanged.
  - `init()`: the log is opened `read + append`. A new `ends_mid_record()` reads the last byte with
    `pread`. If the file is non-empty and that byte is not `\n`, one `\n` is written before the
    handle is installed, so a predecessor's torn record ends on its own line and can never join
    this process's first record. `log::init` runs before the writer lock is taken (`lib.rs`
    `start()`), so a refused second companion also runs this check. Because each record now
    arrives in one `write`, that opener sees a complete line end, not a torn one.
- `tests/native/harness/src/companion.rs` `LogCursor`
  - `read_file()` reads bytes with `read_until(b'\n')`, not `read_line`. Each complete line is
    parsed as exactly one JSON value. An object is handed out; any other complete line (a torn
    record, two records sharing a line, a non-object, invalid UTF-8) is skipped and counted. A
    final line without its newline is never consumed, so its offset is not advanced. Reading
    bytes also fixes a stall: before, a line torn inside a UTF-8 sequence made `read_line` return
    `Err`, and the cursor stopped at that line for good.
  - New `torn_records()` returns the count. `at_end`, `read_new` and `wait_for` keep their
    signatures and behaviour for complete, well-framed lines.
  - Behaviour change for logs written by a pre-repair companion: a line holding two records joined
    by a kill (`b757c75` split these into both records) is now one skipped, counted line. A
    repaired companion cannot produce such a line. A runner re-run against the M0C companion build
    (`fd02d6a`) would count a joined line in `torn_records()` instead of recovering both records.

## Tests

### Kill test — `apps/agent-macos/core/tests/log_framing.rs`

No app bundle is built. `log` is a private module of the crate (`lib.rs`, outside this repair's
ownership), so the test compiles the companion's own source with `#[path = "../src/log.rs"]` and
drives the real `log::init` and `log::info`. The test binary re-executes itself as writer
processes (the `crates/journal/tests/durability.rs` pattern): `writer_child` returns at once
unless `THREADSPACE_LOG_FRAMING_CHILD_DIR` is set.

`records_never_share_a_line_across_a_killed_writer` runs 240 rounds, each in its own temporary
directory, removed afterwards:

1. A writer initialises the log, prints `READY` and logs probe records in a tight loop. Each
   record is a flat JSON object with `tag`, `seq` and a pad of 0–672 bytes, so kills land at
   varied offsets. The writer stops by itself after 3 s if it is never killed.
2. The parent sends `SIGKILL` (`Child::kill`) at a varied delay. In 30 rounds (every 8th) it kills
   right after spawn, during process start-up, before or while the log opens. In the other 210 it
   waits for `READY`, then sleeps `round × 211 mod 2500` µs, or 10 ms in every 16th round. It
   asserts that the writer died by `SIGKILL`.
3. In every 5th round (48 rounds), if the file does not already end mid-record, the parent
   appends a prefix of a probe record with no newline. This stands in for a writer killed partway
   through a write, a case the repaired writer did not produce naturally in any run.
4. A second writer initialises the log and appends 25 records, then exits normally.
5. The file is parsed by physical line. Probe records are flat, so `{` appears only where a record
   starts. The test asserts:
   - the file ends with `\n`, and no line is blank;
   - **no line holds more than one record start** (a joined line);
   - every complete line is either one valid record, in `seq` order for its writer, or a torn
     line. A torn line counts only if the next line is the second writer's `seq 0` record, which
     shows the torn record ended on its own line;
   - **all 25 post-kill records parse**, in order;
   - every planted torn record is found.

### Reader unit test — `companion::tests::reads_one_record_per_complete_line_and_counts_torn_lines`

A record still being written is not consumed. The next process's newline then ends it. A
torn line, two records on one line, `[1,2]`, and a line torn inside a UTF-8 sequence are each
skipped and counted (4). Reading continues past all of them, and the held-back record is handed
out once its newline arrives.

## Commands and observed results

| Command | Result |
| --- | --- |
| `cargo test -p threadspace-agent --test log_framing -- --nocapture` (run 3 times) | **2 passed, 0 failed** each run (about 2.2 s) |
| `cargo test -p threadspace-agent` | **25 passed, 0 failed**: 23 lib unit tests, 2 in `log_framing`; 0 doc-tests |
| `cargo test -p threadspace-harness` | **2 passed, 0 failed**: lib unit tests including the new reader test; 0 in the `threadspace-m0c` bin; 0 doc-tests |

No compiler warnings. Kill-test counts per run:

| Run | Rounds | Confirmed SIGKILLs (at spawn / after READY) | Lines | Killed-writer records | Post-kill records parsed | Torn planted | Torn lines on their own line | Joined lines | Violations |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 240 | 240 (30 / 210) | 37,481 | 31,433 | 6,000 / 6,000 | 48 | 48 | 0 | 0 |
| 2 | 240 | 240 (30 / 210) | 37,108 | 31,060 | 6,000 / 6,000 | 48 | 48 | 0 | 0 |
| 3 | 240 | 240 (30 / 210) | 38,256 | 32,208 | 6,000 / 6,000 | 48 | 48 | 0 | 0 |

Across 720 kills, the repaired writer left no natural torn record: every torn line was a planted
one. A `write` to a local regular file was not observed to be cut short by `SIGKILL`; that is an
observation here, not a guarantee. The planted rounds exercise the `init()` newline and the
reader's skip path directly.

**Control.** The same kill test, with the accepted M0C `log.rs` (`cd9e376`) temporarily swapped in
and restored afterwards (sha1 of the repaired file `89a12e76…` before and after), **failed**
with 2,060 violations, the first being `round 2: line 29 holds 2 record starts`. It found 78
joined lines:

- 46 came from kills that landed between a record and its newline, so the next writer's first
  record shared the line. This is C-13 itself.
- 32 came from the planted torn records. All of them joined the next record, because the old
  `init()` has no newline guard. Only 32 rounds were planted, not 48, because in 16 eligible
  rounds the kill had already left the tail mid-record.

Each round with a joined line lost its post-kill sequence: 4,050 of 6,000 records parsed, which
is 162 intact rounds × 25. The test therefore detects C-13.

`test-output.txt` holds the three kill-test runs, both crate runs, and the control run, each
under the command that produced it. Not run: `cargo clippy` and app bundle builds.
