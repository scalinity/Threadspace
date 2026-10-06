# M0C Codex platform probe

Read-only qualification of the Codex runtimes on this machine and of the existing shared daemon's protocol and version (MILESTONES M0C, "Probe Codex modes and the existing daemon's actual protocol/version without starting/resuming provider work"). Full M6 integration remains later.

- Probe: `tests/native/codex-probe` (`threadspace-codex-probe`), debug build.
- Run recorded in `probe.json`: 2026-10-06T10:03:36Z to 10:03:46Z, macOS 27.2 (26B5091g).
- `$HOME` is written as `~` throughout. No prompt bodies, thread titles or content, account identifiers, tokens or addresses are recorded. Live-process argv is reduced to allowlisted tokens, and `thread/loaded/list` would record only a count.

## Mode table

| Mode | On this machine | Evidence | Limitations |
| --- | --- | --- | --- |
| CODEX_SHARED_DAEMON | **Not running.** No daemon answered. | `codex app-server daemon version` exit 1, `failed to connect … No such file or directory`. A direct AF_UNIX connect to `~/.codex/app-server-control/app-server-control.sock` failed with `NotFound`. The rendezvous symlink points at `/private/tmp/codex-daemon-501/2ef99283…a7b7`, which equals the released derivation (SHA-256 of the canonical socket path), but the target and its directory are absent. `daemon.pid` (pid 17412, started Sep 30 19:26:47) and `daemon-updater.pid` (pid 4527) were recorded on a previous boot, and neither PID is alive. The selected managed daemon package is **0.159.3**. | The live wire was not exercised against Codex. The probe client is verified only against the in-process RFC 6455 fake in the crate tests. A daemon started now would run 0.159.3, not the CLI's 0.160.0. |
| CODEX_EMBEDDED | **Installed, no live session.** | Standalone CLI 0.160.0. `--no-daemon` is present. Feature `hooks` is stable and on by default. No standalone TUI process is running. | Detached-hook ancestry was NOT_RUN (see below). Embedded mode exposes no app-server endpoint for history or status. |
| CODEX_DESKTOP_LOCAL | **Running, stdio only, no passive endpoint.** | `/Applications/ChatGPT.app` (bundle id `com.openai.codex`, 26.930.31730 build 12947) bundles `Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex` = 0.160.0. That binary differs from the standalone binary: sha256 `6b582e88…` vs `112fae7a…`, bundle-signed. Three desktop `app-server` processes run (pids 59764, 22360, 66756), all on default `stdio://`, with 0 path-bound Unix sockets. One `exec-server` also runs. `/Applications/Codex.app` is absent. | A passive observer has nothing to attach to. App activation and the current-conversation route were not probed. |

## Installed runtimes (item 1)

| Runtime | Path | Version | sha256 | Signature |
| --- | --- | --- | --- | --- |
| Standalone CLI | `~/.local/bin/codex` → `~/.codex/packages/standalone/current/bin/codex` → `…/releases/0.160.0-aarch64-apple-darwin/bin/codex` | `codex-cli 0.160.0` (manifest 0.160.0) | `112fae7a5a1223e673c8a1791d32338f37df8b527ff1159bb8adac6c4dbf1b4b` | valid, Developer ID OpenAI OpCo (2DC432GLL2), hardened runtime, Oct 1 2026 13:49:46 |
| Desktop-bundled | `/Applications/ChatGPT.app/…/CodexCLI.app/Contents/MacOS/codex` | `codex-cli 0.160.0` | `6b582e8813ce7e8ed4c52814ee5cf230dba647bf2292df747a4003f2657ef201` | valid, app bundle, same team, Oct 1 2026 14:02:11 |
| Managed daemon package | `~/.codex/packages/app-server-daemon/current` → `releases/0.159.3-aarch64-apple-darwin` | `codex-cli 0.159.3` | `4d210f7c5a18fd0386434df23b5bdbb8c0e7257d3e8a2b30b0769c8bbe99a878` | valid, same team, Sep 30 2026 18:21:14 |

Other installed, unselected releases: standalone 0.145.0, 0.154.0 and 0.157.1. Daemon packages 0.157.1, 0.158.0, 0.159.0, 0.159.1 and 0.159.2.

**Version drift.** SPEC researched 0.160.1, pinned at `d27764b8`. Both installed runtimes are 0.160.0, at `a956835d` (tag object `79b1b666`). `raw/release-diff-0.160.0-0.160.1.json` records the GitHub readback. The two releases share merge base `cb779962`. 0.160.1 adds only a Windows remote-stdio-MCP environment backport (`codex-rs/rmcp-client/src/stdio_server_launcher.rs`, +23) plus its version bump. 0.160.0 adds only its own version bump. So every source file SPEC §12 cites is byte-identical in the installed build. The twelve hook input fixtures were fetched at both tags and are identical.

## Schema compatibility (item 2)

The installed build emits its app-server protocol locally through `codex app-server generate-json-schema`: 314 files / 3,540,819 bytes stable, and 440 files / 4,286,533 bytes with `--experimental`. Per-file SHA-256 manifests, the method lists and the schema files the checks read are under `raw/app-server-schema/`. The comparison covers what SPEC §12.3–§12.6 depend on: 25 checks, **23 MATCH, 1 EXPERIMENTAL_ONLY, 1 MISSING**.

- MATCH:
  - `initialize`, plus `ClientInfo{name,title,version}` and `capabilities`.
  - The `initialized` notification and `InitializeResponse.userAgent`.
  - `thread/loaded/list` with `limit`/`cursor` → `data`/`nextCursor`.
  - `thread/read` with `threadId`/`includeTurns`.
  - Thread `id, sessionId, parentThreadId, forkedFromId, ephemeral, historyMode, status, turns`, with `ThreadHistoryMode` legacy/paginated.
  - `ThreadStatus` notLoaded/idle/active/systemError, and `activeFlags` waitingOnApproval/waitingOnUserInput.
  - Turn `id, status, startedAt, completedAt`, with `TurnStatus` completed/interrupted/failed/inProgress.
  - `thread/turns/list` with `threadId, limit, cursor, sortDirection, itemsView`, and `itemsView: notLoaded`.
  - `thread/status/changed{threadId,status}`.
  - Notifications `turn/started`, `turn/completed`, `error{willRetry}`, `thread/closed|archived|unarchived|deleted`, `item/started|completed`, `serverRequest/resolved`.
- EXPERIMENTAL_ONLY: `Thread.canAcceptDirectInput`. It exists only in the `--experimental` schema, together with `daybreakEnabled`, `environments` and `extra`.
- MISSING: a request-side `historyMode` on `thread/read`. `ThreadReadParams` is `{threadId, includeTurns}` only, and `historyMode` exists only as the response field `Thread.historyMode`.
- Extra on the installed build, beyond SPEC:
  - `hooks/list` (params `cwds`), and notifications `hook/started` and `hook/completed`.
  - `thread/unsubscribe` and `thread/items/list`.
  - `InitializeCapabilities{experimentalApi, explicitGatewayOauth, extensions, mcpServerOpenaiFormElicitation, optOutNotificationMethods, requestAttestation}`.
  - `InitializeResponse{codexHome, platformFamily, platformOs, userAgent}`.
  - Full lists are in `probe.json` → `items.schema.appServerSchema.comparison.extra`.

**Hook input schema.** The CLI has no emitter for the hook stdin schema, so the released fixtures (`raw/hook-input-schema-0.160.0/`, fetched at `a956835d`) are compared with the SPEC §12.2 table: **all 12 rows MATCH**.

- Every event carries `session_id`, nullable `transcript_path`, `cwd` and a constant `hook_event_name`.
- No event carries an event UUID, sequence or terminal/client ID.
- PermissionRequest has no `tool_use_id`.
- The shipped executable contains the identifiers `transcript_path`, `hook_event_name`, `tool_use_id`, `permission_mode`, `stop_hook_active`, `last_assistant_message` and `agent_transcript_path` (bounded `grep -a -F -q`).
- Extras:
  - SessionStart `source` also allows **`fork`**.
  - Most events also carry `model` and `permission_mode`.
  - SubagentStop also carries `last_assistant_message`.
  - Optional `agent_id`/`agent_type` appear on tool, compact and prompt events.

## Hook configuration surface (item 3)

- Feature `hooks` is stable and enabled by default, and `plugin_hooks` is removed. These are build defaults from `codex features list` in an empty CODEX_HOME, not the effective configuration.
- `--dangerously-bypass-hook-trust` exists.
- `HookEventName` has exactly the 12 SPEC events and nothing extra or missing.
- `HookSource`: system, user, project, mdm, sessionFlags, plugin, cloudRequirements, cloudManagedConfig, legacyManagedConfigFile, legacyManagedConfigMdm, unknown.
- `HookTrustStatus`: managed, untrusted, trusted, modified.
- `HookHandlerType`: command, mcpTool, prompt, agent. Released discovery skips prompt and agent as "not supported yet", and rejects mcpTool for SessionEnd.
- `HookScope`: thread, turn.
- Locations, from rust-v0.160.0 `codex-rs/hooks/src/engine/discovery.rs`: each config layer contributes `<layer hooks folder>/hooks.json` and a `[hooks]` table in that layer's `config.toml`. Both forms load together, with a warning. Managed requirements and plugins add further sources.
- Non-managed hooks run only when enabled and trusted for their current hash.
- The executable contains `hooks.json`.
- The owner's configuration was neither read nor modified.

## Detached-hook ancestry (item 4): NOT_RUN

**Blocker.** Every released Codex hook is dispatched from a live session (thread), and every hook except SessionEnd only runs inside a running turn:

- SessionStart runs from `run_turn` immediately before the first sampling request (`codex-rs/core/src/session/turn.rs` L163/L322).
- SessionEnd needs a live Session and flushes the rollout first (`codex-rs/core/src/hook_runtime.rs` L471–499).
- All other events carry a required `turn_id`.

Capturing any hook process therefore needs a thread, and for every event but SessionEnd a model-sampling turn. It also needs an authenticated account for the TUI to start a session, since an empty disposable CODEX_HOME has none. Thread creation, prompts, model calls and login are all outside this probe's limits. No TUI, `exec`, `thread/start` or hook installation was attempted.

**To unblock**, an owner-authorized qualification run must be allowed to create one disposable thread. It would set a SessionStart command hook in a disposable CODEX_HOME `hooks.json`, sign in to an account, and send one prompt in a harness-owned pty. It also needs a Codex executable-identity rule for the M0B capture: `tests/native/hook-probe` selects the provider by the Claude versions directory. The run should happen twice: once with `--no-daemon`, and once attached to the daemon, where the nearest provider ancestor is the daemon and there is no TTY.

## Daemon (item 5)

- Status: **NO_DAEMON_ANSWERING**. No running app-server version exists to record.
- `codex app-server daemon version` was run with the disposable CODEX_HOME. Its `app-server-control/app-server-control.sock` is a symlink to the owner's rendezvous path, so the released read-only probe followed the real endpoint without writing `~/.codex`. The command exited 1 with ENOENT.
- The direct connect to the rendezvous path failed with `NotFound` (errno 2), so the observer handshake was not run.
- `/private/tmp/codex-daemon-501` was absent before and after the run, so no daemon was started.
- `settings.json` is absent. The pid records use the dedicated-package names (`daemon.pid`, `daemon-updater.pid`). The released 0.160 code reads legacy `app-server.pid` names only when the managed binary lives under `packages/standalone`.
- `daemon.pid.executableIdentity.digest` (`d2c0eecf…`) is opaque. It equals neither the file SHA-256 nor the CDHash of the 0.159.3 binary.
- When a daemon does answer, the probe runs the passive sequence:
  1. HTTP/1.1 upgrade to `ws://localhost/` over AF_UNIX, with `Sec-WebSocket-Accept` validated against SHA-1.
  2. Masked text frames.
  3. `initialize` with client `threadspace_m0c_probe` / "Threadspace M0C passive probe" and no capabilities.
  4. Wait for the response.
  5. `initialized`.
  6. One `thread/loaded/list` with `limit: 5`, recording only the count and whether a next cursor exists.
  7. Close with code 1000.

  It records server requests by method and never answers them. The crate test `observer_completes_handshake_against_fake_server` covers ping/pong, fragmented responses and an interleaved notification.

## Findings against SPEC §12 (decision candidates)

1. **`daemon version` is not side-effect-free on CODEX_HOME.** Every CLI start, including `--version`, `--help` and `daemon version`, runs arg0 dispatch first. That creates `CODEX_HOME/tmp/arg0/codex-arg0*` (a lock plus `apply_patch`, `applypatch` and `codex-execve-wrapper` symlinks) and runs a janitor that deletes unlocked stale arg0 directories (rust-v0.160.0 `codex-rs/arg0/src/lib.rs`). Clap-exit paths leave the directory behind. `disposableHomeAfter` in `probe.json` shows both cases. SPEC §12.4 calls the command read-only, which holds toward the daemon only. A companion that runs it periodically writes the owner's `~/.codex`. The decision is how Threadspace probes the version. Options: speak the observer handshake itself (it performs the same `initialize` that `daemon version` does), alias CODEX_HOME as this probe does, or accept the transient writes.
2. **The daemon runtime version is decoupled from the CLI on this machine.** The selected managed package is 0.159.3, the CLI and desktop are 0.160.0, and SPEC pins 0.160.1. Per the released README and `startup_orchestration.rs` L494–L547, the TUI auto-starts the *selected package* (`daemon_auto_start` is stable and on, with no embedded fallback). So a shared-daemon session here would run 0.159.3, outside the "exact 0.160.1 compatibility adapter" of §12.6. Two decisions are needed: how an unqualified daemon version degrades capabilities, and whether 0.160.0 (source-identical for every cited file) is accepted as the 0.160.1 profile.
3. **§12.1 calls CODEX_EMBEDDED "ordinary embedded/`--no-daemon` CLI execution".** In 0.160.x an ordinary TUI launch is CODEX_SHARED_DAEMON, with the daemon auto-started and no embedded fallback. Embedded mode arises only from an exclusion (`--no-daemon`, `--oss`, `--profile`, non-feature `-c` overrides, `--strict-config`, `--dangerously-bypass-hook-trust`, `CODEX_EXEC_SERVER_URL`). The default routing ceiling for a plain `codex` therefore drops to the §12.8 shared-daemon row: no originating terminal.
4. **`thread/read` has no request-side `historyMode`.** §12.4 step 3 ("metadata-only `thread/read`, including the supported `historyMode` field") can only mean `includeTurns: false` plus the response field `Thread.historyMode`. The wording should say so.
5. **`canAcceptDirectInput` requires opting into `experimentalApi`.** §12.3 relies on it "when present". A passive observer that omits capabilities, as §12.4 directs for interactive ones, never receives it. The decision is whether the observer sets `capabilities.experimentalApi: true` (not an interactive capability) and accepts the experimental surface.
6. **SessionStart `source` also includes `fork`.** §12.2 lists startup/resume/clear/compact. For a ThreadSpawn child, a `fork` source dispatches SubagentStart instead.
7. **CODEX_DESKTOP_LOCAL has no passive endpoint.** The desktop runtime's app-servers run on stdio pipes owned by the desktop app. Observation there is limited to hooks/notify. Inferred but not verified: those app-servers keep their arg0 directories in `~/.codex/tmp/arg0`, so they share `~/.codex` hook configuration.

## Never done

The probe never started, resumed, forked or executed a Codex task; never submitted a prompt or called a model; never logged in; never started, stopped, restarted or updated a daemon; never sent `thread/resume`, `turn/*` or any write; and never read or modified the owner's Codex configuration beyond reading package manifests, symlinks and pid records under `~/.codex`. Every CLI run used a cleared environment whose `CODEX_HOME`, `HOME` and `TMPDIR` pointed into `/private/tmp/claude-501/cp-<id>/`, with that directory as the working directory. The directory was removed afterwards (`probe.disposable.removed: true`). No GUI, Terminal window or TUI was opened.

## Rerun

```sh
# from the repository root
cargo build -p threadspace-codex-probe
./target/aarch64-apple-darwin/debug/threadspace-codex-probe all \
  --out evidence/M0C/codex \
  --hook-schemas evidence/M0C/codex/raw/hook-input-schema-0.160.0
# single items: runtime | schema | hooks | hook-ancestry | daemon, then modes (reads <out>/*.json)
cargo test -p threadspace-codex-probe   # SHA-1/base64/accept vectors, framing, fake-server handshake
```

Refreshing the hook fixtures and the release comparison requires network reads. Fetch each fixture with `curl -fsSL -o <out>/<event>.command.input.schema.json https://raw.githubusercontent.com/openai/codex/<commit>/codex-rs/hooks/schema/generated/<event>.command.input.schema.json` for the twelve events, at `a956835d020762cb2b570053af06f643a11c0ecc` and at `d27764b82f7118f674371e6d6e76271d9d606edb`, and compare their SHA-256. The tag and compare API calls are listed in `raw/release-diff-0.160.0-0.160.1.json` under `commands`.

## Files

| Path | Content |
| --- | --- |
| `probe.json` | All items, with timestamps, exact argv and environment of every command, and exit status |
| `raw/help/*.txt` | `codex --help`, `app-server --help`, `app-server daemon --help`, `daemon version --help`, `generate-json-schema --help` |
| `raw/features-list-empty-codex-home.txt` | `codex features list`, build defaults |
| `raw/app-server-schema/{stable,experimental}-manifest.json` | Every generated schema file with SHA-256 and size |
| `raw/app-server-schema/methods.json` | Request and notification method sets, stable and experimental |
| `raw/app-server-schema/stable/**`, `experimental/v2/ThreadReadResponse.json` | The schema files the checks read |
| `raw/hook-input-schema-0.160.0/*.json` | Released hook input fixtures at rust-v0.160.0 |
| `raw/release-diff-0.160.0-0.160.1.json` | Tag commits, merge base, differing files and commit titles |

No probe run failed. The only failing commands are the expected `daemon version` exit 1 and the refused direct connect, both recorded in `probe.json`.
