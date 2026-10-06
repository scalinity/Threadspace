import AppKit

/// Starts the Rust core once AppKit begins launching and keeps the companion
/// in its run loop for as long as it is enabled. A startup failure exits
/// nonzero, which ServiceManagement treats as a crash and relaunches.
final class AgentAppDelegate: NSObject, NSApplicationDelegate {
    private var powerObservers: [NSObjectProtocol] = []

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
        observePower()
    }

    /// Sleep/wake transitions go to the core, which suspends provider polling
    /// on sleep and revalidates native evidence on wake (SPEC §19.5).
    private func observePower() {
        let center = NSWorkspace.shared.notificationCenter
        powerObservers = [
            center.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { _ in
                deliverToCore(["kind": "Power", "phase": "WILL_SLEEP"])
            },
            center.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { _ in
                deliverToCore(["kind": "Power", "phase": "DID_WAKE"])
            },
        ]
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }
}
