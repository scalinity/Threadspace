# M2 F4 final bounded closure

**M2 F4 REMEDIATION INCOMPLETE**

**Final decision: F4 DESIGN-AUTHORITY DECISION REQUIRED.**

The original subprocess wall-budget failure reproduced at 313 ms. A subsequent
unchanged invocation passed; no repair makes that repeat a closure result.
The installed plugin clock was identified more precisely, but no defensible
installed cross-runtime rate/precision bound was obtained. Existing causal
native enclosures are finite and too wide for the frozen gates. No product
performance change, warm-only population substitution, acceptance promotion,
merge, M3 or production migration occurred.

## A. Source and build identities

Startup local HEAD, tracking ref and live remote `m2` were equal to
`aee33d3779a413492136c437b3e552ce45962d0f`; the checkout was clean. Live remote
and tracking `main` were `af9b285da529890bc441ea00f1a92e73e39902a8`.
Main, native source `79e7064db6010c4a9838bbb6015cf09a7bf2093e`, and calculator
source `bcb1d92b0150c019c8fdf5fc135601e3e4942aa7` are ancestors of startup HEAD.
The relay, surfaces, provider mod and native harness trees have no changes
between the retained native source and startup HEAD.

The installed Dev identifier remains `ai.scalinity.threadspace.dev`.
Strict/deep signature verification passed. Installed executables and native
harness were hashed before and after the exact test repetitions:

| Component | SHA-256 |
|---|---|
| Outer | `a56ef09041f4b2de8e61ca120284fed7134ee9a5aa6ab4041f4ac759b735cf6b` |
| Companion | `fe0c1939913879c8abba16adae66a5a4553a50ae94b255c18cdcc309bbf9b439` |
| Signed helper | `f9ea720566710d6c7a05d5d82a433e2208915ed848eb5e10f9df4f35e14fb8fb` |
| Native harness | `ceea9ffb23305437db4f25a0c606bee7688037fc2e6709cc9ef6723d1ea8cab3` |
| Exact test's debug helper | `39c437a0806e933db3a5803f45893636a928c3aafc1bd68bc3559d1f6a027541` |
| Existing mod-batch test executable | `94fff811df4cd3115856c8a0949099f62d6a2d397fc0f2bf0060b59939452616` |

The native build/harness remain the clean `79e7064` build; no rebuild was
necessary because product/helper/native harness sources did not change.
The new Python control ran against startup HEAD plus its recorded file hash;
it is not described as a new clean native build. [Identity](startup-identity.json),
[control identity and raw timings](f4/analysis/startup-control.json).

## B. First execution, host scheduling and queue wait

One bounded comparison acquired four distinct copies of the exact signed
helper: two for direct launches and two for real Claude-host launches. Copies
retain their own device/inode identities and first execution; each call has an
owned new store with no companion locator. Record-bearing calls returned
`LOCAL_SPOOLED`; each direct spool contains its one observation. The repeat
uses the same executable inode, with a fresh disposable store. Empty-batch
controls measure a warm launch without record preparation or spool publication.
First means first execution of that acquired inode. Global loader/security
caches were not evicted; direct controls ran before host controls, and all
copies share the same bytes. The two callers therefore cannot be ranked by
these elapsed values or treated as mutually independent cold OS environments.

| Direct signed-helper copy/call | Parent spawn call ms | Spawn return→reap ms | Launch→reap ms |
|---|---:|---:|---:|
| 0 first | 10.292375 | 651.712917 | 662.005292 |
| 0 repeat | 26.313958 | 54.956875 | 81.270833 |
| 0 empty warm | 24.665083 | 45.756417 | 70.421500 |
| 1 first | 22.178583 | 133.549125 | 155.727708 |
| 1 repeat | 8.820084 | 41.129541 | 49.949625 |
| 1 empty warm | 7.538584 | 29.341958 | 36.880542 |

These three native parent intervals use `CLOCK_UPTIME_RAW`. Reap bounds actual
exit from above; it is not an exact exit timestamp. The large first-copy cost
continues after spawn returns. It is not a measurement of body time, or proof
that executable validation, loading or any named OS mechanism caused it.

| Real Claude-host signed-helper copy/call | Plugin-clock request→callback ms | Receipt |
|---|---:|---|
| 0 first | 85.235667 | LOCAL_SPOOLED |
| 0 repeat | 17.947709 | LOCAL_SPOOLED |
| 1 first | 95.444209 | LOCAL_SPOOLED |
| 1 repeat | 17.599959 | LOCAL_SPOOLED |

The provider is the retained Claude 2.1.295 executable, SHA-256
`0116ee2e0a513900b633d9951367f18747686478e2b462805b8c31609f047f70`.
Only SDK initialization was issued, with no user message or inference request.
An owned plugin uses actual `$.process.run`; its `session.start` handler
forwards the original event once and returns the original result. The process
exited after its own input was closed. No owner Terminal or integration was
opened, changed or terminated. Host spans use only the plugin clock. They are
not subtracted from native elapsed times.

**Unresolved phase boundary:** installed qualification stamps require the Dev
agent and forbid `--store-dir`. An owned disposable-store invocation therefore
cannot return helper entry/answer stamps from this exact binary. Current rows
keep those fields null; no live Dev locator/store was redirected to obtain
them. Native host spawn-return and exact helper exit stamps are also not exposed
by `$.process.run`. The current exact test's excess cannot be partitioned among
pre-main/prologue, spool/fsync, scheduler and teardown from these interfaces.

Retained remediation-4 phase evidence remains authoritative within its scope:

* First acquired helper: launch→mod-batch-entry diagnostic 267.304500 ms,
  entry→answer 4.982041 ms; same inode repeat 3.210458/1.853458 ms.
* Second acquired helper: 63.073584/2.954500 ms; repeat 3.797000/1.945667 ms.
* Last telemetry-on real observer first host span 166.713208 ms on its JS clock;
  native helper elapsed at answer 15.400875 ms. Native locator→receipt is
  1.702834 ms; optional receipt persistence is 11.700334 ms.
* The next drain's three queued captures wait 103.608000, 86.437625 and
  73.386542 ms on that same JS clock. One in-flight drain holds them behind
  the first host call. First capture→first drain is 33.398333 ms; first host
  return→next drain is 14.120042 ms.

The entry stamp is at `mod_batch`, after main's argument/watchdog prologue;
it does not isolate OS pre-main work from that prologue. Neither warm repeats
nor receipt-telemetry-off comparisons identify the mechanism responsible for
the full first-call cost. Prior telemetry-off first-call timeout and prior
telemetry-on slow records remain preserved.

## C. Absent-companion process wall-budget test

The exact named test is
`a_batch_without_a_companion_is_spooled_and_answered_within_its_budget` in
`crates/relay/tests/mod_batch.rs`. It begins an `Instant` before spawning,
passes internal budget 80 ms, validates its returned receipt, and asserts
spawn→wait-with-output completion **<250 ms**. Its assertion and source are
unchanged. The main watchdog uses 250 ms starting inside main, independently
of the elapsed time already spent launching. The 80 ms mod-batch deadline is
checked before spool publication, not an absolute limit on blocking fsync.

| Execution | Actual wall assertion | Receipt assertions |
|---|---|---|
| Retained remediation-4 first | FAIL, 308 ms | LOCAL_SPOOLED passed |
| Retained remediation-4 repeat | FAIL, 311 ms | LOCAL_SPOOLED passed |
| Current source-matched first | FAIL, 313 ms | LOCAL_SPOOLED passed |
| Current source-matched repeat | PASS, exact elapsed not printed by unchanged test | LOCAL_SPOOLED passed |

The successful repeat's runner reports approximately 0.11 seconds for the
whole test. That is not substituted for the unprinted child elapsed value.
Both current repetitions use the exact same helper/test hashes. No retry until
PASS campaign followed. [Failure](checks/original-modbatch-1.log),
[repeat](checks/original-modbatch-2.log).

**Disposition: unresolved wall-budget failure, not closed.** A warm repeat
demonstrates variability. No source-matched performance repair establishes
reliable compliance with the original boundary. The signed-helper direct
662.005292 ms control is a separate first-inode counterexample with a truthful
spool receipt, not another execution of the named debug-helper test.

## D. Installed clock evidence and finite-run limits

The current machine is macOS 27.2 build `26B5101f`, boot
`B25EE6BF-6973-4B99-9543-E37EF1A59C87`, WebKit `22625.2.7.1`.
The retained 41-record window was build `26B5091g`, boot
`F2D2A2E9-7805-43B2-AC3A-85613557FD8E`, WebKit `22625.2.5.11.1`.
New measurements cannot retroactively qualify that different installation/boot.

The actual plugin exposes neither `Bun` nor `process`; `performance.now` is
a guarded helper wrapper with no exposed `timeOrigin`. Inspection of the exact
installed executable identifies the wrapper and its helper-table delegation to
host `performance.now`. Embedded markers identify Bun 1.4.3 revision
`eecfd55debb281c3fe3518098b1a49f76429637c`. The binary's imported Mach/clock
symbols do not bind that builtin to a particular native counter. Attempts to
read the exact Bun revision/native binding and applicable WebKit source tag did
not provide matching source. No unsupported globals or VM escape was used.

A new 1,000-read native control has zero `CLOCK_UPTIME_RAW` reads outside
converted Mach-absolute before/after brackets. Mach timebase is 125/3 and
reported resolution is 42 ns; integer conversion rounding is at most 1 ns.
These confirm native consistency in the sampled window, not a maximum JS
rate/coarsening error. The retained native 10,000-read control remains separate.

The unchanged SQLite writer uses bundled SQLite 3.53.4, source ID
`2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc`,
WAL/FULL, with native `CLOCK_UPTIME_RAW` brackets around the actual SQLite
COMMIT call. That is retained runtime evidence for the identical companion;
no new writer was launched in this bounded attempt.

[Apple's implementation](https://github.com/apple-oss-distributions/Libc/blob/main/gen/clock_gettime.c)
maps uptime raw to Mach absolute time and supplies the conversion.
[Upstream WebKit](https://github.com/WebKit/WebKit/blob/main/Source/WTF/wtf/CurrentTime.cpp)
uses Mach absolute time for Darwin monotonic time;
[Performance](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/Performance.cpp)
reduces timer resolution. Those explain candidate implementations; they do
not certify the two installed WebKit versions or Bun's maximum relative rate.

Approach 1 remains unqualified. The retained 33.135602292-second calibration
span has an empirical constant-affine rate enclosure about [-762,+714] ppm;
it is a consistency observation, not a bound on unobserved rate variation or
installed coarsening. Rounding/representation observations do not close that
missing contract.

Approach 2 has finite source-supported native program-order enclosures. A
captured observer record follows native source-window start and precedes its
helper's entry. A hydrated DOM mark follows its actual COMMIT and precedes
the acknowledged native collector stop. The retained window is 32.802497417
seconds wide. Using those native endpoints without any JS rate translation
gives observer capture→COMMIT p95 upper enclosure 26,666.582459 ms and
COMMIT→DOM p95 upper enclosure 26,363.948375 ms. Sparse calibration alone
brackets DOM over 32,453.169833 ms. These enclosures cannot certify the gates;
they do not establish that actual latency is that large.

The illustrative native enclosure analysis does not change the production
calculator or flag. The required Jhi−Clo, Dhi−Jlo and Dhi−Clo arithmetic
remains unchanged. `qualified:false`, null rate/precision and zero qualified
sample counts remain correct. [Installed audit](f4/analysis/installed-clock-audit.json),
[native reads](f4/analysis/native-clock-control.json),
[finite causal enclosures](f4/analysis/causal-enclosures.json).

## E. Performance repair and bounded stop

**No performance repair was implemented.** Product queue ordering, UUIDs,
one in-flight drain, typed durable receipts, retries, optional telemetry and
fail-open callbacks remain unchanged. New work is the isolated reusable
launch control, its documentation and evidence. No old-versus-new performance
claim exists, so no warm environment is presented as a discriminating repair
control. Prewarming was not installed or qualified.

The bounded attempt was two exact failing-test repetitions, one two-inode
direct/host comparison with empty warm controls, one native clock consistency
control, installed-clock inspection and retained-evidence calculations. It
stops here. A new durable-COMMIT comparison, telemetry-on/off campaign and full
20-hook/21-observer run are **NOT_RUN**: there is no repaired mechanism or
qualified clock strategy to test. The retained actual COMMIT and DOM evidence
is not replaced by the spool controls.

## F. Retained normal-path latency

The retained complete telemetry-on window has 20 independently issued hooks
and 21 observer captures, 41 durable COMMITs, 41 applied hydrated DOM joins,
zero missing captures, no retries in this window, zero exclusions and zero
collector errors. First/cold records remain in the population. Earlier UNKNOWN
and stable-ID retries remain in the earlier run; no telemetry-off population
is substituted. The retained independent hook double-loss control remains
passing under its original scope.

All numbers below are **unqualified diagnostics**. Every qualified count is 0.

| Source / metric | Diagnostic n | p50 ms | p95 ms | max ms | p95 target | Numeric diagnostic |
|---|---:|---:|---:|---:|---:|---|
| hook capture→COMMIT | 20 | 1.230167 | 1.755292 | 4.936541 | 100 | within |
| hook COMMIT→DOM | 20 | 24.792000 | 29.069750 | 30.884833 | 100 | within |
| hook capture→DOM | 20 | 25.602333 | 29.468083 | 31.982291 | 250 | within |
| observer capture→COMMIT | 21 | 22.650125 | 119.213624 | 199.522416 | 100 | exceeds |
| observer COMMIT→DOM | 21 | 28.893041 | 30.069583 | 30.474958 | 100 | within |
| observer capture→DOM | 21 | 51.554583 | 142.730999 | 227.564749 | 250 | within |

With 21 observer samples nearest-rank p95 is the second largest. Three
diagnostic capture→COMMIT bounds exceed 100 ms (199.522416, 119.213624,
102.043249); discarding only the maximum would not close the target.
The unchanged production calculator was rerun on the retained raw input,
exited 1 and again returned zero qualified counts. Its 23 focused controls
pass, including wrong boot/runtime, changed store, unmatched DOM and census
loss controls. This validates the calculator's refusal, not F4 latency.
[Population](f4/analysis/retained-population.json),
[calculation](f4/analysis/recalculated-retained.json),
[execution](checks/recalculation-execution.json),
[calculator controls](checks/calculator-controls.log).

## G. Preserved milestone evidence

Remediation-3 F1A/F1B/F2/F3 native candidate PASS evidence remains at its
original source identities, pending independent Pro review: immutable
real-Claude ownership, independent reload and post-seal Turn, safe installer/
uninstaller cleanup and five minimized positive Returns within two seconds.
The original ten-cycle vertical slice, thirty ordinary exact Returns, twenty
durable Mark handled actions, reducer-4 migration and genuine older-store
fixtures remain unchanged. Accepted M1 and M0C scopes remain unchanged.
D-0009 and D-0010 remain PROPOSED.

**LEGACY DEV-STORE COMPARISON NOT APPLICABLE TO EMPTY GENESIS — qualified anchor/suffix PASS**

The verified checkpoint-plus-suffix reconstruction is preserved. No additional
empty-genesis investigation, store repair or adjacent qualification occurred.

## H. Outstanding issues and minimum next action

1. **Clock binding.** Obtain exact installed Bun/WebKit counter, origin and
   coarsening correspondence supporting a finite bound over one recorded
   window, or select a qualification-only native timestamp interface at the
   actual callbacks. Generic documentation, empirical regression and sparse
   round trips cannot supply the missing bound. A native clock marker at
   callback capture and hydrated DOM application would remove the cross-rate
   inference. WebKit could support a Dev-only synchronous native bridge;
   the current Claude VM has no supported synchronous native counter API.
   A provider-supported host marker or independently verified installed
   host-clock implementation is required for that first endpoint. No such
   extension is implemented or presumed available. Asynchronous marker
   receipts must retain their full causal intervals, including early captures.
   Metadata would be UUID/runtime/boot/counter only; no prompt/output content
   is needed. Any synchronous implementation must measure its callback cost
   and preserve fail-open provider behavior before qualification.
2. **Subprocess boundary.** The original <250 ms launch→exit assertion remains
   failing. Entry/body/teardown of the current exact failure are unresolved;
   installed phase instrumentation cannot measure them on a disposable store.
   An ordinary test-only extension could expose those stamps, but it does not
   create a clock proof or let an in-main watchdog govern pre-main execution.
   Preserve the failure while deciding who owns the launch-inclusive deadline.
3. **Observer upper tail.** Retained diagnostics still exceed 100 ms and the
   accepted queue holds subsequent records behind the first call. Any selected
   preparation or scheduling repair needs an old-versus-new cold-path control,
   retention of all early records, and a representative complete population
   measured with the selected clock proof. There is no qualified repair yet.

The bounded independent ruling is:

> Retain the frozen launch-inclusive <250 ms test and all initial observer
> captures. Can the installed provider/WebKit implementation supply a verified
> common-counter mapping for this finite measurement window? If it cannot,
> which supported native marker interface is approved for the Claude VM
> capture endpoint? Also decide whether cold executable launch must be brought
> inside the existing absolute subprocess boundary through a measured preparation
> mechanism, or whether a separately reviewed criterion change is necessary.
> No entry-only reinterpretation or removal of cold observations is applied.

This is a design ruling about missing clock/startup capability, not an M2
acceptance request. No further instrumentation campaign begins in this task.

## I. Final decision and publication checks

**F4 DESIGN-AUTHORITY DECISION REQUIRED**

The F4 latency measurement is INCOMPLETE, with an independently retained
subprocess wall-budget failure and an observer diagnostic upper-tail excess.
No criterion was waived. SPEC and MILESTONES are unchanged. No
`ORCHESTRATION.html` exists, so no overview was created or realigned.

Raw provider streams, copied executables, configuration and disposable spools
remain outside Git in the private evidence directory. Published output contains
allowlisted timing/fixture IDs and executable hashes. Privacy classification,
consistency checks and final publication equality are recorded separately.
Publication of this completed bounded report is not a native gate PASS.
