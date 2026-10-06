# D-0003 — Apple-event consent is attributed to the containing application

**Status:** Accepted for M0A qualification. Natively confirmed in M0B under real focus and readback (see "M0B confirmation" below). Still open for owner/reviewer confirmation of the SPEC §13.5/§18.9 wording.
**Affects:** SPEC §13.5 ("Put [the usage description and automation entitlement] on the native companion and test the resulting identity"), §18.9 ("The outer app and companion have separate minimal entitlements. Automation belongs on the actual event-sending companion"); MILESTONES G07/G08 and M0B step 6 (native readback "through the packaged companion's actual authorization identity").

## Runtime evidence (macOS 27.2, build 26B5091g)

The system-started `ThreadspaceAgent.app` (`ai.scalinity.threadspace.agent`) was signed with `com.apple.security.automation.apple-events` and declared `NSAppleEventsUsageDescription`. The outer `Threadspace.app` (`ai.scalinity.threadspace`) carried neither.

1. `AEDeterminePermissionToAutomateTarget(Terminal, …, askUserIfNeeded: true)` from the companion returned `errAEEventNotPermitted` (−1743) within 14 ms. No prompt appeared, and a following status query still returned `errAEEventWouldRequireUserConsent` (−1744), so no decision had been recorded.
2. The user-level `tccd` log for that request read: `TCCAccessRequestIndirect` from the Apple Event daemon, target Terminal; `target_executable_path_URL: …/Threadspace.app/Contents/MacOS/Threadspace`; `AccessRequestIndirect: Policy disallows prompt for ai.scalinity.threadspace; access to kTCCServiceAppleEvents denied`.

So TCC resolves the responsible identity of the login-item companion's Apple events to the **containing application**, not to the companion bundle.

After `NSAppleEventsUsageDescription` and `com.apple.security.automation.apple-events` were added to the outer application, the same request prompted the owner. Consent returned `noErr`, and the companion's bundled read-only inventory script, run by its bounded `osascript` worker, enumerated Terminal's windows and tab TTYs. The consent survived rebuilding and re-signing both bundles with the same identity.

## Decision

- The outer `Threadspace.app` carries `NSAppleEventsUsageDescription` (`apps/desktop/src-tauri/Info.plist`) and `com.apple.security.automation.apple-events` (`apps/desktop/src-tauri/Threadspace.entitlements`).
- The companion keeps its own usage description and entitlement, and it remains the only process that sends Apple events. The outer app sends none.
- Consent is requested for the exact event the inventory sends, Core Suite `get data` (`core`/`getd`), not for a wildcard class/ID.

## Consequences

- The consent prompt names "Threadspace", and System Settings › Privacy & Security › Automation lists Terminal access under the outer application.
- M0B's "actual authorization identity" for Terminal readback is the outer application's TCC identity exercised by the companion process. M0B/M0C should confirm the same attribution for focus and readback, and after the outer application is replaced.
- SPEC §13.5/§18.9 describe the entitlement as belonging only to the companion. The owner should either accept this record and update those sections, or decide on another arrangement.

## M0B confirmation (2026-10-06, macOS 27.2 26B5091g; attribution captured on packaged build `46a2a04`, continued operation without prompt on `8980d34` and `fc65641`)

Evidence: `evidence/M0B/native/B-first-return-and-D0003/`.

- **Sender.** Every Terminal script during Return, both the read-only enumeration and the focus/readback, ran as `/usr/bin/osascript` whose parent is the system-started companion (`ai.scalinity.threadspace.agent`). The Tauri UI process sends no Apple events.
- **Consent subject.** During a packaged Return, the receiver (Terminal, pid 95390) called `TCCAccessRequestIndirect` for `kTCCServiceAppleEvents` with `target_prompt=0` once per script. The indirect object was `~/Applications/Threadspace.app/Contents/MacOS/Threadspace`, the outer application. tccd answered `authValue: 2` (allowed).
- **The companion's identity is not the consent subject.** After `tccutil reset AppleEvents ai.scalinity.threadspace.agent`, Return still succeeded exactly, with no prompt, and the selected tab was independently confirmed.
- **Two distinct TCC questions.** The system tccd's generic responsibility attribution for the osascript children names the companion as *responsible process* (seen on WindowServer listen-event preflights). That differs from the Apple-event consent subject and does not affect Terminal automation.
- **Consent survived reinstalls.** Consent granted to the outer app in M0A carried over to the M0B builds `46a2a04`, `8980d34` and `fc65641`, which were installed by replacing the bundle and re-registering the login item.

The decision stands unchanged: the outer app carries `NSAppleEventsUsageDescription` and the automation entitlement, and the companion remains the only sender.
