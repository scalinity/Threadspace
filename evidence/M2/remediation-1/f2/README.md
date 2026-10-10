# F2 — integration ownership and reversible cleanup

Status: **PORTABLE_PASS_NATIVE_PENDING**. This evidence was executed on Linux
against the remediation source identified by `portable-summary.json`. It is not
a development-app build or a native integration smoke result. The rejected
candidate's native integration-cycle evidence is preserved under
`evidence/M2/integration-cycles/` and is not relabeled as qualifying these changes.

## Contract and repair

SPEC §19.2 requires removal of only still-matching owned entries, preservation of
foreign settings and ordering, and a conflict when the owner modified an entry.
The F2 remediation instruction also requires retained resources for unresolved
references and cleanup only of successfully acquired installations. AGENTS.md
resource-ownership rule 8 applies to the harness's configurations and directories.

`setup/settings.rs` now compares the recorded matcher and the installed hook's
complete supported structure. The command remains the ownership marker, but a
different matcher, additional timeout/options, changed type, or changed command
cannot authorize deletion. Foreign sibling hooks retain their values and order.

`setup/mod.rs` retains the helper, observer directory and installation record
when an owner-modified entry or surviving resource reference remains. A partial
report names removed/conflicted entries, retained paths, record and reference
locations; `complete` is false and `uninstalledAtMs` is zero. It records only the
remaining conflicts for retry without turning the partially removed settings into
an original-byte restore point. Reinstall refuses unresolved conflicts rather
than overwriting retained resources. An owner can restore the recorded entry and
retry removal; F2-12 exercises that resolution. Shell commands are not executed to
guess their dependencies. Conflicts conservatively retain the helper/mod set.

New installation records bind scope, absolute configuration path, owned root and
agent identity. Status, reinstall and uninstall verify this identity before
reading provider settings or mutating resources. A copied record, another scope,
another configuration or an old record lacking the required identity refuses
without mutation. Legacy records remain readable but cannot silently acquire
missing ownership authority; their resources are left intact for explicit
resolution.

The pure `m2_ownership.rs` module used by the native harness records the exclusive
scratch directory's device/inode and the successful installation record returned
to this invocation. Failed activation creates no installation lease. Reinstall
and removal recheck the current complete record; changed/unverified ownership
refuses cleanup. Drop rechecks directory identity and preserves the scratch
directory if integration cleanup is incomplete. The vertical, routes and fault
callers use that acquisition, as do the reversible integration cycles. Merely
attempting install no longer authorizes a later uninstall call.

## Executed mandatory cases

All twelve cases execute actual production installer functions on disposable
fixtures. F2-10 executes the actual pure Scratch/Drop module through a supplied
CLI-result runner backed by the real installer. No native application is invoked.

| Case | Discriminating assertion | Result |
|---|---|---|
| F2-01 | Original install/reinstall/remove cycles restore exact bytes for empty and complex configurations | PASS |
| F2-02 | Changed matcher is preserved and reported as a conflict | PASS |
| F2-03 | Added timeout is preserved and reported as a conflict | PASS |
| F2-04 | Changed hook type is preserved and reported as a conflict | PASS |
| F2-05 | Changed command survives; helper/mod/record bytes remain; reinstall refuses | PASS |
| F2-06 | Changed plugin-directory reference survives; referenced resources remain | PASS |
| F2-07 | Foreign siblings retain their values and ordering | PASS |
| F2-08 | Repeated partial removal deletes nothing twice and preserves the retained record/settings | PASS |
| F2-09 | User install/session uninstall refuses with every fixture byte and mode unchanged | PASS |
| F2-10 | Failed session acquisition makes zero uninstall calls on Drop; pre-existing user fixture remains byte-identical; successful owned acquisition/reinstall/removal also works | PASS |
| F2-11 | Different configuration, copied record in another owned root, and legacy identity-less record all refuse without mutation | PASS |
| F2-12 | Restoring the recorded conflicted entry permits a complete retry, preserving foreign configuration | PASS |

The unchanged original installer tests also pass, including ten reversible cycles
for each of its two fixtures, same-directory checked writes, foreign settings,
session scope, backup permissions and Worktree-hook exclusion. The real-mod
staging test checks byte-identical `register.ts`, `delivery.ts`, `ownership.ts`
and `latency.ts`, excludes test/type tooling, and verifies the content-addressed
copy hash and configured helper argv.

`byte-inventories/` retains five before/after inventories from F2-09, F2-10 and
the three F2-11 refusals. They include settings, record, helper, observer files,
backups and unrelated fixture configuration, with SHA-256 and byte length; the
setup inventories also record file modes. Paths are relative to disposable
fixtures. The failed-activation inventory records zero uninstall invocations.

## Execution and controls

`run-portable.py` creates an exclusive temporary workspace, copies the actual
setup and pure Scratch modules byte-for-byte, verifies their hashes, and supplies
only a standalone Rust crate wrapper. Its pinned dependencies match the workspace;
the generated portable lockfile and wrapper are retained. Provider runtime files
are copied for the real-mod staging test and their hashes are also retained.
There are no Darwin API replacements in these tested modules.

Final execution on Rust 1.99.0, `x86_64-unknown-linux-gnu`:

- **43 Rust tests passed**, including all twelve required F2 cases.
- Clippy passed with warnings denied.
- Four targeted mutations were killed by the named test's assertion failure,
  with exit 101; compilation failure alone is not accepted as a killed control.

| Retained mutation | Required failing witness |
|---|---|
| Command-marker-only removal, without matcher/structure protection | F2-02 |
| Delete resources despite remaining conflicts/references | F2-05 |
| Remove the uninstall identity guard | F2-09 |
| Unconditional uninstall in Scratch Drop without acquisition | F2-10 |

Each mutation has its exact patch and execution log. The runner changes only a
disposable copy and deletes only the workspace incarnation it created. Mutant
fixtures are confined to that workspace. The positive source remains unchanged.

`portable-summary.json` records module hashes, source runtime hashes, execution
argv and statuses, output hashes, fixture-inventory hashes and test names. The
workspace lockfile hash is the execution-time hash; the independent portable
lockfile is retained separately. Reproduce with the repository-pinned Rust
toolchain and an available dependency cache:

```sh
python evidence/M2/remediation-1/f2/run-portable.py --target-dir /absolute/disposable/build-directory
```

`workspace-linux-attempt.log` retains the real full-provider compile limitation:
the workspace imports Darwin `libc` process APIs unavailable on Linux. The
portable execution does not replace this with a claim of a full workspace build.
`attempts/` preserves the initial portable-wrapper/lint issues and their output;
the final logs above are the passing rerun after those issues were corrected.

## Native closure still required

The focused development-channel native integration smoke is **NOT EXECUTED**.
It must run against a new source-matched Dev build using dedicated disposable
settings, integration resources and qualification store. It must exercise the
changed CLI path and verify that failed acquisition cannot remove a separately
installed disposable owner-like integration, that foreign settings survive, and
that normal cleanup completes. The Linux result establishes the tested portable
installer/Scratch behavior; it does not establish macOS CLI packaging or native
execution. No production app, owner store, Claude settings or Terminal session
was touched for this evidence.

The historical process-ownership incident and the scope of the cleanup audit are
recorded in `harness-ownership-incident.md` and `harness-ownership-audit.json`.
