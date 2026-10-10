# M2 evidence — Manually launched Claude vertical slice

**Current status: M2 REMEDIATION INCOMPLETE. Do not merge or start M3.**

The independent review of `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`
rejected four bounded areas: observer ownership/reload authority (F1), integration
ownership/cleanup (F2), the minimized-window positive Return case (F3), and the
normal-path latency measurement (F4). The focused implementation and execution
records are under [`remediation-1/`](remediation-1/README.md). The current
[`manifest.json`](manifest.json) is the status and provenance index; it does not
inherit the rejected candidate's PASS labels.

The accepted main/M1 endpoint remains
`af9b285da529890bc441ea00f1a92e73e39902a8`. This remediation does not merge,
deploy, migrate an owner store, change owner Claude configuration, or implement
M3. D-0009 and D-0010 remain **PROPOSED — pending independent re-review**.

## Current qualification

| Finding | Implemented or established | Remaining mandatory evidence |
| --- | --- | --- |
| F1A: delayed callback ownership | An immutable native Turn/actor ownership ledger preserves the original Session, observer epoch and generation. Ambiguous reused IDs and first-seen late events stay nonauthoritative. Actual TypeScript captures are tested through the production adapter and SQLite. | A new source-matched Dev application and focused real-Claude ownership/native-origin smoke. |
| F1B: reload authority | Independent helper-created proof, scope seal and a later qualified Turn start replace permanent authority from historical attachments. The proposed reducer is now version 4, with an explicit retained-fact upgrade for reducer-3 development stores. | Native execution of the new inventory/kernel proof producer and unchanged-ID reload/new-Turn smoke. The old 819 ms RESTORED result does not qualify this predicate. |
| F2: integration ownership | Structural edits become conflicts; still-referenced resources and ownership records survive partial uninstall. Wrong installation identity refuses, and failed acquisition gives the harness no cleanup authority. | Focused Dev native integration smoke using disposable settings/resources. Portable positive and mutation-control results are retained. |
| F3: minimized Return | Both original failed attempts and the inherited M0C source history have been examined. Route product code remains unchanged; the cause is not established. | **BLOCKING:** at least five independently verified minimized-window successes, relevant negatives and complete phase evidence under the unchanged 2,000 ms deadline. No new native repetitions were executed here. |
| F4: latency | Actual SQLite COMMIT brackets, qualified monotonic/cross-runtime bounds, DOM-application marks and explicit population checks replace the old calculation. Incomplete clock, source-census, identity or tail evidence refuses PASS. | Complete independently closed source populations, positive native clock qualification and a focused source-matched native capture → COMMIT → DOM sample for both sources. No native p95 has been established by this remediation. |

The execution environment is Linux. Portable tests and macOS-target Rust type
checks are reported separately from retained macOS execution. A successful
cross-target compilation is not a native application build or native test run.

## Preserved historical qualification

All rejected raw native evidence remains in its original location. Exact copies
of the rejected status documents are retained as
[`historical/rejected-candidate-README.md`](remediation-1/historical/rejected-candidate-README.md)
and [`historical/rejected-candidate-manifest.json`](remediation-1/historical/rejected-candidate-manifest.json).
Their claims must be read with the review dispositions below.

| Area | Retained result and scope | Primary evidence |
| --- | --- | --- |
| Ordinary vertical slice | Ten real Claude Code 2.1.295 cycles; one persistent Session/worker; ordinary completion/follow-up; worker retained after `/exit`. This is historical execution, not a rerun of changed ownership code. | [`vertical/20261009T041131Z-dev/`](vertical/20261009T041131Z-dev/) |
| Owner commands | Twenty Mark handled actions committed and canonically resolved in the original vertical run. | Vertical cycle, trace and owner-command records. |
| Ordinary exact Return | Thirty dedicated exact Returns, independently selected-tab/frontmost read back; zero wrong targets in that population. Native route implementation remains source-applicable. | [`routes/20261009T041519Z-dev/`](routes/20261009T041519Z-dev/) |
| Provider compatibility | Retained 2.1.295 declaration comparison and valid original engine/core, origin and fail-open tests. New ownership changes have separate portable evidence. | [`provider/claude/`](provider/claude/), [`remediation-1/f1a/`](remediation-1/f1a/README.md) |
| Canonical evidence sets | The original 24,400 permutations and 2,440 SQLite cross-checks remain baseline evidence. Changed reducer logic has separate source-identified runs. Genuine reducer-1/2 fixture qualification is preserved. | [`permutations/`](permutations/), [`remediation-1/f1b/`](remediation-1/f1b/) |
| Integration baseline | Ten original native install/reinstall/remove cycles across empty and owner-like configurations. F2 conflict/identity cases require the new focused qualification. | [`integration-cycles/20261009T044548Z-dev/`](integration-cycles/20261009T044548Z-dev/) |
| Mod-batch baseline | Original 500 drains, 3,060 committed records including warm-up, stable UUID retries, spool fallback and retained receipt faults. Changed helper/observer paths require their focused checks. | [`mod-batch/summary.json`](mod-batch/summary.json), [`faults/`](faults/) |

The original native application was `~/Applications/Threadspace Dev.app`, bundle
ID `ai.scalinity.threadspace.dev`, built from
`f4ef5f156f7086962bafa92728cc4cace5a58c3c`. Its outer/companion/helper hashes are
retained in [`native-build.json`](native-build.json). Those executable hashes do
not identify a build of the remediation source.

## Corrected dispositions

- **Minimized-window Return is an M2 blocker.** The two TIMEOUT results at
  2,007 and 2,003 ms were not successful exact Returns. Readback still selected
  the harness spare TTY after the target was no longer minimized. The old
  conditional `wrongTarget:false` field does not establish absence of focus
  side effects. This positive gate is not deferred to M13.
- **Normal-route p95 remains unmet:** 895 ms versus the 750 ms target, with
  maximum 968 ms. Its accepted M0B/M13 deferral is separate from the minimized
  positive-case blocker. The hard per-attempt budget remains 2,000 ms.
- **The original normal-path latency PASS is withdrawn.** The old harness
  measured receipt before the journal transaction as if it were commit, used
  wall-clock-derived differences, and did not reconcile the complete observer
  DOM population. Its reported 2/36 ms capture-to-commit figures are not valid
  evidence for the mandatory capture/commit/visible-state targets. The raw
  historical summary is preserved without alteration.
- **The extra conventional-hook human-input record remains an M3 limitation**
  while it stays unaccepted and semantically inert. Automatic human follow-up
  resolution remains disabled; explicit Mark handled is required.
- **Full Terminal restart remains the separate M15 BLOCKED limitation.** No
  Terminal restart was performed for this remediation.
- **The private time-lapse is unavailable here.** Retained recording metadata
  does not amount to independent pixel inspection.
- **The disclosed pattern-based process termination remains an ownership
  incident.** Its matching PIDs and effects were not recorded; the remediation
  does not assert that no unrelated process was affected. See the
  [`incident record`](remediation-1/f2/harness-ownership-incident.md).

## Reproduction and next gate

Use the focused runners and exact limitations in
[`remediation-1/README.md`](remediation-1/README.md). Do not rerun the complete
historical native campaign merely to refresh a date. Changed paths need a new
source-matched **Dev** build and disposable, harness-owned native fixtures.
Independent M2 acceptance and the appropriate closeout gates precede any merge,
production deployment or M3 work.
