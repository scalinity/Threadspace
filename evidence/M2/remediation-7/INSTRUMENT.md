# Composed duration instrument policy

This policy precedes the single installed conformance execution. No acceptance
population is evaluated in this unit. The machine-readable policy is
[instrument-policy.json](instrument-policy.json); the fixed runner is
`tests/native/tools/m2_duration_conformance.py`.

The instrument is the installed Claude executable's guarded host
`performance.now()` wrapper, read by the actual qualification collector's
`readCapture`, within one source epoch. The native instrument is macOS
`CLOCK_UPTIME_RAW`, shared by the original SQLite COMMIT brackets and new
companion stamps. Provider, observer source, probe source, policy, boot, OS
build and power configuration are bound in the report. Changed identities,
reset, nonfinite readings, unresolved coarsening or a suspend discontinuity
refuse qualification. This does not certify an arbitrary oscillator or a
universal clock rate.

One disposable provider receives initialization control only. Three checks
request timer delays of 0, 100 and 250 ms, once each, in that order. Each JS
endpoint is causally enclosed by independent native subprocess stamps before
and after it. The report retains actual lower/upper elapsed enclosures and
widths. Requested delay is never used as elapsed truth. No capture helper is
invoked or prewarmed. No inference, integration installation or service is
performed. Initial reads and every failure remain retained.

The declared engineering observer allowance is the maximum positive
native-upper-minus-observed-duration residual over this fixed check, plus
the largest positive step in the 256 adjacent reads at each scale and a
1 ms guard. Round it outward to 0.001 ms. The finite engineering scope is
0..400 ms under the pinned implementation and awake platform conditions;
the allowance must be at most 30 ms, each enclosure width at most 60 ms,
and the observed read step at most 1 ms. These limits are a predeclared
usefulness test, not a claim that transport brackets measure clock error
exactly. Enclosure slack stays included. A check outside scope or a larger
allowance fails this prerequisite; it is not repeated or tuned.

The native allowance is a positive 0.001 ms numerical guard for integer-ns
conversion and outward rounding on the supported same-boot native counter.
It is not a physical oscillator-rate certificate. Every reported segment
rounds outward to 0.001 ms; instrumentation work and transport/queue delay
remain counted. No qualification overhead is subtracted.

Observer bounds are `U_CJ = (receiptValidation - originalPhaseCapture) +
observer allowance` and `U_JD = (nativePostDomAck - originalPreCommit) +
native allowance`. Sum them per observation before nearest-rank percentiles.
Hooks retain their native capture semantics and use separate same-boot native
bounds. A central estimate and its outward bound straddling a threshold
stops the verdict. Unqualified historical schema 2 evidence stays unchanged.

Conformance qualifies this installed instrument implementation within the
stated engineering scope. Each later measurement runtime still needs its
own original-epoch receipt join, no-reset readings, final source census and
same-boot power continuity; this report cannot close a lost runtime's tail.
