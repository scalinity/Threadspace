# Final source-bound build checks

**Rust macOS-target typecheck and Clippy: PASS. Native execution: NOT RUN.**

`summary.json` records the actual commands, Rust 1.99.0 toolchain, Linux host,
output hashes and the same 217 source/configuration hashes before and after both
commands. `macos-target-check-final.log` and
`macos-target-clippy-final.log` are the successful final outputs.

The checked packages are `threadspace-provider-claude`, `threadspace-relay` and
`threadspace-harness`, with all their Rust targets, relay qualification enabled,
the pinned lockfile and `aarch64-apple-darwin` target. Clippy denies warnings.
No dependency version was changed to obtain these results.

This is a cross-target Rust check. It does not link or package the outer Tauri
application, build the native Objective-C companion, run Darwin APIs or execute
Claude/Terminal. There is no native executable hash or performance result here.
Actual portable journal/reducer tests, the observer kit and desktop tests have
their own records in the neighboring directories.

## Preserved attempts

- `macos-target-check-01.log` records an earlier successful typecheck before
  the final F3 and census changes. Its complete pre-run source fingerprint was
  not recorded, so it is an interim diagnostic, not final source qualification.
- `macos-target-clippy-01.log` records the then-new helper's collapsible nested
  `if`. The final source uses the equivalent combined condition. This failure
  remains visible rather than being described as an earlier passing run.
- `before-f3-test-lint-fix/` preserves a complete source-bound combined run:
  typecheck passed, Clippy rejected a test-only `unwrap()` in the minimized
  result oracle. Replacing it with a descriptive `expect()` changed no routing
  behavior or assertion requirement. The six oracle tests were rerun, followed
  by the successful final combined check above.
- `integrity-before-generated-cache-cleanup.json` preserves an interim static
  scan: all 55 historical hashes matched and there were no credential-prefix
  findings, but generated Python bytecode remained. Those cache files are not
  review evidence and are removed before publication. The final integrity
  report records the final prospective publication set.
- `before-missing-watchdog-fix/` preserves a successful combined typecheck/Clippy
  before the independent audit found a TypeScript qualification timer-handle
  defect. Its hashes identify that earlier source. The final combined run was
  repeated after the narrow watchdog guard; actual JavaScript behavior is proved
  by the separate observer controls, not by a Rust typecheck.

The generated TypeScript exports preserve the canonical ts-rs output, including
its existing trailing whitespace in generated documentation. The fully staged
scan also reports whitespace and final blank lines in byte-exact execution logs,
mutation patches and a historical source snapshot. One harmless final blank line
in the exact as-tested `ownership_record.rs` is retained with its SHA-256 in the
check. `integrity-first-staged-scan.json` preserves the initial rejection; the
final report classifies each diagnostic and its preservation reason. It does not
claim a clean `git diff --check`, trim raw evidence, or allow an unclassified
formatting diagnostic.

Reproduce the checks with the pinned Rust toolchain and macOS target installed:

```sh
python3 evidence/M2/remediation-1/run-build-checks.py --target-dir /absolute/disposable/build-target
```

This command compiles/checks source only. It does not start or change an installed
application, store, provider configuration or Terminal session.
