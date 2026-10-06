# Native qualification harness

Reusable automation for native macOS qualification (M0C onward). It drives the
installed bundles, the system-started companion and real macOS surfaces, and
writes evidence under `evidence/<milestone>/`. Nothing here ships: every
control it uses exists only in qualification builds (`--features
qualification` on the companion and desktop; the `QUALIFICATION` client role;
`launch.qualificationBuild` in the renderer).

## Components

| Component | What it controls |
| --- | --- |
| `harness/src/identity.rs` | The installed production (`Threadspace.app`) and development (`Threadspace Dev.app`) identities and every path the harness reads |
| `harness/src/run.rs` | Bounded argv execution with a minimal environment; `osascript` with values as argv |
| `harness/src/evidence.rs` | Evidence run directories (`<area>/<UTC>-<label>/`), JSONL appends, hashes |
| `harness/src/native.rs` + `swift/ts-native.swift` | Accessibility window inspection/actions, traffic-light presses, notification banner press, labelled System Settings switches, synthetic drag/keys, window capture, pixel statistics and diffs (optionally cropped), owner idle time, displays, an app-scoped 1x display mode for device-pixel-ratio changes |
| `harness/src/procs.rs` | Process incarnations by kernel executable path (PID + birth), exits, working directories |
| `harness/src/companion.rs` | The companion's verified qualification client, incarnation, relaunch waits and its JSON-lines log |
| `harness/src/app.rs` | Packaged launch without activation (`open -g`), `tauri dev` in its own process group, hydration from the companion log, qualification reports and view commands |
| `harness/src/service.rs` | The outer app's native bootstrap (`--service status|register|unregister|prepare|cancel|stop|enable`) |
| `harness/src/terminal.rs` | Disposable Terminal windows the harness creates, marks and proves it owns before touching |
| `harness/src/idle.rs` | Owner-idle gate and the machine-wide GUI automation lock |
| `harness/src/bin/m0c/` | The M0C runners (one subcommand per gate area) |
| `codex-probe/` | Read-only Codex CLI, schema, hook and daemon probe |
| `journal-crash/` | SQLite commit/receipt crash and backup/restore runner against the real journal |
| `hook-probe/`, `qualify/` | M0B hook-ancestry probe and qualification client (route loops, independent readback) |

## Running

Build once: `cargo build -p threadspace-harness` (the runner builds `ts-native`
itself when its source changes). Install the bundles under test:

```sh
tests/native/build-app.sh prod && tests/native/build-app.sh dev
B=target/aarch64-apple-darwin/release/bundle/macos
M=target/aarch64-apple-darwin/debug/threadspace-m0c
$M install prod "$B/Threadspace.app"
$M install dev  "$B/Threadspace Dev.app"
```

Then, from the repository root (each prints a JSON summary and writes its run
directory under `evidence/M0C/`):

```sh
$M g02-packaged prod 10      # ten packaged launches
$M g02-dev dev 10            # ten tauri dev launches (development identity)
$M g03-ipc prod 1000         # 1,000 round trips + negative cases + ACL/origin probes
$M g04-stream prod 10000 60000
$M view-recovery prod 10
$M g05-denied dev            # notification denied path
$M g06-notifications prod    # ≥10 real banner interactions
$M g08-terminal prod         # Terminal negatives with disposable Claude sessions
$M g09-companion prod 3      # UI failures, companion crash relaunch
$M g10-live prod 5           # live crash/receipt + second writer
$M g11-restarts prod 10
$M maintenance prod          # prepare-before-unregister ordering, failure, second UI launch
$M g13-renderer prod         # WebGPU attestation + forced WebGL2 negative
$M g15-graphics prod 15      # fifteen minutes of packaged graphics
$M g16-window prod           # window, display and accessibility matrix
$M g12-sleep-wake prod 5     # needs six root-scheduled wakes, one spare (see below)
```

Utilities: `env`, `view-command <ch> <command> [json]`, `synthetic <ch> <count> <ms>`,
`clear-notifications <ch>`, `resolve-qualification <ch> [reason]`, `wake-schedule <ch>`,
`motion-fixture <ch>` (puts the on-camera office workers in the attention state, so the
scene visibly animates; resolve afterwards with `resolve-qualification`).

## Rules the harness keeps

- **Ownership before destruction.** It closes or kills only windows and
  processes it created and proved: Terminal windows by recorded window ID,
  TTY and a process in its own disposable directory; companions and UIs by
  kernel executable path and birth.
- **Shared screen.** Every focus-changing, window-changing or input step
  holds `/private/tmp/mac-gui-automation.lock` (`flock(LOCK_EX)`) for one
  short segment, so concurrent automation on the same Mac never races for
  focus. Launches use `open -g`.
- **No owner as test runner.** Owner interaction is limited to what macOS
  reserves to the owner. Waking from sleep needs a root-scheduled power
  event: the owner runs one `sudo pmset schedule wake …` command, after which
  `g12-sleep-wake` reads the schedule (`pmset -g sched`) and runs unattended.
  Schedule one wake more than the cycles, first one about 5 minutes out and
  the rest 4 minutes apart (the spare covers a cycle that overruns). The
  runner holds a `caffeinate -i` assertion until just before each
  `pmset sleepnow`, because macOS otherwise idle-sleeps on battery and uses
  up a scheduled wake; it sleeps into the first wake at least 90 s ahead, and
  a cycle fails unless it slept at least 30 s and the OS-recorded wake is
  within 20 s of that wake's scheduled time. Start the runner once the
  schedule exists (it reads the schedule once, at startup); it finishes with
  the Mac awake, so it can signal completion.
- **Side effects are cleaned up.** Qualification attention items are
  resolved through the journaled owner command with a reason; the companion
  removes its own delivered notifications; System Settings switches the
  harness flips are restored to their original value; a display-mode change
  is scoped to the helper process, so macOS restores the display when the
  helper exits, however it exits.
- **Honest labels.** Injected device losses are labelled injected;
  observations made through another session's actions are labelled observed;
  a check that cannot run safely says BLOCKED (it would disturb unrelated
  owner work, such as quitting Terminal) or MANUAL_EXTERNAL_REQUIRED (it
  needs hardware or an owner-only action, such as an external display or a
  root-scheduled wake), with its reason.
