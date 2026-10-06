# Failed attempt: the discovery check opened its log cursor after the wake

Build `8c82212`, harness `0c24b4f`. Ten of eleven checks passed in all five
cycles; `discoveryAfterWake` failed in all five. The companion log shows a
`DISCOVERY_PASS` 0.2–0.3 s after every `OBSERVER_RESUMED_AFTER_WAKE`
(the forced refresh on `DID_WAKE`), so the product behaved correctly. The
harness created its log cursor after the 12 s post-wake pause, and a cursor
starts at the end of the file, so it could never see the line.

Cycle 2 slept 4 s and woke well before its scheduled wake. The macOS power
log for this window was no longer retained, so the cause is not proven here;
the next attempt shows the same cycle-2 anomaly traced to an OS idle sleep.

The cursor is now created before `pmset sleepnow` (commit `df4c75e`).
