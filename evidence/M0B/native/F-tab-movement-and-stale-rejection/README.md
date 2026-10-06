# M0B native test F: tab movement, close/recreate, natural TTY reuse

**Result: PASS**, with one recorded readback-criterion correction.

**Tab movement.** The owner reordered tabs and used *Move Tab to New Window*.

- On this macOS, Terminal tabs are native window tabs, and AppleScript models each one as its own `window` with a single tab. Window IDs survived the moves; tab ordinals are always 1 (`01`).
- Fresh enumeration found every target, and routes to all three test sessions were exact (`03-*`). Stale positional hints are never consulted. Their permutation is covered by the synthetic `a_moved_tab_is_found_by_fresh_enumeration`.

**Readback correction.** In the first route after the move (`03-return-after-move-a52de5c2.json`) Terminal reported `frontmost of targetWindow = false`, although the target was AppleScript's front window with its tab selected. The owner reports possibly clicking another window, and an independent readback a second later showed another window in front.

- The route had nonetheless reported exact. It is counted in the failure ledger.
- Two hands-off reruns were exact and independently confirmed (`04-rerun-*`).
- Commit `fc65641` now requires every readback signal to agree, including that flag. It was true on all other 42 recorded native routes.

**Close and recreate** (`05`–`10`).

- The owner closed `efa96221`'s tab (pid 54698 exited). The companion ended its activation and invalidated the binding (`PROCESS_EXITED`).
- The owner opened a new window. It received **`/dev/ttys002` again, with the same `st_rdev` 0x10000002** as the old binding: natural TTY path and device reuse.
- A Return to the closed session gave `INSPECTOR_ONLY / UNBOUND / NO_LIVE_MAPPING` with **no focus**, and the selected tab was unchanged. The old Session persists ENDED/STALE with no binding.
- The reused TTY is only a locator; without the ended ProcessKey it carries no route authority.
