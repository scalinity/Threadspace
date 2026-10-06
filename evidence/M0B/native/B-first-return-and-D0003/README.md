# M0B native test B: first exact Return, and the D-0003 re-qualification

**Result: PASS.** D-0003 is **retained**; native evidence confirms it.

**Artifacts:** installed build from `46a2a04`. The companion is pid 56092 and Terminal.app is pid 95390.

## First exact Return (owner click in the packaged UI)

The owner clicked **Return** on test session `efa96221` (canonical `a52de5c2…`) in the packaged Threadspace window. The click registered twice, 37 s apart. Returns are idempotent, and both are recorded (`05-export-routes.json`).

| | route `8a05de01` | route `2347d657` |
| --- | --- | --- |
| result | EXACT_NATIVE_SURFACE / CURRENT_NATIVE_REVALIDATED / FOREGROUND_COMPATIBLE | same |
| latency | 907 ms | 858 ms |
| binding rev loaded → before focus → after focus | 24 → 24 → 24 | 24 → 24 → 24 |
| ProcessKey | pid 54698, birth 1791273926.140544 | same |
| `e_tdev` before / after | 0x10000002 / 0x10000002 | same |
| provider lookup (pid→session, kind) | 54698 → efa96221, interactive (172 ms) | same (177 ms) |
| post-focus lookup | 54698 → efa96221 | same |
| Terminal incarnation before/after enumeration | 95390 / 95390 | same |
| `st_rdev`-matched tabs | exactly `/dev/ttys002` (0x10000002) | same |
| focus script sender | osascript pid 24736, parent 56092 | osascript pid 25891, parent 56092 |
| readback | front window = target window; selected tty `/dev/ttys002`, rdev 0x10000002 | same |
| frontmost (NSWorkspace) | com.apple.Terminal, pid 95390 | same |

Activation worked from the packaged UI: Threadspace was frontmost when the owner clicked, and Terminal came to the front.

## D-0003 qualification

**Static arrangement** (`01-bundles-entitlements-usage.txt`):

- `ai.scalinity.threadspace` (outer) and `ai.scalinity.threadspace.agent` (companion) each carry `com.apple.security.automation.apple-events` and an `NSAppleEventsUsageDescription`.
- The companion reports Terminal automation as `AUTHORIZED` (status 0), asked without prompting (`02-integration-before.json`).

**The process that actually sends the Apple events:**

- The poller in `04-osascript-sender-poller.txt` shows every Terminal script running as `/usr/bin/osascript` whose parent is the companion, pid 56092.
- The Tauri UI process sends none.

**Whose consent is checked** (`08`–`11`). To force a fresh decision, any Apple-event consent stored under the companion's own identity was cleared with `tccutil reset AppleEvents ai.scalinity.threadspace.agent`. One Return then ran (`09-return-after-companion-reset.json`).

- The Return succeeded on all three strongest axes in 871 ms, with no prompt.
- The harness's own readback (`independent`) confirms `/dev/ttys002` was selected, that it is session `efa96221`'s device, and that Terminal was frontmost.
- During that Return the receiver, Terminal (pid 95390), made a `TCCAccessRequestIndirect` call for `kTCCServiceAppleEvents` with `target_prompt=0` once per script. In every call the indirect object was `~/Applications/Threadspace.app/Contents/MacOS/Threadspace`, the **outer application**.
- tccd answered `Handling access request: kTCCServiceAppleEvents:…:com.apple.Terminal … authValue: 2` (allowed). See `10-logshow-reset-test.txt` and `11-tccd-appleevents-indirect-debug.txt`.

**Conclusion.** The companion's osascript worker sends the events. TCC checks Apple-event consent against the containing `Threadspace.app` identity. A consent record under the companion identity plays no part. This matches D-0003's M0A finding under real focus and readback in the packaged M0B build, so no corrective decision is needed.

Also observed: the system tccd's generic responsibility attribution names the companion (`ai.scalinity.threadspace.agent`) as the *responsible process* of its osascript children (`07-…`, kTCCServiceListenEvent preflights). The Apple-event *consent subject* is still the outer app. These are different TCC questions, and only the second governs Terminal automation.

## Notes

- Before the reset, no `kTCCServiceAppleEvents` request reached tccd during the first route. The Apple Events subsystem answered from its cache, which is why the reset step was needed to observe the decision.
- One `#ManagedTCCDefaults showing the prompt for ai.scalinity.threadspace` line at UI launch belongs to a disclosure check for another service (input listening), not to Apple events.
