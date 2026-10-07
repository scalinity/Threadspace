# M0C remediation: C-02, C-11, H-10, H-11, H-12

Focused remediation of the independent review recorded at `24035492eed28c342d09c36b52aed61c2bc17a53`, independently re-reviewed at candidate `ccdfc8a38773aac01121601bde6121dd1760b6ea`, then finally remediated from `02947d8dd3a627afef945a0d57497095cfbbae20`. **M0C FINAL REMEDIATION COMPLETE — candidate pending independent acceptance review:** H-10–H-12 were closed on `89fce79`; C-02 handoff intent ownership and C-11 whole-route deadline enforcement are closed on application build `ab11b8a` ([final remediation](#final-remediation)). Valid native workloads remain credited. M0C stays unmerged; M1 has not started. D-0004–D-0006 and C-04 are unchanged.

## Builds under test

| Build | Source | Use | Install record |
| --- | --- | --- | --- |
| `8c82212` | previously reviewed M0C application | negative controls only (pre-repair) | `../install/20261006T140713Z-8c82212/` |
| `a6da2f1` | C-02 repair + qualification barriers | intermediate; superseded runs only | `../install/20261007T003651Z-a6da2f1/` |
| `89fce79` | application sources as at `c3f6de1` (adds the Return readback settle and the `GPU.prototype` init gate); clean tree | every H-10–H-12 and first C-02 run | `../install/20261007T005438Z-89fce79/` |
| **`ab11b8a`** | C-11 `fdee5ad`, C-02 `1270175`, SPEC `ab11b8a`; clean tree, no lockfile or toolchain change | **every final remediation run** | `../install/20261007T041247Z-ab11b8a/` |

Candidate executables (sha256): prod outer `83508af5…`, prod companion `a384f366…`, dev outer `8fbf7de1…`, dev companion `0a24a986…` (full values, signatures with the certificate holder redacted, build info and the post-install supervision smoke of both identities are in the install record). No lockfile or toolchain changed since `8c82212`.

Application commits: `d0598a5` (C-02 supervision), `a6da2f1` (qualification-only barriers), `88817eb` (C-11 Return readback settle), `c3f6de1` (init gate on `GPU.prototype`). Harness commits: `4c1d1c9`, `d23ae65`, `89fce79`, `2861825`, `0880017`, `2df7316`, `0ac5c2e`, `ec8fd26`, `1ac8b9c`.

## Final remediation

From `02947d8`, two code repairs and their targeted regressions, all on application build `ab11b8a`: prod outer `10667766…`, prod companion `7e2ccfa4…`, dev outer `75bb5dc7…`, dev companion `17a76cb0…` (full values, nested signatures with the certificate holder redacted, build info, the empty lockfile diff and both identities' post-install supervision smoke are in `../install/20261007T041247Z-ab11b8a/`). Harness: `b757c75`, `9a0155e`. Tests and lints: `tests-final.txt` (every affected suite in release and qualification feature sets, clippy clean, UI type-check and tests).

### C-02: accepted intents owned across handoff (`1270175`)

**Ownership protocol.** An intent is accepted when the writer queues it, and it is persisted before any view sees it: the pending notification, navigation and inspector intents are rewritten atomically to `pending-intents.json` in the store (temporary file, fsync, rename, directory fsync), and rewritten when a view consumes one. Whichever companion next takes the writer lock loads the set. A response that this process's writer can no longer take (acceptance closed for an exit, queue full) or that reaches an instance that only forwards is spooled in the store as one file named by its notification request, and the next writer takes it as an inspector intent: a Return planned that late would move focus long after the click. The writer removes a spooled response only once its intent is persisted. An intent is named by its notification request, so a response recovered twice is one intent. The shell remembers intents its views acknowledged and answers a re-sent one with `IntentConsumed`; the view skips an intent ID it already applied; the writer sends each view only intents it has not had.

**When ownership transfers and when the old process may exit.** Responsibility passes to the store at acceptance, so it never depends on the old process surviving, on a reply arriving or on a timer. Every voluntary exit of a writer (a yield, the idle exit) is `WriterCommand::Release` on the writer thread: close acceptance, so later responses are spooled; finish every queued command, persisting each intent as it is queued; wait for any response still being spooled; then exit. The yield replies with the pending IDs for evidence only, then releases. The claimant waits for the writer lock whether or not a reply arrives and stops only on an explicit refusal. The unsupervised Enable refusal no longer exits: the instance stays control-only until the login item's companion claims the store.

**Native closure** (`c02-intent-handoff/`; UI hydration held by a harness-launched UI process suspended before it connects, resumed after the handoff; responses placed with `QualifyNotificationResponse`, the banner click's own acceptance path):

| Scenario | Ordering and result |
| --- | --- |
| AGB post-drain, delayed hydration, four phases (`20261007T043351Z-prod/`) | Unsupervised writer 47827 (LaunchServices start). UI 48028 suspended at …706625, never attached. Response `d13834c8` accepted before the yield (…708702). Yield replied `[d13834c8]` and held at …708921 with the writer still accepting. **Post-drain** response `6674aecf` queued by the old writer at …709015, after the reply and absent from it. `1a673323` queued at …710658 while the claimant waited for the lock. Acceptance closed and drained (…710756); `11357ecc` arriving then was spooled (…710763). `WRITER_RELEASED` at …711768 with three pending and one spooled; exited 53 ms later. Login item 48059 claimed at …711802 and loaded all three plus the spooled response. UI resumed …711829, hydrated on the claimant at …712262, and applied all four exactly once in acceptance order. Store empty afterwards. |
| CF Enable refusal, claimant delayed (`20261007T043638Z-prod/`) | Stop, then a real banner click cold-starts the writer (preference disabled, admission closed). The real `--service enable` bootstrap registers the login item; the old writer refuses Enable at …806500 and keeps accepting while its yield is held five seconds: still alive after five seconds, preference and admission unchanged, a new response accepted. Released at …811712, 5,212 ms after the refusal (the former timer exited at 500 ms). Claimant loaded both intents; Enable succeeded; preference enabled and admission open only on the login item; both applied once. |
| D claimant unavailable (same run) | Login item unregistered; the writer holds `0b2c6a1e` pending with admission closed; the held UI never connected, so 113 s after the harness's last request it reaches its idle exit through the writer (`WRITER_RELEASED` listing the intent). The store still holds it while no companion runs and the lock is free. A login item registered later takes the lock, loads it, and the resumed UI applies it once. |
| E1 reply withheld (same run) | The yield's reply is never sent; the old writer releases with `793d072f` in the store; the claimant records the missing reply, waits for the lock and loads it; applied once. |
| E2 reply withheld, retried by launchd (`20261007T044049Z-prod/`) | Reply withheld and the old writer's exit held at …173807 after acceptance closed; a response arriving then is spooled. The first claimant (1898) gives up after its lock wait (…185837) and exits for relaunch; launchd's relaunched login item (12016) finds no reply (…202907) and waits. Released at …203003: the old writer answers the retried yield while draining (two yields) and exits; 12016 claims at …203012 and loads the pending intent and the spooled response; both applied once. |
| Single writer and observer | Across all runs: maximum writer overlap 0 ms and observer overlap 0 ms over 361, 899 and 676 kernel samples; no unsupervised observer. |

Totals: 10 placed intents across the five scenarios, **0 lost, 0 duplicate applications**. Regressions on `ab11b8a`: supervision A–E (`c02-supervision/20261007T052955Z-prod/` A, B; `…/20261007T044346Z-prod/` C; `…/20261007T050045Z-prod/` D, the original Stop → notification → Enable → crash path; `…/20261007T050722Z-prod/` E); G06 11/11 (`../g06-notifications/20261007T052731Z-prod/`); G09 6/6 (`../g09-companion/20261007T052838Z-prod/`); G10 24,458 acknowledged, 0 lost, second writer refused (`../g10-sqlite-live/20261007T050801Z-prod/`). Cited runs credit only the cases named here: `20261007T043638Z-prod`'s E2 failed on a harness predicate (H-15) and is replaced by `20261007T044049Z-prod`; `20261007T044346Z-prod` credits C only, and `20261007T050045Z-prod` D only.

**Negative control.** Not executable on `89fce79`: that build has neither the response injection nor the handoff holds, so the post-drain ordering cannot be placed on it, and no negative result is claimed. The source-level defect at `89fce79`: `writer.rs:654` drained the queue into the reply, `:665` sent it without waiting, `:667–668` exited 500 ms later regardless, `:629–631` exited 500 ms after an unsupervised Enable, `lib.rs:158` dropped a response when the writer queue was full, and `server.rs:73` exited directly at idle.

### C-11: one Return deadline (`fdee5ad`)

**Where the deadline is created and how it propagates.** `RouteDeadline::for_request(received)` is created once, on the monotonic clock, from the instant the request reached the companion; it has no other constructor and cannot be extended. Every waiting native call receives it and uses `remaining()` as its timeout: the automation check, both provider lookups, Terminal enumeration, the focus script, frontmost readback and both binding-revision reads. **Focus:** the script runs with the remaining budget as its process timeout and is stopped at the deadline; a stopped script may have acted, so `focusPerformed` stays true. **Settle/readback:** the script's Space-settle polling is scaled to the budget it is given (`budgetMs div 50` polls of 50 ms), and the process stop caps the time its Apple-event queries take; frontmost polling is capped by what remains. **Postvalidation:** the post-focus lookup and binding read use what remains, and a spent budget after readback starts no further lookup. **Final check:** every result leaves through `Run::finish`, which checks the deadline last; a result claiming an exact surface or current verification after the deadline becomes `TIMEOUT` (UNAVAILABLE, NATIVE_BOUND_LAST_KNOWN, readiness UNKNOWN, no validated revision). Issued native effects stay in `focusPerformed` and the focus evidence; nothing claims they were undone. **Attention:** M0C acknowledges attention only by the owner's explicit command; the automatic rule of SPEC §7 (exact, current, foreground-compatible) is not implemented, and a TIMEOUT result can never meet it.

**Native closure** (`c11-route-deadline/20261007T052511Z-prod/`; disposable Terminal windows; holds report the remaining budget when reached and released):

| Case | Ordering and result |
| --- | --- |
| D ordinary | Exact in **789 ms**; current-session revalidation present; readback and independent front window are the target. |
| A1 held before focus | Reached with 1,647 ms left; released 306 ms after the deadline (0 left) → TIMEOUT, no focus issued, front window unchanged. |
| A2 focus in flight | Released with 75 ms left; the focus script was stopped at the deadline (`timed out true`) → TIMEOUT at 2,004 ms, recorded as possibly acted; no focus side effect appeared within 3 s. |
| B settle | Fullscreen target in its own Space. Released with 249 ms left: the script was stopped at the deadline while macOS was still switching, and macOS showed the Space 157 ms later → TIMEOUT, exact refused. Held around readback after the settle completed (front window = target) → TIMEOUT. The bundled script run directly: no budget returned in 235 ms with macOS still mid-switch; with budget it waited 521 ms for the target. |
| C decision | Focus, readback and post-focus revalidation all proved the target (lookup without error); the decision came 300 ms after the deadline → TIMEOUT; the side effect (target front) is real and recorded. |
| E fullscreen | Exact Return into the fullscreen Space in **1,058 ms**; exit and an exact route after. |
| F attention | A notification Return held past the deadline after focus started → `NOTIFICATION_RETURN` exact false, TIMEOUT; attention unacknowledged after it and after a later exact retry; the owner's explicit acknowledgement then applies. |

Wrong targets 0; Terminal incarnation unchanged. Regression: H-10 on `ab11b8a` passes (`h10-terminal/20261007T052629Z-prod/`); its close races, held about two seconds, now end in TIMEOUT, with populated unrelated-selection inventories (7 windows before, 6 after, no unrelated change). **Negative controls:** removing `Run::finish`'s deadline check fails `a_late_decision_with_valid_proof_is_not_exact` and `a_focus_completing_after_the_deadline_is_recorded_but_not_exact` (`c11-route-deadline/unit/negative-control-gate-removed.txt`); on `89fce79`, a route held before focus started its focus script 2,142 ms after reaching the barrier (latency 2,745 ms; `h10-terminal/20261007T015957Z-prod/`), reaching no exact result only because the target had been closed.

### Qualification isolation

`QualifyNotificationResponse`, `QualifyReleaseHandoffBarrier` and the `HOLD_NEXT_ROUTE_BEFORE_DECISION`, `HOLD_NEXT_YIELD_AFTER_REPLY`, `HOLD_NEXT_YIELD_BEFORE_EXIT` and `DROP_NEXT_YIELD_REPLY` faults exist only with the `qualification` feature; the release-build contract test proves they do not parse. The route hook `RouteNative::reached` is a no-op in release builds, and the handoff barrier module compiles out.

### Superseded runs and owner action

`attempts/c02-intent-handoff/`, `attempts/c02-supervision/` (`20261007T044651Z`, `T045019Z`, `T045704Z`, `T050456Z`) and `attempts/c11-route-deadline/` each carry an `ATTEMPT-NOTE.md` (H-15 in the failure ledger): a hydration hold the retiring view consumed, banner presses that waited on another session's GUI lock, attention raised after Stop, a stale retry predicate, torn log lines (C-13), and a Terminal selection script whose separator printed as the word "tab". The owner's Terminal incarnation refused every element query for more than 50 minutes; with the owner's agreement, the owner restarted Terminal, the only owner action. No other owner application was touched.

### Public-evidence privacy

All 765 M0C evidence files were rescanned for the home path, account names, owner session titles, e-mail addresses and credential patterns. The only matches are milestone names, which are project vocabulary. 114 home paths in ten new run files (process and lock paths inside error strings, and the Claude CLI's path in route evidence) were replaced with `~`; the originals and a redaction log are kept outside the repository. No captures were added.

## Independent re-review

Two source-established correctness defects remain; neither is claimed as a reproduced failure in the retained native runs. This review changes documentation/status only and does not rerun macOS qualification from its Linux environment.

1. **C-02 — handoff can lose accepted notification intent (G06/G09 FAIL).** `apps/agent-macos/core/src/writer.rs` drains `self.intents` in `Yield`, attempts a nonblocking reply, then exits after 500 ms. The writer and native notification callback remain active; a callback accepted after that drain can queue an inspector intent which no successor receives. Case B itself records drain at `1791338491593`, the old writer still handling hydration at `1791338491871`, and the new writer claiming at `1791338492109`; its only pending click preceded the drain. The separate unsupervised Enable exit timer can also terminate pending intents before a delayed/unavailable claimant takes responsibility. **Minimum repair:** preserve all accepted intents across both exit paths, including late callbacks and a failed handoff reply, until responsibility transfers. Add deterministic post-drain/delayed-hydration and delayed/unavailable-claimant coverage plus the affected native handoff/notification regression. Durable attention and acknowledged journal records are not shown lost; positive supervision and single-writer exclusion are sound.
2. **C-11 — the complete Return deadline is unenforced (G08 FAIL).** `crates/surfaces/src/lib.rs::route` checks its deadline only before focus. The focus script then has a fresh two-second subprocess timeout, followed by frontmost polling and post-focus validation with their own budgets; `Run::finish` can return exact/OK after the attempt deadline. Twenty 50 ms delays bound sleep, not synchronous AppleEvent query time. The gap predates C-11, but its added wait must fit the existing hard budget (SPEC §§13.2, 20.2). **Minimum repair:** carry one remaining deadline through native validation/focus/readback/postvalidation, reject expired exact results, and retain honest evidence of any already-issued OS effect. Test expiry during focus/settle and postvalidation, then requalify ordinary and fullscreen Return. No full M0B replay is required.

**Accepted:** H-10 fullscreen/Space and close overlap; H-11 real pending-init/resource overlap; H-12 actual VoiceOver operation. C-11's observed Space-switch false negative is repaired, apart from the deadline obligation above. C-12 is diagnostic-only and assigned to M1: complete asset outcome accounting after quiescence with exactly-once texture/bitmap disposal. H-14 remains a historical qualification side effect. G15/G16 PASS under D-0006; G17 BLOCKED by the two defects above.

## C-02: positive supervision before capture admission (G09)

**Defect.** Startup restored the saved `observation_enabled` into the runtime and `Runtime::admission_open()` checked only preference, maintenance and sleep, so a LaunchServices (notification) cold start that won the writer lock with the preference enabled ran discovery and admitted capture. A missing or unrecognized launchd label also counted as supervised.

**Admission repair (`d0598a5`) accepted.** Launch provenance is classified positively: `LOGIN_ITEM` requires launchd as parent *and* the job label equal to the companion bundle identifier; a `application.<id>.…` label is `LAUNCH_SERVICES`; anything else is `UNKNOWN`, the default. Effective admission = preference ∧ `LOGIN_ITEM` ∧ no maintenance ∧ awake. An unsupervised writer is control-only with a 120-second idle-exit policy; active connections reset the timer. `YieldWriter` transfers the intents present at its drain, and the login item's companion takes the lock before admission opens; preservation throughout handoff remains defective as described above. The qualification durability fixture enforces enabled preference, positive supervision and no maintenance, supporting the C-02 admission probes. Diagnostics report preference, provenance and effective admission separately.

**Cited run** `c02-supervision/20261007T020101Z-prod/` (`c02-supervision prod all`), all six executed cases PASS; they do not cover the post-drain callback/exit races:

| Case | Witnesses |
| --- | --- |
| A enabled preference, login item unregistered, notification cold start | service `NOT_REGISTERED`, lock free, no login-item job; PID 8124 label `application.…`, parent launchd; `LAUNCH_SERVICES`, preference **enabled**, admission **closed**; plan INSPECTOR, intent applied after hydration; capture probe and forced discovery refused `NOT_SUPERVISED`; 0 discovery passes; a disposable Claude session started meanwhile was **not** journaled; attention still outstanding; one writer |
| B login item returns | `register`; incumbent yielded with 1 unconsumed intent (…491593), login item PID 8716 (label = bundle id, parent launchd) claimed with `inheritedIntents: 1` (…492109); login item owned the store after 650 ms; admission opened only now; the provider session was captured by the login item's discovery; the second notification's intent reached the UI; projection equal; capture `COMMITTED` |
| C crash, no UI, no hook | UI absent; SIGKILL of 8716; launchd relaunched `LOGIN_ITEM` 8963 in 2,117 ms; the only client after the kill was this harness's qualification client; discovery resumed; capture `COMMITTED` |
| D original regression | `stop` → banner → cold start 9040 (`application.…`, preference disabled, admission closed, inspector) → `enable` hands over to the login item (owner after 8 ms) → SIGKILL → relaunched `LOGIN_ITEM` 9208, admission open |
| E unknown or malformed provenance | started by the harness with no label, `0` and the login item's own label: all `UNKNOWN`, preference enabled, admission closed, capture and discovery refused `NOT_SUPERVISED`, journal cursor unchanged (255159), lock released |
| F across A–E | No overlap observed in 337 process samples; both admitted records replay `ALREADY_COMMITTED` at their original cursors; all three attention items still outstanding |

The sampler's derived `0 ms` is not continuous proof: scans average about 257 ms apart, only first/last observations survive, and startup admission does not reflect later preference changes. Single-writer/effective-observer exclusion is supported by the lifetime exclusive `flock`, admission ordering and native second-writer refusal. Case E deliberately kills each probe after ten seconds; it does not qualify automatic idle exit.

**Negative controls on `8c82212`** (`c02-supervision/negative-control-8c82212/`): the same case A shows the LaunchServices instance opening admission (`COMMITTED` capture, forced discovery, 3 discovery passes, the disposable session journaled, plan RETURN), and after re-registration the old login item could not take the store back for 45 s; case E shows all three malformed provenances classified supervised and capturing. The assertions therefore detect the defect.

## H-10: Terminal fullscreen/Space and close overlap (G08)

**Cited run** `h10-terminal/20261007T015957Z-prod/` (`h10-terminal prod`), four cases PASS, wrong targets 0, Terminal incarnation unchanged. The owned windows are addressed by CoreGraphics window number (Terminal's AppleScript window id) and AX frame, never by title.

- **Baseline** exact route.
- **Fullscreen/Space**: pre-state not fullscreen with one AX match; entry witnessed in 673 ms (AX `fullScreen`, a fullscreen frame 26 pt below the panel's 21.5 pt camera band, the spare off screen); leaving that Space witnessed in 374 ms (target off screen, spare on); Return was **exact in 1,109 ms** (front window = target 17329), followed by a 96 ms Space-visibility witness; exit restored the original frame in 628 ms with the spare back on screen; the route after exit was exact in 821 ms (baseline: 805 ms).
- **Close during a route held before focus**: barrier reached …423538 → close started …423612 → window gone → provider gone → release …425680 → `TARGET_GONE`, no focus performed.
- **Close during a route held before independent readback/revalidation**: reached …439303 → close …439387 → window gone → release …441260 → `READBACK_FAILED`; the only focus performed named the target window itself. Both close cases' supplementary before/after selection inventories are empty; they do not independently prove unchanged unrelated selections. Target-only script mutations and retained native focus evidence support the zero-wrong-target result.

**C-11 (Space-switch false negative fixed in `88817eb`; deadline repair outstanding).** On the intermediate build Return reported `READBACK_FAILED` although it focused the right tab while macOS was still switching Spaces. The script now polls Terminal's front-window ID against the proven target, read-only, with twenty 50 ms delays; exact TTY/front-window readback is unchanged. Exhausting the loop does not bypass those exact readback checks. The whole-route deadline defect is described above; qualification holds are not ordinary timing evidence.

## H-11: renderer init and resource overlap (G15)

**Cited run** `h11-graphics/20261007T015658Z-prod/` (`h11-graphics prod`), both cases PASS, no outside interference (owner HID idle longer than each case; the office in 3D before captures). Barriers are qualification-only (see below).

- **Pending init**: the genuine WebGPU adapter result, awaited by the pinned backend's `init()`, was held at 231011; the window minimize was requested at 231071; the generation was retired **while its init was pending** at 231614 (`initPending: {generation, retired: true}`); released at 232214; `late-init-discarded` at 232216; hidden-disposed with no live generation; ten hidden seconds scheduled no frame (`pending 0`, `requestedSinceDisposal 0`, requests unchanged); projection equal while hidden; on return a fresh generation attested WebGPU and the canvas changed. `adapterSettledWhileHeld=true`: the native request had settled, but real renderer initialization still awaited its withheld result; this does not claim the GPU driver remained busy.
- **Pending resource**: the office view's `tauri://` request for the scene texture (`/assets/floor-grain-…png`) was held in the shell's protocol handler at 252275, adding exactly one outstanding request; the reload was sent at 252748 and the shell retired that view incarnation at 252866 while the request was outstanding; the late response was released at 253263 to an incarnation no longer active (wry validates the task before responding); the new view hydrated, applied its own texture and consumed nothing late; the application stayed alive and the canvas changed. Retained window shells are measured (C-04, M1), not closed here.

Window captures show the fleet, which carries owner session names, so they stay in the git-ignored `../captures-full/remediation/`; every capture's pixel statistics and diffs are in `cases.jsonl` and `timeline.jsonl`.

## H-12: actual VoiceOver operation (G16)

**Cited run** `h12-voiceover/20261007T023154Z-prod/` (`h12-voiceover prod`), all checks PASS. VoiceOver is turned on with its System Settings switch (its Command-F5 shortcut is disabled on this Mac) and driven through its own scripting interface, which the owner enabled for the run in VoiceOver Utility (one administrator authentication, reverted afterwards with another). No keystroke reaches any app while VoiceOver runs.

- Prior state: VoiceOver off; output not muted.
- VoiceOver answered its scripting interface; its cursor moved out to the web view's scroll area, into the content, to its end and left through the Diagnostics panel to the Office region, into it, to its last item and left: VoiceOver reported **"2D view toggle button"** (93 recorded steps).
- VoiceOver's `perform action` switched the office **3D → 2D** (read from the renderer lifecycle).
- Fullscreen entered and exited.
- VoiceOver navigated again (88 recorded steps) and reported **"2D view selected toggle button"**; its activation switched the office back to **3D**.
- VoiceOver turned off (its quit, then its switch; verified by `NSWorkspace` and the absence of its process); output mute restored; keyboard regression after the transition: 9 distinct labelled controls, Shift-Tab back; projection equal.
- `owner-settings-restored.json`: AppleScript control 0 → 1 → **0** (marker file removed), welcome dialog 1 → 0 → **1**.

The owner-authorized Terminal → VoiceOver Automation consent remains recorded; it is inert while VoiceOver scripting is disabled. No blanket restoration of every machine permission is claimed.

## Regressions on `89fce79`

| Area | Run | Result |
| --- | --- | --- |
| G06 notification interactions (cold start changed) | `../g06-notifications/20261007T020240Z-prod/` | Workload PASS: ten native banner presses plus one enable handoff; gate FAIL for the untested C-02 handoff race |
| G09 independent observer | `../g09-companion/20261007T020340Z-prod/` | Workload PASS 6/6; gate FAIL for C-02 handoff intent preservation |
| G10 live single writer / crash | `../g10-sqlite-live/20261007T020446Z-prod/` | PASS: 58,895 acknowledged, 0 lost over 5 crash rounds; second writer refused (75); idempotent retry `ALREADY_COMMITTED` at its cursor |
| G08 baseline route | H-10 cited run | exact |
| G15 fresh WebGPU/pixels | H-11 cited run | attested WebGPU, changing canvas |
| G16 keyboard after transition | H-12 cited run | 9 labelled, reversible |
| Suites | `tests.txt` | all tests pass; clippy clean for companion and shell in release and qualification builds; harness clippy retains one preexisting `type_complexity` error |

Not rerun because unaffected: G12 sleep/wake, the 15-minute G15 workload (renderer production code changed only by an `initPending` diagnostic field and a `generation-retired` report), the full M0B routing matrix.

## Qualification-only controls

Route barriers (`QualifyArmFault HOLD_NEXT_ROUTE_*`, `QualifyReleaseRouteBarrier`) exist only with the `qualification` feature: the contracts crate omits the variants, and a release-build test proves the requests do not parse. The shell's resource hold and its `on_web_resource_request` hook compile out; the known UI action still parses but a release shell answers `NOT_IMPLEMENTED`. Frontend helpers remain bundled, inactive behind the native immutable launch flag: no release registration, command handler or adapter wrapping. A unit test proves a release launch never wraps `GPU.prototype.requestAdapter`. Every hold resumes after a bounded timeout. No supported release fault-execution path is exposed; M15's production-artifact cleanup remains required.

## Superseded runs

`attempts/<area>/<run>/ATTEMPT-NOTE.md` records each superseded run: a harness ordering defect (C-02 banner), the run that found C-11, two harness predicate defects (fullscreen frame), the inert instance-level init gate, outside input and a counter gap in H-11, and the VoiceOver keystroke and scripting development runs. Nothing was overwritten.

## Public-evidence privacy

Before publication, every file under `evidence/M0C` was scanned for the home path, the account name, owner session titles (the private redaction record plus the journal's non-harness titles; milestone-style titles are project vocabulary), e-mail addresses and credential patterns. Redactions, with originals kept outside the repository:

- the eight system logout-menu AX labels in four committed G16 trees: the account name → `[account]`;
- copied companion logs in C-02 runs: home paths → `~`, client executable paths → file names (the runner now writes them this way);
- two early VoiceOver attempts: focus labels other than the app's fixed controls → `digest:<sha256 prefix>` (the runner now writes them this way).

No technical value, count, timestamp or result was changed.

## Owner-environment side effects

The VoiceOver work affected the owner's Mac beyond Threadspace; see H-14 in `../failure-ledger.md`. While VoiceOver was on, Unreal Engine's crash reporter agent crashed twice (21:27, 22:26 local) and VoiceOver's Tutorial once took the front and consumed injected keystrokes; the alerts were dismissed and the Tutorial closed. A `forge` process also aborted at 22:34 during the cited run; it had crashed on earlier days too, and a causal link is not established. The final run uses VoiceOver scripting, with a frontmost guard before activation and ordinary keys only after VoiceOver exits; its own cursor reports and independent application state support both transitions. Retain this guarded path for future qualification. Scripting/welcome/mute/VoiceOver state were restored; the explicit Automation consent above remains. Private crash originals are not required for publication, and unrelated crash causality is not independently established.
