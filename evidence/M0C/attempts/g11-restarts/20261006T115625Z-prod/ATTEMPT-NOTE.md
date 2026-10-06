# Stopped attempt: the harness deadlocked on its own GUI lock

Build `a6ef67a`. Cycle 1 (`ui`) took the machine-wide GUI lock
(`/private/tmp/mac-gui-automation.lock`, label `g11 cycle 1 ui`), stopped
the UI and called `ensure_ui`, which acquired the same lock again. `flock`
locks belong to an open file description, so the second acquire in the same
process waited on the first; a process sample showed the runner blocked in
`GuiLock::acquire` under `ensure_ui` for 22 minutes, with the companion
alive and idle (PID 97889, same incarnation). Nothing was written for the
cycle, and the runner was stopped.

The product was not exercised past stopping the UI. The lock is now
re-entrant on one thread (commit 8e5481c), and G11 was rerun.
