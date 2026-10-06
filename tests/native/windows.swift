// Prints on-screen windows whose owner name contains the given text, one JSON
// object per line: window id, owner, owner PID, layer and bounds. Used to
// capture and drive only Threadspace's own windows during qualification.
//
//   swiftc -O tests/native/windows.swift -o <out>/windows && <out>/windows "Threadspace"
import CoreGraphics
import Foundation

let needle = CommandLine.arguments.dropFirst().first ?? "Threadspace"
let options: CGWindowListOption = [.optionAll, .excludeDesktopElements]
guard let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else { exit(1) }
for window in list {
    let owner = window[kCGWindowOwnerName as String] as? String ?? ""
    guard owner.contains(needle) else { continue }
    let bounds = window[kCGWindowBounds as String] as? [String: Double] ?? [:]
    let record: [String: Any] = [
        "id": window[kCGWindowNumber as String] as? Int ?? 0,
        "owner": owner,
        "pid": window[kCGWindowOwnerPID as String] as? Int ?? 0,
        "layer": window[kCGWindowLayer as String] as? Int ?? 0,
        "onScreen": window[kCGWindowIsOnscreen as String] as? Bool ?? false,
        "name": window[kCGWindowName as String] as? String ?? "",
        "x": bounds["X"] ?? 0, "y": bounds["Y"] ?? 0,
        "width": bounds["Width"] ?? 0, "height": bounds["Height"] ?? 0,
    ]
    if let data = try? JSONSerialization.data(withJSONObject: record, options: [.sortedKeys]),
       let line = String(data: data, encoding: .utf8) {
        print(line)
    }
}
