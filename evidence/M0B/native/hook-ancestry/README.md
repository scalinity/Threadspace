# Claude command-hook ancestry (SPEC §4.5, G07 M0B portion)

`session-start.jsonl` holds one line per SessionStart hook fired when each of the three test sessions launched. The hook was the test-only `threadspace-hook-probe`, configured only in the test directory's project settings (`test-dir-settings.local.json`). The probe keeps only allowlisted hook fields and never reads `/dev/tty`.

Observed on Claude 2.1.291:

- The hook process has **no controlling terminal** (`selfHasControllingTerminal: false`, `e_tdev` NODEV). Its stdin is JSON, not a terminal.
- Its validated parent is the Claude CLI process itself (provider depth 1, no intermediate shell). The parent's `e_tdev` equals that session's Terminal tab device.
- The walk continues through the interactive `zsh` and stops at root-owned `login`, whose BSD info an unprivileged reader is denied (`UNREADABLE / PROCESS_READ_DENIED`). That is an honest stop: the provider ancestor is already proven below it.

In this profile, hook ancestry is a valid corroboration path. Native inventory stays the primary authority: discovery never consumes these records.
