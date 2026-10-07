# M1 C-12 — renderer asset outcome accounting

**Result: CLOSED.** After quiescence every started asset has exactly one terminal outcome, the
pre-scene receipt path included, and every received asset is disposed exactly once. Diagnostics
only: no lifecycle decision reads the asset counters, and no resource is held longer than before.

## Defect

`evidence/M0C/failure-ledger.md`, row C-12: a texture received before its generation's scene
exists is disposed on retirement without moving an asset outcome counter, so
`started - (applied + aborted + discardedLate + failed)` grows by one per such generation.

In the accepted controller (`cd9e376`, `packages/scene/src/lifecycle.ts`), `loadAsset()` held an
asset that arrived while `entry.scene === null`, `build()` counted `applied` only if it later
built a scene, and `retire()` disposed `entry.asset` with no counter. Every path that retires a
generation between receipt and scene construction lost the outcome:

| Path | Before | After |
| --- | --- | --- |
| Confirmed hide, 2D, or detach while `init()` is pending, then the late init is discarded | no outcome | `releasedBeforeScene` |
| `init()` rejects (fallback, or `retired-init-failed`) | no outcome | `releasedBeforeScene` |
| `createScene()` or `scene.update()` throws (scene construction failed) | no outcome | `releasedBeforeScene` |
| Device loss before the scene exists | no outcome | `releasedBeforeScene` |
| `scene.applyAsset()` throws inside `build()` (falls back as before) | no outcome | `failed` |
| `scene.applyAsset()` throws on a live scene (was an unhandled rejection) | no outcome | `failed`, reported as `asset-failed`; the generation stays live |

No path counted an outcome twice. The other paths (`applied`, `aborted`, `failed` request,
`discardedLate`) were already correct and are unchanged.

## Repair

- `packages/scene/src/lifecycle.ts`
  - `AssetCounts` gains `releasedBeforeScene` (received and held, then released on retirement
    before any scene applied it) and `disposed` (`dispose()` calls on received assets). Existing
    fields keep their names and meaning; `failed` also covers a scene that throws while applying.
    The doc comment states the quiescence identity
    `started === applied + aborted + discardedLate + failed + releasedBeforeScene`.
  - `Generation.assetSettled` records whether the held asset already has its outcome.
  - New `applyAsset(entry, scene, asset)`: the one place an outcome is recorded for a held asset
    (`applied`, or `failed` and rethrow). `build()` calls it, so a throwing scene still reaches
    `fail("scene construction failed: …")` exactly as before.
  - New `disposeAsset(asset)`: the lifecycle's only call to an asset's `dispose()`; counts
    `disposed`. Called from the late-arrival path and from `retire()`.
  - `retire()` clears `entry.asset` before disposing it and counts `releasedBeforeScene` (and emits
    `asset-released-before-scene`) when the held asset was never settled.
  - `loadAsset()`'s resolve path applies through `applyAsset()`; a scene that throws is reported as
    `asset-failed` instead of escaping as an unhandled rejection.
- `packages/scene/src/threePlatform.ts` `loadAsset()`: the returned `dispose()` is idempotent, so
  `Texture.dispose()` and `ImageBitmap.close()` run at most once structurally, whatever the caller.

The snapshot shape is additive. Consumers in `apps/desktop` (`App.tsx`,
`OfficeFallback2D.tsx`, `rendererQualification.ts`) only read `RendererLifecycleSnapshot`; none
constructs one or reads `asset` fields by name.

Not changed (outside this repair's file ownership): `outstanding_assets()` in
`tests/native/harness/src/bin/m0c/graphics_overlap.rs` computes
`started - applied - aborted - failed - discardedLate`. It reads the same value it read before
this repair, because `releasedBeforeScene` counts exactly the residue it used to see. To read zero
after quiescence it should also subtract `releasedBeforeScene`; that is a one-line change for the
owner of that runner.

## Tests

`packages/scene/src/lifecycle.test.ts`. The fakes gained `FakeAsset.disposeCalls` (a raw call
count, so the lifecycle disposing twice is visible), `FakeEnv.failNextScene`, and
`FakeEnv.failApply` / `FakeScene.failApply`. New `describe("RendererLifecycle asset outcome accounting")`:

| Test | Asserts |
| --- | --- |
| releases an asset received before its scene once when the generation retires before a scene exists | asset delivered during a held `init()`, confirmed hide retires the pending generation, init then settles: no scene, `disposeCalls === 1`, counts exactly `{started 1, releasedBeforeScene 1, disposed 1}`, one `asset-released-before-scene` report |
| releases an asset received during init once when the init rejects | `failed-fallback`, renderer never disposed, asset disposed once, `releasedBeforeScene 1` |
| releases a held asset once when scene construction fails | `createScene` throws: fallback reason `scene construction failed`, renderer disposed once, asset disposed once, `releasedBeforeScene 1` |
| releases a held asset once when a device loss retires the generation before its scene | loss during init (`outcome: rebuild`), late init discarded, generation 2 live; asset 1 disposed once; generation 2's asset applied; counts `{started 2, applied 1, releasedBeforeScene 1, disposed 1}` |
| counts an asset the scene throws on as failed, once, and still disposes it once | `applyAsset` throwing during `build()` (falls back) and on a live scene (stays live, one `asset-failed`): `failed 1`, `releasedBeforeScene 0`, disposed once |
| gives every started asset one outcome and one disposal across seeded random interleavings | see below |

The randomized test uses mulberry32 with seed `0xc12` (3090) and 600 iterations. Each iteration
runs a fresh lifecycle with `init()` held open and 6–23 random steps drawn from: resolve an asset
request, reject one (an `AbortError` if its signal was aborted, else an HTTP failure), resolve or
reject a pending `init()`, hide (hidden or minimized), show, enter 2D, leave 2D, device loss on the
newest renderer, make the next `createScene` throw, and toggle `applyAsset` failure. Between steps
it either yields a full macrotask or 0–5 microtasks, so actions also land mid-chain. It then drives
to quiescence by settling every outstanding init and request until the lifecycle starts no more,
and asserts:

- `initPending === null`; `started === ` the number of requests made;
  `applied + aborted + discardedLate + failed + releasedBeforeScene === started`;
- every delivered asset has `disposeCalls <= 1`, at most one is undisposed, and that one belongs
  to `liveGeneration`; `asset.disposed === delivered - held`;
- after `detach()` and `settled()`: every delivered asset has `disposeCalls === 1`,
  `asset.disposed === delivered`, and the identity still holds.

At the end it asserts that each of the five outcomes occurred at least once over the run.

## Commands and observed results

Vitest includes `packages/scene` (`vitest.config.ts` excludes only `node_modules`, `dist`,
`.claude`, `target` and `packages/provider-mod`). Run from the repository root:

```
npx vitest run --reporter=verbose
```

- **43 passed, 0 failed** (3 files): 30 in `packages/scene/src/lifecycle.test.ts` (24 existing, 6
  new), plus the other two suites unchanged. The randomized test took about 4.2 s.
- Randomized run totals (seed 3090, 600 iterations, 8,573 steps): 908 started = 33 applied + 219
  aborted + 356 discardedLate + 165 failed + 135 releasedBeforeScene; 538 assets delivered, 538
  disposed, each exactly once; 1,200 outcome-identity checks (two per iteration), all held. The
  totals were identical on every run, as expected from a fixed seed.
- Control run, the new tests against the accepted M0C controller (`cd9e376`): **6 failed,
  24 passed**. The randomized test fails at iteration 0, where the outcome sum is `NaN` because no
  pre-scene outcome existed.
- Mutation checks on the repaired controller (each reverted afterwards), randomized test only:
  storing a late asset as held as well, so that a retirement in progress disposes it twice, fails
  at iteration 2 (dispose count 3 against 2 deliveries). Dropping the `releasedBeforeScene`
  increment fails at iteration 5 (outcome sum 1 against 2 started).

`test-output.txt` holds the full verbose run, then the control-run and mutation-check summaries.
`tsc` and app bundle builds were not run.
