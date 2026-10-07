# M0C remediation: C-02, H-10, H-11, H-12

Focused remediation of the independent review recorded at `24035492eed28c342d09c36b52aed61c2bc17a53`. It closes the four required items with automated native evidence and reruns only the affected regressions. It does not start M1, merge M0C, or reopen D-0004, D-0005, D-0006 or C-04.

## Builds under test

| Build | Source | Use | Install record |
| --- | --- | --- | --- |
| `8c82212` | accepted M0C candidate | negative controls only (pre-repair) | `../install/20261006T140713Z-8c82212/` |
| `a6da2f1` | C-02 repair + qualification barriers | intermediate; superseded runs only | `../install/20261007T003651Z-a6da2f1/` |
| **`89fce79`** | application sources as at `c3f6de1` (adds the Return readback settle and the `GPU.prototype` init gate); clean tree | **every cited run** | `../install/20261007T005438Z-89fce79/` |

Candidate executables (sha256): prod outer `83508af5…`, prod companion `a384f366…`, dev outer `8fbf7de1…`, dev companion `0a24a986…` (full values, signatures with the certificate holder redacted, build info and the post-install supervision smoke of both identities are in the install record). No lockfile or toolchain changed since `8c82212`.

Application commits: `d0598a5` (C-02 supervision), `a6da2f1` (qualification-only barriers), `88817eb` (C-11 Return readback settle), `c3f6de1` (init gate on `GPU.prototype`). Harness commits: `4c1d1c9`, `d23ae65`, `89fce79`, `2861825`, `0880017`, `2df7316`, `0ac5c2e`, `ec8fd26`, `1ac8b9c`.

## C-02: positive supervision before capture admission (G09)

**Defect.** Startup restored the saved `observation_enabled` into the runtime and `Runtime::admission_open()` checked only preference, maintenance and sleep, so a LaunchServices (notification) cold start that won the writer lock with the preference enabled ran discovery and admitted capture. A missing or unrecognized launchd label also counted as supervised.

**Repair (`d0598a5`).** Launch provenance is classified positively: `LOGIN_ITEM` requires launchd as parent *and* the job label equal to the companion bundle identifier; a `application.<id>.…` label is `LAUNCH_SERVICES`; anything else is `UNKNOWN`, the default. Effective admission = preference ∧ `LOGIN_ITEM` ∧ no maintenance ∧ awake. An unsupervised writer is control-only with a bounded lifetime. When the login item's companion finds the lock held it sends `YieldWriter`; only an unsupervised incumbent agrees, handing over its unconsumed intents and exiting, and the login item's companion takes the lock before admission opens. The qualification durability fixture is admitted only where capture is. Diagnostics report preference, provenance and effective admission separately.

**Cited run** `c02-supervision/20261007T020101Z-prod/` (`c02-supervision prod all`), all six cases PASS:

| Case | Witnesses |
| --- | --- |
| A enabled preference, login item unregistered, notification cold start | service `NOT_REGISTERED`, lock free, no login-item job; PID 8124 label `application.…`, parent launchd; `LAUNCH_SERVICES`, preference **enabled**, admission **closed**; plan INSPECTOR, intent applied after hydration; capture probe and forced discovery refused `NOT_SUPERVISED`; 0 discovery passes; a disposable Claude session started meanwhile was **not** journaled; attention still outstanding; one writer |
| B login item returns | `register`; incumbent yielded with 1 unconsumed intent (…491593), login item PID 8716 (label = bundle id, parent launchd) claimed with `inheritedIntents: 1` (…492109); login item owned the store after 650 ms; admission opened only now; the provider session was captured by the login item's discovery; the second notification's intent reached the UI; projection equal; capture `COMMITTED` |
| C crash, no UI, no hook | UI absent; SIGKILL of 8716; launchd relaunched `LOGIN_ITEM` 8963 in 2,117 ms; the only client after the kill was this harness's qualification client; discovery resumed; capture `COMMITTED` |
| D original regression | `stop` → banner → cold start 9040 (`application.…`, preference disabled, admission closed, inspector) → `enable` hands over to the login item (owner after 8 ms) → SIGKILL → relaunched `LOGIN_ITEM` 9208, admission open |
| E unknown or malformed provenance | started by the harness with no label, `0` and the login item's own label: all `UNKNOWN`, preference enabled, admission closed, capture and discovery refused `NOT_SUPERVISED`, journal cursor unchanged (255159), lock released |
| F across A–E | 337 process samples: max writer overlap 0 ms, max observer overlap 0 ms, no unsupervised observer; both admitted records replay `ALREADY_COMMITTED` at their original cursors; all three attention items still outstanding |

**Negative controls on `8c82212`** (`c02-supervision/negative-control-8c82212/`): the same case A shows the LaunchServices instance opening admission (`COMMITTED` capture, forced discovery, 3 discovery passes, the disposable session journaled, plan RETURN), and after re-registration the old login item could not take the store back for 45 s; case E shows all three malformed provenances classified supervised and capturing. The assertions therefore detect the defect.

## H-10: Terminal fullscreen/Space and close overlap (G08)

**Cited run** `h10-terminal/20261007T015957Z-prod/` (`h10-terminal prod`), four cases PASS, wrong targets 0, Terminal incarnation unchanged. The owned windows are addressed by CoreGraphics window number (Terminal's AppleScript window id) and AX frame, never by title.

- **Baseline** exact route.
- **Fullscreen/Space**: pre-state not fullscreen with one AX match; entry witnessed in 673 ms (AX `fullScreen`, a fullscreen frame 26 pt below the panel's 21.5 pt camera band, the spare off screen); leaving that Space witnessed in 374 ms (target off screen, spare on); the route brought the Space back in 96 ms and was **exact** (front window = target 17329); exit restored the original frame in 628 ms with the spare back on screen; the route after exit was exact.
- **Close during a route held before focus**: barrier reached …423538 → close started …423612 → window gone → provider gone → release …425680 → `TARGET_GONE`, no focus performed.
- **Close during a route held before readback**: reached …439303 → close …439387 → window gone → release …441260 → `READBACK_FAILED`; the only focus performed named the target window itself; no other window's selected tab changed.

**C-11 (found here, fixed in `88817eb`).** On the intermediate build a Return into a fullscreen window in another Space reported `READBACK_FAILED` although it focused the right tab: the focus script read `front window` while macOS was still switching Spaces. The script now waits at most one second, read-only, for the target to lead Terminal's window order; the readback is unchanged and must still match.

## H-11: renderer init and resource overlap (G15)

**Cited run** `h11-graphics/20261007T015658Z-prod/` (`h11-graphics prod`), both cases PASS, no outside interference (owner HID idle longer than each case; the office in 3D before captures). Barriers are qualification-only (see below).

- **Pending init**: the WebGPU adapter request, the first await in the pinned backend's `init()`, was held at 231011; the window minimize was requested at 231071; the generation was retired **while its init was pending** at 231614 (`initPending: {generation, retired: true}`); released at 232214; `late-init-discarded` at 232216; hidden-disposed with no live generation; ten hidden seconds scheduled no frame (`pending 0`, `requestedSinceDisposal 0`, requests unchanged); projection equal while hidden; on return a fresh generation attested WebGPU and the canvas changed.
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

## Regressions on `89fce79`

| Area | Run | Result |
| --- | --- | --- |
| G06 notification interactions (cold start changed) | `../g06-notifications/20261007T020240Z-prod/` | PASS 11/11, incl. stopped-observation cold start and enable hand-over to the login item |
| G09 independent observer | `../g09-companion/20261007T020340Z-prod/` | PASS 6/6 |
| G10 live single writer / crash | `../g10-sqlite-live/20261007T020446Z-prod/` | PASS: 58,895 acknowledged, 0 lost over 5 crash rounds; second writer refused (75); idempotent retry `ALREADY_COMMITTED` at its cursor |
| G08 baseline route | H-10 cited run | exact |
| G15 fresh WebGPU/pixels | H-11 cited run | attested WebGPU, changing canvas |
| G16 keyboard after transition | H-12 cited run | 9 labelled, reversible |
| Suites | `tests.txt` | all pass; clippy clean for companion and shell in release and qualification builds |

Not rerun because unaffected: G12 sleep/wake, the 15-minute G15 workload (renderer production code changed only by an `initPending` diagnostic field and a `generation-retired` report), the full M0B routing matrix.

## Qualification-only controls

Route barriers (`QualifyArmFault HOLD_NEXT_ROUTE_*`, `QualifyReleaseRouteBarrier`) exist only with the `qualification` feature: the contracts crate omits the variants, and a release-build test proves the requests do not parse. The shell's resource hold and its `on_web_resource_request` hook are feature-gated; a release shell answers the action `NOT_IMPLEMENTED`. The init gate wraps `GPU.prototype.requestAdapter` only when the launch is a qualification build; a unit test proves a release build never wraps it. Every hold resumes after a bounded timeout.

## Superseded runs

`attempts/<area>/<run>/ATTEMPT-NOTE.md` records each superseded run: a harness ordering defect (C-02 banner), the run that found C-11, two harness predicate defects (fullscreen frame), the inert instance-level init gate, outside input and a counter gap in H-11, and the VoiceOver keystroke and scripting development runs. Nothing was overwritten.

## Public-evidence privacy

Before publication, every file under `evidence/M0C` was scanned for the home path, the account name, owner session titles (the private redaction record plus the journal's non-harness titles; milestone-style titles are project vocabulary), e-mail addresses and credential patterns. Redactions, with originals kept outside the repository:

- the eight system logout-menu AX labels in four committed G16 trees: the account name → `[account]`;
- copied companion logs in C-02 runs: home paths → `~`, client executable paths → file names (the runner now writes them this way);
- two early VoiceOver attempts: focus labels other than the app's fixed controls → `digest:<sha256 prefix>` (the runner now writes them this way).

No technical value, count, timestamp or result was changed.

## Owner-environment side effects

The VoiceOver work affected the owner's Mac beyond Threadspace; see H-14 in `../failure-ledger.md`. While VoiceOver was on, Unreal Engine's crash reporter agent crashed twice (21:27, 22:26 local) and VoiceOver's Tutorial once took the front and consumed injected keystrokes; the alerts were dismissed and the Tutorial closed. A `forge` process under the owner's home also aborted at 22:34 during the cited run; it had crashed on earlier days too, and a causal link is not established. Threadspace's own state changed only by its qualification toggle.
