# D-0003 — Apple-event consent is attributed to the containing application

**Status:** Accepted for M0A qualification; open for owner/reviewer confirmation.
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
