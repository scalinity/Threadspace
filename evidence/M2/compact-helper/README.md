# Compact-helper bounded comparison

**COMPACT-HELPER COMPARISON INCONCLUSIVE_BENEFIT — M2 UNACCEPTED**

Source base `52c121b58df92daccf9654400342dba63dc18672` plus the source-frozen helper-only profile and comparison harness. No installed application or default build profile changed. One `capture-compact` profile inherits release, sets `opt-level="z"`, one codegen unit and fat LTO, and explicitly preserves unwind. Every subcommand and all helper source bytes remain unchanged. Rust 1.99.0 commit `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, LLVM 23.1.1, aarch64-apple-darwin and Cargo.lock are pinned.

## Predeclared acquisition and execution

The comparison froze two independently acquired inodes per arm, for intended `qualification` and separate diagnostic `qualification-startup` builds. Every acquired helper was signed with the same Apple Development identity, hardened runtime and no timestamp; exact signatures, file identities and acquisition order are retained. Source and artifact identities were fixed before any measured helper execution. Pair one runs baseline then compact, pair two compact then baseline. Each inode has exactly its first and one repeat execution. No prewarm, cache reset, favorable retry or second configuration occurred.

Each call used the same synthetic original mod-batch input and 80 ms budget against a fresh private absent-companion store. Parent CLOCK_UPTIME_RAW begins before argv construction and includes launch, kernel/file ownership checks, input delivery, waiting, output read and reap. Nothing is subtracted. The 10 ms exact-wait polling quantum is accounted for by a premeasurement discrimination requirement greater than 11 ms (10 ms plus 1 ms engineering guard); initial planning was refined to that value before acquisition or measurement. These are first executions of acquired inodes; global OS caches are uncontrolled.

The unchanged original named Rust test and `<250` assertion are byte-identical to `85cbffc3d53da45d0ab63b514dd43b193f2abb16`. Its intended compact test artifact compiled but was not executed after the fixed comparison stopped. The profile is not selected by the application build script.

## All executions

| Variant | Pair | Arm | First ms | Repeat ms | Receipt/spool/cleanup |
|---|---:|---|---:|---:|---|
| qualification | 1 | release | 618.910875 | 17.018625 | PASS |
| qualification | 1 | capture-compact | 105.901833 | 27.460791 | PASS |
| qualification | 2 | capture-compact | 80.859250 | 29.055917 | PASS |
| qualification | 2 | release | 100.491625 | 16.330500 | PASS |
| qualification-startup | 1 | release | 118.567208 | 17.156041 | PASS |
| qualification-startup | 1 | capture-compact | 112.997125 | 16.547291 | PASS |
| qualification-startup | 2 | capture-compact | 81.310625 | 16.998083 | PASS |
| qualification-startup | 2 | release | 92.454000 | 16.745250 | PASS |

The first signed baseline execution took **618.910875 ms**, exceeding 250 ms. It is retained and never replaced. All compact first and repeat executions stayed below 250 ms. Intended-artifact first-execution improvements were 513.009042 and 19.632375 ms. Diagnostic first-execution improvements were 5.570083 and 11.143375 ms; the first diagnostic pair did not meet the predeclared discrimination guard. Therefore the full GO conjunction is false. Smaller bytes and the intended-arm improvement are not substituted for the required launch-phase corroboration.

All 16 executed rows returned the original UUID with LOCAL_SPOOLED, retained one actual ready spool record with matching UUID/epoch, and exited successfully. Exact owned-process kernel/file identity and terminal wait evidence are retained. No unrelated process was signaled, and no comparison process remains. The two artifacts are about 1.665 MB baseline and 1.057 MB compact for the intended feature set; that size reduction alone earns no acceptance.

The original Rust subprocess assertion, additional live COMMITTED/ALREADY_COMMITTED controls and other-command equivalence remain **NOT_RUN**, because the numerical comparison reached its mandatory stop. There is no GO claim and no installed performance repair. Historical 289/308/311/313 ms failures and this 618.911 ms baseline failure remain preserved. The original 250 ms wall contract and all 100/100/250 F4 targets remain unchanged.

## Publication and next gate

The full frozen plan, source and inode identities, build commands, every native row and diagnostic phase are included. Raw spool bytes, private build outputs and certificate display names stay outside Git; private-original integrity uses HMAC with a private key. Public qualification inputs, owned temporary paths, executable signatures/fingerprints, boot/build identities and synthetic UUIDs are functional provenance. No credentials, prompts, provider history or model output are published. Five portable verdict controls passed; no unrun control is reported as PASS.

The cold-helper gate is still open. This result does not justify trying another compact configuration within the consumed comparison or installing the candidate. It calls for a separate startup-cause/design decision and complete cold-budget closure. The independently qualified timing instrument remains scoped to its original runtimes; the full schema-3 WebKit/native campaign and M2 acceptance remain open. No ORCHESTRATION.html exists.
