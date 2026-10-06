# D-0004 — Codex 0.160.x runtime modes and the daemon version probe

**Status:** ACCEPTED by independent review on 2026-10-06 of candidate `6f903c273eedc135123e3a7968d3c00887e8cc70`. This resolves the milestone-contract contradiction; it does not pass unexecuted native facets or accept M0C as a whole. Codex product integration remains M6.
**Affects:** SPEC §12.1 (runtime modes), §12.2 (SessionStart sources), §12.3 (`canAcceptDirectInput`), §12.4 (read-only daemon probe, `historyMode`), §12.6 (exact 0.160.1 adapter); MILESTONES G07 (Codex detached-hook ancestry) and M6.

## Evidence (M0C probe, macOS 27.2 26B5091g)

`evidence/M0C/codex/` (`probe.json`, `README.md`, `raw/`), produced by `tests/native/codex-probe` without starting, resuming or creating provider work and without touching the owner's `~/.codex` (every run used a disposable `CODEX_HOME`).

- Installed standalone CLI **0.160.0** (`a956835d`); SPEC researched **0.160.1** (`d27764b`). The two differ by one Windows-only MCP file and the version bump, so every source file SPEC §12 cites is byte-identical.
- `daemon_auto_start` is a stable, enabled feature: an ordinary `codex` TUI launch runs **shared-daemon** mode, auto-starting the selected managed daemon package with no embedded fallback (`startup_orchestration.rs` L494–L547). Embedded execution arises only from an exclusion (`--no-daemon`, `--oss`, `--profile`, non-feature `-c`, `--strict-config`, `--dangerously-bypass-hook-trust`, `CODEX_EXEC_SERVER_URL`).
- The selected managed daemon package on this Mac is **0.159.3**, independent of the CLI version. No daemon was answering during the probe.
- `codex app-server daemon version` is read-only toward the daemon, but every CLI start (including `--version`) creates `CODEX_HOME/tmp/arg0/codex-arg0*` and runs a janitor over stale entries (`codex-rs/arg0/src/lib.rs`).
- `Thread.canAcceptDirectInput` exists only in the experimental schema and is delivered only to clients that send `capabilities.experimentalApi: true`; `thread/read` has no request-side `historyMode` (it is the response field `Thread.historyMode`, with `includeTurns: false` the request control).
- Released SessionStart `source` also allows `fork`.
- Every released hook event fires inside a live thread (SessionStart immediately before the first model request), so capturing a Codex hook's process ancestry requires creating a thread and, normally, a model turn.
- The ChatGPT desktop app bundles its own Codex 0.160.0 app-servers on stdio pipes with no bound socket: no passive endpoint exists for `CODEX_DESKTOP_LOCAL`.

## Decision

- **Default mode.** A plain `codex` launch is classified CODEX_SHARED_DAEMON; CODEX_EMBEDDED is the exclusion case. The default routing ceiling for Codex is therefore SPEC §12.8's shared-daemon row (no originating terminal) until a client→thread→TTY mapping qualifies.
- **Version probe.** The companion learns the daemon version from its own observer handshake (`initialize` reports it), not by running the CLI periodically, so observation never writes the owner's `~/.codex`. Any CLI invocation for setup diagnostics is explicit and owner-initiated.
- **Daemon version is its own profile axis.** The answering daemon's version is qualified separately from the CLI and desktop builds; an unqualified daemon version runs at LIMITED coverage. Installed macOS 0.160.0 has proved 0.160.1 cited-source equivalence, not blanket runtime certification; 0.159.3 is unqualified. No daemon answered the M0C probe, so a live observer handshake remains NOT_RUN until M6.
- **Passive capability set.** `canAcceptDirectInput` is treated as unavailable to the passive observer (it sends no experimental capability). `historyMode` is read from responses only. SessionStart `fork` is accepted as a source value.
- **Detached-hook ancestry for Codex** stays NOT_RUN in M0C, because M0C forbids creating provider work. M0C records configuration/trust/install surfaces without claiming an installed-hook execution test. M6 qualifies safe installation and ancestry in an owner-authorized disposable thread, once with `--no-daemon` (embedded: the nearest provider ancestor is the TUI) and once attached to the daemon (the nearest provider ancestor is the daemon, with no TTY), using a Codex executable-identity rule analogous to M0B's Claude rule.
- **Desktop runtime.** CODEX_DESKTOP_LOCAL observation is limited to hooks/notify; no passive app-server attach.

## Consequences

- SPEC §12.1/§12.2/§12.3/§12.4/§12.6 and MILESTONES G07/M0C/M6 now carry this contract; M6 implements against it.
- G07 may pass the revised M0C mechanism/probe scope. Codex detached-hook ancestry and the absent live-daemon handshake remain explicitly NOT_RUN, assigned to M6. The two required live ancestry runs cannot be inferred from source inspection or the Claude proof.
