# C-04 — retired office window shells

Each run is `threadspace-m0c view-recovery prod <N>` with
`THREADSPACE_EVIDENCE_MILESTONE=M1`. The runner writes
`evidence/M1/view-recovery/<UTC>-prod/`; each run directory was moved here
unchanged and renamed after the build it ran against. `environment.json` in
every run holds the installed app and companion executable hashes.

| Run | App build (commit) | App sha256 | N | Office shells before → after | Footprint before → after (bytes) | WebContent | Cases |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `20261007T231508Z-baseline` | `cd9e376` (accepted M0C) | `19efc692…` | 20 | 9 → 29 (all layer-0) | 35,505,304 → 46,957,792 | 1 → 1 | 21/24 |
| `20261007T233730Z-diagnostic` | `76bcaa7` | `feaab22d…` | 5 | 5 → 14 (all layer-0) | 30,475,416 → 34,161,840 | 1 → 1 | 9/9 |
| `20261007T234229Z-experiment-release` | `e47bd16` | `0006df7b…` | 5 | 2 → 6 (all layer-0; office 1 → 1) | 28,312,728 → 31,868,080 | 1 → 1 | 6/9 |
| `20261008T000018Z-oneshot-on-close` | `e57bbea` | `bf97cdcb…` | 5 | 4 → 9 | 29,983,848 → 34,260,168 | 1 → 1 | 9/9 |
| `20261008T000903Z-oneshot-order-out` | `f1abfa8` | `415217a1…` | 3 | 4 → 7 | 31,868,056 → 33,883,336 | 1 → 1 | 7/7 |
| `20261008T002643Z-final-released-when-closed` | `26f4577` | `c47efcaa…` | 20 | 1 → 1 | 31,818,904 → 31,163,520 | 1 → 1 | 24/24 |

The first three runs used the harness before `eec546f` and counted every
layer-0 window of the UI as a shell. AppKit also gives the process windows
of its own: four off-screen 1168×26 menu-bar strips appeared after a
concurrent display-mode change (experiment run, between `before` and the
first repeated case; diagnostic run, at case 5), and a 500×500 text-input
window is always present. From `eec546f` on, shells are the windows titled
like the office window (the live view included) and every layer-0 window
is recorded beside them.

## Root cause

- Diagnostic (`76bcaa7`): after each recovery every retired office window is
  still alive, with tao's delegate and the WKWebView gone and its content
  view alive; `retainCount` is 5 just before destruction and 2 afterwards,
  one of which is the probe's own load. AppKit's window list still holds the
  closed `TaoWindow`, and CGWindowList counts it.
- Experiment (`e47bd16`): one `release` sent once tao's delegate is gone
  deallocates the window and its content view, removes it from AppKit's
  window list and keeps office windows flat. The one remaining reference is
  the only thing keeping the shell alive.
- Source: `tao-0.37.0/src/platform_impl/macos/window.rs` `create_window`
  sends `alloc` and `initWithContentRect:…` into a raw `id` (+1, owned by
  nobody) and then wraps it with `Retained::retain` (+1, owned by tao). tao
  releases only its own reference; `setReleasedWhenClosed(false)` keeps
  AppKit from releasing the other. tao 0.37.1 and the tao `dev` branch
  (read 2026-10-08) have the same lines.
- `oneShot`, alone or with an explicit `orderOut:`, does not free the closed
  window's device: each retired window keeps its window number and shell.

## Final run (`26f4577`)

All 23 retired windows deallocated (windows, delegates, content views and web
views alive: 0). Office shells 1 → 1 at once and after settling, layer-0
windows 6 → 6, one WebContent process before and after, footprint samples
30.5–32.8 MB with no trend across the 20 recoveries. Repeated recoveries ran
back to back, so the rate limit delayed them 5–20 s; during each delay the
reloaded old document's `ui_connect`/`ui_query` retries were refused as
`STALE_VIEW` (5–7 per case).

Earlier runs on a busy machine recorded some recoveries as
`UNCONSUMED_DATA_ON_RETIREMENT` where the case expects
`MAIN_DOCUMENT_REPLACED` (baseline cases 01, 04, 06; experiment cases
in-flight, 01, 05). Each of those views was still recreated, hydrated and
projection-equal; the reason depends on unconsumed data at the moment of the
reload, not on window destruction.

## On the M1 build (`73636ec`, D-0008)

The containment above, merged into `m1` and kept under D-0008, qualified on
the M1 application build (app `3b6a4e79…`, companion `510daaa0…`, store
upgraded to schema 3). These runs stay where the runner wrote them, in
`evidence/M1/view-recovery/`; each later one follows a runner change made
after the one before it:

| Run | N | Outcome | What changed next |
| --- | --- | --- | --- |
| `20261008T012315Z-prod` | 20 | Aborted at case 3: its recovery came 110 s after the trigger, past the 60 s window, while another application was in front | Each case raises the office window behind the idle gate: WebKit suspends a covered page's timers and with them the view's stall watchdog |
| `20261008T012838Z-prod` | 20 | 24/24 cases, shells 1 → 1, everything held except footprint before → after +5.9 MiB against a 4 MiB bound | Footprint judged by its least-squares trend, each sample taken with the window raised for a second |
| `20261008T013530Z-prod` | 60 | 64/64 cases, shells 1 → 1; whole-run slope 0.116 MiB per recovery against 0.1: one step from about 28 to 32 MiB at recovery 14, then flat for 46 recoveries (medians by third 29.1, 32.6, 32.6; second-half slope 0.07) | The gate became the second-half slope, since a leak costs every recovery alike (the pre-repair leak measures 0.567 there) |
| `20261008T015231Z-prod` | 60 | **PASS**: 64/64 cases; office shells 1 → 1 → 1; all 63 retired windows, delegates, content views and web views freed; WebContent 1 → 1; footprint slope 0.075 MiB per recovery over the whole run and 0.017 over the second half (medians 29.7, 32.1, 32.1); bounds and visibility kept; durable intent backlog unchanged; IPC suite 27/27 and the retired view's subscription refused (`UNKNOWN_SUBSCRIPTION`) in the recreated view | — |

The four extra layer-0 windows in the passing run (2 → 6) are AppKit's
off-screen 1168×26 menu-bar strips described above, not office windows.
