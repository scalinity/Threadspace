# Attempt: one exact claim the independent readback could not confirm

Build `8c82212`. Nine cases passed, including `minimize-then-return`
(minimized and restored by recorded window ID) and
`foreground-process-mismatch` (Claude stopped as a shell job, the shell
holding the terminal's foreground, the route not FOREGROUND_COMPATIBLE).
Terminal restart is BLOCKED.

In `selection-readback-race` (a spare window raised 450 ms into each route)
route 1 returned exact after the companion's own readback succeeded
(864 ms); the harness's single independent readback 300 ms later failed
(`tty of selected tab of front window` returned an error), and the scoring
counted any exact route without a matching readback as a wrong target.
Routes 2 to 5 were READBACK_FAILED in the companion and unreadable in the
harness alike. The previous build's run of this case passed only because
all five routes were READBACK_FAILED. Outside the race the same query reads
the front window's TTY at once.

The harness now retries its independent readback briefly, records the
AppleScript error when it cannot read, and separates a wrong target (a
different TTY read back) from an unverified exact claim (nothing read back).
The case passes only with neither.
