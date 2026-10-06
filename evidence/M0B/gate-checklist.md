# M0B gate checklist and GO/NO-GO

**Candidate:** branch `m0b`, application commit `fc65641`, built clean. It is the installed `~/Applications/Threadspace.app`; hashes are in `native/reinstall-final/04-hashes.txt`.
**Base:** `f94dd7b` (M0A, merged `main`).
**Target:** macOS 27.2 (26B5091g), Apple M5 Pro. Claude CLI **2.1.291**, native installer. Terminal 2.15, dictionary sha256 `cd4dad3e…`, unchanged from M0A.

**Rule (MILESTONES traceability):** each row scores only the **M0B portion** of the original gate. M0C portions stay NOT_RUN, and no gate is claimed complete.

## M0B portions of the original gates

| Gate | M0B portion | Result | Evidence |
| --- | --- | --- | --- |
| G03 | Real Return command | **PASS** | Owner clicked Return in the packaged UI; the request went UI → `ui_action ReturnToSession` → companion → exact route (`native/B-…`, build `46a2a04`; the UI command path is unchanged in the candidate). The same companion operation is exercised by the harness on the candidate (`native/C-final-…`). |
| G04 | Stream real identity/binding changes | **PASS** (M0B portion) | Real Session/binding/route-result changes were committed and broadcast as patches to the hydrated packaged view: 40 patches, no backpressure retirement. The new `SessionView` fields validate in the frontend (`validate.test.ts`). The full 10,000-change workload is M0C's. |
| G06 | Connect available Return path | **PASS** (M0B portion) | Return is a native companion operation, the one any intent (UI, notification) uses. Wiring the notification response to Return is M0C/M5, and the M0A notification evidence is reused. |
| G07 | Real Claude ancestry, birth/executable and controlling-device proof | **PASS** | `native/hook-ancestry/`: real SessionStart/UserPromptSubmit/Stop hooks have no controlling terminal, their validated parent is the Claude process, and the walk stops honestly at root-owned `login`. `native/A-…`: incarnation samples (birth µs, executable, `e_tdev`) bracket the inventory. A native mechanism test shows `exec` keeps PID and birth but changes the image. |
| G08 | Three same-cwd sessions; ten routes each; reorder/move/close/recreated-tab rejection | **PASS** | `native/C-same-cwd-30-routes/`: 30/30 exact, 0 wrong, 0 unrelated changes. `native/C-final-candidate-30-routes/`: candidate regression, 30/30. `native/F-…`: moves handled; a closed tab and a naturally reused `/dev/ttys002` (same `st_rdev`) cannot revive the old binding. |
| G09 | Reuse background substrate | **PASS** (reuse) | Discovery and routing run in the M0A login-item companion with the UI closed. Late start (`native/A-…`); the UI was not running during the C runs. |
| G10 | Persist actual correlation records | **PASS** (M0B portion) | Journal migration 2 on SQLite **3.53.4** (same source ID, same M0A store generation). Sessions, ProcessKeys, activations, bindings and route results are journaled. They survived two further companion restarts (installs of `8980d34` and `fc65641`) unchanged: each startup pass joined every live session with 0 started, 0 ended, 0 bound and nothing committed (`native/reinstall-*`, companion log `DISCOVERY_PASS`). The crash matrix is M0C's. |
| G11 | Basic observed Session continuity | **PASS** (M0B portion) | `native/D-…`: two turns, same Session/activation/binding. Companion restarts kept the same Sessions, activations and binding IDs (startup passes: 7 joined / 0 started; 6 joined / 0 started). |
| G16 | Real focus interaction | **PASS** (M0B portion) | Packaged UI frontmost → Return → Terminal frontmost with the exact tab selected (`native/B-…`). Fullscreen/Spaces/minimize matrix is M0C's. |
| G17 | Explicit correlation GO/NO-GO contributes | **GO** (below) | This file, `failure-ledger.md`, D-0003 confirmation. Final G17 sign-off is M0C's. |

## M0B GO criteria

| # | Criterion | Result | Evidence |
| --- | --- | --- | --- |
| 1 | Real manually launched direct Claude observed | PASS | `native/A-…`: three manual `claude` launches; `kind=interactive`; executables in the installer's versions directory |
| 2 | Threadspace starts after Claude | PASS | Companion born 04:06:50, after sessions born 04:05:26–33 |
| 3 | Full native persistent session ID recovered | PASS | Full `sessionId`s from `claude agents --json --all`; no prompt and no hook needed |
| 4 | Session→current ProcessKey join is safe | PASS | Bracketed lookups (171/178 ms) with stable birth µs, executable and `e_tdev`; PID-reuse, image-replacement and race permutations are covered synthetically |
| 5 | Controlling terminal device proven | PASS | `e_tdev` from `proc_pidinfo(PROC_PIDTBSDINFO)` at exact structure size; NODEV, denied and short reads handled |
| 6 | Exact Terminal tab proven | PASS | Exactly one tab with `stat(tty).st_rdev == e_tdev`; independent harness enumeration agrees for all 7 sessions; `st_dev` shown identical across tabs |
| 7 | Return verified through native readback | PASS | Selected-tab TTY and its `st_rdev`, front window, Terminal frontmost-window flag, NSWorkspace frontmost PID, and post-focus provider/process revalidation, on every exact route |
| 8 | Three same-cwd Sessions independent | PASS | Three Sessions, ProcessKeys and bindings in one cwd |
| 9 | 10 routes per Session | PASS | 10/10 each |
| 10 | 30 exact routes | PASS | 30/30, plus 30/30 on the final candidate |
| 11 | Wrong-target count zero | PASS | 0 in the counted runs. One disputed result (ledger B-07, owner click during the route) led to a stricter readback rule in the candidate |
| 12 | Turn completion preserves Session | PASS | `native/D-…` |
| 13 | Follow-up preserves Session identity | PASS | `native/D-…` |
| 14 | Resume preserves Session with new activation/surface | PASS | `native/E-…`: same Session and native ID; activation 2; pid 27964; ttys007; old authority retired on proven exit |
| 15 | Stale/recreated bindings rejected | PASS | `native/F-…`, `native/G-…`: stale A/B after in-place switches and a closed tab with reused TTY all refuse with no focus |
| 16 | Ambiguous/unsupported case causes no guessed focus | PASS | `native/H-…` (`script` pty, fixture); `native/E-…` (multiple attachments) |
| 17 | D-0003 confirmed | PASS | `native/B-…`: Terminal's `TCCAccessRequestIndirect` for `kTCCServiceAppleEvents` names the outer `Threadspace.app` (authValue 2) while the companion's osascript child sends. Clearing the companion's own record changes nothing. |

**Verdict: PASS — GO candidate pending independent review.**
