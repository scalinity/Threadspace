# Superseded attempt: macOS idle sleep consumed a scheduled wake

Build `8c82212`, harness with the cursor fix. All five cycles reported
`pass: true`, but the schedule did not do what the gate intends
(`os-power-log.txt` is the macOS power log for the window):

- 16:32:50 `Idle Sleep` (not the harness) while cycle 2 was waiting for its
  slot, ended by the 16:35:14 scheduled wake. That wake was counted for
  cycle 2, which recorded a 2 s gap.
- Every later harness sleep (`Software Sleep`) therefore ended on the next
  wake in the list, and the fifth sleep, at 16:47:33, had none left. It ended
  at 16:59:12 on a multi-touch wake when the owner logged in (gap 704 s).
- The harness could not signal completion while the Mac was asleep.

The product's state checks held through every sleep and wake, but the run is
not accepted as G12 evidence: one cycle was a mis-attributed idle sleep and
one wake was not scheduled. The harness now holds a `caffeinate -i`
assertion until just before each `pmset sleepnow`, sleeps into the first
wake at least 90 s ahead, and requires a sleep of at least 30 s with the
OS-recorded wake within 20 s of its scheduled time (commit `df4c75e`).
