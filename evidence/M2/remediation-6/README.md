# M2 F4 cold-path boundary resolution

**M2 F4 REMEDIATION INCOMPLETE**

**F4 DESIGN BLOCKED — OWNER MEASUREMENT DECISION REQUIRED**

This is the bounded design-authority unit following remediation-5. It adds only
qualification startup tracing, executes the original test boundary and owned
absent-companion controls, and assesses supported clock APIs. No performance
repair, Dev installation, 41-record campaign, acceptance promotion, merge, M3,
production migration or criterion amendment occurred.

## 1. Identities and scope

Startup HEAD, local `m2`, tracking `origin/m2` and live remote `m2` were all
`2562d6226e623243dbe83ab208c808dba80ed1cf`; the checkout was clean. Accepted
local/tracking/live remote main remains
`af9b285da529890bc441ea00f1a92e73e39902a8`. Source ancestry for main, the previous
report, the native build and calculator is verified. Qualification source is
`1176253fe7871d20b7b3c4c34a2962e2ae76d776`.

| Artifact | Exact source or SHA-256 |
| --- | --- |
| Installed Dev build and native harness source | `79e7064db6010c4a9838bbb6015cf09a7bf2093e` |
| Calculator source | `bcb1d92b0150c019c8fdf5fc135601e3e4942aa7` |
| Installed outer executable | `a56ef09041f4b2de8e61ca120284fed7134ee9a5aa6ab4041f4ac759b735cf6b` |
| Installed companion executable | `fe0c1939913879c8abba16adae66a5a4553a50ae94b255c18cdcc309bbf9b439` |
| Installed helper | `f9ea720566710d6c7a05d5d82a433e2208915ed848eb5e10f9df4f35e14fb8fb` |
| Native harness | `ceea9ffb23305437db4f25a0c606bee7688037fc2e6709cc9ef6723d1ea8cab3` |
| Claude 2.1.295 executable | `0116ee2e0a513900b633d9951367f18747686478e2b462805b8c31609f047f70` |
| Preserved original debug helper | `39c437a0806e933db3a5803f45893636a928c3aafc1bd68bc3559d1f6a027541` |
| Preserved original test executable | `94fff811df4cd3115856c8a0949099f62d6a2d397fc0f2bf0060b59939452616` |
| Initial instrumented helper | `fd96c945fe40ad50d1298ca8830643f35178e0e7d36d6f50e72cb77e76243a21` |
| Clean-source instrumented helper | `26de33c789c2f3306961b5bfdc8d3782baeb0339bb625808e1bbbde27649520f` |
| Instrumented test executable | `eb7606ecd4e6a8027a310599464a68cdd802c0a818c45de2e2ebc36ccf3df1f1` |
| Current qualification-only debug helper | `0210395a0e6fa3d3351dc0ddf2056b69cbce62c111969728d4624fa51d69c78d` |
| Current qualification-only test executable | `3c5ddef0c40bf2451eb33eb112d04c453af35d59e4eae9d472f0b541b2f8e7d9` |

The clean-source diagnostic build uses Rust 1.99.0 commit
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, LLVM 23.1.1, target
`aarch64-apple-darwin`, `test` profile and `qualification-startup` feature.
It is a test artifact, not the signed installed helper. Strict outer/companion/
helper and deep outer signature verification pass; all frozen installed hashes
are unchanged. [Identity readback](startup/identities.json).

Initial diagnostic executions were built from startup HEAD plus the three
explicit code-file hashes in [first-build identity](startup/instrumented-identities.json).
After a two-line comment clarification, the instrumentation was committed and
rebuilt from the clean qualification commit. That changed helper bytes; it is
kept as a separate first/repeat pair, not relabeled as the earlier artifact.
The original and both diagnostic binary versions remain in private evidence.
The first-build source file is retained there and verified against its recorded
hash. [Clean build](startup/clean-build-identities.json).
The final qualification-only build excludes the new feature, has no startup
diagnostic tags in its helper bytes/output, and its two-test suite passes.
It is also a debug test artifact; the installed signed helper remains frozen.
[Feature exclusion](startup/qualification-only-identity.json).

## 2. Startup phase accounting

The extension is a distinct `qualification-startup` feature, not enabled by
the accepted Dev/release packaging's `qualification` feature. It collects six
helper timestamps and emits two diagnostic stderr records after the stdout
flush attempt, only with an explicit environment flag and `--store-dir`.
The integration test adds launch/spawn/reap stamps to the existing `run` path.
The original named test body, typed receipt assertions and `elapsed < 250`
assertion are byte-for-byte unchanged. Its `Instant` interval still begins
before command construction/spawn and ends after `wait_with_output` returns;
cleanup and diagnostic interpretation are outside it. New tracing overhead
inside that interval remains counted.

All stamps use `CLOCK_UPTIME_RAW` on boot
`B25EE6BF-6973-4B99-9543-E37EF1A59C87`, macOS 27.2 build `26B5101f`.
The Rust parent uses the same libc constant as the helper; the Python parent
uses native clock ID 8, as in the retained native consistency control.
There is no JavaScript/native subtraction.

| Native phase | Attainable boundary / limitation |
| --- | --- |
| Parent launch request | Immediately before command construction in the test; immediately before Popen in the disposable control |
| Successful spawn return | First native read after the parent spawn returns; child main may precede or follow it |
| Helper main entry | First native read in Rust main, before Instant, panic hook, argv collection and watchdog setup |
| Startup initialization completed | After main-side watchdog spawn returns; watchdog thread scheduling/sleep start is not separately observed |
| mod-batch entry | First read inside mod-batch, before initialization/validation/spool work |
| Receipt ready | After mod-batch returns the serialized answer, before stdout write |
| Stdout flush completed | Only after successful write and flush; missing on failure |
| Parent reap | First read after wait/output collection returns; **not exact helper exit** |

Launch-to-main includes process creation, loader/validation/cache behavior,
OS scheduling, Rust runtime prologue and reaching the first clock read.
This is the earliest attainable main marker, not an OS exec event or an exact
first-instruction timestamp. It does not isolate those constituent mechanisms.
A first marker can itself encounter cold code/lazy-binding work.

The clean-source first execution **fails at 289 ms**. Native launch-to-reap is
289.210750 ms; the original test floors its Instant interval to integer ms.
Its typed receipt is `LOCAL_SPOOLED`: the failure occurs only at the original
elapsed assertion, after all receipt assertions pass.

| Phase | Clean source first ms | Same path/hash repeat ms |
| --- | ---: | ---: |
| Parent spawn call (part of launch-to-main) | 0.237292 | 0.198042 |
| Launch → earliest main read | 279.945500 | 2.024334 |
| Main → initialization complete | 0.186167 | 0.037791 |
| Initialization → mod-batch | 0.010125 | 0.002167 |
| mod-batch → locator read | 1.545333 | 1.343833 |
| Locator read → receipt ready | 7.093250 | 9.274459 |
| Receipt ready → stdout flush | 0.020458 | 0.008500 |
| Stdout flush → parent reap | 0.409917 | 0.400666 |
| **Launch → parent reap** | **289.210750** | **13.091750** |
| Original elapsed assertion | **FAIL: 289 ms** | **PASS: 13 ms** |

The first diagnostic stderr-record emission takes 0.072459/0.075083 ms in
that pair and is included in flush-to-reap. The second diagnostic line,
final `_exit`, kernel teardown, pipe collection and parent scheduling remain
inside the residual; none is mislabeled as exact exit. Timestamp collection
and the pre-main cost of an enlarged diagnostic executable are not bounded
by that post-answer emission interval.

The flag-off empty-batch control ran between the clean first execution and
the named-test repeat; that repeat is the third helper invocation, not an
immediate second call. Both intermediate and repeated calls are warm controls.
The clean helper hash was recorded before its first call and verified after
repeat. Its inode was observed after the pair, not at each endpoint. The
independently acquired disposable copies below have both pre-execution and
post-execution device/inode/hash checks, so their same-inode identity is
stronger evidence. All first observations are retained.

| Initial diagnostic artifact / acquired inode | First native launch→reap ms | Repeat ms | First launch→main ms | First mod-batch body ms |
| --- | ---: | ---: | ---: | ---: |
| Exact named test, initial artifact | 161.800042 | 12.017042 | 105.137542 | 55.835834 |
| Disposable copy 0 | 204.229083 | 13.642083 | 189.949083 | 12.986625 |
| Disposable copy 1 | 94.640458 | 13.004000 | 83.176625 | 10.489625 |

The four copy executions each return `LOCAL_SPOOLED` and leave exactly one
ready record in their owned store. Two independently acquired inodes were
used; OS caches were not evicted and acquisition order is retained. A first
inode execution is not a guaranteed globally cold machine.
[All exact-test stamps](startup/exact-test-phases.json),
[disposable stamps](startup/disposable-phases.json),
[clean failure](checks/clean-instrumented-test.log),
[clean repeat](checks/clean-instrumented-repeat.log).

The retained original 308/311 ms failures and remediation-5's 313 ms failure
remain unresolved original-artifact results. The original unchanged binary
was run once before rebuilding in this unit and passed; its exact elapsed
was not emitted. The newly bracketed 289 ms failure is a different, identified
test-feature artifact exercising the same assertion, not a retrospective
phase decomposition of 308/311/313 ms. No repeat repairs a first failure.
[Original repetition](checks/original-test.log).

## 3. Dominant supported bottleneck and ranked direction

**The demonstrated failing boundary is dominated by launch-to-earliest-main,
not companion connection, spool processing, stdout or parent reap.** This
interval is already greater than 250 ms in the clean-source failure.
An in-main watchdog or faster post-entry operation cannot remove it.
Absent locator means this fixture does not wait out a companion-connect
budget. Processing after mod-batch entry takes 8.638583 ms in that failure.
The earlier 54.561875 ms locator-to-answer control includes validation and
spool publication/fsync; individual I/O calls were not split because they do
not dominate the reproduced failure.

There is no demonstrated product performance repair yet. Loader, executable
validation/signature handling, caching, Rust pre-main initialization and
scheduling remain competing causes. Parent spawn returning in 0.237292 ms
rules out spending the whole 279.945500 ms synchronously in the parent spawn
call; it does not reveal when the child was runnable or why it reached main
late. The application controls its payload/build and post-entry work, not
kernel scheduling or validation cache policy.

**Rank 1, separate unit only: a compact build of the existing capture helper.**
Assess helper-specific size optimization/code generation/LTO, preserving the
same `threadspace-hook` executable contract, all subcommands, shared
mod-batch implementation, ancestry, typed receipts, spool fsync and watchdog.
This targets executable payload/prologue exposure, the only implicated phase
with a narrow application-controlled lever. It is a falsifiable build-profile
hypothesis, not a proven causal mechanism or authorization to install it.

Source basis: the capture helper retains conventional capture, observer-proof
inventory and optional qualification paths through its main dispatch. The
installed release helper's Mach-O TEXT segment is 1,146,880 bytes, including
119,612 bytes of exception frames; the diagnostic helper's TEXT segment is
2,310,144 bytes with a 2,801,664-byte LINKEDIT segment. Both load only libSystem
and libiconv, so removing UI-framework startup is not a supported explanation.
The signed helper is already release-built and previously had a 267.305 ms
launch-to-mod-batch-entry control. Debug-symbol stripping alone cannot be
claimed to fix both installed and test workloads. Splitting off observer-proof
would also touch accepted ownership routing; it is not the recommendation.

Falsification test: predeclare two first/repeat inode pairs per build arm,
alternate baseline/compact acquisition order, use the same compiler, feature,
signing policy, machine/boot, original record/budget and fresh absent-companion
store. Bind all artifacts to clean sources. Retain every first execution and
phase trace; require unchanged receipt bytes/UUID/status and durable spool
readback. Exercise the original launch-inclusive test on its intended helper
artifact as well as the signed disposable helper. Reject the hypothesis if
cold launch-to-main does not fall in the discriminating comparison, any
receipt/durability behavior changes, or the 250 ms test still fails. A warm
pass or a different signing/cache environment is insufficient. A smaller
file alone is not evidence that startup improved. Do not install after only
this control; F4 still needs a defensible callback measurement method.

## 4. Clock feasibility

**CLOCK QUALIFICATION CAPABILITY BLOCKED**

Installed evidence remains Claude 2.1.295 with embedded Bun 1.4.3 revision
`eecfd55debb281c3fe3518098b1a49f76429637c`. The previously acquired actual VM
has no Bun/process globals; performance.now delegates through guarded
helpers.now to host performance.now, without an exposed timeOrigin or a
certified native-rate/precision relationship. Current WebKit is 22625.2.7.1;
the retained cohort's 22625.2.5.11.1 and different boot remain separate.
Same native counter/boot proof for these subprocess stamps does not qualify
JavaScript or WebKit capture/DOM timestamps.

The [supported host API guide](https://code.claude.com/docs/en/plugins/mods/api)
describes mediated process calls that complete asynchronously. The
[API inventory](https://code.claude.com/docs/en/plugins/mods/reference) supplies
clock/timer, process and HTTP methods, with no documented boot-tagged native
callback marker. The [public declarations](https://raw.githubusercontent.com/anthropics/claude-code/main/mods/types/claude-code.d.ts)
identify 2.1.277: clock.now is an asynchronous epoch-millisecond number;
performance.now is a number with no native mapping contract. They are not
installed-version proof. The retained exact 2.1.295 declaration identity is
`e2e13b2ceb80ff3cde49d8427ebe19c782a24caa817c7b20c56f91925a06f9cb`,
21,463 lines, with the selected APIs compared in the retained M2 evidence.
Its full file was unavailable in bounded local lookup, including the recorded
bundled-skill/type locations. The [authoring documentation](https://code.claude.com/docs/en/plugins/mods/create)
requires the installed build's declarations to control. No new inference or
interactive provider session was launched to recover types. No unsupported
VM access or invented native-counter call was attempted. The finding is that
an adequate supported method is **not established**, not a claim about every
private capability of Claude's implementation.

Supported asynchronous native calls can provide **finite causal intervals**:
a native marker acknowledged before capture supplies Clo; a helper native
marker following capture supplies Chi. A marker requested only after capture
cannot supply a nearby Clo. Periodic/previous acknowledgements leave their
full gaps, especially for initial events arriving before the first marker.
Awaiting a marker before recording an already-entered callback would shift
the target and block provider progress. A streamed marker or HTTP request
does not turn its receipt into callback-entry time. No persistent marker
service or replacement protocol is introduced.

For the retained actual window the closest source-supported native enclosures
remain observer capture→COMMIT p95 upper 26,666.582459 ms and COMMIT→DOM p95
upper 26,363.948375 ms. Their width cannot certify the gates; it does not
establish actual latency of that size. Tight native DOM-after-application
acknowledgement is separately feasible in the app, but cannot supply the
missing observer capture lower endpoint. Empirical affine calibration is not
a maximum clock-rate proof. Qualified sample counts remain zero; qualified
stays false. Jhi−Clo, Dhi−Jlo and Dhi−Clo are unchanged.
[Clock capability assessment](startup/clock-feasibility.json).

## 5. Explicit decision and next unit

The smallest missing capability is a provider-supported, engine-origin,
boot-tagged native marker immediately before actual callback dispatch, or a
vendor-supported finite counter/rate/precision binding for the installed
performance.now implementation. A before-dispatch marker conservatively
bounds callback capture without replacing it; its scheduling gap remains in
the interval. Its proposed payload is only runtime/boot/counter/correlation
metadata. It needs no prompt/output text, and should not add a callback wait.
It is a capability requirement, **not an API that exists in the current VM**.

The explicit owner measurement decision is: retain callback-entry C and make
that provider clock/marker capability a prerequisite, or commission a separate
measurement-contract amendment if it cannot be obtained. No alternative
endpoint, conditional empirical rate model or threshold waiver is authorized
or applied here. With current established interfaces, closure remains blocked.
Separately authorize the compact-helper comparison if that build hypothesis
is worth one implementation unit; no install/full cohort follows automatically.

A next unit starts from this report and the clean qualification source, owns
only the selected helper build/profile and startup-control paths, freezes all
receipts/durability/observer ordering, and predeclares the comparison above.
It must settle the measurement capability before a new complete cohort.
A failed cold control is retained and ends the attempt; it is not followed
by warm retries until PASS. If no supported clock capability is provided,
return the precise blocker and stop. F4 normal-path thresholds and the original
250 ms assertion remain unchanged.

## 6. Preserved scope and checks

The full retained 20-hook/21-observer population remains 41 captured, committed
and hydrated applied-DOM joins, with no exclusions/missing captures and the
existing hook double-loss control. It was not rerun. Observer diagnostic
capture→COMMIT p95 remains 119.213624 ms; all qualified counts remain zero.
No new observer, host, COMMIT/DOM or telemetry-on/off campaign occurred.

F1A/F1B/F2/F3 native candidate PASS evidence, accepted M1/M0C scope, UUIDs,
typed receipts, one in-flight drain, order/retries, durable SQLite/spool and
fail-open provider callbacks are preserved. D-0009/D-0010 remain PROPOSED.

**LEGACY DEV-STORE COMPARISON NOT APPLICABLE TO EMPTY GENESIS — qualified anchor/suffix PASS**

Executed: initial and clean test-feature builds; qualification-only build and
two-test suite (2 PASS, no startup diagnostics); original-artifact single test
repetition; diagnostic exact-test first/repeat pairs; two disposable-inode
first/repeat controls; the two-test diagnostic suite (2 PASS); flag-off empty
batch (silent stderr); four rejection controls (wrong boot, wrong parent clock,
missing main marker, reversed flush); installed hash/signature readback.
The exact test's clean first execution fails at 289 ms; its repeat passes at
13 ms. These results coexist. A suite warm pass is not closure.

Not run: compact build/performance repair, Dev rebuild/install, real-provider
host timing, full F4 campaign, new cross-runtime rate calibration, actual writer
COMMIT/DOM qualification, previous acceptance requalification or M3 work.

SPEC, MILESTONES, prior raw evidence, provider mod, spool implementation and
journal are unchanged. No ORCHESTRATION.html exists; none was created or
realigned. Private binaries, exact source snapshots, raw logs and disposable
spools remain outside Git. Published clocks/PIDs/fixture identifiers are
functional measurement data; no prompt, model output, credentials or owner
Terminal content is collected. [Phase controls](startup/phase-controls.json),
[diagnostic suite](checks/instrumented-suite.log),
[publication checks](checks/publication.json).

**F4 DESIGN BLOCKED — OWNER MEASUREMENT DECISION REQUIRED**
