# M0B native test E: resume elsewhere and simultaneous attachment

**Result: PASS.**

1. With `f8cd39d3` (canonical `ea8e75e4…`) still running in ttys005 (pid 55189), the owner opened a new tab and ran `claude --resume f8cd39d3-…`. **Claude 2.1.291 accepted the simultaneous attachment.** Its inventory listed **two** live interactive processes for the same session ID: 55189 (ttys005) and 27964 (ttys007). The hook recorded `SessionStart source=resume` from 27964 (`01-inventory-raw.json`).
2. Threadspace kept **one Session with two live bindings** (`03`, `liveBindings 2`).
3. A plain Return gave `AMBIGUOUS / UNBOUND / MULTIPLE_ATTACHMENTS`, offered both choices and sent **no focus**; the selected tab was unchanged (`04`–`06`). Each explicit choice routed exactly to its own tab (`07-…-8ccd273d`, `07-…-5110e97b`). The fresh lookup in each route showed 2 session rows.
4. The owner ran `/exit` in the original tab. The kernel reported pid 55189 gone, and the periodic pass ended that activation and invalidated its binding (`PROCESS_EXITED`).
   - The Session is unchanged and its native ID is unchanged.
   - It now has exactly one live attachment: activation 2, ProcessKey pid 27964, surface ttys007, binding rev 157 (`10`).
   - A plain Return goes straight there, exact / current / foreground (`11`).

Prior route authority was retired only on proven exit. The newest attachment was never chosen automatically.
