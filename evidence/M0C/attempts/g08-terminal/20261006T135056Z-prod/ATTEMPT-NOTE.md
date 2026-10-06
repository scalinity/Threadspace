# Attempt: two cases whose setup never happened

Build `5a6149a`. Eight of eleven cases passed with no wrong target
(baseline, reorder, move, fullscreen Space then return, five-route
selection/readback race, target closed, stale TTY pathname, target closed
during the route); Terminal incarnation unchanged.

- `minimize-then-return`: the window was looked up by its `TSQ-` title, but
  Claude Code sets its own terminal title, so no window matched, nothing
  was minimized, and the exact route proved nothing. The case now minimizes
  and reads back by the recorded window ID through Terminal's
  `miniaturized` property.
- `foreground-process-mismatch`: the session ran as `exec claude`, so no
  job-control shell sat behind it; its process group was orphaned and the
  kernel discarded `SIGTSTP`. Claude stayed in the foreground, the route was
  correctly FOREGROUND_COMPATIBLE, and the follow-up `fg` reached the
  disposable Claude session as a prompt (one model turn in that throwaway
  session). The case now runs its session as a job of the window's shell,
  stops it with `SIGSTOP`, reads the terminal's foreground process group,
  and types `fg` only when the shell holds the foreground (otherwise it
  sends `SIGCONT`).
- `terminal-restart`: relabelled BLOCKED: quitting Terminal ends every
  Terminal session on the Mac, including unrelated owner sessions.
