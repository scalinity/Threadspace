# M0B native test A — late-start discovery

**Result: PASS (discovery, process join, controlling device and surface join).** Focus/Return is not part of this test; it runs in B/C.

**Artifacts:** installed `~/Applications/Threadspace.app` built from `46a2a04` (clean tree). Companion executable sha256 `32d257de…c384e0`, outer `3be48770…ffa45c0` (`05-installed-hashes.txt`). Claude CLI 2.1.291 at `~/.local/share/claude/versions/2.1.291`.

## Sequence (2026-10-06, America/New_York)

1. The owner opened a new Terminal window with three tabs and ran `cd ~/Documents/Tools/ts-m0b-samecwd && claude` in each. No prompt was sent. The sessions were born at 04:05:26, 04:05:31 and 04:05:33 (`01-pre-restart-ps.txt`).
2. The M0A production companion (pid 99986) was unregistered and exited (`02-unregister-old.json`). The new bundle was installed and registered (`03-register-new.json`).
3. The new companion, pid 56092, started at 04:06:50, after all three sessions (`04-new-companion-ps.txt`). It migrated the journal 1→2 on SQLite 3.53.4, source ID `2026-07-24 19:02:57 bf7c7f30…`, keeping the M0A store generation `f3eefe4f…`. Its startup pass ran without any provider hook (`08-diagnostics.json`).
4. Startup discovery pass (`07-export.json`, observation cursor 24):
   - Two bounded inventory lookups, 171 ms and 178 ms, each returned 7 rows.
   - All 7 interactive rows joined. For each, the incumbent and post-lookup kernel samples agree on PID, birth (s.µs), executable and `e_tdev`.
   - The surface join found exactly one Terminal tab per `e_tdev`, so all 7 were bound. Nothing stayed provisional.
5. The persisted projection is in `06-snapshot.json`. The three test sessions are three Sessions with distinct canonical IDs, ProcessKeys and bindings:

| native session | pid | kernel birth | e_tdev | Terminal tab |
| --- | --- | --- | --- | --- |
| `efa96221…` | 54698 | 1791273926.140544 | 0x10000002 | /dev/ttys002 |
| `0d750db7…` | 55002 | 1791273931.431354 | 0x10000004 | /dev/ttys004 |
| `f8cd39d3…` | 55189 | 1791273933.567057 | 0x10000005 | /dev/ttys005 |

6. Independent check (`09-…`, `10-independent-binding-check.json`): the harness, not the companion, re-enumerated Terminal read-only, stat'ed each tab TTY and read each process's `e_tdev` and birth with `proc_pidinfo`. All 7 bindings agree. Every tab's `st_dev` is the same devfs value, while each `st_rdev` is distinct, which shows that only `st_rdev` can be the join.

## Why this does not rely on a hook

The companion has no hook ingestion path in M0B. The test directory's hook probe writes only to a local evidence file (`../hook-ancestry/`), so discovery used only the native inventory and kernel/Terminal evidence.

## Notes

- Session identity needed no prompt: freshly launched sessions report a full `sessionId` immediately.
- Display labels of the owner's four other live sessions are redacted in `06-snapshot.json`; home paths are written as `~`.
- Each inventory call costs about 0.14 CPU-seconds and a transient 146 MB process. Two calls per 5 s pass average roughly 5–6 % of one core, which is above the SPEC §20.2 idle-companion target that M13 owns. This is recorded as a limitation, not tuned here.
