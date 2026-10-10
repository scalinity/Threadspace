# Independent F4 timer audit

This directory retains an actual TypeScript fault execution, not a native Claude or macOS run. The exact pre-repair runtime sources and their hashes are preserved. The original `missing-timer.mjs` was run from `/workspace/scratch/c8272833f682/review-checks/f4-timer-review/`; its relative import resolves the actual repository runtime. `failed-before-repair.log` preserves exit 1 and the discriminating assertion, while `failed-before-repair.json` preserves both observed cases.

With `clock.after()` returning no handle, the fast case sent a census confirmation anyway. With an otherwise individually bounded 40 ms mod-batch operation followed by a 90 ms census page, the actual `session.end` wrapper remained pending at 110 ms; the full observed fixture completed after about 135 ms. The normal end-drain ceiling is 100 ms. In both cases the original event/result references were preserved and `next(e)` ran once.

The minimum repair is to require a usable timer handle before awaiting optional census work. A missing/malformed handle or a throwing cancellation-property getter must leave census incomplete and preserve only the original drain. The same script was rerun after that repair and exited 0. These elapsed times describe a controlled Node fault fixture; they do not qualify native performance.

The rest of the targeted source audit found no additional concrete failure in original result/exception propagation, canonical typed-ACK isolation, independent native deadline anchors, or the positive close/echo protocol. The native helper retains the echoed challenge before checking its completion time, and the exporter requires that durable echo and matching hash before the original cutoff. A missing native anchor, census page/close/echo, or a late durable echo prevents a qualifying seal. Real clock-rate/precision evidence and normal-path closed populations for both producer sources remain separate native qualification obligations.

## Executed repair closure

The repair validates and caches a callable cancellation method inside the timer-registration guard. Missing handles, missing/noncallable cancellation methods, and throwing getters leave the optional source window unsealed and run only the original drain. Normal mode remains unchanged.

`passed-after-repair.json` and `.log` retain the exact independent script rerun: zero census commands/confirmations, `next(e)` once, original result preserved, and the result already settled when observed at 110 ms. The original script computes its final `elapsedMs` after that deliberate 110 ms wait, so this value is **harness observation duration, not result-settlement latency** in the passing run.

For an independently measured settlement time, the final project census suite was also executed directly under Node 26.11.1. `final-census-node-controls.log` reports **20 passed, zero failed**. The timed missing-watchdog control records actual promise settlement at **41.505 ms** in `final-census-node-witnesses/node-missing-watchdog-timed.json`, with no census call. Its seven invalid-watchdog controls cover undefined/null handles, missing/undefined/noncallable cancellation, a throwing cancellation getter, and a thrown timer call. These remain controlled portable fixture observations, not native timing results.

From a checkout, reproduce the final controls with:

```sh
node --test packages/provider-mod/tests/latency.node.mjs
```

The original independent script is archived exactly as executed and contains this review environment's module paths. `final-census-node-execution.json` binds the reproducible project tests and final runtime files to their actual source hashes. The source-matched Dev/native campaign, clock calibration, and real closed populations for both producers remain outstanding.
