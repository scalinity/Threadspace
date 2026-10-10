# Disclosed harness process-ownership incident

Recorded during the F1–F4 remediation on 2026-10-10. The command disclosed by the
implementer and repeated in the owner's remediation instruction was:

```sh
pkill -f 'sleep 40'
```

The disclosed action occurred during the earlier M2 implementation/qualification
work. The retained materials examined here do not establish a more precise time,
the invoking process, matching PID list, process incarnations or command result.
No specific affected process can be identified from those materials. Therefore
this record does **not** assert that no unrelated process was affected, that any
particular process was terminated, or that a particular evidence cycle was
altered. The possible owner/evidence impact remains unestablished. The disclosed
single action is not evidence that every earlier native result is invalid.

The action violated AGENTS.md and CLAUDE.md resource-ownership rule 8: destructive
test actions must operate on resources whose harness ownership is proved. A
process-name pattern does not prove that ownership. No unrelated application,
Terminal session or process was restarted or manipulated to investigate this
historical uncertainty.

## Prevention and current evidence

- The static literal-command scan retained in `harness-ownership-audit.json`
  found no remaining `pkill` or `killall` occurrence in the scanned implementation
  languages under `tests`, `crates`, `apps` and `packages`. This is a literal
  source scan, not proof about every possible constructed command.
- The focused source review distinguishes the `Child` handles returned by the
  harness's own display-mode and caffeinate spawns from the low-level
  `procs::signal` primitive. That primitive's contract requires callers to verify
  process incarnation. This limited scan does not certify every historical
  caller or retroactively prove the disclosed command's ownership.
- F2 removes the analogous failed-acquisition cleanup error. A Scratch directory
  is exclusively created and its device/inode is recorded and rechecked.
  Integration cleanup requires the successful recorded acquisition and fresh
  record equality; a failed installation creates no cleanup authority.
- The actual F2-10 regression test records zero uninstall calls after failed
  acquisition and byte-identical pre-existing disposable settings, helper, mod,
  record, backups and unrelated files. Its assertion fails when unconditional
  Drop cleanup is reintroduced in a disposable mutation control.

No native process termination was executed as part of the F2 portable tests or
this incident audit. Future termination must target a verified ProcessKey,
specific harness-created `Child`, or proven owned process group, according to the
existing repository rule.
