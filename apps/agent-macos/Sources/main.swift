// ThreadspaceAgent.app — the independently supervised LSUIElement companion.
//
// The notification delegate and application delegate are created here, at
// top level, and held for the life of the process. The notification delegate
// is assigned before `NSApplication.run()`, i.e. before AppKit finishes
// launching, so a launch caused by a notification response still reaches it
// (SPEC §7.5).

import AppKit
import UserNotifications

let notificationController = NotificationController()
let agentDelegate = AgentAppDelegate()

UNUserNotificationCenter.current().delegate = notificationController

let application = NSApplication.shared
application.setActivationPolicy(.accessory)
application.delegate = agentDelegate
application.run()
