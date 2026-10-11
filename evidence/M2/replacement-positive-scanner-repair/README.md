# Positive-module scanner repair

**F4 POSITIVE-MODULE SCANNER REPAIR ACCEPTANCE CANDIDATE — NATIVE ACTIVATION UNVERIFIED**

The exact installed Claude Code 2.1.295 non-executing scanner admitted the corrected positive timing package and, separately, the unchanged activation-only package. One positive-package validation and one activation-package validation ran; both returned exit 0, `success: true`, no scanner errors, and complete owned process reaping. No callback was evaluated or dispatched. Native execution ended after those two scans.

## Source correction and identities

Starting `m2`: `445794e778e27ed071f8e053aadce8b401c64ba4`; accepted `main`: `af9b285da529890bc441ea00f1a92e73e39902a8`. The commit containing this record is the publication candidate. The target remains `/Users/danny/.local/share/claude/versions/2.1.295`, SHA-256 `0116ee2e0a513900b633d9951367f18747686478e2b462805b8c31609f047f70`. Its version binding is the previously independently verified executable identity; no extra `--version` command was run in this scanner-only unit. Each actual scan independently verified the kernel image, UID, parent and PID-birth identity for these exact bytes.

`original-positive-probe.ts` preserves the scanner-refused source, SHA-256 `bfe2cbba9b1bdfa3c307f3044b262aee76aa155cee54f584c91ba3ae4af0b73c`. `original-scanner-refusal.json` is a byte-identical copy of the previous primary refusal. That refusal reports `startBackground($)` passing `$` across an import from `./background.ts`; the installed scanner follows `$` only into supported functions declared in the same file.

The corrected positive source, `tests/native/tools/m2_socket_probe.ts`, has SHA-256 `0e5f94b669ec2936323a9056a545886ee60333d110db2a63cbacf68772d41cfa`. It removes the background import and declares the same routine locally. The routine body is byte-identical to the frozen reference after removing only its export keyword. The complete `export const register` suffix is byte-identical to the failed positive module. This preserves forwarding, four-marker protocol, response checks, row ordering, clock readings, start/completion placement, asynchronous behavior and Unix-socket transport. The actual scanner notes now admit `session.start` and `$.clock.after`, `$.fs.write`, `$.http.fetch`.

Canonical observer aggregate remains `d461d6d6cb8769dbe65d3e229212f052f5ba0c45a936f92ba76d18b18e88214b`. The activation module remains `ea54a1cbe9253388f6e4a0e2575ee31470746e0c236e2d99ad4e5fc56c273136`. Frozen reference fixture remains `9379a83f4f6f009d989856697379f541596c8dfa451cf4885327f5310b9feb58`. The original fixture file was not edited or removed, and remains staged as an unimported historical reference. The observer, activation controller, stager, evaluator, calculator, policy and timing runner are unchanged.

## Background fidelity and portable controls

`m2_positive_scanner_controls.js` executes the actual reference and local implementation with a deterministic manual scheduler. Only fixture imports, export keywords, explicit TypeScript annotations and the non-null assertion are removed for the JavaScript test runtime. The scheduler performs no real timer wait, native clock reading or HTTP exchange.

Both routines return 256 ticks, 524,288 operations, checksum **1722810369**, and `complete: true`. They request the same 256 intervals of 5 ms; the synthetic scheduler's completion position is 1,280 virtual units. Requested intervals and that synthetic position are not actual elapsed-time measurements. Each tick retains exactly 2,048 operations and `x = ((x ^ i) * 1664525 + 1013904223) >>> 0`.

Actual original and corrected callbacks also run under synthetic host APIs with two fixture rows per condition, not a native conformance grid. Their results match: one `next(e)`, the same original promise, 258 readings per synthetic row, and marker sequences `captureLower`, `captureUpper`, `endLower`, `endUpper`. Background scheduling precedes the first collector reading; final publication waits for the complete background result. A foreign response is rejected by the actual callback's validation.

Nine targeted mutations are demonstrably rejected: tick count, operations per tick, arithmetic, requested schedule, operation accounting, completion accounting, start boundary, completion-await boundary, and duplicate forwarding. These are executable source controls, not only textual comparisons. `background-controls.json` records the source identities, deterministic output and results.

Six focused Python tests pass. Their guards prohibit every subprocess creation attempt. They check original/corrected callback bytes, same-file routine identity, canonical observer copies, distinct plugin roots/names/modules, one loading argument per plugin, strict empty MCP, inherited loading removal, source/config substitution rejection, frozen input roundtrip, one-scan attempt guards, updated source identities, and continued rejection of the consumed 0/14 conformance even with matching relabeled metadata. Thirty existing portable staging/evaluator/calculator tests also pass. No native protocol test class was selected. The separate JavaScript control uses the Node test interpreter only; no provider, responder or application executable is launched by these portable controls.

Two development failures are retained honestly. The initial JavaScript harness drained insufficient cross-context promise continuations; its failed output is preserved, and the bounded manual scheduler was corrected without changing the candidate. Next, a pre-launch verifier refused a scanner config whose JSON key order changed after freeze/readback. No provider process or scan-start marker existed. The scan-only controller now canonicalizes config order before staging; its roundtrip control passes. `preflight-input-failure.json` labels the recorded reproduction and retired invalid input freeze. Neither failure is relabeled as a native refusal or a consumed validation attempt. There was only one timing-module source candidate throughout.

## Frozen non-executing scans

The final `scanner-input-freeze.json` was exclusively written, fsynced, independently read back and verified before provider execution. Canonical digest: `d340511436bbaa69fbb3499a10eb5329c6e932826e5a2715cbeca3ac25a0664b`. It binds source dependencies, effective local background identity, canonical observer, candidate/activation files, package manifests, private settings and the unchanged 20-second external deadline. Its `plan` and `delays` are empty and `runtimeId` is explicitly `not-a-runtime`. It is a scanner-input record, never a positive conformance or activation manifest. No new conformance UUID, epoch, challenge, readiness nonce or timestamp responder was created.

The corrected positive package uses the reviewed `threadspace-socket-duration` manifest and its independent `duration-plugin` root. A byte-identical observer is staged in a distinct `observer-plugin` root with disabled capture delivery. The activation package is staged separately with its unchanged entry and explicit activation-only description. None of those staging facts establishes runtime discovery. The canonical observer's historical native scanner admission remains at its unchanged identity; it was not repeated.

The supported command is `plugin validate <path> --json`, with private settings, empty owner setting sources, strict empty MCP and no session persistence. The installed-interface proof remains at `evidence/M2/replacement-activation-native/installed-interface.json`: CLI validation and runtime loading share the registration/module scanner, while this validator does not evaluate register or callbacks.

| Exact package | Actual result | Scanner registration/call notes |
|---|---|---|
| Corrected positive | Exit 0; success true; no errors | `session.start`; `$.clock.after`, `$.fs.write`, `$.http.fetch` |
| Unchanged activation-only | Exit 0; success true; no errors | `session.start`; `$.fs.write`, `$.http.fetch` |

Original manifest warnings are retained: missing description/author for the positive manifest and missing author for activation. The existing manifests were not changed to suppress them; no alternate strictness or scanner configuration was tried. Full primary stdout, empty stderr, parsed results, exact argv, staged hashes and independent process identities are retained. These individual-package results establish scanner admission only.

The deadline begins before process creation and covers identity sampling and the remaining wait. Both invocations completed before expiry and were reaped normally. Cleanup records contain only these two independently verified children. No unrelated process was signaled, no socket service started, and no owner settings, launcher or session changed.

## Identity binding and preserved qualification state

The existing stager and evaluator obtain the corrected probe hash from actual source bytes. Their complete composition now binds the new effective background through `hooks/register.ts`, while `backgroundSha256` still binds the frozen reference. The new record separately identifies the local declaration and all affected controller/test sources. Portable controls prove that substituting the old probe hash or altered source/configuration fails the actual joins. No check or identity field was removed, and no passing conformance/profile was created.

The old activation controller's `prepare` entry intentionally remains pinned to the earlier admitted source record and therefore refuses this changed positive hash. That guard is not bypassed. After independent source review and separate activation authorization, its reviewed-source pin must be reconciled with the accepted candidate before creating a fresh activation freeze. Retired activation freezes/nonces remain unusable. This is documented in the conditional handoff, not silently repaired or run in this scanner unit.

All 1,183 protected baseline files retain their hashes, including historical M2 evidence, canonical observer bytes, SPEC/MILESTONES, activation sources and frozen background. The original consumed conformance remains failed at 0/14 with no native requests and its retired identities. Historical F4 measurements remain unqualified. Accepted M0C/M1, F1A/F1B/F2/F3 candidate evidence, the reviewed legacy Dev checkpoint-plus-suffix result and proposed D-0009/D-0010 status remain unchanged. The original capture wall budget, timing policy and cold-helper gate remain intact.

Joint runtime discovery, actual callbacks, observer attribution, native forwarding, callback persistence, installed-mod HTTP readiness and timing qualification are **UNVERIFIED**. No positive activation, inference, timing grid, calibration, conformance, app build/install, compact-helper experiment, merge or M3 occurred. Full F4 and M2 acceptance remain gated.

## Publication and next boundary

`manifest.json` binds the public evidence and executed dependencies. `privacy-scan.json` classifies functional owned paths, fresh scanner process identities and synthetic fixture values. No owner prompts/history, credentials, complete vendor binary excerpts or unrelated process data are published. Private originals remain outside Git. No orchestration overview exists to realign.

Request independent source review of this acceptance candidate and separate authorization for the existing activation-only smoke. `activation-only-handoff.md` records that bounded next gate; it grants no execution authority. Any later timing conformance needs another explicit authorization, fresh identities and completed native activation evidence. This unit stops after scanner-result publication without merging.
