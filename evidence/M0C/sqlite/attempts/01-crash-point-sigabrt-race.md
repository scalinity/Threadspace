# Attempt 01: crash point died by SIGABRT instead of SIGKILL

**Status:** failed, fixed before commit `1e38b20`; kept as the record of the failure.

## What ran

`cargo test -p threadspace-journal --features qualification --test durability`, test
`crash::crash_points_kill_the_process_at_their_boundary`. It re-executes the test binary
as a child that arms a crash point and admits three records, then asserts that the child
died by `SIGKILL` at the armed boundary.

## What failed

One of four consecutive full-suite runs failed. The raw log was not saved; the assertion
below is copied from the terminal output:

~~~text
thread 'crash::crash_points_kill_the_process_at_their_boundary' panicked at crates/journal/tests/durability.rs:251:17:
assertion `left == right` failed: after-commit-before-receipt via setter: child must die at the crash point
  left: Some(6)
 right: Some(9)
~~~

## Cause

The first implementation of the crash was `kill(getpid(), SIGKILL)` followed by
`std::process::abort()` as an unreachable fallback. A process-directed signal can be
taken by a thread other than the caller (the libtest child has a runner thread and a test
thread), so `kill(2)` can return before the process is torn down. The caller then reached
`abort()` and the process died by `SIGABRT` (6). The commit/receipt boundary itself was
not violated: the child still printed no receipt for the crashing record.

## Fix

After sending `SIGKILL`, the crashing thread blocks in `pause(2)` in a loop and never
continues (`crates/journal/src/crash.rs`, `die`). It cannot return a receipt, run
destructors or reach `abort()`.

## Verification after the fix

- The crash-point test passed 30 consecutive times (`cargo test -q -p threadspace-journal
  --features qualification --test durability crash::`), six child crashes per run.
- The runner's crash matrices then recorded death by signal 9 in every crash-point run:
  100 runs in `../results.jsonl` and 1,000 in `../soak/results.jsonl`.
