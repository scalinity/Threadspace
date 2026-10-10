# M2 focused F1–F4 remediation

**M2 REMEDIATION INCOMPLETE. Do not merge, begin M3, or deploy this revision to a production store.**

This work repairs the four bounded findings from the first independent M2 review.
It starts from rejected candidate `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`
on `m2`, above accepted main/M1
`af9b285da529890bc441ea00f1a92e73e39902a8`. It preserves the rejected candidate
and its raw native evidence. It does not change SPEC, MILESTONES or accepted M1
architecture to waive an exit criterion.

The work was executed on Linux. No macOS application, Terminal session, native
Claude inference session, owner configuration or production store was exercised.
The former qualified Dev build at
`f4ef5f156f7086962bafa92728cc4cace5a58c3c` is historical evidence, not a build of
these changed paths. There is no new outer executable, companion or helper hash
to report. Source-bound portable tests and macOS-target Rust checks are distinct
from native execution throughout this handoff.

## Disposition and primary evidence

| Finding | Implemented and executed here | Mandatory closure still open |
| --- | --- | --- |
| F1A: delayed callback ownership | Immutable original Turn/actor/occurrence ownership; actual TypeScript counterexamples and adapter/SQLite chain | Focused real-Claude native-origin/ownership smoke on a matching Dev build |
| F1B: stale reload authority | Separate fresh inventory/kernel proof, generation-bound seal, causal Turn start/outcome join; genuine old-store migration and replay controls | Source-matched native unchanged-ID reload with independently committed proof and post-seal Turn |
| F2: integration ownership | Full structural ownership check, retained conflicted resources, strict installation identity, acquired-resource-only harness cleanup | Disposable development-channel native integration/CLI smoke |
| F3: minimized Return | Failed raw history examined; unchanged route code compared; focused guarded native runner and portable result-oracle controls prepared | Root cause and at least five successful exact current minimized Returns within the existing 2,000 ms budget |
| F4: latency evidence | Actual SQLite COMMIT bracket, applied-DOM marks, monotonic interval calculator, complete observer census protocol, discriminating loss/boundary controls | Independent hook census, native clock qualification, and a complete native sample for both sources |

Detailed records, assertions, limitations and reproduction commands are in
[`f1a/README.md`](f1a/README.md), [`f1b/README.md`](f1b/README.md),
[`f2/README.md`](f2/README.md), [`f3/README.md`](f3/README.md), and
[`f4/README.md`](f4/README.md). Each directory preserves failures as failures,
separately from its corrected executions. Test summaries identify consumed
source bytes rather than treating a PASS label as provenance.

## F1 ownership and durable state

The observer previously read the current mutable Session when a late callback
entered. It now retains original ownership through a bounded ledger keyed by
native work identity and observer context. Reuse across Sessions or generations
becomes ambiguous; saturation does not evict an old record and reuse its identity.
First-seen late work remains unknown. A callback that already entered under A
retains A while its asynchronous result settles after B begins. The original
event, result, exception, generator chunks and exactly-once `next(e)` behavior
remain covered by the actual observer tests and provider kit.

Reload host reads remain claims. The native helper samples its actual provider
parent and joins two fresh inventories to the exact Session, ProcessKey and
executable. Only a successful independent join creates a fresh token. The proof
is separately durably admitted or spooled before the token returns. The observer
seals it only in the unchanged immutable Session/epoch/generation. A qualified
Turn must start after that seal, and its outcome must follow that original
start. The reducer can receive these records in any permitted delivery order;
historical A/P attachment alone grants no authority. Missing, forged, stale,
wrong-process and pre-seal claims stay pending.

The changed representation and authority semantics require an explicit proposal:
**reducer 4, fact payload version 2, journal observation payload version 2**.
Existing reducer-3 development stores are not silently relabeled. Their retained
facts and owner decisions are recovered before authority is re-derived. Direct
native evidence and owner-command receipts survive; unsupported historical output
is auditable with `EVIDENCE_UNAVAILABLE`; unsent unsupported work is suppressed;
new catch-up work is held rather than submitted as fresh notification work.

Every newly admitted observation carries the versioned journal header, including
metadata-only rows with no facts. This records the actual transition boundary for
genesis replay and old-checkpoint recovery. A fact-only transition marker was
insufficient: zero-fact admissions could move the apparent boundary and alter
full-state revisions. That failure and the earlier Process/Execution revision
and native-only-prefix failures are preserved in `f1b/runs/` and
`f1b/independent-review/`. The repaired oracle checks complete state, projection,
materialized rows and checkpoint hashes; it does not erase meaningful differences
to manufacture convergence.

The genuine reducer-3 fixture databases were created by the actual rejected
source. The preserved reducer-1/2 fixtures and regressions remain separate. No
production migration occurred.

## F2 ownership and cleanup

Uninstall compares the recorded supported hook structure, including matcher,
type, command and extra options such as timeout. Modified entries remain in
settings and become conflicts. Partial removal retains the helper, mod and
installation metadata while references remain, reports `complete:false`, and
permits a later legitimate retry. It does not turn a partly removed configuration
into the original-byte restoration baseline.

Scope, absolute configuration path, owned root and agent identity must match the
installation record before mutation. Missing legacy identity refuses safely.
The native harness stores only an installation lease actually acquired by this
invocation and checks the record plus directory incarnation during cleanup.
Failed activation supplies no uninstall authority. Portable tests retain byte
and mode inventories across wrong-scope/configuration/record cases and the
failed-activation Drop path. Four targeted mutations fail the required assertions.

The disclosed historical `pkill -f 'sleep 40'` incident remains documented in
[`f2/harness-ownership-incident.md`](f2/harness-ownership-incident.md). The matching
PIDs and effects were not recorded, so this work does not assert that no unrelated
process was affected. The new focused cleanup paths require resource ownership;
the scope and limits of the static audit are explicit.

## F3 native routing remains blocking

The two retained M2 minimized positive cases ended at 2,007 ms and 2,003 ms with
TIMEOUT/UNAVAILABLE. Their records report a focus attempt and an unminimized
target, but independent selection still identifies the spare TTY. The conditional
`wrongTarget:false` field is not a successful Return and does not prove that no
focus side effect occurred. The earlier approximately 1,540 ms M0C success used
an older application/harness revision; intervening deadline work prevents using
it as a current positive witness.

The route product files remain byte-identical to accepted main and the rejected
M2 native build. No speculative route patch or timeout extension was made.
`threadspace-m0c m2-minimized dev 5` now prepares only acquired disposable Terminal
resources, verifies actual native Session/process/image/character-device identity,
reproduces the moved-window/minimized/alternate-foreground setup, retains the full
production route evidence even on failure, and performs separate independent
readbacks. Cleanup verifies the owned resource and current process incarnation.
An absent-Session negative requires no focus or unrelated-window changes.

This runner is **not executed natively**. The new positive count is **0 of the
required 5**. Its portable oracle tests validate assertions, not Terminal behavior.
The unchanged production telemetry groups tab selection, restoration, activation
and readback in a combined focus phase; it does not provide every fine AppleEvent
duration requested for root-cause analysis. Both that diagnostic gap and the
unknown root cause remain open. A new Mac campaign must retain refusals and total
wall time without resetting or excluding any part of the 2,000 ms route deadline.

The historical normal-route p95 of 895 ms still misses 750 ms. Its accepted M13
disposition is preserved separately; it does not waive this required positive
minimized scenario.

## F4 measurement and population truth

The old normal-path PASS is withdrawn. Receipt time preceded the SQLite
transaction, the timing used wall-clock differences, and 127 exact DOM matches
did not account for the full 225 observer observations. The retained data cannot
recover the missing post-COMMIT boundary after the fact.

The qualification path now records native timestamps around actual successful
SQLite COMMIT, joins each UUID/cursor to its batch projection, and records the
hydrated visible DOM application separately from receipt/store update. A SQLite
commit-hook witness and an isolated mutation moving the end timestamp before
COMMIT discriminate the boundary even when its label remains unchanged.

The calculator requires same-boot native clocks and explicitly qualified
cross-runtime rate/precision bounds. It uses conservative timestamp intervals,
coalesced-render coverage, exact source/runtime/store identity and a closed capture
population. Both hook and observer must separately satisfy all applicable
100/100/250 ms p95 thresholds. Missing telemetry and unmatched visible work are
failures, not silently excluded slow samples. A no-projection batch has an
explicit DOM exclusion and still counts toward capture-to-commit.

The observer census retains bounded source captures, pages, digest, close receipt
and confirmation so a surviving prefix cannot masquerade as a complete run.
Conventional hooks still lack an independent total-invocation witness when both
capture delivery and measurement fail. Saved helper files alone cannot establish
that missing final invocation. This source remains unsealed. Native clock bounds
also remain unqualified, and the new source-specific native sample counts are
zero. Native p50/p95/max are therefore **not established**; no latency PASS is
claimed for either source.

## Preserved acceptance and future boundaries

The following remain retained, with their original source/build attribution:

- The ten ordinary real-Claude vertical cycles and persistent worker history.
- Thirty dedicated exact Returns with zero wrong-target results in that
  population, for unchanged routing product code.
- Twenty durable Mark handled owner commands and canonical resolution.
- Supported Claude 2.1.295 declaration/compatibility evidence and unchanged
  pass-through/transport invariants.
- Genuine previous-version migration fixtures and accepted M1 journal authority.
- Original mod-batch qualification, separate from the normal native UI latency
  gate; added qualification instrumentation is not substituted for that evidence.

Changed ownership, integration and measurement paths require their focused
requalification. The old native reload restoration or integration cycles cannot
be relabeled as execution of the new implementation. There is no blanket repeat
of the ten-cycle or thirty-route campaigns solely for ceremony.

The unaccepted extra human-input record remains an inert M3 deduplication
obligation. M4 broader presentation, M5 notification delivery and retained
Terminal reliability issues, M6 Codex, M13 performance/stress, and M15 controlled
updates/full-Terminal-restart remain outside this repair. The private time-lapse
was unavailable; no pixels were independently verified.

## Execution, provenance and integrity

`ui/` records the desktop typecheck and tests. `build-checks/` records
source-bound macOS-target Rust typecheck/Clippy and the exact Linux limitations.
The F1B directory records the new seeded campaign, including 24,400 permutations,
the 4,000 evidence-set permutations and 2,440 stepwise SQLite cross-checks.
These are fresh executions of their recorded source, not borrowed baseline totals.
Use the final summaries in each directory for counts and any overlapping tests;
the original 513 Rust / 97 UI figures are not claimed as the new whole-workspace
execution.

| Final executed check | Result and scope |
| --- | --- |
| Core Rust/contracts/journal | 306 tests in 13 nonempty suites; includes contract exports and 36 ownership-suite tests |
| Retained scenario/previous-version migration regressions | 21 tests in 5 suites |
| Actual observer TypeScript | 14 cases; final captures separately rerun through the real adapter, SQLite and restart |
| Actual Claude 2.1.295 Linux plugin kit | 38/38; plugin validation exit 0 |
| Installer and acquired Scratch cleanup | 43 tests, all 12 focused cases, 4 targeted mutations rejected |
| Desktop | Typecheck and 104 tests in 9 files |
| Observer census | 16 Rust and 20 Node controls, 4 targeted mutations rejected |
| Minimized-route result oracle | 6 portable tests; no native repetitions |
| Latency calculator | 21 tests and 8 adversarial inputs rejected; actual SQLite pre-COMMIT mutation rejected |
| macOS-target Rust source checks | Provider, relay and harness check/Clippy pass with warnings denied; no native link or application build |

These populations overlap and are not added into a purported whole-workspace
total. The final F4 watchdog guard changed only observer qualification code and
regenerated the F1A capture fixture. Its consumer was rerun against the final
fixture; the original 306-test source record remains intact. All consumed Rust
sources and the independent synthetic campaign remained unchanged. The missing
watchdog counterexample, its exact repaired execution and its distinction from
native latency are retained under `f4/independent-review/`.

[`historical/index.json`](historical/index.json) binds exact copies of the old
manifest and README to the rejected Git objects. All 55 original evidence hashes
are checked against the preserved bytes. `check-integrity.py` checks prospective
published files, prohibited generated sidecars, unchanged contract documents and
credential-pattern findings; its method and limitations are retained in
`integrity.json`. No raw owner histories, private recording or provider token was
intentionally added. A static scan is not a guarantee about unseen owner data.

`provenance.json` and the parent manifest bind the committed source/harness revision
and committed evidence bytes. The final provenance-only commit changes no product
code, test assertions or harness behavior. Each execution record also retains its
own consumed source hashes so earlier attempts are not attributed to later code.
There is no current native build identity to infer from the old installation.

## Decisions and minimum next step

**D-0009: PROPOSED — pending independent re-review.**

**D-0010: PROPOSED — pending independent re-review.**

Finish only the remaining focused gates: source-matched Dev build/provenance;
real-Claude ownership/reload smoke; disposable native integration smoke; diagnosed
and repeatable minimized Return with at least five positives and relevant
negatives; independent hook population, native clock qualification and complete
capture/COMMIT/DOM samples for both sources. Use dedicated development resources
and preserve every failure. Do not modify the owner's production app/store or
Claude settings, and do not restart unrelated Terminal work.

Then submit the repaired candidate and exact evidence to independent GPT-6 Pro
M2 re-review. This publication does not authorize merge, M3 or deployment.
