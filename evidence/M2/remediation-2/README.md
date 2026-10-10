# M2 Mac-native remediation attempt

**M2 NATIVE REMEDIATION INCOMPLETE. Do not merge, start M3 or deploy to production.**

This attempt ran on macOS 27.2 (26B5091g), arm64, with Rust 1.99.0. It began
from published `m2` commit `877ecf243760ee17dce61e8809f09919aa729a39`.
Local and remote main remained `af9b285da529890bc441ea00f1a92e73e39902a8`.
The original portable implementation and evidence remain intact. D-0009 and
D-0010 remain PROPOSED.

## A. Candidate and provenance

The clean Dev build and installed application use source
`4aef4a96f0e21844b35271246a151629a77bceb2`. The only implementation change
since `877ecf2` selects a retained Claude executable in the unshipped M2 harness;
it leaves the default launcher unchanged. The default launcher now reports
2.1.296; the retained executable independently reports 2.1.295. No owned Claude
Session was established, so executable/version identification is not Session proof.

`build-pinned-execution.json`, `build-pinned-info.json`,
`build-pinned-hashes.json` and `install-execution.json` bind the build and install.
The initial successful `877ecf2` build is separately retained. The accepted
inside-out workflow signed the helper, companion and outer application; strict
and deep verification succeeded. Required Tauri/Tao/Wry pins and D-0008 remained
unchanged. Qualification controls retain their existing native feature gates;
frontend production stripping is still the separate M15 obligation.

| Executable | Installed SHA-256 |
| --- | --- |
| Outer | `d849801d798576a67fedb763b6fe843297fef91a6e12f3064ade741d6becf8f6` |
| Companion | `43d59886d6a8b7cd5c04734b8e738e4883970cbbabe5cd812a8516b039d50f1b` |
| Helper | `554ce67cb42f9443cb23a2f7eae13a878c35502a04275e5230e1ef56e120c60f` |
| Rust harness | `0874be1567b8f6573aa15634f84f50e56583c85e2ddbd231f0300cab9d7aeb73` |

All installed application hashes matched the source build. The new companion
started under launchd. A consistent SQLite backup of the existing Dev store
passed integrity verification before installation; auxiliary files were also
preserved privately. The Dev store was not reset. The production application,
production store and owner Claude settings were not modified.

The later Python CLI runner is bound by its recorded file hash. It is not a
component of the signed application or compiled Rust harness. Publication adds
qualification tooling and evidence; it does not change product, reducer,
generated-contract, observer or native route source after this build.

## B. F1A — INCOMPLETE

The prepared owned-Terminal fixture could not acquire a scriptable surface.
There are no new original Session/Turn IDs, callback-entry ownership records,
native observation UUIDs or real engine/core settlements from this attempt.
The existing 14 controlled observer cases and their adapter/SQLite/restart
witnesses remain credited at their recorded source, without a new native claim.
The original-scope and wrong-Session native smoke remains required.

## C. F1B — INCOMPLETE

The source-matched helper is built and installed, but no owned native Session
was established. Independent ProcessKey/executable/inventory proof, durable token
admission, generation/epoch seal, post-seal Turn, pending promotion and native
replay were not exercised here. The retained previous-version migrations and
causal controls remain unchanged; the historical 819 ms result is not credited.

Source review identified a limitation in the legacy `m2-faults reload` runner:
it waits for RESTORED before submitting the post-reload Turn. That ordering does
not establish the revised seal-before-start-before-outcome contract. It was not
executed or represented as qualification. A resumed native attempt needs the
explicit proof/seal/post-seal sequence and required negative cases.

## D. F2 — PASS, focused native qualification pending review

`f2-native-cli-final/summary.json` records 13 cases across 31 actual installed
Dev CLI calls. The runner uses exclusively created disposable configurations.
It checks installation identity and exact staged helper/observer hashes against
the installed bundle. Foreign hooks/settings survive; unchanged removal restores
the original bytes. Matcher, timeout, command, type and plugin-directory changes
each yield `complete:false`, preserve the edit and helper/mod/record resources,
refuse reinstall without mutation, and permit complete cleanup after resolution.
Wrong scope and configuration refuse with all fixture bytes/modes unchanged.

The final additional asset case runs actual Claude 2.1.295 `plugin validate` on
the installed observer copy using an exclusive disposable provider configuration.
Validation passes. The staged helper also executes silently against an isolated
disposable hook store; helper and observer source hashes stay stable until valid
cleanup. This controlled hook fixture has no Claude parent and claims no native
Session or outcome authority. The earlier 12-case CLI campaign remains preserved
separately and is not added to the final population.

Nine additional native CLI refusals ran while the F3 invocation held its own
session installation, preserving that installation byte-for-byte and by mode.
The F3 runner subsequently removed only its acquired installation. The native
Rust executions passed 42 installer tests and one actual Scratch failed-activation
test. These include ten-cycle restoration, changed-between-read-and-write refusal,
real mod staging, foreign preservation, identity and conflict controls. Atomic
file replacement uses the tested production same-directory sync/rename path;
power-loss qualification is not claimed.

`f2-native-cli/` preserves the first failed test and matched-record cleanup. The
test incorrectly required an unchanged installation timestamp on reinstall;
`setup::install` deliberately refreshes that field. The corrected runner requires
all remaining record fields, resources, rollback point and settings to remain
identical. The failed runner source is retained. A first Scratch test filter
matched zero tests; no PASS is credited from it. The corrected filter ran the
failed-acquisition test successfully.

Reproduce only when the Dev integration slot is unoccupied:

```sh
python3 tests/native/tools/m2_integration_native.py \
  --app "$HOME/Applications/Threadspace Dev.app" \
  --claude "$HOME/.local/share/claude/versions/2.1.295" \
  --output /absolute/new/disposable-output-directory
```

The runner refuses an existing installation and retains fixtures on failure.
It starts no provider Session and alters no owner settings. Fixture files are retained
privately; every acquired integration was removed. No broad process cleanup ran.

## E. F3 — INCOMPLETE, blocking

Both new native attempts are retained under
`../remediation-1/f3/native/`. The first stopped with
`TargetNotRunning (-600)` before acquisition because Terminal was not running.
Terminal was then launched in the background. The second passed the owner-idle
gate and acquired its disposable session installation, but Terminal never
answered window-element queries during the existing 600-second readiness wait.
Measured setup wait: **615,244 ms**; full runner: **616.134 s**. Its complete
cleanup report confirms its acquired integration was removed, no uncertain open
occurred and no fixture window was created.

Read-only native probes independently returned Terminal version 2.15 in
30.852 ms; window count timed out at 5,004.732 ms and selected-TTY query at
5,007.585 ms. Those probes identify an unavailable prerequisite, not the cause
of the historical minimized-route failure. No Terminal restart, permission
change or interference with another window was attempted.

**Successful minimized Returns: 0 of 5 required. Returns issued: 0.** There is
no pre-route Session/process/TTY/minimized witness, selected-tab readback, route
phase timing or absent-Session result from this attempt. Zero attempts cannot
establish zero wrong targets. The 2,000 ms product deadline is unchanged; the
setup wait is not a route attempt. The original 2,007/2,003 ms failures remain
untouched. Fine AppleEvent timing and the original root cause remain unqualified.

## F. F4 — INCOMPLETE

The owned native provider fixture was not established and the Dev UI measurement
fixture was not initialized. The exporter and calculator were not executed.
No independent hook issuance witness, double-output-loss discriminating case,
closed observer population or positive clock rate/precision qualification was
established. The built source still intentionally refuses these missing inputs.

For both sources, issued/captured populations, accepted UUIDs, retries,
rejections, zero-projection/projection-changing work, matched/unmatched DOM,
telemetry failures, exclusions and close proof are **NOT ESTABLISHED**, not zero.
New qualified samples are zero. Native p50/p95/max are unavailable for all three
metrics. No capture/COMMIT/DOM records or calibration bounds are fabricated.
The source-specific 100/100/250 ms p95 targets and the retained actual SQLite
commit-hook negative control remain unchanged. No historical invalid metric is
reinstated. Census design, clock qualification and native measurement remain work
for the resumed bounded attempt after the owned fixture can be established.

## G. Preserved evidence

The ten-cycle vertical slice, thirty dedicated ordinary Returns, twenty Mark
handled commands, provider compatibility, original transport and previous-version
migration records remain at their original identities. Product sources did not
change in this Mac attempt. Changed F1/F2/F4 paths retain their remediation-1
qualification boundaries; historical executions are not attributed to the new
build. No private recording pixels or owner histories are published.

## H. Remaining limits and checks

The immediate blocker is Terminal window/tab scripting. No root cause is inferred
from a successful application-property query. Native F1A/F1B, the minimized
positive and conservative negatives, and all F4 measurement prerequisites remain
open. M3 deduplication, M5 notification qualification, the separate M13 normal-route
p95 miss and M15 production exclusion/update/full-Terminal-restart obligations
retain their existing dispositions. M1 architecture and acceptance contracts were
not changed. No ORCHESTRATION.html exists, so no orchestration page was created.

The changed Rust harness passed check, build and Clippy with warnings denied.
The native F2 tests/CLI results above are fresh executions. Workspace-wide tests,
schema generation/freshness, UI tests, provider kit tests, reducer permutations,
native ownership/reload and latency measurement were not rerun here. Their earlier
records stay source-scoped; an unrun check is not a new PASS.

`publication-record.json` identifies curated copies. Unedited originals and exact
redaction values were preserved outside Git before literal path-prefix and signer
display-name redaction. Private Dev backups and histories are excluded. The
publication scan and committed provenance are separate close-out records.

## I. Decision

| Gate | Verdict |
| --- | --- |
| Source-matched Dev build/install | PASS |
| F1A | INCOMPLETE |
| F1B | INCOMPLETE |
| F2 | PASS |
| F3 | INCOMPLETE / BLOCKING |
| F4 | INCOMPLETE |

Stop at this incomplete candidate. No merge, M3, production deployment or owner
acceptance is authorized. Preserve the exact blocker and resume only this native
closure unit when its required owned surfaces can be verified.
