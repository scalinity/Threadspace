// Activates a presented notification banner through the accessibility API,
// the path VoiceOver uses: it waits for a Notification Center element whose
// text contains the given title and performs its press action. The system
// then routes the default action to the posting app's notification delegate.
// Prints one JSON report.
//
//   swiftc -O tests/native/press-notification.swift -o <out>/press-notification
//   <out>/press-notification "Fixture worker finished a turn" [timeoutSeconds]
import AppKit
import ApplicationServices
import Foundation

let arguments = Array(CommandLine.arguments.dropFirst())
guard let needle = arguments.first else { exit(64) }
let timeout = arguments.count > 1 ? Double(arguments[1]) ?? 10 : 10
guard AXIsProcessTrusted() else { print("{\"pressed\":false,\"reason\":\"accessibility trust required\"}"); exit(1) }

func attribute<T>(_ element: AXUIElement, _ name: String) -> T? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value as? T
}

func text(_ element: AXUIElement) -> String {
    [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute, kAXHelpAttribute]
        .compactMap { attribute($0 == kAXValueAttribute ? element : element, $0) as String? }
        .joined(separator: " ")
}

func actions(_ element: AXUIElement) -> [String] {
    var names: CFArray?
    AXUIElementCopyActionNames(element, &names)
    return (names as? [String]) ?? []
}

/// Depth-first search for the shallowest pressable element whose subtree text contains the needle.
func find(_ element: AXUIElement, depth: Int = 0) -> (AXUIElement, String)? {
    guard depth < 14 else { return nil }
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    for child in children {
        let own = text(child)
        if own.contains(needle) || subtreeContains(child, depth: depth + 1) {
            if actions(child).contains(kAXPressAction) { return (child, own) }
            if let deeper = find(child, depth: depth + 1) { return deeper }
        }
    }
    return nil
}

func subtreeContains(_ element: AXUIElement, depth: Int) -> Bool {
    guard depth < 14 else { return false }
    if text(element).contains(needle) { return true }
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    return children.contains { subtreeContains($0, depth: depth + 1) }
}

let deadline = Date(timeIntervalSinceNow: timeout)
var report: [String: Any] = ["needle": needle, "pressed": false]
while Date() < deadline {
    for app in NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.notificationcenterui") {
        let root = AXUIElementCreateApplication(app.processIdentifier)
        if let (element, label) = find(root) {
            let role: String = attribute(element, kAXRoleAttribute) ?? ""
            let status = AXUIElementPerformAction(element, kAXPressAction as CFString)
            report["pressed"] = status == .success
            report["role"] = role
            report["label"] = String(label.prefix(160))
            report["axStatus"] = status.rawValue
            report["pressedAt"] = ISO8601DateFormatter().string(from: Date())
            let data = try! JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
            print(String(decoding: data, as: UTF8.self))
            exit(status == .success ? 0 : 1)
        }
    }
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.25))
}
report["reason"] = "no pressable element containing the title appeared"
let data = try! JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
exit(1)
