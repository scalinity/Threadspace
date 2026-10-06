import AppKit
import CoreServices
import UserNotifications

/// Sends one event (a JSON object) to the Rust core.
func deliverToCore(_ event: [String: Any]) {
    guard
        let data = try? JSONSerialization.data(withJSONObject: event),
        let json = String(data: data, encoding: .utf8)
    else { return }
    json.withCString { ts_core_deliver($0) }
}

/// The C callback the core uses for requests. It copies the request and
/// returns immediately; answers go back through `deliverToCore`.
let agentBridgeCallback: @convention(c) (UnsafePointer<CChar>?) -> Void = { pointer in
    guard let pointer else { return }
    let data = Data(bytes: pointer, count: strlen(pointer))
    guard
        let object = try? JSONSerialization.jsonObject(with: data),
        let request = object as? [String: Any],
        let kind = request["kind"] as? String
    else { return }
    AgentBridge.handle(kind: kind, request: request)
}

enum AgentBridge {
    static func handle(kind: String, request: [String: Any]) {
        let correlationId = request["correlationId"] as? UInt64 ?? 0
        switch kind {
        case "NotificationSettings":
            UNUserNotificationCenter.current().getNotificationSettings { settings in
                deliverToCore([
                    "kind": "NotificationSettings",
                    "correlationId": correlationId,
                    "settings": encode(settings),
                ])
            }
        case "RequestNotificationAuthorization":
            let center = UNUserNotificationCenter.current()
            center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, _ in
                center.getNotificationSettings { settings in
                    deliverToCore([
                        "kind": "NotificationAuthorization",
                        "correlationId": correlationId,
                        "granted": granted,
                        "settings": encode(settings),
                    ])
                }
            }
        case "PostNotification":
            postNotification(request, correlationId: correlationId)
        case "AutomationPermission":
            let target = request["bundleIdentifier"] as? String ?? ""
            let askUser = request["askUser"] as? Bool ?? false
            // May block until the owner answers the consent prompt; never on main.
            DispatchQueue.global(qos: .userInitiated).async {
                let status = automationPermission(bundleIdentifier: target, askUser: askUser)
                deliverToCore([
                    "kind": "AutomationPermission",
                    "correlationId": correlationId,
                    "status": Int(status),
                ])
            }
        case "RunningApplication":
            let target = request["bundleIdentifier"] as? String ?? ""
            DispatchQueue.main.async {
                let running = !NSRunningApplication.runningApplications(withBundleIdentifier: target).isEmpty
                deliverToCore(["kind": "RunningApplication", "correlationId": correlationId, "running": running])
            }
        case "AccessibilityPreferences":
            DispatchQueue.main.async {
                let workspace = NSWorkspace.shared
                deliverToCore([
                    "kind": "AccessibilityPreferences",
                    "correlationId": correlationId,
                    "preferences": [
                        "reduceMotion": workspace.accessibilityDisplayShouldReduceMotion,
                        "increaseContrast": workspace.accessibilityDisplayShouldIncreaseContrast,
                        "reduceTransparency": workspace.accessibilityDisplayShouldReduceTransparency,
                        "differentiateWithoutColor": workspace.accessibilityDisplayShouldDifferentiateWithoutColor,
                    ],
                ])
            }
        case "OpenContainingApp":
            DispatchQueue.main.async { openContainingApp() }
        default:
            break
        }
    }

    private static func postNotification(_ request: [String: Any], correlationId: UInt64) {
        let requestId = request["requestId"] as? String ?? ""
        let content = UNMutableNotificationContent()
        content.title = request["title"] as? String ?? "Threadspace"
        content.body = request["body"] as? String ?? ""
        content.sound = .default
        // Internal identifiers and a schema version only (SPEC §7.5).
        content.userInfo = [
            "schema": 1,
            "attentionId": request["attentionId"] as? String ?? "",
            "sessionId": request["sessionId"] as? String ?? "",
        ]
        let notification = UNNotificationRequest(identifier: requestId, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(notification) { error in
            var event: [String: Any] = [
                "kind": "NotificationPosted",
                "correlationId": correlationId,
                "requestId": requestId,
            ]
            event["error"] = error.map { "\(($0 as NSError).domain) \(($0 as NSError).code)" } ?? NSNull()
            deliverToCore(event)
        }
    }

    /// `AEDeterminePermissionToAutomateTarget` under this app's own identity,
    /// for the exact event the read-only inventory sends: Core Suite "get
    /// data". A wildcard class/ID only reports status; it is refused at once
    /// (errAEEventNotPermitted) instead of prompting when consent is needed.
    private static func automationPermission(bundleIdentifier: String, askUser: Bool) -> OSStatus {
        let target = NSAppleEventDescriptor(bundleIdentifier: bundleIdentifier)
        guard let address = target.aeDesc else { return OSStatus(paramErr) }
        return AEDeterminePermissionToAutomateTarget(address, AEEventClass(kAECoreSuite), AEEventID(kAEGetData), askUser)
    }

    /// The containing application is four levels up:
    /// Threadspace.app/Contents/Library/LoginItems/ThreadspaceAgent.app.
    /// It must carry this companion's bundle identifier minus `.agent`.
    private static func openContainingApp() {
        let ownIdentifier = Bundle.main.bundleIdentifier ?? ""
        guard ownIdentifier.hasSuffix(".agent") else { return }
        let expected = String(ownIdentifier.dropLast(".agent".count))
        var url = Bundle.main.bundleURL
        for _ in 0..<4 { url.deleteLastPathComponent() }
        guard url.pathExtension == "app", Bundle(url: url)?.bundleIdentifier == expected else { return }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        NSWorkspace.shared.openApplication(at: url, configuration: configuration) { _, _ in }
    }

    static func encode(_ settings: UNNotificationSettings) -> [String: String] {
        func setting(_ value: UNNotificationSetting) -> String {
            switch value {
            case .enabled: return "enabled"
            case .disabled: return "disabled"
            case .notSupported: return "notSupported"
            @unknown default: return "unknown(\(value.rawValue))"
            }
        }
        let status: String
        switch settings.authorizationStatus {
        case .notDetermined: status = "notDetermined"
        case .denied: status = "denied"
        case .authorized: status = "authorized"
        case .provisional: status = "provisional"
        @unknown default: status = "unknown(\(settings.authorizationStatus.rawValue))"
        }
        let style: String
        switch settings.alertStyle {
        case .none: style = "none"
        case .banner: style = "banner"
        case .alert: style = "alert"
        @unknown default: style = "unknown(\(settings.alertStyle.rawValue))"
        }
        return [
            "authorizationStatus": status,
            "alertSetting": setting(settings.alertSetting),
            "soundSetting": setting(settings.soundSetting),
            "badgeSetting": setting(settings.badgeSetting),
            "notificationCenterSetting": setting(settings.notificationCenterSetting),
            "lockScreenSetting": setting(settings.lockScreenSetting),
            "alertStyle": style,
        ]
    }
}
