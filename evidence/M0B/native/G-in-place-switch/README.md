# M0B native test G: in-place /clear and /resume (A→B→A), currentness

**Result: PASS.** The installed inventory reports the *current* conversation of a direct interactive process, so `currentSessionProcessLookup` is qualified for Claude 2.1.291.

All steps ran in one Claude process, pid 55002 on ttys004:

1. **A** (`0d750db7`, canonical `5c4f593b…`) received a prompt, then the owner ran `/clear`.
   - The hook saw `SessionStart source=clear` for **B** (`670bc29d`) at 1791276253978.
   - The companion pass committed at 1791276256060 (2.1 s later, cursor 144). It ended A's activation with `SESSION_SWITCHED`, using a bracketed join of pid 55002 to B. It started B's activation and bound B to ttys004 by `st_rdev`.
2. Returns after the switch:
   - Return to stale A: `INSPECTOR_ONLY / UNBOUND / NO_LIVE_MAPPING`, **no focus**; the selected tab was unchanged (`04`–`06`).
   - Return to B: exact / current / foreground, 922 ms (`07`).
3. The owner ran `/resume` and chose A.
   - While the picker was open the inventory reported B as `waiting`.
   - The hook saw `SessionStart source=resume` for A at 1791276362415. The companion committed at 1791276365301 (2.9 s later, cursor 151). It ended B's activation with `SESSION_SWITCHED` and started **A activation 2**, a new activation rather than a revival, bound at rev 151.
4. Returns after the second switch:
   - Return to stale B: `INSPECTOR_ONLY / UNBOUND / NO_LIVE_MAPPING`, **no focus**, selection unchanged (`09`–`11`).
   - Return to A: exact / current / foreground, 857 ms, on the activation-2 binding (`12`).
5. Final state (`13`): A is act 2, LIVE, CURRENT. B persists as ENDED/STALE. Both record their last invalidation as `SESSION_SWITCHED`.

Detection latency is bounded by the 5 s discovery interval. A route in that window would still re-check currentness with its own fresh lookup before focusing.
