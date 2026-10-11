# Replacement F4 instrument prerequisite

**REPLACEMENT F4 INSTRUMENT INCOMPLETE**

The mandatory installed-runtime duration premise is **BLOCKED**. No calibration,
timestamp responder, falsification suite or positive conformance was executed.
The implementation stopped at the prerequisite boundary. M2 remains unaccepted.

## A. Identities

The fetched local and remote `m2` base is
`bf26535bf0ce8582281cc4e7cd56ab4f96fa25f6`. Local and remote accepted `main` are
`af9b285da529890bc441ea00f1a92e73e39902a8`. The composed implementation ancestor
is `d81f3f14684e2cd6c1dcb24fb5540b50e713262d`. Startup had no uncommitted work.

The provider's canonical executable is
`/Users/danny/.local/share/claude/versions/2.1.296`, SHA-256
`c9b5341637becbd423ddffc5b254afb645682a3868cb708bbc6cc0e7bb419937`.
The executable's `--version` independently answered `2.1.296 (Claude Code)`.
The isolated native callback answered version/base `2.1.296`, built at
`2026-10-09T01:31:52Z`. Kernel process path/parent/UID/birth readback verified
the disposable executable. Code-signature verification succeeded; this is
identity evidence, not clock-semantics evidence.

The corrected adapter's canonical four-source identity is
`d461d6d6cb8769dbe65d3e229212f052f5ba0c45a936f92ba76d18b18e88214b`.
The collector is `packages/provider-mod/hooks/latency.ts`, SHA-256
`ba8deb1751f9f9a6d969489d138a53b02923a8832b442c8d8f433284b574c8bc`.
The existing calculator SHA-256 is
`fc0950f92c4d322cffb7bad4af545521c9693799612028d25ec142ac544c4464`;
the existing failed runner SHA-256 is
`038fbbf24d6a53d121127a961628352356d2efb8edcc92a98906aa64893f004e`.
Both are unchanged. There is no replacement evaluator or frozen replacement
policy. Boot is `B25EE6BF-6973-4B99-9543-E37EF1A59C87`; macOS is 27.2,
build `26B5101f`.

[identities.json](identities.json) records native identity, per-source hashes,
and declaration extraction. The final publication commit is the Git commit
containing this directory; its exact SHA and remote equality are reported at
close-out, outside its self-referential contents.

## B. Installed-runtime semantics

The declarations were extracted from the exact executable's embedded
`claude-code.d.ts-77980b00.txt.zst` asset, not inherited from an older release.
At binary offset 232158688 its compressed frame is 138638 bytes; decompression
yields 621811 bytes, SHA-256
`cf4afc180ffef03cb8138916c0e8f49b8186242a81e226f01ba24341018e5e90`.
Complete vendor declarations and binary ranges are retained privately, because
the repository's existing provider evidence excludes complete vendor declarations
for licensing reasons. [installed-api.json](installed-api.json) records exact
line ranges and hashes.

The installed declarations support the `session.start` callback, typed `next`,
`$.session.version()`, and `$.http.fetch(url, init)` with `socketPath`.
`HttpInit` has method, headers, body, auth and socketPath, with no timeout or
signal field. The binary's mod HTTP adapter forwards those fields. No live
Unix-socket request was made; declaration/implementation support is distinguished
from a successfully exercised request. No unsupported fields were introduced.

The installed mod environment's performance declaration at line 14877 supplies
only `now(): number`. It states no duration, precision, coarsening or awake-time
semantics. The isolated native callback returned
`"() => guarded(helpers.now)()"`. Static binary inspection binds that wrapper
through the injected helper to host-global `performance.now()`. The guard
translates host exceptions; the inspected helper directly returns the host
clock's result. This proves the wrapper-to-host join, not the native host
clock implementation's duration contract.

Embedded runtime build literals identify Bun `1.4.3`, revision
`eecfd55debb281c3fe3518098b1a49f76429637c`. Exact-revision upstream source
retrieval did not supply an auditable implementation: the GitHub tree request
returned HTTP 422, "Invalid object requested. SHA must identify a commit or a
tree." The three individually recorded source-path requests returned HTTP 404;
those alone do not establish that a revision is unavailable. The tree response
is the revision-level failure. [runtime-source-access.json](runtime-source-access.json)
retains the statuses and addresses.

The vendor's [mod API guide](https://code.claude.com/docs/en/plugins/mods/api)
and [build-specific declaration guidance](https://code.claude.com/docs/en/plugins/mods/create#get-type-definitions-for-your-version)
were inspected. The latter explains that declarations are normally emitted in
interactive sessions. The initialization-only run did not emit them; extraction
from the executable supplies the installed bytes without opening an owner
session. The public [Bun benchmarking guide](https://bun.sh/docs/project/benchmarking)
describes its performance clock, but does not supply the missing exact-build
duration, precision and awake-time premise. Generic documentation and one
runtime reading were not promoted to proof.

**Failed premise:** no exact-installed-API vendor duration statement or completed
auditable join to the exact host runtime clock implementation establishes
monotonic elapsed milliseconds across asynchronous waits, supported precision/
coarsening and awake-time behavior. Those properties remain **UNVERIFIED**.
This does not establish defective clock behavior or an unavailable socket API.
The historical 2.1.295 executable has a different hash and supplies no current
proof. No calibration was used to fill this gap.

## C. Native responder

**NOT IMPLEMENTED / NOT BUILT / NOT RUN** after the mandatory stop. There is
no responder source, executable, native transcript or socket qualification.
The only native provider run was an initialization-only semantic probe, under
a 20-second external deadline, in an exclusively created directory with mode
0700. The harness verified the owned provider through libproc before sending
the initialization control. It closed only that process's stdin; the process
exited 0 and was reaped. No signal, Terminal session or owner process was acted
on. [execution.json](execution.json) distinguishes this cleanup from unrun
responder security/deadline controls. Raw process identity and provider output
remain private; their hashes are retained.

The [probe template](runtime-gate-probe.ts) and
[archived controller](runtime-gate-controller.py) preserve this native automation
as reusable qualification components. The controller is a readable transcription
of the executed inline controller, archived after execution; its file hash was
not frozen before the probe and is not offered as conformance source identity.
Its timeout-escalation paths were not exercised. It expects newly owned inputs:
a `gate-plugin` manifest with modules `["./register.ts"]`, the probe template's
single placeholder replaced with the exact owned `gate-host.json` output path,
and `provider-config/settings.json` containing an empty hooks object and
`disableAllHooks:false`. The ownership receipt supplies that newly created mode
0700 private root. These components were not rerun during evidence close-out.

## D. Identity-binding remediation

**NOT IMPLEMENTED / CONTROLS NOT RUN** because the semantic prerequisite failed.
Source inspection of `m2_composed_duration.py` lines 29–46 confirms the defect:
it checks profile/artifact provider and adapter equality, but does not join
those identities to conformance-owned identities; it checks only selected
numeric results from `qualify()`. Actual version, OS, epoch, collector,
responder and evaluator identities are not fully joined there. No new bad-join
execution is claimed, and no repaired evaluator is paired with old evidence.

All eight prescribed identity substitution controls remain unrun. The existing
failed profile remains unqualified. This publication contains no replacement
profile that can be used to qualify F4 samples.

## E. Predeclared conformance

[authorized-criteria.json](authorized-criteria.json) records the supplied
authorization, including 0/25/50/100/200/250/405 ms stimuli in quiet and fixed
background conditions, once each; later scope 0–400 ms; width 10 ms; fixed
observer allowance 10 ms; residual plus guard at most 10 ms; zero upper 20 ms;
405-ms coverage lower at least 400 and upper at most 425 ms; adjacent-step ceiling
1 ms; native numerical guard 0.001 ms; outward rounding.

This is an authorization record, **not an implemented or frozen instrument
policy**. No background fixture bytes/counts/schedule were selected or frozen,
and no positive runtime identities exist. The semantic-probe runtime ID is
`f2a1fca1-04e4-41b5-b15b-09114704d22f` and is not a conformance runtime or source
epoch. The supplied implementation prompt quotes the latest Pro dispositions;
a separate full independent reviewer response was not present in the repository
and is not claimed as independently inspected.

## F. Execution

Positive attempts: **0**. Positive rows: **0 of 14**, **NOT RUN**. All native
enclosures, observer elapsed intervals, residuals, precision controls and
complete-row conformance assessments are **UNVERIFIED**, not PASS.
The single semantic-probe clock value is an API reading, not an elapsed interval
or calibration. Every requested falsification test, including constant-offset,
0.9-scale, paused-clock and delayed-socket controls, remains **NOT RUN**.

The original remediation-7 result is preserved byte-for-byte: its first native
enclosure was 75.557417 ms and its retained full-row allowance 36.857 ms. Its
original rejection is unchanged; neither rerun nor allowance widening occurred.

## G. Preserved behavior

Every previously tracked file was compared with the supplied base, and all were
byte-identical before this evidence publication. [preservation.json](preservation.json)
records the comparison and the full remediation-7 inventory. Callback capture,
stable UUID/time/epoch retries, one in-flight delivery, typed receipts,
fail-open/exactly-once next, SQLite COMMIT, hydrated DOM authority, 100/100/250 ms
targets and the original capture wall budget were unchanged. Their behavioral
tests were **not rerun** in this unit.

F1A/F1B/F2/F3 native candidate evidence, accepted M0C/M1 contracts, proposed
D-0009/D-0010, and the reviewed legacy Dev-store checkpoint-plus-suffix PASS
were not amended. SPEC and MILESTONES were not edited.

## H. Remaining F4 gates

The exact-runtime semantic prerequisite and the replacement instrument itself
remain open. Subsequent source-matching installed application, composed
WebKit/native round trip, schema-3 full native exporter, compact cold-helper
gate and complete acceptance campaign remain separately gated. No installation,
application build, compact-helper experiment, acceptance cohort, merge, M2
acceptance or M3 work occurred. No ORCHESTRATION.html exists to realign.

## I. Publication

Only this distinct evidence directory is added. Private provider output,
declarations, binary excerpts, configuration and process readback remain outside
Git. [privacy-scan.json](privacy-scan.json) classifies the literal private
resource paths before replacement in publication copies. Real functional
executable paths, boot/runtime identifiers and source hashes are identified as
such; no pattern-based redaction of source or historical evidence occurred.
[manifest.json](manifest.json) hashes every artifact except itself. Publication
is restricted to `m2`, with normal push and no force push; accepted main is
unchanged. Post-push SHA equality, clean tree and manifest verification are
reported in the final close-out.
