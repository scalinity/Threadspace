# M0C remediation: C-02, H-10, H-11, H-12

Focused remediation of the independent review recorded at `24035492eed28c342d09c36b52aed61c2bc17a53`, independently re-reviewed at candidate `ccdfc8a38773aac01121601bde6121dd1760b6ea`. **M0C REMEDIATION REQUIRED:** H-10–H-12 close, but C-02 handoff intent preservation and C-11 whole-route deadline enforcement need code repairs. Valid native workloads remain credited. M0C stays unmerged; M1 has not started. D-0004–D-0006 and C-04 are unchanged.

## Builds under test

| Build | Source | Use | Install record |
| --- | --- | --- | --- |
| `8c82212` | previously reviewed M0C application | negative controls only (pre-repair) | `../install/20261006T140713Z-8c82212/` |
| `a6da2f1` | C-02 repair + qualification barriers | intermediate; superseded runs only | `../install/20261007T003651Z-a6da2f1/` |
| **`89fce79`** | application sources as at `c3f6de1` (adds the Return readback settle and the `GPU.prototype` init gate); clean tree | **every cited run** | `../install/20261007T005438Z-89fce79/` |

Candidate executables (sha256): prod outer `83508af5…`, prod companion `a384f366…`, dev outer `8fbf7de1…`, dev companion `0a24a986…` (full values, signatures with the certificate holder redacted, build info and the post-install supervision smoke of both identities are in the install record). No lockfile or toolchain changed since `8c82212`.

Application commits: `d0598a5` (C-02 supervision), `a6da2f1` (qualification-only barriers), `88817eb` (C-11 Return readback settle), `c3f6de1` (init gate on `GPU.prototype`). Harness commits: `4c1d1c9`, `d23ae65`, `89fce79`, `2861825`, `0880017`, `2df7316`, `0ac5c2e`, `ec8fd26`, `1ac8b9c`.

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
