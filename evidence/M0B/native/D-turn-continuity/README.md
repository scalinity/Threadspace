# M0B native test D: turn completion and follow-up keep the Session

**Result: PASS.**

The owner sent two prompts in test session `f8cd39d3` (canonical `ea8e75e4…`, pid 55189). The native inventory status recorded by the companion (`02-export.json`, cursors 137–141) went **idle → busy → idle → busy → idle** on the same pid and session ID.

Across both turns:

- no activation ended and no activation started;
- afterwards the Session is unchanged: same canonical ID, activation 1, LIVE, CURRENT, binding `8ccd273d…` at rev 24 (`03-snapshot-after.json`).

The test-directory hook probe independently recorded `UserPromptSubmit` → `Stop` twice for the same session ID from the same Claude process (`../hook-ancestry/`). Stop and idle are not translated into an end: the Session is never removed. The companion's own process watcher (`01-…`) followed a different session, because the owner used another test tab. It is kept as-is.
