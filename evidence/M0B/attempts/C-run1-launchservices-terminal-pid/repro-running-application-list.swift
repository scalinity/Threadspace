import AppKit
// Query the running-application list for Terminal repeatedly while an
// osascript read-only enumeration runs, on the main queue as the companion does.
let script = CommandLine.arguments[1]
var counts: [Int: Int] = [:]
var odd: [String] = []
for i in 0..<20 {
    let p = Process(); p.executableURL = URL(fileURLWithPath: "/usr/bin/osascript"); p.arguments = [script]
    p.standardOutput = FileHandle.nullDevice
    try! p.run()
    let start = Date()
    while p.isRunning || Date().timeIntervalSince(start) < 0.3 {
        RunLoop.main.run(until: Date().addingTimeInterval(0.005))
        let apps = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.Terminal")
        counts[apps.count, default: 0] += 1
        if apps.count != 1 { odd.append("iter \(i) t=\(String(format: "%.3f", Date().timeIntervalSince(start))) count=\(apps.count) pids=\(apps.map{$0.processIdentifier}) running=\(p.isRunning)") }
    }
}
print("counts:", counts)
odd.prefix(12).forEach { print($0) }
