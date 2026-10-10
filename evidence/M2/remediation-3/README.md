# M2 bounded Terminal recovery and native closure attempt

**M2 NATIVE CLOSURE INCOMPLETE. Do not merge, deploy to production or start M3.**

## A. Exact refs and native build identity

This attempt started at published `bdb12078eafd38698ece9c876cd059d6aab624be`.
Accepted main remains `af9b285da529890bc441ea00f1a92e73e39902a8`. Product,
reducer, generated-contract, observer and route inputs still match the clean
signed Dev build `4aef4a96f0e21844b35271246a151629a77bceb2`; only unshipped
qualification sources/tests and evidence changed. The lockfile adds dependency
edges to the unshipped harness, without changing native product pins.

`final-native-identity.json` rechecks installed hashes and deep strict signing:

| Component | SHA-256 |
| --- | --- |
| Outer | `d849801d798576a67fedb763b6fe843297fef91a6e12f3064ade741d6becf8f6` |
| Companion | `43d59886d6a8b7cd5c04734b8e738e4883970cbbabe5cd812a8516b039d50f1b` |
| Helper | `554ce67cb42f9443cb23a2f7eae13a878c35502a04275e5230e1ef56e120c60f` |
| Final Rust harness, compiled from `9e13bad17c498282a02232d850db19b0ef01aa57` | `5b6a3111dca483cb5b281afc169336e8eafd6836a5c74719ee5f972b68087200` |

The original Dev store preservation remains intact; it was never reset. New
consistent backups and all complete legacy state remain private. The production
application/store, global launcher and owner Claude settings were untouched.
The running owned providers were the retained 2.1.295 executable (hash
`0116ee2e0a513900b633d9951367f18747686478e2b462805b8c31609f047f70`).
The companion's inventory launcher selects 2.1.296; independent qualification
witnesses use 2.1.295 and agree on Session/PID/birth/image/TTY. This is not a
claim that 2.1.296 provider callbacks have been qualified.

## B. Terminal scripting diagnosis

**TERMINAL SCRIPTABILITY RESTORED.** `terminal-environment.json` and `terminal/`
retain the direct and harness probes. macOS 27.2 (26B5091g), arm64, boot
`F2D2A2E9-7805-43B2-AC3A-85613557FD8E`; console UID 501, on-console and login
complete. WindowServer and trusted Accessibility were observed. Screen lock
state was not independently established. Terminal was initially absent from
NSWorkspace. The application query answered, and the following window query
implicitly launched a new Terminal incarnation. The resulting default window
35046 on `/dev/ttys002` was treated as unowned and preserved.

Terminal 2.15 then had regular activation policy, completed initialization and
PID 55965, birth 1791612993/586317. The source-bound signed/ad-hoc harness
preflight executed two serial six-query passes under that same incarnation:
application, count, IDs, front window, selected tab and selected TTY. All 12
answered in 30–113 ms; total 1,135 ms. Queries have 3-second AppleEvent,
4-second subprocess and 55-second overall limits. No timed-out subprocess was
left overlapping a subsequent query. Production Return remains 2,000 ms.

Read-only TCC database access was unavailable. Direct/harness AppleEvents
answered without authorization denial, and the actual companion reported
Terminal Automation authorized. No screencapture or QuickTime process was
observed; replayd existed without recent capture entries. This does not prove
the absence of every capture, or identify the cause of the previous hang.
No Terminal restart, broad kill, permission reset or capture termination ran.
The previous 615,244 ms readiness failure is preserved, not reclassified.

## C. F3 — PASS, focused native qualification pending review

Five independent positives ran under harness source
`3cd838aa73a78e73709d41325ed814047acf11d3`, harness hash
`a367917de55d791470932b78968e98e278c107fe1e55b5605f8486e0777b32d4`, and the
unchanged signed Dev product. Native Session
`03d3937d-c60c-4f6b-add4-853250cb84fc`, provider PID 63528, birth
1791613418/645529, exact 2.1.295 image/inode, controlling device 268435459,
window 35063 and `/dev/ttys003` were independently established. Every attempt
verified minimization, spare window foreground, exact selected target TTY,
restoration/raise, independent frontmost/readback and post-focus currentness.

| Attempt | Product total ms | Request/readback ms | Lookup/enumeration ms | Focus/readback ms | Post-focus currentness ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1221 | 1222 | 232 | 855 | 133 |
| 2 | 1213 | 1215 | 224 | 859 | 130 |
| 3 | 1206 | 1207 | 225 | 850 | 131 |
| 4 | 1208 | 1209 | 225 | 848 | 135 |
| 5 | 1260 | 1262 | 245 | 870 | 144 |

All exact-surface results were `CURRENT_NATIVE_REVALIDATED`, within the unchanged
2,000 ms deadline, with zero observed wrong targets. Frontmost/stat time was
0–1 ms; other short phases are retained per record. Separate selection,
unminimize and activation durations are not observable in this combined phase.
The old 2,007/2,003 ms failures did not reproduce; their cause is not inferred.

The original new run
`../remediation-1/f3/native/20261010T062222Z-dev-1231131f-b0e9-4dcb-bc05-b13ef9ec2f23/`
still reports aggregate failure: its absent-Session query parsed empty SQLite
output, and cleanup could not inspect root login wrappers. The repaired finish
runner retains those five positives at their original source. An intermediate
finish also preserves its unsuccessful title guard. The final finish at
`20261010T064635Z-dev-finish-ea91a2e6-c874-4553-a2f1-e4fc7e3cad44` uses source
`711d70453e782a8388adfe7fc1fe5f453e510f09` and passes `SESSION_NOT_FOUND`
without focus/selection mutation, plus verified acquired-resource cleanup.

Cleanup verifies original Terminal/provider identity, the sole acquired tab,
native TTY jobs and owned working directory. Root `/usr/bin/login` wrappers
are checked without borrowing PID signal authority. It closes only acquired
windows, checks successful empty TTY census and exact installation identity,
then removes that integration. It sends no direct PID signals. Provider-owned
dynamic titles do not confer cleanup authority. Draft/refusal/failure records
remain in `supplemental/`; no failed parent summary was rewritten as PASS.

## D. F1A — PASS, native smoke plus controlled ownership qualification

`observer-native/20261010T070719Z-dev/` ran source
`295cac9577a020e21ad2d477c21562bf5c9debb7`, harness
`8df66685781af0328b690bb51fd33fe05050918b36ade4ea4e9ae093bb955377`.
Its native Session was `9a666124-4134-4e09-82b5-616374c1a14d`, canonical Session
`bb84179e-3362-437a-acf1-e81bd28ac5a8`, PID 13284, birth 1791616130/927233,
exact 2.1.295 image and `/dev/ttys003`. Two bracketing inventories and native
kernel/device observations agree. The actual profile is `claude-cli:~/.claude`,
with a disposable settings/mod overlay and an explicit empty strict MCP scope;
the provider's normal credentials/Session metadata are not a separately isolated
home. No existing provider Session was reset. A real tool/answer Turn completed in 11,874 ms.
Original Turn `07bf39db-d6c5-41f4-af14-34d697bc8a4b` retains epoch
`293cfc6a-831c-4779-8962-0b1efac4f8fb`, generation 1 and engine/core origin.
Completion UUID `027f270a-ab14-464a-96b2-3251658ad7d2` has entry/result sequences
18/19, genuine core settlement and a canonical COMPLETED outcome for that Session.

`controlled-host/` separately labels 14 actual TypeScript host-fixture cases
executed on macOS: delayed entry/return, A→B→A, reused Turn/actor IDs, unknown
late outcome, delayed subordinate ownership, fake origins and fail-open values,
exceptions/generators/cancellation with `next(e)` exactly once. Their captures
pass through production adapters, SQLite, pure reduction and restart in the
36-test native Rust execution. These difficult schedules are controlled host
fixtures, not claims that a real inference performed every Session transition.
No authoritative wrong-Session outcome occurred in these witness populations.

## E. F1B — PASS, focused proof/seal/Turn and replay qualification

The unchanged-ID reload observed LOWER_TIER in 845 ms, then admitted a separate
native `ownership.proven` at cursor 5954, UUID
`e67c6430-cc6f-4ff4-b76b-c845edb9a16b`, token
`97b26edf-f2db-44f0-9d85-aa6da433b194`. The native producer's process/image and
inventory bracket (1791616156542–1791616156896 ms) agree with the independent
harness join. The observer sealed it at cursor 5956 under new epoch
`e642a93b-22bb-4185-802e-ea310eaaf2bf`, generation 1. Post-seal Turn
`65c36ae7-fe50-42cf-9c89-76891519363f` started at 5961 and completed at 5966,
UUID `9a34f99f-2696-42db-97b6-cbe58bb4a3ed`; the view then read RESTORED and
COMPLETED. Its actual post-seal Turn took 3,823 ms. No RESTORED-before-Turn wait
from the legacy reload harness was reused.

Controlled production-adapter/SQLite negatives cover stale A/P current B/P,
wrong incarnation/image/epoch/generation/token, intercepted token without proof,
pre-seal/old-generation Turns and A→B→A. All 24 proof/seal/start/outcome delivery
orders converge, with stable UUID retries, checkpoint/restart and full genesis
hash checks. Four additional Mac-compiled native producer tests use controlled
kernel/inventory readers to discriminate old Session claims, wrong birth/image,
an inventory switch and a switch between qualified executables. Real parent IO
is established separately by the native smoke. Genuine reducer-3 migration
fixtures retain their complete oracle.

`native-isolated-replay/` re-admits 19 unchanged envelopes from the real native
run through the actual observer/proof adapters to a newly acquired disposable
journal. Full state, stepwise SQLite, genesis, projection and checkpoint/restart
agree; both original native Turns complete and authority is RESTORED. State hash:
`f8eda726d5b00645ac426d520cae9b1e9b1c61a850714d755e2f4ca33dd8f6ab`.
This is replay of captured native evidence, not a repeated inference.

Separately, a consistent copy of the long-lived Dev store fails empty-genesis
full-state comparison. Recovery, tables and checkpoint/restart agree. Its first
canonical cursor is 3532; 630 records absent from empty canonical genesis belong
to earlier history. All new owned Session categories and its provider process
agree exactly, but the full hash still fails. Complete states and 697 differences
remain private, with public hashes/category counts. That diagnostic is not a
passing full-store check, no oracle was weakened, and no accepted M1 baseline
behavior was changed. The isolated complete native dataset supplies F1 replay.

## F. F4 — INCOMPLETE

`latency-fixture/20261010T073144Z-dev/` owns its fresh hydrated Dev UI, disposable
Terminal/Claude 2.1.295 Session and acquired integration. Measurement is enabled
only in its staged observer manifest. The original manifest/settings are restored
before matched cleanup. The UI is stopped only by its acquired incarnation.
Raw collection is under `../remediation-1/f4/native/capture/20261010T073149Z-dev/`,
source `6b6cfceecfa78fd0be3465d86ac525ae3c32dec4`, harness
`0c86f712683727ea3bdfe756270fd19a248a13a8819ab448f7e496cc21860024`.

The deterministic hook fixture records 20 expected issuances before launching
the installed helper, all child exits, and joins each PID/time/Session to its
canonical UUID. No census count comes from surviving helper files. Ordinary
provider hooks are disabled only in this disposable measurement configuration;
their real native path was exercised in the preceding F1 smoke. This is a
controlled conventional-hook workload, not 20 Claude-inference hook firings.
The independent double-loss control issues four calls; one valid call uses an
owned impossible store and loses both normal delivery/spool and local telemetry.
All four remain issued, three join, and one is explicitly missing. No provider
is blocked for this control and ordinary fail-open behavior is unchanged.

The real observer closes its original engine/core Session and durably confirms
its independent 21-UUID census. Twenty hook candidates and 21 observer candidates
all have COMMITTED records, projection-changing patch cursors and matching
hydrated visible applied-DOM marks (33 marks, including coalesced coverage).
Both populations have zero retries/duplicates, rejects, no-projection records,
unmatched DOM records, missing telemetry and valid exclusions in this run.
`core-runtime-proof.json` separately joins each of the 41 records to CORE_READY,
stable PID/birth/boot and store/core generation; no intervening core restart occurs.

The original raw/exporter/calculator results remain intact. A separately derived
`raw-with-hook-census.json` attaches only the independently verified hook seal;
its calculator still returns INCOMPLETE_OR_FAIL. Clock qualification remains
false. Native diagnostics verify CLOCK_UPTIME_RAW against converted Mach absolute
brackets in 10,000 reads, timebase 125/3, reported resolution 42 ns and 1 ns
conversion rounding. The same UI runtime brackets span 35,924 ms. Their empirical
constant-rate enclosure is about −954 to +784 ppm; it is conditional, not a
guaranteed maximum rate error. Installed provider/WKWebView rate, precision and
rounding bounds across the complete calibration span remain unqualified. No
`qualified:true` was inserted. Qualified end-to-end sample count remains **zero**.

The following calculator output is **unqualified diagnostic output**, using
its zero-rate/zero-precision fallback. It is not a native latency PASS and cannot
be submitted as final conservative upper bounds:

| Source / metric | n | p50 ms | p95 ms | maximum ms |
| --- | ---: | ---: | ---: | ---: |
| Hook capture→COMMIT | 20 | 0.825 | 1.774 | 4.803 |
| Hook COMMIT→DOM | 20 | 22.671 | 26.354 | 28.954 |
| Hook capture→DOM | 20 | 23.604 | 26.852 | 29.385 |
| Observer capture→COMMIT | 21 | 28.042 | 216.025 | 396.571 |
| Observer COMMIT→DOM | 21 | 24.342 | 27.354 | 29.077 |
| Observer capture→DOM | 21 | 50.824 | 239.529 | 425.188 |

Even this optimistic observer capture→COMMIT p95 exceeds 100 ms; its broad
calibration/batch intervals require diagnosis with qualified clocks before a
performance conclusion or product repair. The 100/100/250 ms thresholds remain
unchanged. The actual SQLite COMMIT-hook control passes; pre-COMMIT substitution
and incomplete census/clock/DOM controls remain discriminating. Old invalid
2/8/9 and 36/8/41 results are not reinstated.

## G. F2 and historical evidence preserved

F2 remains PASS pending independent review: 13 native cases, 31 Dev CLI calls,
nine additional refusals, 42 installer tests and one failed-acquisition Scratch
test at their recorded source. Product integration/cleanup source did not change.
New cleanup changes belong to unshipped minimized/observer fixture controllers.
The ten-cycle vertical slice, thirty ordinary Returns, twenty Mark handled
actions, transport, mod-batch and migration campaigns remain historical within
their original product-source boundaries. They are not new executions here.

## H. Verification, privacy and remaining obligations

Fresh checks: native harness builds/checks, workspace check, workspace Clippy
with warnings denied, 14 TypeScript controlled cases, 36 Rust ownership/adapter/
migration controls, one complete isolated native-envelope replay, 17 COMMIT/census
fixtures, four native producer controls, one real SQLite COMMIT-hook control and 21 calculator controls. The
initial workspace Clippy unused public re-export failure is preserved; the
included test-only helper module now allows its intentionally unused re-exports.
A final harness-only repair keeps explicit acquired cleanup running after owned
file restoration errors, records those errors, and requires `complete:true`
before reporting integration removal complete. Its partial-uninstall negative
control, focused harness Clippy and final source-matched build pass. This changes
qualification failure reporting/cleanup, not the product or recorded native
positive paths; the earlier native runs retain their exact source identities.
Source-built qualification code stays unshipped. UI tests, generated schema
export/freshness, provider kit and accepted milestone campaigns were not repeated;
their relevant product/generated inputs are unchanged, and no new PASS is claimed.

`publication-record.json` hashes raw originals copied outside Git before literal
home/owned-temp redaction and removal of unrelated global PID rows from published
census stdout. Native UUIDs, process births, source epochs, timings and executable
hashes remain. Original hash references in curated records identify those private
originals; public file hashes are separate. No private database, broad environment
inventory, provider history, secret, runtime state or pixel capture is published.

The exact unfinished M2 obligation is positive cross-runtime clock qualification
and subsequent conservative source-specific latency calculation/diagnosis. No
further native retry is justified by unchanged instrumentation. M3 deduplication,
M5 notification qualification, M13 ordinary-route p95 and M15 production exclusion,
update/full-Terminal-restart obligations retain their existing dispositions.
No ORCHESTRATION.html exists. D-0009 and D-0010 remain PROPOSED.

## I. Decision

| Gate | Result |
| --- | --- |
| Terminal prerequisite | RESTORED |
| F1A | PASS, focused native smoke plus controlled schedules, pending independent review |
| F1B | PASS, focused native proof/seal/post-seal Turn plus complete isolated replay and controlled negatives, pending independent review |
| F2 | PASS retained, pending independent review |
| F3 | PASS, five positives plus absent-Session refusal and acquired cleanup, pending independent review |
| F4 | INCOMPLETE |

M2 is not self-accepted. Stop at this incomplete closure candidate; no merge,
production deployment or M3. Focused GPT-6 Pro acceptance re-review requires
closure of the remaining F4 gate first.
