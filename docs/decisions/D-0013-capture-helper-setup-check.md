# D-0013 — Capture-helper self-check at setup

**Status:** ACCEPTED by the owner on November 5, 2026 (EST).
**Affects:** SPEC §8.3 capture helper budgets; MILESTONES M2 capture-budget control;
`crates/provider-claude/src/setup/mod.rs`; `threadspace-hook`.

## Context

Every capture runs the staged helper as a new process. The helper's own work takes
about 3 ms. macOS adds a cost before the helper's first instruction on the first
execution of each new executable file (about 80 ms for the 1.7 MB helper) and a
one-time assessment of never-seen executable content (measured up to 332 ms, and
618.911 ms in the retained compact-helper comparison). No application code bounds time
spent before `main`; see the
[cold-start diagnostic](../../evidence/M2/helper-cold-start-cause/README.md).

## Decision

Setup runs the staged helper once, `threadspace-hook self-check`, immediately after
writing it and before any provider configuration names it. The helper answers exactly
`threadspace-hook self-check ok` and does nothing else. A helper that does not answer
within 10 seconds fails setup with `HELPER_UNUSABLE`, and no settings, session file or
install record is written. The install record keeps the check's elapsed milliseconds.

The check proves the staged file runs, and it moves macOS's first-execution assessment
of the new file into setup. The helper's 250 ms application wall budget and p95 ≤25 ms
target apply to its captures, which all follow that check. The original subprocess
control runs the same self-check before its timed capture, exactly as setup does; its
threshold and its receipt, spool and UUID assertions are unchanged.

## Consequences

- A capture never pays the first-execution assessment of a helper that setup installed.
- A helper replaced outside setup (for example by hand) is not covered; setup is the
  only supported way to stage one.
- Historical cold-start failures remain retained as recorded; none is relabeled.
