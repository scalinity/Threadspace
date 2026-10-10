# Independent F1B implementation audit

**Portable result: PASS after a retained migration failure and correction. Native macOS execution: NOT RUN.**

This audit examined the current remediation worktree based on rejected candidate `2c4b5012f59189dcb38d61ced0b0b9fe96fb043e`. It did not modify product code, run the Mac application, operate Terminal, change owner configuration, commit, push or merge. The observer reviewer read the native proof producer and reducer independently of their implementer. The new authority source has not yet been qualified by actual macOS inventory/kernel execution.

## Authority review

- `crates/relay/src/bin/threadspace-hook.rs::observer_proof` obtains the helper's actual parent through kernel ancestry. The request cannot supply a ProcessKey or executable. It invokes inventory through that parent's executable path, creates a fresh token only after corroboration, and commits or durably spools the independent proof before returning the token. A typed committed receipt, rather than subprocess exit, is the success condition. A two-second watchdog bounds this detached probe.
- `crates/provider-claude/src/ownership.rs::corroborate` requires a qualified actual provider image, two fresh inventory observations, kernel incarnation samples around the lookup, the same Session in both inventory observations, and the original helper-parent birth and full executable identity. The before/after join is the existing production discovery implementation. The qualified profile is selected from the kernel-observed executable, not the host version reply or current launcher.
- `crates/relay/src/modbatch.rs::envelope` fixes the observer adapter/source and allowlists events. A batch record cannot choose the independent proof adapter or submit `ownership.proven`. An intercepted `process.run` reply can supply only an untrusted correlation claim to the observer; it cannot itself create the separate native proof fact.
- The observer retains its original immutable epoch, Session generation and ownership. A response is sealed only if all echoed fields match and that exact generation survived the asynchronous probe. A pre-seal callback or retained Turn does not acquire the token retroactively. The seal itself remains `HOST_READ`.
- The reducer requires an exact Session, ProcessKey, executable identity, epoch, generation and token match with a separately admitted native proof. It also requires causal `seal < qualified Turn start < outcome` in the observer's order domain. Historical attachments are not evidence for this predicate. Wrong Session/process/image/epoch/generation/token and an intercepted token without native proof remain nonauthoritative. Delivery order may change; the causal conditions do not.
- The added native-producer unit seam includes an actual `2.1.295` image and a mismatch between two otherwise qualified images. These macOS-linked tests were source-reviewed here; their native execution is still pending.

No remaining concrete authority bypass was found within the accepted capture trust boundary. This does not qualify the native probe's real runtime availability or latency.

## Executed portable checks and retained failure

`environment-attempt.log` records an initial attempt which inherited the repository's macOS target and could not compile in this Linux environment. The executable test was then run with an explicit Linux target:

```text
cargo test --locked --offline --target x86_64-unknown-linux-gnu \
  -p threadspace-journal --test observer_ownership f1b_ -- --nocapture
```

The first executable attempt (`migration-mismatch.log`) passed eight tests and failed the genuine reducer-3 upgrade test. At old checkpoint 11, genesis replay retained original Process/Execution revisions while SQLite's existing materialization bookkeeping assigned the upgrade cursor. The hashes differed. The reducer implementer corrected `Engine::upgrade` to assign those transition revisions consistently; no oracle field was removed and no assertion was weakened.

**The exact full pre-repair source snapshot was not captured.** Its failure log is retained, but no retrospective source identity or overwritten executable identity is claimed. This in-progress remediation failure is separate from the retained immutable rejected-candidate negative control in the parent directory.

The independent rerun (`repaired-audit.log`) passed **9 tests, 0 failures**. `repaired-context.json` records the exact focused source hashes before and after execution; they were unchanged. Copies in `tested-source/` match those recorded hashes. These are the tested source files, not a claim that the entire concurrent worktree was frozen or committed.

`repaired-records/` contains 44 actual test-emitted admission/migration records. Complete canonical state equals genesis replay at old checkpoints **0, 2 and 11**. All three produce state hash `5d1457917ddba0e7951b3e514b69eebf72250a7dd783ad0729cd17552978d37d`, semantic hash `d84dea3bcca7f53fddd01abb8429417924d2acf5d32550e5dc505ec5c46bc0ef`, and matching projection/table hashes. The tests preserve owner command receipts and decisions, suppress withdrawn stale-authority work, retain independent native completion, and verify fixed-point restart and subsequent scoped proof admission.

The same compiled binary then independently passed the actual fourteen-case TypeScript fixture chain through production mod-batch parsing, the production observer adapter, SQLite, public projection and restart (`f1a-sqlite-chain.log`). This is one Rust test containing fourteen recorded cases, not fourteen additional native executions.

`summary.json` distinguishes these results and limitations; `artifact-hashes.json` indexes the retained artifacts. Exact native-source/build identity and actual Mac qualification remain the final campaign's responsibility.
