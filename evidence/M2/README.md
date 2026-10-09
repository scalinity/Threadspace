# M2 evidence — Manually launched Claude vertical slice

**Status: M2 PASS CANDIDATE — pending independent review.** Branch `m2` from accepted main `af9b285da529890bc441ea00f1a92e73e39902a8`. Every native run ran on the development identity (`~/Applications/Threadspace Dev.app`, `ai.scalinity.threadspace.dev`), built from `f4ef5f1` with a clean tree (`native-build.json`). The production application and the owner's live store stay on accepted main. `manifest.json` carries every figure below with the digests of the files it was read from.

A real Claude Code 2.1.295 session is started in an ordinary Terminal window by typing the command a person types. Threadspace's observer, hooks and inventory follow it as one persistent worker through ten tool-using prompt → completion → Return → follow-up cycles. Mark handled clears its outputs through the durable owner command. After `/exit` the worker stays as history.

## Where the code is

| Concern | Code |
| --- | --- |
| Reducer 3: evidence sets, re-derived frontiers, host-read lifecycle evidence, reducer-1/2 upgrade | `crates/state-engine/src/`, `crates/journal/src/canonical.rs`; [D-0010](../../docs/decisions/D-0010-m2-evidence-sets.md) |
| Observer profiles (2.1.291, 2.1.295) gated on the kernel-read executable | `crates/provider-claude/src/profiles.rs`; [D-0009](../../docs/decisions/D-0009-claude-2.1.295-observer-requalification.md) |
| Observer adapter (mod records → canonical facts with provenance tiers) | `crates/provider-claude/src/observer.rs`, `apps/agent-macos/core/src/adapters.rs` |
| `mod-batch` transport | `crates/relay/src/modbatch.rs`, `crates/relay/src/bin/threadspace-hook.rs`; the mod in `packages/provider-mod/` |
| Reversible integration setup | `crates/provider-claude/src/setup/`, `apps/desktop/src-tauri/src/integration.rs` (SPEC §19.2) |
| Inventory passes and session-scoped waits | `apps/agent-macos/core/src/discovery.rs` |
| Worker, coverage, Return and Mark handled UI | `apps/desktop/src/ui/`; qualification commands in `apps/desktop/src/qualification/m2.ts` |
| Native runners | `tests/native/harness/src/bin/m0c/m2.rs`, `m2_faults.rs`; `tests/native/harness/src/terminal.rs` |

## Areas

| Area | What it shows | Evidence |
| --- | --- | --- |
| Vertical slice | Ten cycles in one session: every cycle one Session view and one worker row, a completed tool-using turn, an exact Return with independent selected-tab and frontmost readback, a follow-up into the same Session. 20 Mark handled presses each resolved in the view, journaled with an owner command and resolved canonically; the worker survives them and `/exit` | [`vertical/20261009T041131Z-dev/`](vertical/20261009T041131Z-dev/) |
| Latency (SPEC §20.2) | p95 hook capture → commit 2 ms, commit → DOM 8 ms, event → DOM 9 ms; observer 36 / 8 / 41 ms (the observer's includes its bounded drain) | `latency` in the vertical summary |
| Trace | 357 observations of the session in ingest order, each with the canonical fact kinds it produced; payloads not copied | `vertical/…/trace.json` |
| Uncut time-lapse | One display still per second for the whole run, 190 frames played in real time, kept only privately outside the repository; its sha256 is in the summary | `recording` in the vertical summary |
| Routes | 30 exact Returns across three sessions, each after fronting another window, each read back independently; 0 wrong targets; p95 895 ms, max 968 ms | [`routes/20261009T041519Z-dev/`](routes/20261009T041519Z-dev/) |
| Faults | Stop continuation, interrupt, failure, blocked submission, unqualified 2.1.292, mod reload and restoration, forged spawn and nonengine lifecycle, parent completed with child waiting, delayed submission, partial/malformed/exit-1 mod-batch receipts, companion stopped and restarted | [`faults/20261009T041720Z-dev/`](faults/20261009T041720Z-dev/) (all cases), [`faults/20261009T043718Z-dev/`](faults/20261009T043718Z-dev/) (four cases rerun after their harness fixes) |
| Terminal negatives (G08 on the dev channel) | Reorder, move, fullscreen Space, foreground mismatch, readback race, closed target, stale TTY, closed during route: all honest; 0 wrong targets. `minimize-then-return` fails (below); Terminal restart BLOCKED (D-0006) | [`g08-terminal/`](g08-terminal/) |
| Integration | Ten install/reinstall/remove cycles on an empty and an owner-like configuration: 15 owned hooks and one owned plugin directory each time, no `WorktreeCreate`/`WorktreeRemove`, foreign settings kept, original bytes restored | [`integration-cycles/20261009T044548Z-dev/`](integration-cycles/20261009T044548Z-dev/) |
| mod-batch | The installed helper: 500 drains p95 5.4 ms, 3,060 records committed exactly, retry `ALREADY_COMMITTED`, end-of-session drain ≤ 5.5 ms, companion absent → spooled ≤ 51 ms | [`mod-batch/summary.json`](mod-batch/summary.json) |
| Provider | 2.1.295 declarations against 2.1.291 (92 of 97 identical, five additive), `claude plugin validate` and `claude plugin test` (38/38), human-typed `composer` origins with engine/core dispatch | [`provider/claude/`](provider/claude/) |
| Permutations | 24,400 seeded permutations (20,000 in the required families, including `evidence-sets`), 2,440 SQLite cross-checks, 0 failures | [`permutations/summary.json`](permutations/summary.json) |

Superseded runs are kept, each with its reason, in `vertical/attempts/` and `integration-cycles/attempts/`.

## Known limitations

- **Minimized-window Return exceeds the 2 s budget on this machine.** In both G08 runs, `minimize-then-return` returned `TIMEOUT` (surface `UNAVAILABLE`, no wrong target). The lookup took 394 ms and focus plus readback 1,609 ms, until the budget ran out. A bare restore call measured 547–926 ms. M0C passed the same case at 1,540 ms, and M2 changed no route code. Owner: M13, with route latency.
- **Route latency is above the p95 target.** p95 is 895 ms against 750 ms, inside the 2 s hard budget, as M0B recorded. Owner: M13.
- **Terminal restart** stays BLOCKED on the owner's machine (D-0006). Owner: M15.
- **Two input records per human prompt.** The classic `UserPromptSubmit` hook and the observer's `prompt.submit` both record the submission. Claude's mod API gives `prompt.submit` no prompt ID, so they cannot be joined: the hook's record stays `UNCLASSIFIED` and unaccepted, the observer's is the accepted `HUMAN_COMPOSER` input. Inputs are not projected to the view, and an unaccepted input never qualifies a follow-up or queues a turn, so nothing visible or derived depends on the extra record. Joining them needs a native prompt identity. Owner: M3.
- **No real-time video.** Screen video capture blocks Terminal scripting (SPEC §13.3; measured: full-display and region video refuse every probe, stills refuse none), so the uncut recording is a one-second time-lapse.
- **Terminal refusals.** Terminal also refuses element queries for seconds to minutes with no Threadspace capture running; the cause is not established. The runners wait them out and record each wait (the full faults run 5 waits, 25.8 s; the rerun 12 waits, 229.7 s).
- **Owner interactions: one.** The dev identity had never held Terminal automation consent, and macOS's TCC dialog may not be answered by software, so the owner clicked Allow once. Every other step was automated.
- **Production is unchanged.** Deploying an M2 build upgrades a store to reducer 3 irreversibly for reducer-2 builds, so it follows the controlled backup and rehearsal procedure used for reducer 2 (D-0010).

## Reproduce

From the repository root, with the dev app built (`tests/native/build-app.sh dev`) and installed (`threadspace-m0c install dev <bundle>`), and `THREADSPACE_EVIDENCE_MILESTONE=M2` set:

```text
threadspace-m0c m2-cycles dev 10
THREADSPACE_M2_RECORDING_DIR=<private dir outside the repository> threadspace-m0c m2-vertical dev 10
threadspace-m0c m2-routes dev 30
threadspace-m0c m2-faults dev all
threadspace-m1 capture-mod-batch <installed threadspace-hook> 500
threadspace-m1 permutations 20000
cargo test --workspace --no-fail-fast --features threadspace-journal/qualification,threadspace-agent/qualification,threadspace-relay/qualification
```
