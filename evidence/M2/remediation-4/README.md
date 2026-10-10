# M2 F4 focused forensic attempt

**M2 F4 REMEDIATION INCOMPLETE. No merge, M3 or production deployment.**

F4 has two independently closed native populations with actual COMMIT/DOM
joins, useful phase evidence and a demonstrated first-execution startup cost.
The installed observer/WebKit clock rate and precision contract remains
unqualified. No performance PASS is claimed, no early observation is excluded,
and no production performance repair is claimed. The separate legacy-store
anchor/suffix comparison passes with exact full-state equality.

## A. Identities and scope

Startup local/remote m2 was `1f5c6c17a570744050696c2239c1fb5099330772`.
Accepted main remains `af9b285da529890bc441ea00f1a92e73e39902a8`.
The earlier signed `4aef4a96f0e21844b35271246a151629a77bceb2` product and
`9e13bad17c498282a02232d850db19b0ef01aa57` qualification harness remain the
source identities of remediation-3's F1A/F1B/F2/F3 evidence.

This attempt committed a read-only retained-checkpoint test at `bf3f436`,
Dev-only helper phase/telemetry comparisons at
`ac7d1c92b81c775a964e9dbc81d35a8c1a7a87ee`, and additional bounded helper
progress/host-error evidence at `79e7064db6010c4a9838bbb6015cf09a7bf2093e`.
The final signed Dev build and native harness use the latter clean source.
The later calculator-label correction is a Python-only evidence change.
Every fixture retains its own source/build/harness hashes; publication is not
another native execution. [Final installed identity](final-native-identity.json).

| Installed Dev component | SHA-256 |
|---|---|
| Outer | `a56ef09041f4b2de8e61ca120284fed7134ee9a5aa6ab4041f4ac759b735cf6b` |
| Companion | `fe0c1939913879c8abba16adae66a5a4553a50ae94b255c18cdcc309bbf9b439` |
| Helper | `f9ea720566710d6c7a05d5d82a433e2208915ed848eb5e10f9df4f35e14fb8fb` |
| Harness | `ceea9ffb23305437db4f25a0c606bee7688037fc2e6709cc9ef6723d1ea8cab3` |

Inside-out repository builds and strict/deep signing verification passed.
Dev backups were preserved before replacement; source-tree cleanliness and
installed hashes were verified. Production was untouched. The retained real
Claude executable is 2.1.295; the global launcher was not modified.

## B. Slow-path evidence and its limits

The original raw run remains unchanged at
`../remediation-1/f4/native/capture/20261010T073149Z-dev/`. Its first host-call
span is 319.193584 ms on one observer JavaScript clock, exceeding the requested
250 ms host timeout. Its three subsequent captures wait 187.105709,
146.419584 and 110.616250 ms until the next drain. The first capture waits
78.897541 ms before that first invocation; first return to next drain is
47.459750 ms. These queue durations are same-runtime measurements, without
cross-clock subtraction. [Exact original observations](f4/analysis/original-slow-observations.json).

The source inserts callbacks into the bounded UUID queue, schedules its timer,
enters one drain, invokes `$.process.run`, and removes records only from typed
accepted receipts. It waits for that host promise before another drain.
Consequently later captures wait behind the first call. An UNKNOWN/failed call
keeps the same UUID and schedules the existing 250 ms minimum retry backoff.
Callback insertion and timer registration are source-supported ordering; the
retained old data does not individually time those operations or provider-host
dispatch/startup/return delivery. Helper startup before `main` is outside the
helper's watchdog. The actual COMMIT brackets remain independent of the host
promise and of diagnostic progress files.

Three bounded, owned real-Claude comparison runs were preserved:

| Mode / capture run | Source | First host span | Next-drain waits |
|---|---|---:|---|
| telemetry on / `20261010T092414Z-dev` | `ac7d1c9` | 273.125709 ms; UNKNOWN then COMMITTED retry | 563.151, 461.265, 442.857, 416.246, 225.365 ms |
| receipt telemetry off / `20261010T092616Z-dev` | `ac7d1c9` | 253.626917 ms; UNKNOWN then ALREADY_COMMITTED retry | 550.610, 445.800, 429.250, 417.573, 218.913 ms |
| telemetry on + progress / `20261010T093850Z-dev` | `79e7064` | 166.713208 ms; COMMITTED | 103.608, 86.438, 73.387 ms |

The final first helper's native stamps measure 1.675708 ms from mod-batch entry
through context/locator resolution, 1.702834 ms from locator resolution through
receipt, and 11.700334 ms of receipt telemetry persistence. Helper elapsed time
at answer is 15.400875 ms. The second host call is 16.219208 ms. On the earlier
successful retry, persistence is 8.581625 ms; without persistence, a subsequent
call is 6.350625 ms with a 3.508416 ms helper. Removing telemetry did not remove
the first over-budget host call. It cannot be used as an alternative COMMIT or
population measurement. [Native comparison](f4/analysis/native-comparison.json),
[off-runtime diagnostic records](f4/analysis/telemetry-off-runtime-records.json).

A separate native-only control uses two acquired copies of the signed helper,
valid empty batches, no provider inference, and `CLOCK_UPTIME_RAW` timestamps
before process creation, at helper entry, and after exit. Fresh-copy entry waits
are 267.304500 and 63.073584 ms; repeating those exact files yields 3.210458 and
3.797000 ms. Their helper bodies are 4.982041/2.954500 ms initially and
1.853458/1.945667 ms on repeats. This establishes a substantial first-execution
pre-entry cost on this Mac. It does not identify which OS/loader/security stage
caused it, nor prove that all of the original provider-host 319 ms was startup.
[All native control timestamps](f4/analysis/native-startup-control.json).

No warm-only cohort was substituted for the original population. The provider
host's scheduling, pre-main startup and result-delivery portions are still not
separately measured under its actual caller. A production repair must address
that demonstrated first-call/queue delay with a source-bound counterexample.

## C. Clock qualification

Native helper, writer and harness stamps use same-boot `CLOCK_UPTIME_RAW`.
Boot identity is `F2D2A2E9-7805-43B2-AC3A-85613557FD8E`; WebKit bundle version
is `22625.2.5.11.1`. A fresh 10,000-read native control has zero samples outside
converted Mach-absolute brackets, timebase 125/3, reported resolution 42 ns,
one-nanosecond conversion-rounding envelope and minimum observed positive step
1,041 ns. That observed step is not asserted as a maximum precision error.
[Native reads](f4/analysis/native-clock-reads.json).

Apple documents the native Mach/uptime equivalence. Upstream WebKit Darwin
MonotonicTime uses Mach absolute time and Performance applies reduced
resolution. These sources do not identify the installed Claude mod runtime or
certify an installed JS/native maximum rate/precision bound:
[Apple](https://developer.apple.com/documentation/kernel/1462446-mach_absolute_time),
[WebKit native clock](https://github.com/WebKit/WebKit/blob/main/Source/WTF/wtf/CurrentTime.cpp),
[WebKit Performance](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/page/Performance.cpp),
[High Resolution Time](https://www.w3.org/TR/hr-time-3/),
[Bun timing APIs](https://bun.sh/docs/project/benchmarking).

The two telemetry-on UI calibration spans are 32.391 and 33.136 seconds.
Assuming a constant affine rate, their observed enclosures are approximately
[-759,+759] and [-762,+714] ppm. This is empirical consistency, not a maximum
rate contract. Without rate extrapolation, monotonic ordering alone brackets
the DOM points across roughly 31.9/32.5 seconds between start/end calibration
groups, which cannot certify the 100 ms gate. Observer origin, coarsening,
numeric-conversion error, and a defensible cross-runtime rate bound remain
unqualified. Sleep/runtime-change coverage is not newly qualified.
[Model audit and raw identities](f4/analysis/clock-model-audit.json).

The collector retains `qualified:false`, null rate/precision and every raw
interval. Conservative qualified computation remains Jhi−Clo, Dhi−Jlo and
Dhi−Clo. No unrelated clocks are directly subtracted, no rate-zero assumption
is certified, and no manual positive clock flag is introduced. The calculator
now explicitly labels unqualified diagnostic percentiles and returns zero
qualified sample counts; valid slow measurements retain their qualified count
while failing the target. Clock/census/DOM negative controls remain intact.

## D. Source changes

Changes are a read-only journal test, Dev feature-gated helper phase/progress
records, optional diagnostic host metadata, collector paths, comparison flags,
and truthful calculator labels. Unsynced progress records have no receipt,
durable admission or census authority. Receipt-telemetry-off cannot close an
observer population. Bounded UUID queue, single in-flight delivery, typed
receipts, commit-only ACK, original result identity, exactly-once `next(e)`,
250/100 ms budgets and renderer independence are unchanged. No relay redesign,
reducer change, integration cleanup change or product routing repair occurred.

## E. Native populations and measurements

Each telemetry-on run independently issues 20 controlled conventional hooks
and closes a 21-capture real observer ledger. All 41 observations are committed,
projection-changing and matched to actual hydrated DOM marks; zero unmatched,
rejected, spooled or no-projection samples and zero census failures/exclusions.
The first on run retains one UNKNOWN attempt and its stable-UUID retry; it is
not removed from the captured denominator. Actual UUIDs, source epochs, cursors,
patch cursors, store/core/view/runtime/boot identities and COMMIT brackets remain
in raw files. Coalesced DOM marks cover the exact applied cursor.

The off run has only a diagnostic observer stream, no native receipt tokens
and no closed observer census. Its exporter correctly refuses the gate.
It is not silently substituted for either complete on population.

The new double-loss control issues two native children, accepts one and reports
the known zero-output invocation as missing. Its denominator remains two and
`complete:false`; neither capture nor local measurement can erase that entry.
[Independent issuance and result](f4/double-loss-control/).

Latest full on-run diagnostics, **unqualified**; all qualified n are zero:

| Source / metric | Diagnostic n | p50 ms | p95 ms | max ms | Frozen p95 target |
|---|---:|---:|---:|---:|---:|
| hook capture→COMMIT | 20 | 1.230167 | 1.755292 | 4.936541 | 100 ms |
| hook COMMIT→DOM | 20 | 24.792000 | 29.069750 | 30.884833 | 100 ms |
| hook capture→DOM | 20 | 25.602333 | 29.468083 | 31.982291 | 250 ms |
| observer capture→COMMIT | 21 | 22.650125 | 119.213624 | 199.522416 | 100 ms |
| observer COMMIT→DOM | 21 | 28.893041 | 30.069583 | 30.474958 | 100 ms |
| observer capture→DOM | 21 | 51.554583 | 142.730999 | 227.564749 | 250 ms |

The earlier on diagnostic observer capture→COMMIT/capture→DOM p95s are
476.518124/506.169958 ms. No source-specific threshold is certified PASS.
Each production-calculator invocation exits 1, retains its input/output hashes
and errors. [Latest raw and calculator records](f4/native/capture/20261010T093850Z-dev/),
[earlier on](f4/native/capture/20261010T092414Z-dev/),
[off refusal](f4/native/capture/20261010T092616Z-dev/).

## F. Legacy store adjudication

**LEGACY DEV-STORE COMPARISON NOT APPLICABLE TO EMPTY GENESIS — qualified anchor/suffix PASS**

The preserved consistent backup SHA is
`23a786392c9bd82e417994ad52266c0d0f679ddeba66e54673f4d2374f02a2b1`.
All observations 1–3531 remain present as noncanonical payload-version-1
history, with zero corresponding canonical facts. Canonical coverage begins
3532. The store is M0-fixture seeded; 601 baseline identity assignments at
3529 match the M0 bootstrap design in D-0007 and `bootstrap_canonical`.
The preserved pre-M1 backup through 3528 corroborates the materialized baseline:
120 Sessions, 103 processes, 105 executions, 48 Turns, 48 attention items,
seven commands and 47 outbox records. Later preserved backups retain successive
reducer-3 checkpoints. The production checkpoint writer retains three latest
checkpoints; the original M0 baseline checkpoint is no longer available here.
This is not evidence of a lost canonical prefix. [Coverage](legacy/legacy-prefix-coverage.json),
[historical backup coverage](legacy/legacy-history-coverage.json).

The original 630 missing items are map entries, including the attention index,
not 630 missing canonical journal records. The original 697 empty-genesis
differences remain preserved privately. Empty canonical genesis lacks the M0
materialized baseline and is therefore not this store's valid full-state oracle.

The new ignored journal test opens the preserved backup read-only, checks its
integrity and unchanged byte hash, then backs up into owned memory. Only that
in-memory copy changes checkpoint selection. Verified reducer-4 checkpoint 6
at cursor 5871, SHA
`0907e2e3f80ff0e827b9f8b5dddcf038731663055075d3bdec411cb38064b429`,
plus every contiguous accepted suffix entry 5872–6021 (150 entries), reconstructs
the exact full state. Commands/receipts, attention, outbox, Session/Turn IDs and
materialized tables match, and repeated recovery is exact. Full differences: 0.
The in-memory corrupted-checkpoint negative is rejected. State SHA is
`5fcd7d77e14118fb09f0d2c8ff1c2513a11599f552ecb3b2d28c5b83852496ad`;
materialized projection SHA is
`c281c550ba1eb7aeb8755811de6ddf19c4d3e643e0954c2426d84ff77db74f4c`.
[Machine verdict](legacy/legacy-anchor-suffix.json), [test output](checks/legacy-anchor-test.log).
The separate complete isolated native-envelope genesis oracle remains unchanged.
No live database was reset, overwritten, downgraded or repaired.

## G. Retained work

Remediation-3's F1A/F1B/F2/F3 candidate PASS evidence remains at its original
source identities. Ownership, proof/reducer logic, installation ownership and
route behavior were not changed. The new default-versus-diagnostic timeout
control, 14 ownership cases, 38 provider-kit tests and focused native smokes
cover affected optional telemetry behavior; they do not represent another full
F1/F2/F3 campaign. Historical ten-cycle vertical slice, 30 ordinary Returns,
20 Mark handled commands, transport and reducer-3 migration remain credited
within their original scopes. The 895 ms ordinary-route p95 remains M13 debt.
D-0009/D-0010 remain PROPOSED; future M3/M5/M13/M15 work is not started.

## H. Checks and privacy

Workspace Clippy, nonqualification relay check, schema freshness (3), UI tests
(104), provider validation/kit (38), Node collector tests (21 plus the contained
14 ownership cases), relay latency controls (16), journal latency controls (17),
SQLite COMMIT controls, calculator controls (23), native signing/build and the
read-only legacy replay pass. Relevant raw outputs are under `checks/`.

The separate absent-companion mod-batch wall-time test failed twice at 308/311
ms while still returning the expected LOCAL_SPOOLED receipt; the other test
passed. Both failures are retained. Its entire parent-to-exit interval is not
decomposed by the Dev-only phase sink because it uses an arbitrary disposable
store; it is not asserted to have the same cause as the signed startup control.
No budget or assertion was weakened. This is an additional unresolved wall-time
control, not a passing test or a renderer/native gate result.

Private original archives, store copies, install/build logs and diagnostics
remain outside Git. Publication replaces exact private path literals and
filters unrelated process inventory to the acquired TTY. Functional UUIDs,
timestamps, hashes, return codes and failure records are retained. Recomputing
the published raw data reproduces the labelled results.
[Publication identities/classifications](privacy-publication.json).

## I. Final gates and remaining action

F1A/F1B/F2/F3: prior native PASS pending independent Pro review.
F4: **INCOMPLETE**. Legacy diagnostic: qualified anchor/suffix PASS.

Remaining work is to establish the installed observer/UI clock bounds or a
complete tight causal native-bracket method, and to isolate/remediate the actual
provider-host first-call startup/return path without removing early captures.
The repeated absent-companion wall-time control also remains unresolved.
The current evidence cannot certify the 100/100/250 ms source-specific gate.
Independent acceptance re-review becomes eligible after that gate actually
passes. No merge, production migration or M3 authorization is inferred.
