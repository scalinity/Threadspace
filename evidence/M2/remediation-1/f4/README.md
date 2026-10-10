# F4 — actual commit/DOM boundaries and complete latency populations

**Status: INCOMPLETE. No native latency PASS is claimed.** The implementation,
portable tests and retained negative controls below correct the identified
measurement defects. A complete independently closed hook capture population,
positive macOS clock qualification and a source-matched native end-to-end sample
are still required. Linux execution does not establish those native facts.

## Why retained telemetry cannot repair the old PASS

The rejected harness subtracted `captured_wall_ms` from `received_wall_ms`, but
the receipt timestamp was assigned before journal admission and SQLite COMMIT.
The original native summary has 81 hook observations and 225 observer
observations; only 127 observer observations have an exact DOM-cursor match.
Neither a post-COMMIT timestamp nor a complete explanation for the other 98
observer observations exists in that retained measurement. The observer count
gap alone does not show that 98 visible updates were slow: some observations may
have produced no projection change or shared a render. The old data does not
establish which explanation applies to each observation.

Those raw results remain unchanged in
[`vertical/20261009T041131Z-dev/summary.json`](../../vertical/20261009T041131Z-dev/summary.json).
The old status documents remain exact copies under `../historical/`. Their
normal-path PASS is withdrawn; the data cannot reconstruct the missing COMMIT
boundary or establish qualified cross-runtime monotonic timing after the fact.

The old `m2.rs::latency` calculation now explicitly returns INCOMPLETE with
`normalPathPass:false`. It cannot silently certify only the hook source. Ordinary
vertical workflow assertions remain separate from this latency gate.

## Measurement boundaries

### Actual journal commit

`crates/journal/src/canonical.rs` samples the native monotonic clock immediately
before `tx.commit()` and again only after it successfully returns. The optional
`BatchOutcome.commit_timing` reports `SQLITE_COMMIT_CALL`, clock identity, and
decimal nanosecond bounds. A missing or reversed clock produces missing evidence,
never a zero duration. The field exists only with `qualification`; it is not
persisted as a canonical fact or used by the reducer.

The companion writes one `M2_COMMITTED_MEASUREMENT` metadata row per record after
that return and before broadcasting the batch projection. The row includes the
original observation UUID, source/epoch and capture clock, actual receipt status,
cursor, store generation, boot identity, commit bracket and batch patch cursor.
Socket receipt, transaction entry and receipt dispatch are not called COMMIT.
The common patch cursor means all admitted records in a projection-changing
batch can join that batch's actual render. It does not claim each metadata
record independently changed a worker's visible appearance.

`crates/journal/tests/qualification_latency.rs` runs real SQLite admission,
checks the committed observation from a separate read-only connection and checks
a stable UUID retry. The latency unit test also installs SQLite's actual commit
hook in a disposable journal and requires its in-COMMIT clock witness to lie
inside the reported bracket. The hook's five millisecond delay is test-only.

The isolated mutation in [`commit-boundary/`](commit-boundary/) moves the end
stamp before COMMIT **without changing the boundary label**. The actual SQLite
test fails on its timing assertion. This control discriminates the original
boundary error instead of accepting a renamed timestamp. The exact patch, raw
failure, compiled-source hashes and successful control are retained.

### Applied, hydrated DOM

`apps/desktop/src/qualification/latencyStore.ts` retains store application and
DOM application as different times. A `MutationObserver` in `m2.ts` reads the
actual rendered diagnostics cursor after React's DOM update. Store receipt or a
client subscription callback alone cannot produce a DOM mark. Marks require a
live hydrated view, unchanged core/store/view identity and visible document.

If React coalesces updates, a rendered cursor may cover earlier committed patch
cursors in the same view epoch. The collector preserves that actual render time
and cursor. It uses `BigInt` cursor comparison, bounded storage with no eviction,
explicit overflow/invalid counters and immutable pages after stop. View/epoch
change leaves samples unmatched and invalidates the window. Pagination is
bounded to 100 records per response, with all 4,096 retained slots accounted for.

The full desktop execution records are in [`../ui/`](../ui/). They establish
portable store/DOM-collector logic, not macOS WebView painting or native timing.

## Clock method and conservative bounds

On macOS, the helper, harness and journal use `CLOCK_UPTIME_RAW` on the recorded
boot. Portable journal tests use `CLOCK_MONOTONIC` and explicitly identify that
Linux clock. Browser and provider JavaScript measurements use their own
`performance.now()` runtime. The calculator never subtracts unrelated origins
or uses `Date.now`, wall time, `performance.timeOrigin`, or writer receipt time.

The observer records a JavaScript interval around the actual mod-batch process
call. After obtaining a real typed receipt, the native helper retains a native
clock stamp independently and returns only its join token with that receipt.
An observer report alone cannot invent the corresponding native helper record.
Observation UUIDs and receipt statuses must match that native record.

The native harness brackets each UI clock query with native clock readings
before and after the query. Multiple queries before and after the capture window
bound the JavaScript/native mapping. DOM samples outside the calibration span,
wrong boots/runtimes, incompatible intervals or changed identities fail.

The calculation also requires a positively supported clock-rate error bound and
precision bound, with primary qualification evidence. Bracketing by itself does
not prove relative clock rate or clock precision. The native exporter writes
`qualified:false` and missing bounds until that evidence exists. Setting a
boolean by hand is not qualification. The precision bound must include runtime
resolution and numeric conversion/rounding error.

For capture interval `[Clo, Chi]`, COMMIT-call interval `[Jlo, Jhi]` and applied
DOM interval `[Dlo, Dhi]`, the reported conservative upper bounds are:

| Metric | Bound |
| --- | --- |
| Capture → durable commit | `Jhi - Clo` |
| Commit → applied DOM | `Dhi - Jlo` |
| Event/capture → applied DOM | `Dhi - Clo` |

Clock intervals include the qualified rate/precision uncertainty. The calculator
retains each UUID/cursor join and interval, so another reviewer can recompute the
nearest-rank p50, p95 and maximum from the raw samples. Clock uncertainty counts
toward the upper bound; it is not subtracted to make a target pass.

## Population, loss and inclusion rules

`tests/native/tools/m2_latency.py` requires both `claude.hook` and
`claude.observer`, with nonempty applicable metric populations. For each source
it reports captured UUIDs, durable acceptance, deduplicated attempts, rejected
observations, projection-changing batches, batches with no projection change,
matched/unmatched DOM samples, missing telemetry and explicit exclusions.

A retry is another attempt for the same stable UUID, not another capture. A
typed helper receipt alone is not the journal's original durable-commit timing.
Local spool acceptance alone is not COMMITTED. Missing capture/commit/DOM
telemetry is not an exclusion, and all such cases prevent a normal-path PASS.
A batch with no public projection change can be excluded from DOM metrics with
that exact reason; it remains in capture-to-commit accounting. Rejected
observation records are counted explicitly, distinct from a committed native
event reporting that a human input was rejected.

Each source also needs a closed capture census: final count, exact UUID digest,
boot/runtime identity, explicit close boundary and zero unaccounted telemetry
failures. A previously successful export cannot establish that nothing was lost
at the end of the window. The observer's bounded close protocol and typed native
verification retain the full source ledger and final receipt confirmation.
Missing page, final export, ACK, digest, close boundary or runtime prevents a
sealed observer population. Its precise source-boundary assumptions and focused
tests are retained with the census evidence.

The production qualification-only session-end wrapper starts one deadline
before the existing drain. Census finalization can use only the unused part of
that same 100 ms budget; it neither resets nor extends it. The callback is
released when the timer expires even if optional finalization is still pending.
A missing timer, clock anchor or receipt leaves the census incomplete. The
native verifier ties the cutoff to an independently retained helper receipt and
checks a timestamp sampled after confirmation persistence, so late persistence
cannot retroactively become an on-time seal. This portable protocol qualification
does not establish native host liveness or a platform clock-rate bound.

The independent actual-module audit found that an undefined/malformed timer
handle could previously bypass this watchdog: a 40 ms original drain plus a
90 ms census page kept the callback pending past 110 ms. The repaired wrapper
validates a callable cancellation handle, including guarded property access,
before adding any census wait. Seven missing/malformed/throwing handle variants
run only the original drain and produce no confirmation. The original failure,
exact sources and repaired run are preserved under `independent-review/`.

**The conventional-hook census remains open.** Hooks are independent short-lived
processes. If both a hook's canonical delivery/spool and its measurement write
fail, earlier saved helper rows cannot reveal that final missing invocation.
The currently retained provider boundary supplies no shared invocation identity
or long-lived independent hook counter. Enumerating successful measurement files
is therefore not an independent total-capture census. The exporter must keep
that source unsealed until a dedicated native fixture supplies a complete,
independently verified hook invocation/capture witness. Canonical capture is not
disabled on measurement failure to manufacture a complete-looking population.

## Executed controls and retained attempts

- `tests/native/tools/test_m2_latency.py` has 21 focused calculation controls,
  including true COMMIT versus receipt, both mandatory sources and thresholds,
  unmatched samples, coalesced cursors, received-but-not-applied DOM, wrong boot,
  unqualified clocks, changed view/store/source epochs, UUID retries and final
  census loss.
- [`adversarial-closure/`](adversarial-closure/) retains one explicitly synthetic
  calculation control and eight rejected negative variants: wrong store,
  wrong observer epoch, missing rate bound, null view identities, missing final
  seal, wrong digest, lost tail and unsealed/unqualified production shape.
- [`attempts/`](attempts/) retains the earlier calculator weaknesses and an
  actual observer collector lost-tail witness. Those attempts are not relabeled
  as passing source qualification.
- The actual SQLite COMMIT-hook mutation is additional to the JSON controls.
  Its same-label precommit timestamp fails the intended assertion.
- [`observer-census/`](observer-census/) records **20 actual Node controls** and
  **16 portable Rust controls**, Clippy with warnings denied, and four
  assertion-killed mutations. They cover the complete ledger, missing pages,
  lost final sample, missing/forged close confirmation, digest/UUID mismatch,
  source identity, concurrent capture/export, bounded retention, the original
  end-drain deadline, and confirmation persistence after its native cutoff.

The earlier COMMIT mutation output remains under
`commit-boundary/before-journal-header-update/`. The final mutation uses the
current journal source; the boundary assertion and exact failed output are
retained separately from the original positive COMMIT-hook execution. The final
core campaign also runs that actual positive SQLite test.

Synthetic fixtures may establish that an arithmetic gate accepts a known input,
but report `nativeExecution:false` and `INCOMPLETE_OR_FAIL`. The command-line
calculator returns success only for complete, qualified **native** evidence.
It cannot turn a synthetic example into M2 native qualification.

## Current native performance verdict

| Source | New qualified native samples | Capture → COMMIT p95 ≤100 ms | COMMIT → DOM p95 ≤100 ms | Event → DOM p95 ≤250 ms |
| --- | ---: | --- | --- | --- |
| `claude.hook` | 0 | INCOMPLETE | INCOMPLETE | INCOMPLETE |
| `claude.observer` | 0 | INCOMPLETE | INCOMPLETE | INCOMPLETE |

Native p50/p95/max values are **not established**. This is a missing measurement
qualification verdict; it does not establish that the application is too slow.
The separate historical route p95 of 895 ms versus 750 ms remains the accepted
M13 obligation and is not included in this normal-path calculation.

## Reproduce portable checks

From the repository root with pinned dependencies/toolchain available:

```sh
python3 -m unittest discover -s tests/native/tools -p test_m2_latency.py -v
cargo +1.99.0 test -p threadspace-journal --lib --features qualification --target x86_64-unknown-linux-gnu latency::tests::
cargo +1.99.0 test -p threadspace-journal --test qualification_latency --features qualification --target x86_64-unknown-linux-gnu
python3 evidence/M2/remediation-1/f4/run-commit-negative.py
python3 evidence/M2/remediation-1/f4/run-adversarial-closure.py
```

The native raw exporter commands are `threadspace-m0c m2-latency-begin dev` and
`threadspace-m0c m2-latency-end dev <runDirectory>`. They collect metadata from an
already safely prepared, source-matched Dev qualification fixture; they do not
create ownership of an arbitrary running application, provider configuration or
store. Enable `--qualification-latency` only in that fixture's dedicated helper
argv. Never edit the owner's installed mod or configuration to enable it.

The exporter does not launch/stop applications, change settings, focus Terminal,
or certify clock/census completeness. It retains incomplete conditions in the
raw result. Once independent source census and native clock qualification exist,
the raw data can be processed with:

```sh
python3 tests/native/tools/m2_latency.py <runDirectory>/raw.json --output <runDirectory>/calculated.json
```

A new Dev build with matching outer/companion/helper and observer hashes, closed
source populations, source-specific joins and raw native results is required
before F4 can be accepted. None was executed from this Linux environment.
