import AppKit

/// Starts the Rust core once AppKit begins launching and keeps the companion
/// in its run loop for as long as it is enabled. A startup failure exits
/// nonzero, which ServiceManagement treats as a crash and relaunches.
final class AgentAppDelegate: NSObject, NSApplicationDelegate {
    func applicationWillFinishLaunching(_ notification: Notification) {
        let bundle = Bundle.main
        let config: [String: String] = [
            "bundleIdentifier": bundle.bundleIdentifier ?? "",
            "bundlePath": bundle.bundlePath,
            "resourcesPath": bundle.resourcePath ?? "",
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: config),
            let json = String(data: data, encoding: .utf8)
        else {
            exit(64)
        }
        let status = json.withCString { ts_core_start($0, agentBridgeCallback) }
        if status != 0 {
            exit(status)
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }
}
