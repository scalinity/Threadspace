# M0C failure ledger

Every failure met while qualifying M0C natively, in order of discovery, with
its classification, evidence and disposition. Superseded runs are kept under
`attempts/`, each with an `ATTEMPT-NOTE.md` giving its build and diagnosis.
Nothing was overwritten to make the final runs look clean.

Classes are the M0B failure-policy categories: implementation defect,
permissions/config issue, provider limitation, macOS limitation, SPEC
assumption failure. Harness defects are listed separately: they changed what
a run could prove, not what the product does.

Builds: `26ce728` and `a6ef67a` (pre-checks), `a983837` (smoke), `5a6149a`,
then the official `8c82212`. Harness fixes after `8c82212` (`1b46091`,
`89d8572`) touch only `tests/native`; the installed application is unchanged.

## Product and platform

| ID | What failed | Class | Evidence | Disposition |
| --- | --- | --- | --- | --- |
| C-01 | The renderer set its pixel ratio once, at creation. A window whose display changed scale kept drawing at the old ratio (blurry on a move to a Retina display) until a rebuild. Found when G16's DPR cell, recorded NOT_RUN on an unmeasured "every mode is 2x" claim, was made real: the built-in panel has 60 usable 1x modes once low-resolution duplicates are listed. | Implementation defect | `attempts/g16-window/20261006T122912Z-prod/` | Fixed in `7e8043d`: the platform re-applies the capped ratio on every resolution change. G16 `dpr-change` passes on `8c82212` (2x -> 1x -> 2x with the scene live, display restored by an app-scoped mode that macOS reverts when the helper exits). |
| C-02 | After Stop Observation, a notification click cold-starts the agent app through LaunchServices. On re-enable, the login item's own instance found the writer lock held, forwarded a pending response and exited 0; launchd treats 0 as a successful exit and did not relaunch it, while the cold-started instance accepted `SetObservationEnabled` and became the enabled writer. A crash then left no companion (G10 round 1: `Connection refused`). Contradicts SPEC §18.9 and §19.5. | Implementation defect | `attempts/g10-sqlite-live/20261006T135257Z-prod/` (launchd log: `removing service`, then `application.ai.scalinity.threadspace.agent…` spawned "because launch job demand") | Fixed in `d602c6b`: the core records whether launchd started it as the login-item job (`XPC_SERVICE_NAME`); an unsupervised instance refuses the enable with `NotSupervised` and exits with observation disabled; a forwarder never exits 0; the bootstrap's enable waits for the login item's companion. G06 `enable-hands-over-to-login-item` passes on `8c82212` (one refusal, hand-over in 15.2 s, launchd's login-item PID is the running companion's); G10 passes. |
| C-03 | Entering and leaving fullscreen, or toggling zoom, left the window itself as first responder, so Tab no longer reached the office until a click. | Implementation defect | `attempts/g16-window/20261006T133328Z-prod/` | Fixed in `8c82212`: the shell makes the web view first responder again on resize or key focus (never raising the window). G16 `keyboard-navigation` passes on `8c82212` after the fullscreen and zoom steps (6 distinct labelled controls, Shift-Tab back). |
| C-04 | Each office-view recovery leaves the retired window registered with the window server, off screen at the same size, after Tauri alpha.4 reports it removed. Measured: +1 window per recovery and about 33 KB of physical footprint each (6 recoveries: +6 windows, 34.0 MB to 34.2 MB); the WebContent process count stayed at 1, so web views, their content processes and per-view caches are released. | Implementation defect (framework destroy path) | `attempts/g15-graphics/20261006T142000Z-prod/ATTEMPT-NOTE.md`; `view-recovery/…/summary.json` `nativeWindowShells` on `8c82212` | **Open risk**, not blocking: bounded memory, recoveries are rare and rate-limited, and nothing the recovery exists to release survives. Owner: M1 (or the next Tauri pin): close the retired NSWindow through the framework, then require `nativeWindowShells.after == before`. |
| C-05 | Claude Code 2.1.291's folder-trust dialog preselects "No, exit"; a disposable session answered with Return declined it and exited. | Provider limitation (behaviour change) | `attempts/g09-companion/20261006T122110Z-prod/ATTEMPT-NOTE.md` | The harness selects "Yes, I trust this folder" explicitly (Down, Return) in its own disposable tab. |
| C-06 | macOS 27 platform facts that invalidated harness assumptions: WebKit builds a web view's accessibility tree on the first request (the first read lists only native chrome); Reduce Motion is under Accessibility > Motion as an unlabelled switch after its row text; plain Tab walks WebKit controls while Option-Tab does not move focus. | macOS limitation | `attempts/g13-g14-renderer/20261006T122629Z-prod/`, `attempts/g16-window/…` | Harness reads wait for the web area, find row switches by their row text, and walk with plain Tab. |
| C-07 | Observation: a re-subscribed view can receive its sessions in a different order, and the office lays workers out in view order, so workers change desks after a minimize/restore. | SPEC assumption (layout stability unstated) | `attempts/g15-graphics/20261006T125650Z-prod/ATTEMPT-NOTE.md` | Not an M0C criterion; noted for M1's office layout (stable worker placement). |

## Owner-limited cells

| ID | Cell | Status | Reason |
| --- | --- | --- | --- |
| C-08 | G08 Terminal restart | BLOCKED | Quitting Terminal.app ends every Terminal session on this Mac, including unrelated owner sessions and the one running the harness. Covered meanwhile by route-model tests and M0B's closed/recreated-tab evidence. |
| C-09 | G16 display disconnect | MANUAL_EXTERNAL_REQUIRED | The only display is the built-in panel; the case needs an external display physically attached and removed. Display reconfiguration and DPR change are covered natively. |
| C-10 | G12 sleep/wake | see the gate checklist | Waking needs a root-scheduled power event (`pmset schedule wake`), one owner command; the harness then sleeps, wakes and verifies unattended. |

## Harness defects

| ID | What failed | Evidence | Fix |
| --- | --- | --- | --- |
| H-01 | G03's cancellation case retired a subscription after the 12-page burst; replies above 8 KiB make the shell recreate the view (SPEC §18.5), killing the suite before it reported. | `attempts/g03-ipc/20261006T112507Z-prod/` | `9548043`: cancellation runs first. |
| H-02 | View recovery awaited hydration of the first attached subscription, a deliberately stalled one. | `attempts/view-recovery/20261006T114242Z-prod/` | `a6ef67a`: await a hydration at or after the recovery event. |
| H-03 | G09 waited for any `DISCOVERY_PASS`, which discovery logs only when a pass commits a change. | `attempts/g09-companion/20261006T115522Z-prod/` | `8e5481c`: a disposable Claude session gives the loop a real change; no UI or hook client may connect. |
| H-04 | G11 deadlocked on its own GUI lock (a nested `flock` in one process). | `attempts/g11-restarts/20261006T115625Z-prod/` | `8e5481c`: the lock is re-entrant per thread. |
| H-05 | G13/G14 read the badge before WebKit built its accessibility tree. | `attempts/g13-g14-renderer/20261006T122629Z-prod/` | `00a316d`: `Native::ax_tree` waits for the web area. |
| H-06 | G15 compared whole windows of a settled, motionless office (DOM text counted as animation); then its fixture slid off camera after re-subscriptions; then it captured the retired window after a recovery. | `attempts/g15-graphics/…` (three notes) | `a983837`, `5a6149a`, `89d8572`: centre-worker attention fixture re-checked per phase, canvas-only comparison, on-screen window preferred. |
| H-07 | G16 counted the system window buttons as unlabelled, could not find the Reduce Motion switch, read the renderer once, and assumed Option-Tab. | `attempts/g16-window/…` | `7e8043d`, `a983837`, `5a6149a`, `8c82212`. |
| H-08 | G08 looked windows up by a title Claude overwrites; its foreground case stopped an `exec`'d session whose orphaned process group discards `SIGTSTP`, and then typed `fg`, which reached that disposable Claude session as a prompt (one model turn in a throwaway session); its race scoring counted an unreadable readback as a wrong target. | `attempts/g08-terminal/…` | `d602c6b`, `1b46091`: recorded window IDs, a shell-job session stopped with `SIGSTOP` and `fg` only when the shell holds the foreground, and wrong (different TTY) separated from unverified (no readback). |
| H-09 | `Tab::close` reported `closed: false` when Terminal had already closed the window as its shell exited. | `attempts/g09-companion/20261006T125433Z-prod/` | `5a6149a`: reports whether the window still exists. |
