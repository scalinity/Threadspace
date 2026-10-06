# M0B native test H: unsupported case, no guessed focus

**Result: PASS.**

**Unsupported surface.** The owner ran `script -q /dev/null claude` in a Terminal window (`ttys002`).

- Claude (pid 95964, session `937e323e`) has controlling terminal **`ttys008`**, the inner pty, while the Terminal tab belongs to `script` (`03-ps.txt`).
- The process join is valid (direct interactive, qualified executable), but no Terminal tab carries ttys008's device. The session stays unbound with `NO_MATCHING_TAB` (`04`, `05`).
- Return gave `INSPECTOR_ONLY / UNBOUND / UNKNOWN / NO_MATCHING_TAB`, **no focus**. The selected tab and frontmost app were unchanged (`06`–`08 *c9d97eb2*`).

**No native surface.** The M0A fixture session's Return gave `INSPECTOR_ONLY / UNBOUND / NO_NATIVE_SURFACE`, **no focus**, selection unchanged (`*5bd921fd*`).

No cwd, recency, frontmost or "only candidate" fallback was used.

**Stale provider rows after abrupt exit** (`09`). The script-wrapped test Claude was killed with SIGKILL. The CLI inventory dropped its row within 0.16 s and never re-listed it, so the CLI validates process liveness itself. The companion ended the activation (`PROCESS_EXITED`); the Session persists as ENDED/STALE.
