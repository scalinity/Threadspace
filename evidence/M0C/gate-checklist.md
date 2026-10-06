# M0C gate checklist: consolidated G01–G17

**Candidate:** branch `m0c`, application commit `8c82212`, built clean and
installed for both identities (`install/20261006T140713Z-8c82212/`:
build-info with `sourceTreeDirty: false`, signatures with the holder name
redacted, executable hashes). Harness commits after it (`1b46091`,
`89d8572`) change only `tests/native`.
**Base:** `8558854` (M0B accepted on `main`).
**Target:** macOS 27.2 (26B5091g), Apple M5 Pro, built-in Retina panel
only. Claude Code 2.1.291; Terminal 2.15 (dictionary sha256 `cd4dad3e…`,
unchanged since M0A); SQLite 3.53.4; Three r186 on Tauri 3.0.0-alpha.4.

**Rule (MILESTONES traceability):** a gate is PASS only when each mapped
portion passes. Each row cites the M0A (`evidence/M0A/`), M0B
(`evidence/M0B/`) and M0C (`evidence/M0C/`) evidence for its portion. Every
M0C run below is on `8c82212` unless noted; failed and superseded runs are
under `attempts/` with their notes, and every failure is in
`failure-ledger.md`.

| Gate | M0A | M0B | M0C portion and result | M0C evidence |
| --- | --- | --- | --- | --- |
| G01 minimal build | PASS: pinned arm64 build, bridge/ACL compile (`final/build/`) | Reuse | **PASS** (regression): both identities rebuilt clean and signed from `8c82212`; Rust suites 156 passed / 0 failed, TypeScript suites 33 passed / 0 failed | `install/…-8c82212/`, `build/tests.txt` |
| G02 dev + package launches | PASS: one launch each (`final/dev/`, `final/packaged/`) | Reuse installed candidate | **PASS**: ten packaged launches (10/10, executable does not embed the checkout path) and ten `tauri dev` launches (10/10) | `g02-launches/20261006T141307Z-packaged-prod/`, `g02-launches/20261006T143510Z-dev/` |
| G03 request/response IPC | PASS: typed success/error, ACL smoke | PASS: real Return command | **PASS**: 1,000 round trips (p50 1 ms, p95 1 ms, max 9 ms, 0 failures); 27/27 negative cases (cancellation, close, malformed, stale/future context and ACK, concurrency bound, durable command retry/conflict); unlisted-view ACL probe and remote-origin probe refused everything | `g03-ipc/20261006T141342Z-prod/` |
| G04 sustained Channel | PASS: bounded snapshot, patch, ACK | PASS: real identity/binding changes | **PASS**: 10,000 committed changes in 59,996 ms, 0 failed, final projection equal, no backpressure retirement; view recovery 14/14 (document replacement, replacement with a request in flight, retirement with unconsumed cached frames behind a stalled ACK, ten repeated replacements, bootstrap with the companion frozen; retired window shells measured, C-04) | `g04-stream/20261006T141347Z-prod/`, `view-recovery/20261006T144300Z-prod/` |
| G05 notifications | PASS: permission/settings, granted and denied, UI-quit send | Reuse | **PASS** (recheck after lifecycle changes): denied authorization read natively, submission FAILED with the attention retained (dev identity) | `g05-notifications/20261006T143608Z-denied-dev/` |
| G06 notification interaction | PASS: UI-open/UI-quit callback, cold intent | PASS: Return path connected | **PASS**: ten real banner interactions (UI open, UI-quit cold start, helper restarted, attention already resolved, real-session exact Return, target changed, response during companion recovery, maintenance prepared, observation-stopped cold start, UI open after recovery) plus the login item's hand-over after re-enable (11/11) | `g06-notifications/20261006T140836Z-prod/` |
| G07 native mechanisms | PASS: bounded argv, peer checks, bridges | PASS: Claude ancestry, birth/executable, controlling device | **PASS with one decision open**: supervised-launch detection (`XPC_SERVICE_NAME`), writer-lock forwarder and hand-over exercised natively (G06, G10); Codex runtimes/daemon probed read-only (`codex/`). Codex detached-hook ancestry is **NOT_RUN by D-0004** (M0C forbids starting provider work; M6 runs it), open for reviewer confirmation | `codex/README.md`, `docs/decisions/D-0004-…` |
| G08 Terminal inspection/return | PASS: dictionary, bridge availability | PASS: three same-cwd sessions, 30/30 routes, reorder/move/close/recreated tab | **PASS**: baseline, reorder, move, minimize then return (by recorded window ID), fullscreen Space then return, foreground-process mismatch (shell job stopped, shell holding the foreground), five-route selection/readback race (2 exact and independently confirmed, 3 refused READBACK_FAILED while the racing window was front; 0 wrong, 0 unverified), target closed, stale TTY pathname, target closed during the route; Terminal incarnation unchanged. Terminal restart **BLOCKED** (C-08) | `g08-terminal/20261006T144151Z-prod/` |
| G09 independent observer | PASS: single supervised app, UI quit and dev restart | Reuse | **PASS**: UI Cmd-Q, SIGTERM and SIGKILL each left the same companion capturing durably, and its own discovery loop recorded a new disposable Claude session with no UI or hook client; three companion SIGKILLs with the UI closed were relaunched by the system with one writer and resumed capture (6/6) | `g09-companion/20261006T141203Z-prod/` |
| G10 SQLite persistence | PASS: one writer, WAL/FULL, commit/ACK, reopen | PASS: correlation records persisted | **PASS**: second writer refused (exit 75, incumbent unchanged); five live crash rounds, 57,799 acknowledged records, 0 lost; idempotent retry across restart. Crash-boundary and checkpoint runner: 0 lost acknowledged records and 0 duplicates in every case (before, in and after transaction, after receipt, spill, concurrent backup, restore) | `g10-sqlite-live/20261006T140951Z-prod/`, `sqlite/summary.json` |
| G11 restoration | — | PASS: Session continuity | **PASS**: ten UI/helper restart cycles (UI, helper, both, capture with the UI absent, same-label view) 10/10 | `g11-restarts/20261006T141850Z-prod/` |
| G12 sleep/wake | — | — | **PENDING OWNER ACTION** (C-10): five actual cycles need a root-scheduled wake | — |
| G13 renderer init | PASS: TSL scene, dev and package | No art dependency | **PASS** (regression): packaged WebGPU attestation (Three r186, `tauri://localhost`, secure context) | `g13-g14-renderer/20261006T141258Z-prod/` |
| G14 actual WebGPU | PASS: attestation, forced-WebGL2 negative | Reuse | **PASS** (final package): WebGPU gate PASS; forced run attests `WEBGL2_COMPATIBILITY`, gate FAIL, and shows the diagnostic badge (read back) | `g13-g14-renderer/20261006T141258Z-prod/` |
| G15 packaged animation | PASS: brief changing scene | No sustained prerequisite | **PASS**: 906 s of packaged output with the lifecycle exercised (minimize disposes and stops frames; return rebuilds a fresh attested renderer; 2D disposes with the DOM operational; hide during init revives nothing; injected visible loss one bounded rebuild; repeated loss falls back operational; loss while hidden revives nothing; repeated hide/show fresh generations; reload with a resource in flight); DOM/journal projection equal every minute; canvas-only pixels changed in 111 of 123 live pairs and differed in all 123 (the 12 below the 0.0005 threshold are sample-period aliasing of the marker's spin and pulse, smallest difference 8e-6). Every device loss was qualification-injected and labelled so | `g15-graphics/20261006T144536Z-prod/` |
| G16 native window/titlebar | PASS: traffic lights, drag, minimize | PASS: real focus interaction | **PASS**: traffic lights, focus, content vs titlebar drag, minimize/restore focus, zoom, fullscreen enter/exit, keyboard navigation after those transitions (Tab through 6 labelled controls, Shift-Tab back), accessibility tree (125 content buttons labelled, headings and regions), bounds across relaunch, off-screen correction, Reduce Motion (canvas moving, then still; renderer and companion follow; switch restored), device-pixel-ratio change 2x -> 1x -> 2x (drawing buffer follows, display restored). Display disconnect **MANUAL_EXTERNAL_REQUIRED** (C-09) | `g16-window/20261006T140724Z-prod/` |
| G17 final platform disposition | PASS: open risks recorded | GO: correlation | **Candidate, pending review**: see the disposition below | this file, `failure-ledger.md`, `manifest.json` |

## Provider semantics (M0C cell)

| Facet | Result | Evidence |
| --- | --- | --- |
| Claude 2.1.291 observer profile | Declarations generated and pinned; accepted-input provenance qualified (`true`); automatic human-follow-up resolution NOT_SUPPORTED (no positive original-order witness), so explicit Mark handled remains the path | `provider/claude/`, `docs/decisions/D-0005-…` |
| Codex modes and daemon | Read-only probe: shared daemon not running, embedded CLI 0.160.0 installed, desktop-local stdio only; schemas recorded | `codex/`, `docs/decisions/D-0004-…` |

## G17 disposition

Every required M0C portion passes on `8c82212` except G12, which waits on one
owner action. Three implementation defects found by this qualification are
fixed and re-qualified (C-01 device pixel ratio, C-02 companion supervision
after re-enable, C-03 keyboard focus after fullscreen/zoom). One measured
open risk remains (C-04: retired office window shells, about 33 KB each, web
content released), which does not invalidate the architecture. Two cells
cannot be run safely on this machine (C-08 Terminal restart BLOCKED, C-09
display disconnect MANUAL_EXTERNAL_REQUIRED), and two provider facets are
decided rather than passed (D-0004, D-0005); each needs the reviewer's
acceptance. M1 is not started.
