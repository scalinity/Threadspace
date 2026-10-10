# M2 evidence — Manually launched Claude vertical slice

**Current status: M2 REMEDIATION INCOMPLETE. Do not merge or start M3.**

The independent review of `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`
rejected four bounded areas: observer ownership/reload authority (F1), integration
ownership/cleanup (F2), the minimized-window positive Return case (F3), and the
normal-path latency measurement (F4). The focused implementation and execution
portable records are under [`remediation-1/`](remediation-1/README.md). The
source-matched Mac build, native F2 qualification and blocked F3 attempts are
under [`remediation-2/`](remediation-2/README.md). Bounded Terminal recovery,
five minimized positives, native ownership/reload and the incomplete latency
measurement are under [`remediation-3/`](remediation-3/README.md). The current
[`manifest.json`](manifest.json) is the status and provenance index; it does not
inherit the rejected candidate's PASS labels.

The accepted main/M1 endpoint remains
`af9b285da529890bc441ea00f1a92e73e39902a8`. The production application/store
and owner Claude settings remain untouched. The existing Dev store was backed
up before installing the reducer-4 Dev candidate. No merge or M3 occurred.
D-0009 and D-0010 remain **PROPOSED — pending independent re-review**.

## Current qualification

| Finding | Implemented or established | Remaining mandatory evidence |
| --- | --- | --- |
| F1A: delayed callback ownership | PASS in the focused real-Claude 2.1.295 native-origin smoke plus 14 separately labelled controlled ownership/fail-open cases, through adapter/SQLite/restart. | Independent review of source-bound native and controlled records. |
| F1B: reload authority | PASS in fresh independent native proof, matching reload seal, post-seal Turn and RESTORED outcome; 19 retained native envelopes pass the unchanged complete isolated replay oracle. Causal negatives and genuine reducer-3 migrations pass separately. | Independent review. The long-lived Dev store's empty-genesis full hash still fails on legacy baseline history; that diagnostic is preserved and is not represented as a passing full-store check. |
| F2: integration ownership | Structural conflicts, retained resources, strict installation identity and acquired-resource cleanup passed the new focused native CLI/asset campaign: 13 cases/31 calls plus 9 refusal checks, 42 native installer tests and one native Scratch test. | Independent review of the new native records; historical portable controls remain source-scoped. |
| F3: minimized Return | PASS: five exact verified minimized Returns in 1,206–1,260 ms, absent-Session refusal without focus mutation and verified acquired cleanup. Previous failures remain preserved. | Independent review of the original positives and separate source-bound finish; immutable deadline is 2,000 ms. |
| F4: latency | 20 independently issued hook captures and 21 independently closed real observer captures join true COMMIT and applied-DOM records. The double-loss census control retains the missing invocation. | **INCOMPLETE:** cross-runtime maximum rate/precision qualification and conservative source-specific performance proof. Unqualified observer capture→COMMIT diagnostic p95 is 216.025 ms; no valid native latency PASS is established. |

The portable remediation ran on Linux. The subsequent Mac attempt built and
installed source `4aef4a96f0e21844b35271246a151629a77bceb2`, qualified F2,
and preserved the Terminal scripting blocker. The separate remediation-3
attempt records actual new native execution after two bounded element-query
passes. None of its evidence is inferred from a successful build or F2 result.

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

- **The original minimized-window Return failures were M2 blockers.** The two TIMEOUT results at
  2,007 and 2,003 ms were not successful exact Returns. Readback still selected
  the harness spare TTY after the target was no longer minimized. The old
  conditional `wrongTarget:false` field does not establish absence of focus
  side effects. The five source-matched remediation-3 positives close that
  focused native gate pending review; it was not deferred to M13.
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
