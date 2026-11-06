# Implementation status

Status reviewed on November 5, 2026. The accepted base is `main` at
`af9b285da529890bc441ea00f1a92e73e39902a8`; active implementation is on `m2`.
The personal release is incomplete. Milestone acceptance remains defined by
[MILESTONES.md](MILESTONES.md), with architecture in [SPEC.md](SPEC.md).

## Current work

Ghostty is the primary terminal. The installed application is Ghostty 1.4.0
build 18457, whose dictionary exposes readable terminal `tty` and `pid`
properties. Discovery, persisted bindings, Return, setup and diagnostics are
implemented for Ghostty through the shared route engine: the provider's
controlling device joins exactly one terminal TTY of one Ghostty process, the
native terminal ID is the persisted locator, and every Return re-enumerates,
focuses and reads back every level. A Ghostty process is scripted only while
it is the sole process running its executable path, and only for the image it
actually maps. [D-0011](decisions/D-0011-ghostty-primary-terminal.md) records
the decision; native session qualification of this slice is open.

A Ghostty process started before an in-place update keeps running its old
image and reports that a restart is required; its sessions have no automatic
Return until it restarts. Terminal.app support remains available.

The installed Claude CLI is 2.1.318. Its observer kit qualification is
recorded in
[`evidence/M2/provider/claude-2.1.318/`](../evidence/M2/provider/claude-2.1.318/README.md):
41/41 kit tests after fixture-only corrections for the kit's nested
`tool.check` preflight. Native interactive qualification is required before
2.1.318 joins the qualified observer builds.

## Milestone state

| Milestone | Current state | Remaining work |
| --- | --- | --- |
| M0A–M0C | Accepted under recorded scopes | Regress changed native paths; preserve the release/hardware limitations assigned to M15. |
| M1 | Accepted and merged into `main` | Reuse the journal, reducer, contracts and deterministic harness. |
| M2 | Implemented vertical slice; acceptance incomplete | Ghostty integration, current Claude compatibility, cold capture-helper latency, complete schema 3 latency collection and source-matched native acceptance. |
| M3 | Partial reconciliation and routing primitives | Complete and qualify resume, concurrent attachments, in-place session changes, late discovery and active-session recovery. |
| M4 | Actor facts and relationships exist | Expose actor/run relationships in projections and UI; qualify role changes, child attention and supported team modes. |
| M5 | Durable attention and native notification primitives exist | Connect canonical outbox delivery, recovery and suppression; complete snooze and queue controls. Canonical intents currently are not submitted to macOS. |
| M6 | Native Codex qualification probe exists | Build the production provider adapter, mode-specific hooks/daemon observation, pairing and recovery. |
| M7 | Not implemented | Project/repository/worktree indexing and persistent spatial placement. Current scene positions depend on fleet order and size. |
| M8 | Ghostty brought forward into current work | Qualify supported Ghostty surfaces. iTerm2 and tmux remain unsupported until their separate adapters and native gates pass. |
| M9 | Not implemented | Session-enrolled semantic MCP and bounded annotation handling. Semantic hashing is not an MCP adapter. |
| M10 | Endpoint primitives only | Optional remote collector, SSH transport and original-URL routing; native remote proof needs a second endpoint. |
| M11 | Not implemented | Optional linked ChatGPT worker and original-URL/manual semantics, with explicit experimental coverage. |
| M12 | WebGPU lifecycle, fallback and accessibility foundations exist | Integrated appearance/accessibility polish, project/actor rendering, interchangeable skins and keyboard switching. |
| M13 | Foundational durability/fault harnesses exist | Retention, full workload measurements, long-run recovery, capacity and final integrated performance qualification. |
| M14 | Not implemented | Reusable 31-step integrated acceptance runner, three native runs and the required adversarial campaign. |
| M15 | Signing, setup and maintenance foundations exist | Production packaging without qualification features, update/recovery orchestration and clean install/update/uninstall cycles. |

M8–M11 profiles that are not part of the supported personal release remain
disabled and `NOT_SUPPORTED`. Their absence does not authorize advertising them
as supported. Ghostty is required by the active primary-terminal scope.

## M2 completion path

1. Finish and qualify the Ghostty slice against the actual installed capability
   profile, preserving exact session identity and current native readback.
2. Requalify the installed Claude runtime natively. The production observer
   allowlist is exactly 2.1.291 and 2.1.295; neither executable is installed.
   2.1.318's declarations, kit and portable suites pass; ownership/reload,
   native outcomes and inventory need native qualification before the
   allowlist changes.
3. Resolve cold capture-helper startup. The
   [compact-helper comparison](../evidence/M2/compact-helper/README.md) was
   inconclusive and did not select an installed repair. The first baseline
   execution took 618.911 ms. Keep the launch-through-reap 250 ms control and
   receipt/spool behavior intact; a watchdog starting in `main` cannot bound
   time before `main`.
4. Finish the schema 3 native collector using the existing composed DOM,
   native receipt and duration components. The older `m2_latency.rs` runner
   exports schema 2. Qualify the timing instrument in the actual provider
   runtimes used by the new campaign, then measure the complete original
   capture population through COMMIT and visible DOM.
5. Meet the required capture-to-COMMIT / COMMIT-to-DOM / event-to-visible
   targets of 100/100/250 ms, then independently review M2. F1–F3 have focused
   native candidate records pending review; they do not provide an F4 pass.
   D-0009 and D-0010 remain proposed until that review closes them.
6. Complete M2's branch exit, then continue M3–M7 and the local release path
   through M12–M15. Integrate shared contracts, journal/reducer changes and
   generated types serially even when independent provider or harness work
   runs in parallel.

The [duration instrument qualification](../evidence/M2/replacement-duration-qualified/README.md)
is scoped to its recorded runtimes. The
[runner cleanup correction](../evidence/M2/runner-cleanup-closure/README.md)
does not requalify different provider or application binaries. The
[M2 manifest](../evidence/M2/manifest.json) retains the older campaign details;
the later source-bound records above supply the subsequent findings.

## Verification baseline

With the Ghostty slice, the focused Rust library suites pass (provider 83,
surfaces 38, surfaces-macos 39), as do the journal suites, the companion and
contracts tests, TypeScript checking, the 130 frontend tests and a workspace
check of all targets. These are source checks, not installed-native or release
acceptance.

The current `tests/native/build-app.sh` and companion build script enable
qualification features for both named build modes. Their `prod` label is not
proof of a production artifact. M15 must supply and inspect a build with the
qualification server, synthetic commands and test permissions absent.
