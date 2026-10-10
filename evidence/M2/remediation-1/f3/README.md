# F3 — Minimized-window Return: retained failure and closure status

**M2 blocking requirement remains open. No native execution occurred in this remediation environment. New successful repetitions: 0 of at least 5 required. The route deadline remains 2,000 ms.**

This is a source and retained-evidence review plus focused qualification-harness preparation from Linux. The new runner is **UNEXECUTED on macOS**. This work does not qualify a repaired native route, change route product code, interact with owner Terminal windows, or operate any owner application or store. The root cause is **not established** by the committed evidence.

## What the native records actually show

The original JSONL records are preserved in their existing directories. `primary-records.json` retains their complete minimized-case rows, same-run baseline rows, Session metadata, idle gates, summaries and source-file hashes.

| Retained run | Route / harness time | Result | Independent selected TTY | Target after Return |
|---|---:|---|---|---|
| M0C `20261006T144151Z-prod` | 1,540 / 1,543 ms | `EXACT_NATIVE_SURFACE`, `CURRENT_NATIVE_REVALIDATED`, `FOREGROUND_COMPATIBLE`, `OK` | `/dev/ttys002`, matching target | Not miniaturized |
| M2 `20261009T042955Z-dev` | 2,007 / 2,009 ms | `UNAVAILABLE`, `NATIVE_BOUND_LAST_KNOWN`, `UNKNOWN`, `TIMEOUT` | `/dev/ttys011`, the harness spare; target was `/dev/ttys001` | Not miniaturized |
| M2 `20261009T044059Z-dev` | 2,003 / 2,004 ms | `UNAVAILABLE`, `NATIVE_BOUND_LAST_KNOWN`, `UNKNOWN`, `TIMEOUT` | `/dev/ttys011`, the harness spare; target was `/dev/ttys001` | Not miniaturized |

Every row records `minimizedByWindowId: true`. The harness's `SET_MINIATURIZED` script sets the recorded window's property and reads it back; the harness then waits one second before Return. The M2 rows contain a timed-out `osascript` focus error, `focusPerformed: true`, and no successful internal selected-tab/window readback. The independent readback occurs after the returned route and a 300 ms pause. Terminal is frontmost at that later readback in both failures.

The recorded `wrongTarget: false` is narrower than proof that no focus side effect occurred. The old harness sets `wrongTarget` only when the companion claims an exact success and an independent selected TTY differs. Neither failed route claimed exact success. The source explicitly records a potentially performed focus when a script errors or times out. The observed target restoration and spare selected TTY must remain visible in the evidence; an honest timeout does not satisfy the positive Return requirement.

The first failed run's target was native Session `4a770162-3809-4f93-965b-19c05223403d`, canonical Session `7d8a60a1-86d6-4d69-af35-327ecea4962c`, PID 61357, window 27819. Its request was `5ae13e5b-49f3-4be7-be7f-ce3e43cb169f`. The second target was native Session `53cf39c4-3b94-4380-8f29-9e7a0eaf70f4`, canonical Session `6e925fa3-f014-489b-b3f2-dd94925bbabe`, PID 81814, window 27889. Its request was `a9243bdf-a290-431d-905d-4a910e1902f4`. Each target used `/dev/ttys001`; each same-run baseline succeeded and recorded device number 268435457. Those baseline samples are not fresh ProcessKey/device proof for the later minimized attempt.

The two enclosing fault runs (`20261009T041720Z-dev`, case line 14, and `20261009T043718Z-dev`, case line 4) explicitly reference these G08 directories and retain `terminal-negatives: false`. The later run does not close minimized Return. Full Terminal restart is a separate retained `BLOCKED` M15 limitation under D-0006.

## Exact historical source identities

The M2 application is the recorded development bundle `~/Applications/Threadspace Dev.app`, bundle ID `ai.scalinity.threadspace.dev`, built from `f4ef5f156f7086962bafa92728cc4cace5a58c3c`. `evidence/M2/native-build.json` records a clean source tree, installed/build equality, and executable hashes:

- Outer executable: `f8bd554b01c0611334100316b050dec287a627ac93fa005312c679e96b93c991`.
- Companion: `3b54b8369ecae00b6d53938c790fae69e44dc44bf0d08ad7295a208bbb18dfcc`.
- Helper: `b714aca95e4ac7b262997bec5e4006ef0c4b556a98e090f3cd6d660a0f619c72`.

The recorded first and second harness revisions are respectively `40869f36fda2ca6d504470000bb2d0c4b3e0e72a` and `66e53f0f190bdba08b16325919ad52072a1c4d8b`. The G08 runner and Terminal harness helper are byte-identical between those revisions. The evidence was committed by the rejected M2 candidate `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`.

The inherited M0C positive belongs to application `8c82212b7ac84abfb88ef2b2239f68de9aed49eb` and harness `89d857267ba0fe5ffde7cbad51db5b4123189f98`, as the original manifest at `0c24b4fc785309e5a1d788bfcefdff02e05017cd` records. Its production companion hash is `12d397871d32f7043e623fa378cf5c523c46505feb5aa325231193e4f3acab0d`. The later final M0C manifest's application identity must not be retroactively assigned to that original G08 run.

`source-equivalence.json` records SHA256 equality for eight concrete route implementation files across accepted main `af9b285da529890bc441ea00f1a92e73e39902a8`, the qualified M2 build, rejected M2 candidate, and this remediation worktree. These include the companion route operation, pure route model, native Terminal adapter, fixed focus/inventory scripts, process/TTY primitives and bounded execution worker. None was edited by this F3 task.

That equality does **not** extend back to the original M0C G08 build: the accepted M0C C-11 work subsequently changed the overall deadline handling and focus settling. Those accepted changes are preserved. M2 also added wait interpretation and canonical discovery bookkeeping in shared inventory/discovery files; their route-used CLI execution, Terminal-generation and authorization functions did not change. Source equality since accepted main does not establish why the M2 native attempts failed.

## What can and cannot be diagnosed

The committed per-attempt rows establish that the focus subprocess timed out and exact readback did not complete. They do not identify the AppleEvent or suboperation that consumed the time. In particular:

- The stored `focus.elapsedMs: 0` is the failure structure's initial value in `crates/surfaces/src/lib.rs`, not a measured zero-duration focus.
- The router constructs phase timings and before/after ProcessKey, inventory and surface evidence. The original G08 serialization retained only `result.evidence.focus`, so the full phase and proof arrays are absent from these committed minimized-case rows.
- The README/manifest at rejected candidate `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`, preserved exactly at `evidence/M2/remediation-1/historical/rejected-candidate-README.md` and `rejected-candidate-manifest.json`, report lookup 394 ms, combined focus/readback 1,609 ms, and standalone restoration 547–926 ms. This review cannot independently reconstruct those subphase measurements from the committed primary G08 records. They remain attributed diagnostic claims, not a proven root cause. The current top-level M2 status documents are not the source of those historical claims.
- The fixed focus script rechecks unique TTY, unminimizes, selects the tab, raises the window, activates Terminal and polls front-window order before final readback. Its current order is established by source. The records do not show which step stalled or whether changing that order would repair the failure.
- No specific Terminal `-1728`/`-1708` element refusal is present in these failed focus records. The broader documented refusal behavior does not diagnose this particular timeout. There is no committed per-AppleEvent duration or contemporaneous refusal witness for either attempt.

The evidence therefore does not justify choosing between restoration latency, another environmental condition, focus-script ordering, serial native work, provider lookup/currentness, or harness synchronization as the cause. No speculative product patch is supplied.

## Prepared focused runner — UNEXECUTED

`tests/native/harness/src/bin/m0c/m2_minimized.rs` adds the development-only command below. It refuses a missing/production channel and counts outside 5–100 before context initialization. This command is prepared for a controlled native campaign; **it was not executed in this Linux remediation environment**.

```sh
cargo +1.99.0 run --locked -p threadspace-harness --bin threadspace-m0c -- m2-minimized dev 5
```

Each invocation gets a UUID-qualified evidence directory under `evidence/M2/remediation-1/f3/native/`. It installs only an acquired session-scope integration in its own disposable directory, independently launches a real Claude 2.1.295 process in an ordinary owned Terminal window, and opens an owned inert spare. It preserves the inherited G08 moved bounds `(120, 140, 900, 640)` and one-second setup settle. Every attempted positive independently establishes the target's minimized state and the spare's selected tab and foreground window before sending an explicit companion `ReturnToSession` request. The runner requests no model Turn.

Before and after Return, production `discover`, `KernelSampler` and `character_device` independently corroborate the native Session, ProcessKey, kernel executable image/version and controlling character device. The canonical endpoint comes from the Dev journal in read-only mode. The original Session/process/image/device must remain identical. Cwd is used only to prove ownership of the disposable fixture and never to choose the native Session or route target.

The runner uses the existing `deadline::route_full` helper and retains its **complete `RouteResult` / `RouteEvidence`**, including lookup, enumeration, sample, focus, binding-revision, post-focus currentness and phase objects on failures. Missing fields remain missing. Independent selected-tab/front-window/application and minimized-state readbacks occur after the returned product result and are timed separately. They cannot convert `TIMEOUT`, `UNAVAILABLE`, a late result, an unqualified Session or a wrong target into a positive. The original product deadline remains 2,000 ms. This focused runner additionally requires request-through-receipt elapsed time to be at most 2,000 ms; it does not exclude slow receipt work to obtain a PASS.

Every attempt retains selected tabs before and after, target restoration state, reported focus-changing AppleEvents and changes to unrelated windows. It reports unknown readbacks separately. It does not derive “no side effects” from the older conditional `wrongTarget: false` field. A safe conservative negative requests a newly generated canonical Session UUID only after a read-only journal query proves it absent. That case needs `SESSION_NOT_FOUND`, no reported focus event and independently unchanged application, front window, selections and minimized state. The broader existing negative command is `THREADSPACE_EVIDENCE_MILESTONE=M2 threadspace-m0c g08-terminal dev`; it is not automatically invoked by this focused runner. Full Terminal restart remains outside this runner.

Setup and cleanup hold the existing shared GUI lock and check the recorded directory incarnation, Terminal incarnation, sole owned tab and TTY. Cleanup uses no pattern-based signals or legacy broad close. Before each scoped `SIGHUP`, it verifies kernel birth, executable image, controlling device and owned cwd and re-samples the same incarnation at the signal boundary. It sends a window-close event only when the recorded sole tab is empty. Its cleanup PID inventory is typed: a failed/nonzero `ps` read is retained as unknown, never interpreted as an empty TTY. An unknown/replaced resource, uncertain open receipt, changed process or failed cleanup retains the scratch integration acquisition; it never broadens cleanup to another window, process or installation.

The unchanged product currently records tab selection, restoration, window raise, activation and focus readback as one combined focus phase. The new runner preserves that limitation explicitly. It supplies no fabricated separate AppleEvent timings and no inferred root cause. A native investigator still needs the focused phase/refusal evidence required below if the combined phase is insufficient to explain a reproduced failure. Even a future runner PASS is a focused execution result pending independent acceptance, not an automatic M2 acceptance update.

### Portable validation of the new assertions

`m2_minimized_oracle.rs` has six portable classification tests: a complete synthetic witness, missing-proof/unchanged-deadline controls, absent evidence, the recorded timeout/restoration pattern, conservative-negative side effects, and unrelated window changes. These tests exercise the actual oracle source and **do not execute or qualify native routing**. `portable/oracle-tests.log` and `portable/oracle-execution.json` retain the actual output and source hash. The retained initial macro-expansion compilation error was corrected by splitting a large synthetic JSON fixture. The first executable run also caught a diagnostic presence bug: absent expected/selected TTY values compared equal. The repaired oracle requires explicit identity/window presence; `portable/oracle-null-readback-failure.log`, its source hash and the prior oracle source preserve that failure. Neither correction changed route product code or converted a native failure to PASS.

Root's combined Clippy run also identified a test-only `unwrap()` prohibited by the workspace lint. It was replaced with an explanatory `expect()` without changing assertion semantics; the same six portable oracle tests then passed again. The parent remediation validation preserves the failed combined Clippy output.

The macOS-target harness compilation is a source/type check from Linux, not native execution. Root's final combined check remains authoritative for the final remediation source after the other F1/F4 changes finish.

## Minimum focused closure still required

SPEC §§13.2–13.3 and the inherited G08 positive requirement remain applicable. The owner's F3 instruction expressly requires at least **five independently verified successful minimized-window repetitions**, including the conditions that previously failed, plus relevant conservative negative cases.

The focused campaign must use a harness-owned real Terminal window and independently launched real Claude session, retain the exact current Session/ProcessKey/executable and controlling character-device proof, and independently read minimized state and the other foreground window immediately before explicit Return. Retain the complete route evidence and bounded timings for lookup, enumeration, selection/restoration, raise/activation, readback and post-focus currentness. Each positive needs the same target restored, exact independently selected TTY, Terminal frontmost and matching current provider Session, all within the unchanged 2,000 ms attempt deadline and with zero wrong-target outcomes.

The deadline starts at companion receipt, includes queueing and all native route phases, cannot be reset or increased, and cannot be replaced with optimistic focus or a safe-refusal PASS. Return must not type, resume a provider, submit an approval or choose another tab. Record environmental refusal and failed attempts explicitly. If route product code changes, rebuild and qualify the development app with exact source/build hashes.

**Five current positive repetitions remain missing.** The inherited M0C positive is preserved but does not count as a new remediation repetition. The normal-route p95 miss (895 ms versus 750 ms) remains the separately accepted M13 performance obligation. It does not defer this M2 positive-case failure.
