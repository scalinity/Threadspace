# Failed attempt: stopped at the first case needing a Claude session

Build `a6ef67a`, harness `8e5481c`. Four real banner interactions
passed (`ui-open`, `ui-quit-cold-start`, `helper-restarted`,
`attention-already-resolved`); the runner then stopped at the first case
that starts a disposable Claude session as a Return target.

Claude Code 2.1.291 opens a new folder with its trust dialog preselecting "No, exit". `spawn_claude` answered the dialog with a bare Return, which declined it: Claude exited with status 1 (reproduced in a disposable window with the shell kept alive: the dialog text, then `TSQ-EXIT-1`), no session was created, and the window closed with its shell. No `~/.claude/projects` entry has ever existed for a harness directory. The harness now sends Down then Return (`ESC [ B` + Return through Terminal's `do script … in` the disposable tab only), which selects "Yes, I trust this folder"; a reproduction then listed the session in `claude agents --json --all` as interactive with a full session ID.
