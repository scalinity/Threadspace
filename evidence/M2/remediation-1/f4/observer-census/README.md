# F4 observer census implementation and portable qualification

Status: **portable software controls PASS; native F4 qualification remains INCOMPLETE**.

The observer producer, native metadata collector and native export path now implement an actual bounded source census. The final seal is derived from the retained source ledger, not from a count of successful sample files. No native Claude/macOS execution or clock qualification is asserted by these records.

## Source and protocol

The exact source hashes and command outputs are in `summary.json`. The exercised TypeScript files are the actual `packages/provider-mod/hooks/latency.ts`, `delivery.ts` and `register.ts`; the Rust wrapper copies `crates/relay/src/latency.rs`, `latency_census.rs` and `latency_census_tests.rs` without source changes. It imports the real contracts crate for the receipt/envelope types. The wrapper avoids importing unrelated Darwin process APIs; it does not simulate their execution.

The source retains every captured observation UUID in an independent ledger, including accepted, spooled, rejected, evicted, retried and never-drained observations. Acceptance does not delete a ledger entry. Stable UUID retries count once. The ledger is bounded at 4,096 entries, exports at most 128 entries per page, and explicitly rejects an overflowed population as incomplete.

Only an original engine/core Session-end result closes the source window. A plugin-origin Session end and `/clear` do not close it. The boundary is the capture window at Session end; it is not a claim that macOS has proved the complete provider process lifetime over. A pending delivery or a capture racing the closing stages prevents a clean close.

The qualification helper accepts a dedicated metadata-only `latency-census` protocol. Immutable pages are written with exclusive creation and file/directory synchronization. Close validates every page, ordered unique UUID, count and SHA-256 digest, then persists a fresh native challenge. The observer must receive that positive close receipt and echo the challenge. A fabricated subprocess reply alone cannot produce the independently retained matching native confirmation.

The native helper retains the actual echoed receipt first. It samples the native completion clock after that receipt is synchronized, then publishes a completion witness bound to the exact receipt bytes. A missing, changed or late echo/witness cannot establish a seal. These metadata receipts never admit observations or replace the existing canonical mod-batch ACK.

Real mod-batch helper receipts separately retain their submitted source runtime and UUID list, without provider payloads. The exporter verifies that each selected runtime has a complete closed ledger and confirmation, and that the actual submitted UUIDs fit that ledger. A lost final sample cannot make the surviving prefix appear complete.

## One unchanged end-drain ceiling

Normal-mode callback timing and canonical delivery are unchanged. Qualification mode establishes one 100 ms deadline **before** the existing end drain, and gives census only the unused part of that deadline. It does not reset or extend the ceiling. The original callback result is preserved. A deadline race releases the callback even if the census promise never settles; expired work cannot start a later close or confirmation.

The returned watchdog handle must expose a callable cancellation function before any census wait is allowed. Absent, null, missing/noncallable cancellation, a throwing cancellation getter or a thrown timer acquisition leaves census incomplete and preserves only the original drain. The actual register-level fault test retains the original 40 ms helper delay and possible 90 ms census-page delay, observes the callback at 110 ms, and verifies that no census was started.

The close also names the last independently retained helper clock token and only the observer budget remaining at the subprocess return. The native helper uses that retained point to bound the close and durable confirmation. It refuses missing/wrong-runtime clock tokens and checks the echo's actual post-sync completion against the cutoff. The source and native clocks still require the separate platform rate/precision qualification; this guard does not claim that qualification or subtract unrelated time origins.

## Actual executed controls

| Control | Result |
|---|---|
| Real relay metadata modules, pinned Rust 1.99.0, Linux disposable fixtures | 16 tests PASS |
| Real observer collector/delivery/register modules, Node 26.11.1 | 20 tests PASS |
| Rust Clippy, all targets of the portable wrapper, warnings denied | PASS |
| Mutant: accept a retained prefix without final challenge confirmation | Intended assertion FAIL |
| Mutant: omit the full-ledger digest check | Intended assertion FAIL |
| Mutant: delete accepted UUIDs from the capture ledger | Intended assertion FAIL |
| Mutant: trust a missing watchdog handle | Intended assertion FAIL |

The Node controls cover an exhausted initial budget, a pending finalization released by the deadline, immediate census completion and finalization failure. They also cover lost final samples/pages/close/confirmation, fabricated or malformed receipts, one export in flight, stable retries, bounded overflow and preservation of the original Session-end result. The Rust controls cover actual file persistence, immutable/idempotent retries, wrong store/runtime/boot, missing pages/count/digest, a native submitted UUID absent from the final ledger, historical telemetry failures, the 4,096-entry bound and late durable confirmation.

`portable-positive/` contains the actual metadata files read back by the Rust tests and their verifier results. `node-positive/node-lost-final-sample.json` retains the actual runtime collector reports from the original lost-tail counterexample: the earlier sample still says one capture/no export failures, but the final ledger reports both captures and the export failure. These are expressly portable/synthetic-clock fault fixtures, not native performance measurements.

Run the retained reproduction command with the pinned tools on PATH:

```text
PYTHONDONTWRITEBYTECODE=1 python3 evidence/M2/remediation-1/f4/run-observer-census.py \
  --target-dir /absolute/disposable/build-target \
  --node /absolute/path/to/node-26.11.1
```

The four mutations are applied only to acquired disposable source copies. Their exact patches, assertion failures, command lines and output hashes are retained. The source set remained unchanged during the final run. The earlier completed census run before adding the shared end-drain deadline is preserved under `../attempts/observer-census-before-end-budget/`. The later 16-Rust/18-Node campaign that missed invalid watchdog handles is preserved under `../attempts/observer-census-before-watchdog-validation/`, with the independent failing witness under `../independent-review/`. Neither older campaign qualifies the final source.

## Exact remaining F4 limits

**Conventional hook census is INCOMPLETE.** Each hook is a separate short-lived invocation. The conventional hook API supplies no independent shared invocation identity/counter to this adapter. If both its real delivery/spool path and its telemetry write fail, neither the surviving hook files nor the journal can establish that invocation's existence. Counting those files would manufacture a closed population. The exporter therefore emits an explicit unsealed hook status and no hook population seal. Closing this gap requires a qualified independent host-side invocation witness that includes zero-output failures; it cannot be established by another successful-file count.

**Native observer close is NOT EXECUTED here.** The protocol and bounded callback behavior are implemented and tested, but actual Claude/macOS Session-end scheduling, final helper execution and durable readback must still be qualified. A canceled, unfinished or late native close stays incomplete. No repetitive owner test or live configuration mutation was performed.

**Native clock/rate qualification and normal latency are INCOMPLETE.** The native exporter still emits `clockQualification.qualified = false`. It never supplies invented rate bounds or precision evidence. The independent calculator still requires both source populations, complete identities, matching store generation/runtime epochs and positive clock qualification. These new portable controls cannot produce or imply a native normal-path PASS.
