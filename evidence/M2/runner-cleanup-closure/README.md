# Timing runner identity-write cleanup repair

Source-only correction after candidate `ecc4a4934f6cbec0cd1798baed369e4667cf9dcc`. The published timing conformance at `52c121b58df92daccf9654400342dba63dc18672` remains unchanged and bound to its original source snapshot.

## Defect and correction

After a successful provider launch, the runner appended the process identity and wrote `process-identities.json` before entering its provider cleanup `try/finally`. A write failure could therefore skip provider cleanup while the outer responder cleanup still ran. The actual published conformance did not encounter this failure and retains successful original cleanup receipts.

The two identity-record operations now run inside the existing provider cleanup block, immediately after launch. The exception remains a failed attempt, all available rows remain retained, the next runtime is not launched, and both existing cleanup paths execute. No timing callback, protocol, workload, policy, executable, observer, deadline or evaluator logic changed.

## Executed controls

`PYTHONPATH=tests/native/tools python3 -m unittest test_m2_socket_lifecycle test_m2_activation_process test_m2_socket_duration.EvaluatorControls test_m2_socket_plugins test_m2_composed_duration -v`

All **47 portable tests PASS**. The added control exercises the actual runner, owned guard, termination checks and terminal-wait verifier with synthetic process/kernel/system-call records. It injects the identity-write error and requires both process cleanup receipts, exact child waits, closed output streams, no initialization bytes, zero rows and no second provider launch. No native provider or responder is launched by this control.

Restoring the original write-before-`try` placement in an in-memory copy of the actual runner makes that same test fail its cleanup-verification assertion. The retained mutation log records one expected failure and zero test errors. The test also closes synthetic guards on its failure path. This demonstrates that the new control distinguishes the original defect from the correction.

`candidate.patch` and `checks.json` bind the exact change and source hashes. The original qualified profile must not be relabeled with this new runner hash. A future native run requires fresh source/run identities and the pending same-runtime/source-epoch design decision. No native attempt, build, installation, calibration, conformance or acceptance cohort ran in this repair.

## Privacy and preservation

The published test records contain source paths and synthetic process identities only. No owner prompts, provider history, credentials or vendor excerpts are present. All previously committed evidence directories are untouched. The current independent review remains directed at exact candidate `ecc4a4934f6cbec0cd1798baed369e4667cf9dcc`; this separate correction does not alter its reviewed bytes. No ORCHESTRATION.html exists.
