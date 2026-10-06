# Threadspace

## Finishing a branch

When the work on a branch is complete and verified, close the branch out:

1. Commit all remaining work on the branch and push it to `origin`.
2. Merge the branch into `main` and push `main`, so the next piece of work starts from the finished state.
3. Delete the merged branch locally and on `origin`; once merged it is stale, and keeping only live branches makes it obvious what is in progress.

"Complete" means the branch met its own exit criteria (for a milestone, its acceptance in `docs/MILESTONES.md`).

## Automated verification and owner interaction

Threadspace agents automate verification by default. This rule binds every milestone, remediation, acceptance run, regression test and release qualification, in every session.

1. **The agent owns the test harness.** A verification step that code can reasonably perform — native automation, a controlled fixture, XCTest/XCUITest, Accessibility, AppleScript, process control, synthetic input, screenshot capture and readback, a provider API, or another deterministic mechanism — is automated.
2. **The owner is not a manual test runner.** The harness itself opens, rearranges and closes windows and tabs; types test prompts; clicks buttons and notifications; minimizes, restores and fullscreens windows; kills and restarts processes; sleeps and wakes the machine; inspects screenshots and compares UI state; repeats routing tests; collects logs; and confirms any technical state that software can read.
3. **Owner interaction is a last resort**, reserved for an action that genuinely cannot be automated safely or reliably: an OS permission dialog that programmatic interaction is prohibited from bypassing, credential or secret entry, account authentication requiring the owner, a physical hardware action, an irreversible owner decision, or another externally imposed human-only step.
4. **Exhaust reasonable automation before asking.** When interaction is still unavoidable, explain exactly why automation is unavailable, reduce it to the fewest possible actions, give one concise instruction, and continue autonomously afterward.
5. **Acceptance evidence is machine-verifiable**: native readback, provider state, process state, OS logs, screenshots captured and evaluated by the harness, deterministic assertions, persisted state, test output. Owner visual confirmation is not acceptance evidence.
6. **Verification infrastructure is reusable.** When a milestone needs native automation, add it as a documented qualification/test harness component that later milestones can run, rather than one-off manual instructions.
7. **Qualification-only automation is isolated from production behavior.** Test hooks, fault injection, synthetic controls, test permissions and automation helpers are feature-gated or test-only, and stay out of production artifacts unless they are explicitly part of the product.
8. **Destructive tests use disposable resources.** Tests that close windows, kill processes, alter provider sessions, mutate temporary configuration or exercise failure recovery run against dedicated disposable resources — their own Terminal windows, temporary directories, test bundle IDs and qualification stores — and prove a resource belongs to the harness before acting on it, so unrelated owner work is never disturbed.
9. **A PASS is never faked.** A required criterion that truly cannot be automated, and cannot be safely exercised without the owner, is recorded with its exact limitation and marked `BLOCKED` or `MANUAL_EXTERNAL_REQUIRED`; the gate is never silently weakened.
