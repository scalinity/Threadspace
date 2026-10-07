// Reusable native qualification helper (never shipped). Public AppKit,
// Accessibility, CoreGraphics and ImageIO APIs only. Every subcommand prints
// one JSON object on stdout and exits 0 on success, 1 on a failed check,
// 64 on bad usage. The Rust harness (tests/native/harness) drives it with
// bounded argv execution.
//
//   ts-native windows <owner-pid>
//   ts-native ax-window <pid> [title]
//   ts-native window-state <pid> <window-number>       (CoreGraphics facts plus the AX window with exactly that frame)
//   ts-native ax-action-number <pid> <window-number> <fullscreen|exit-fullscreen|raise>
//   ts-native voiceover                                (VoiceOver on/off, its on-screen windows such as its cursor, and their AX text)
//   ts-native ax-find <pid> <label>                    (frame of the first element whose label contains <label>)
//   ts-native ax-action <pid> <minimize|unminimize|fullscreen|exit-fullscreen|raise|press-close|press-minimize|press-zoom|press-fullscreen|set-frame x y w h> [title]
//   ts-native ax-tree <pid> [max-depth]
//   ts-native notification <find|press> <needle> [timeout-seconds]
//   ts-native drag <x1> <y1> <x2> <y2>
//   ts-native click <x> <y>
//   ts-native key <keycode> [shift|cmd|option|control ...]
//   ts-native pixels-stats <png> [x y w h]
//   ts-native pixels-diff <png-a> <png-b> [x y w h]
//   ts-native ax-switch <bundle-id> <label> [press]   (read, or press, a labelled switch/checkbox)
//   ts-native ax-press <bundle-id> <label>            (press any labelled pressable element)
//   ts-native ax-focused <pid>                         (the focused element's role and label)
//   ts-native idle
//   ts-native displays
//   ts-native display-mode-hold <seconds>             (main display at its nearest 1x mode, this process only)
import AppKit
import ApplicationServices
import CoreGraphics
import Foundation
import ImageIO

func emit(_ object: [String: Any], ok: Bool = true) -> Never {
    let data = (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])) ?? Data("{}".utf8)
    print(String(decoding: data, as: UTF8.self))
    exit(ok ? 0 : 1)
}

func usage() -> Never {
    FileHandle.standardError.write(Data("bad usage; see the header of ts-native.swift\n".utf8))
    exit(64)
}

func pause(_ seconds: Double) { RunLoop.current.run(until: Date(timeIntervalSinceNow: seconds)) }

// MARK: Accessibility

func attribute<T>(_ element: AXUIElement, _ name: String) -> T? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value as? T
}

func frame(_ element: AXUIElement) -> CGRect {
    var origin = CGPoint.zero
    var size = CGSize.zero
    if let value: AXValue = attribute(element, kAXPositionAttribute) { AXValueGetValue(value, .cgPoint, &origin) }
    if let value: AXValue = attribute(element, kAXSizeAttribute) { AXValueGetValue(value, .cgSize, &size) }
    return CGRect(origin: origin, size: size)
}

func rect(_ r: CGRect) -> [String: Double] {
    ["x": Double(r.origin.x), "y": Double(r.origin.y), "width": Double(r.size.width), "height": Double(r.size.height)]
}

func window(pid: pid_t, title: String?) -> AXUIElement? {
    let app = AXUIElementCreateApplication(pid)
    let windows: [AXUIElement] = attribute(app, kAXWindowsAttribute) ?? []
    if let title { return windows.first { ((attribute($0, kAXTitleAttribute) as String?) ?? "").contains(title) } }
    return (attribute(app, kAXMainWindowAttribute) as AXUIElement?) ?? windows.first
}

func describe(_ w: AXUIElement) -> [String: Any] {
    var buttons: [String: Bool] = [:]
    for (name, key) in [("close", kAXCloseButtonAttribute), ("minimize", kAXMinimizeButtonAttribute), ("zoom", kAXZoomButtonAttribute), ("fullScreen", kAXFullScreenButtonAttribute)] {
        buttons[name] = (attribute(w, key) as AXUIElement?) != nil
    }
    return [
        "title": (attribute(w, kAXTitleAttribute) as String?) ?? "",
        "frame": rect(frame(w)),
        "minimized": (attribute(w, kAXMinimizedAttribute) as Bool?) ?? false,
        "fullScreen": (attribute(w, "AXFullScreen") as Bool?) ?? false,
        "main": (attribute(w, kAXMainAttribute) as Bool?) ?? false,
        "focused": (attribute(w, kAXFocusedAttribute) as Bool?) ?? false,
        "trafficLights": buttons,
    ]
}

func press(_ w: AXUIElement, _ key: String) -> Bool {
    guard let button: AXUIElement = attribute(w, key) else { return false }
    return AXUIElementPerformAction(button, kAXPressAction as CFString) == .success
}

func setBool(_ w: AXUIElement, _ name: String, _ value: Bool) -> Bool {
    AXUIElementSetAttributeValue(w, name as CFString, value ? kCFBooleanTrue : kCFBooleanFalse) == .success
}

func tree(_ element: AXUIElement, depth: Int, maxDepth: Int, into nodes: inout [[String: Any]]) {
    guard depth <= maxDepth, nodes.count < 4000 else { return }
    let role: String = attribute(element, kAXRoleAttribute) ?? ""
    let label = [kAXTitleAttribute, kAXDescriptionAttribute]
        .compactMap { attribute(element, $0) as String? }
        .filter { !$0.isEmpty }
        .joined(separator: " | ")
    var node: [String: Any] = ["depth": depth, "role": role]
    if !label.isEmpty { node["label"] = String(label.prefix(160)) }
    if role == "AXStaticText" || role == "AXHeading", let value: String = attribute(element, kAXValueAttribute), !value.isEmpty {
        node["value"] = String(value.prefix(200))
    }
    if let subrole: String = attribute(element, kAXSubroleAttribute) { node["subrole"] = subrole }
    if let focused: Bool = attribute(element, kAXFocusedAttribute), focused { node["focused"] = true }
    if let enabled: Bool = attribute(element, kAXEnabledAttribute), !enabled { node["enabled"] = false }
    var actions: CFArray?
    AXUIElementCopyActionNames(element, &actions)
    if let names = actions as? [String], names.contains(kAXPressAction) { node["pressable"] = true }
    nodes.append(node)
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    for child in children { tree(child, depth: depth + 1, maxDepth: maxDepth, into: &nodes) }
}

// MARK: Windows by CoreGraphics number (titles can be rewritten by the app)

func cgWindow(pid: Int, number: Int) -> [String: Any]? {
    let list = (CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]) ?? []
    return list.first { ($0[kCGWindowOwnerPID as String] as? Int) == pid && ($0[kCGWindowNumber as String] as? Int) == number }
}

func cgBounds(_ info: [String: Any]) -> CGRect {
    let b = info[kCGWindowBounds as String] as? [String: Double] ?? [:]
    return CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0)
}

/// The AX windows of `pid` whose frame equals `bounds` (within half a point).
func axWindows(pid: pid_t, matching bounds: CGRect) -> [AXUIElement] {
    let app = AXUIElementCreateApplication(pid)
    let windows: [AXUIElement] = attribute(app, kAXWindowsAttribute) ?? []
    return windows.filter {
        let f = frame($0)
        return abs(f.origin.x - bounds.origin.x) <= 0.5 && abs(f.origin.y - bounds.origin.y) <= 0.5
            && abs(f.size.width - bounds.size.width) <= 0.5 && abs(f.size.height - bounds.size.height) <= 0.5
    }
}

func windowState(pid: pid_t, number: Int) -> [String: Any] {
    var report: [String: Any] = ["pid": pid, "number": number, "atMs": Int(Date().timeIntervalSince1970 * 1000)]
    let main = CGDisplayBounds(CGMainDisplayID())
    report["mainDisplay"] = rect(main)
    report["frontmostPid"] = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
    guard let info = cgWindow(pid: Int(pid), number: number) else {
        report["exists"] = false
        return report
    }
    let bounds = cgBounds(info)
    report["exists"] = true
    report["bounds"] = rect(bounds)
    report["onScreen"] = info[kCGWindowIsOnscreen as String] as? Bool ?? false
    report["layer"] = info[kCGWindowLayer as String] as? Int ?? 0
    report["coversMainDisplay"] = bounds.equalTo(main)
    // A fullscreen window on a panel with a camera housing sits below the
    // screen's top safe-area inset.
    let safeTop = Double(NSScreen.screens.first?.safeAreaInsets.top ?? 0)
    report["safeAreaTop"] = safeTop
    report["fullscreenFrame"] = bounds.equalTo(main)
        || bounds.equalTo(CGRect(x: main.origin.x, y: main.origin.y + safeTop, width: main.width, height: main.height - safeTop))
    let matches = axWindows(pid: pid, matching: bounds)
    report["axMatches"] = matches.count
    if matches.count == 1 { report["ax"] = describe(matches[0]) }
    return report
}

// MARK: Notifications (the Accessibility path VoiceOver uses)

func text(_ element: AXUIElement) -> String {
    [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].compactMap { attribute(element, $0) as String? }.joined(separator: " ")
}

func subtreeContains(_ element: AXUIElement, _ needle: String, depth: Int) -> Bool {
    guard depth < 14 else { return false }
    if text(element).contains(needle) { return true }
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    return children.contains { subtreeContains($0, needle, depth: depth + 1) }
}

func findPressable(_ element: AXUIElement, _ needle: String, depth: Int = 0) -> (AXUIElement, String)? {
    guard depth < 14 else { return nil }
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    for child in children where subtreeContains(child, needle, depth: depth + 1) {
        var actions: CFArray?
        AXUIElementCopyActionNames(child, &actions)
        if (actions as? [String] ?? []).contains(kAXPressAction) { return (child, text(child)) }
        if let deeper = findPressable(child, needle, depth: depth + 1) { return deeper }
    }
    return nil
}

// MARK: Labelled controls in another app (System Settings switches)

func isSwitch(_ element: AXUIElement) -> Bool {
    let role: String = attribute(element, kAXRoleAttribute) ?? ""
    return role == "AXCheckBox" || role == "AXSwitch" || role == "AXToggle"
}

func ownLabel(_ element: AXUIElement) -> String {
    [kAXTitleAttribute, kAXDescriptionAttribute, "AXLabel"]
        .compactMap { attribute(element, $0) as String? }
        .joined(separator: " ")
}

/// A switch labelled `needle`: by its own title/description, or (a System
/// Settings row) an unlabelled switch after a run of static texts, one of
/// which reads exactly `needle` (a row's title can be followed by its
/// description and hint before the switch).
func findLabelled(_ element: AXUIElement, _ needle: String, depth: Int = 0) -> AXUIElement? {
    guard depth < 30 else { return nil }
    if isSwitch(element) && ownLabel(element).localizedCaseInsensitiveContains(needle) { return element }
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    var rowTexts: [String] = []
    for child in children {
        if isSwitch(child), ownLabel(child).trimmingCharacters(in: .whitespaces).isEmpty,
           rowTexts.contains(where: { $0.caseInsensitiveCompare(needle) == .orderedSame }) {
            return child
        }
        let role: String = attribute(child, kAXRoleAttribute) ?? ""
        if role == "AXStaticText" {
            if let text: String = attribute(child, kAXValueAttribute) { rowTexts.append(text) }
        } else {
            rowTexts = []
        }
        if let hit = findLabelled(child, needle, depth: depth + 1) { return hit }
    }
    return nil
}

func findPressableLabelled(_ element: AXUIElement, _ needle: String, depth: Int = 0) -> AXUIElement? {
    guard depth < 30 else { return nil }
    let label = [kAXTitleAttribute, kAXDescriptionAttribute, "AXLabel"]
        .compactMap { attribute(element, $0) as String? }
        .joined(separator: " ")
    if label.localizedCaseInsensitiveContains(needle) {
        var actions: CFArray?
        AXUIElementCopyActionNames(element, &actions)
        if (actions as? [String] ?? []).contains(kAXPressAction) { return element }
    }
    let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
    for child in children {
        if let hit = findPressableLabelled(child, needle, depth: depth + 1) { return hit }
    }
    return nil
}

func switchValue(_ element: AXUIElement) -> Int? {
    if let number: NSNumber = attribute(element, kAXValueAttribute) { return number.intValue }
    return nil
}

// MARK: Synthetic input

func mouse(_ type: CGEventType, _ at: CGPoint) {
    guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .left) else { return }
    event.setIntegerValueField(.mouseEventClickState, value: 1)
    event.post(tap: .cghidEventTap)
}

// MARK: Pixels

func load(_ path: String) -> CGImage? {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil) else { return nil }
    return CGImageSourceCreateImageAtIndex(source, 0, nil)
}

/// RGBA8 pixels of `image`, cropped to `crop` (in image pixels) when given.
func rgba(_ image: CGImage, _ crop: CGRect?) -> (bytes: [UInt8], width: Int, height: Int)? {
    let source = crop.flatMap { image.cropping(to: $0) } ?? image
    let width = source.width
    let height = source.height
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    guard let context = CGContext(data: &bytes, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                  space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    context.draw(source, in: CGRect(x: 0, y: 0, width: width, height: height))
    return (bytes, width, height)
}

func cropArgument(_ args: ArraySlice<String>) -> CGRect? {
    let values = args.compactMap(Double.init)
    guard values.count == 4 else { return nil }
    return CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
}

// MARK: Dispatch

let args = Array(CommandLine.arguments.dropFirst())
guard let command = args.first else { usage() }

switch command {
case "windows":
    guard args.count >= 2, let pid = Int(args[1]) else { usage() }
    let list = (CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]) ?? []
    let windows: [[String: Any]] = list.compactMap { info in
        guard (info[kCGWindowOwnerPID as String] as? Int) == pid else { return nil }
        let bounds = info[kCGWindowBounds as String] as? [String: Double] ?? [:]
        return [
            "id": info[kCGWindowNumber as String] as? Int ?? 0,
            "layer": info[kCGWindowLayer as String] as? Int ?? 0,
            "onScreen": info[kCGWindowIsOnscreen as String] as? Bool ?? false,
            "name": info[kCGWindowName as String] as? String ?? "",
            "x": bounds["X"] ?? 0, "y": bounds["Y"] ?? 0, "width": bounds["Width"] ?? 0, "height": bounds["Height"] ?? 0,
        ]
    }
    emit(["pid": pid, "windows": windows])

case "ax-window":
    guard args.count >= 2, let pid = pid_t(args[1]), AXIsProcessTrusted() else { usage() }
    guard let w = window(pid: pid, title: args.count > 2 ? args[2] : nil) else { emit(["pid": pid, "found": false], ok: false) }
    var report = describe(w)
    report["pid"] = pid
    report["found"] = true
    report["frontmostPid"] = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
    emit(report)

case "ax-action":
    guard args.count >= 3, let pid = pid_t(args[1]), AXIsProcessTrusted() else { usage() }
    let action = args[2]
    let isFrame = action == "set-frame"
    let title = isFrame ? (args.count > 7 ? args[7] : nil) : (args.count > 3 ? args[3] : nil)
    guard let w = window(pid: pid, title: title) else { emit(["pid": pid, "found": false], ok: false) }
    let before = describe(w)
    var done = false
    switch action {
    case "minimize": done = setBool(w, kAXMinimizedAttribute, true)
    case "unminimize": done = setBool(w, kAXMinimizedAttribute, false)
    case "fullscreen": done = setBool(w, "AXFullScreen", true)
    case "exit-fullscreen": done = setBool(w, "AXFullScreen", false)
    case "raise":
        NSRunningApplication(processIdentifier: pid)?.activate()
        done = AXUIElementPerformAction(w, kAXRaiseAction as CFString) == .success
    case "press-close": done = press(w, kAXCloseButtonAttribute)
    case "press-minimize": done = press(w, kAXMinimizeButtonAttribute)
    case "press-zoom": done = press(w, kAXZoomButtonAttribute)
    case "press-fullscreen": done = press(w, kAXFullScreenButtonAttribute)
    case "set-frame":
        let values = args.dropFirst(3).prefix(4).compactMap(Double.init)
        guard values.count == 4 else { usage() }
        var origin = CGPoint(x: values[0], y: values[1])
        var size = CGSize(width: values[2], height: values[3])
        if let position = AXValueCreate(.cgPoint, &origin), let extent = AXValueCreate(.cgSize, &size) {
            let a = AXUIElementSetAttributeValue(w, kAXSizeAttribute as CFString, extent)
            let b = AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, position)
            done = a == .success && b == .success
        }
    default: usage()
    }
    pause(action.contains("fullscreen") ? 2.5 : 1.0)
    let after = window(pid: pid, title: title).map(describe) ?? ["gone": true]
    emit(["pid": pid, "action": action, "performed": done, "before": before, "after": after], ok: done)

case "window-state":
    guard args.count >= 3, let pid = pid_t(args[1]), let number = Int(args[2]), AXIsProcessTrusted() else { usage() }
    let report = windowState(pid: pid, number: number)
    emit(report, ok: report["exists"] as? Bool ?? false)

case "ax-action-number":
    // Resolves the AX window by its CoreGraphics number's exact frame; an
    // absent or ambiguous match acts on nothing.
    guard args.count >= 4, let pid = pid_t(args[1]), let number = Int(args[2]), AXIsProcessTrusted() else { usage() }
    let action = args[3]
    let before = windowState(pid: pid, number: number)
    guard let info = cgWindow(pid: Int(pid), number: number) else { emit(["before": before, "performed": false, "reason": "no such window"], ok: false) }
    let matches = axWindows(pid: pid, matching: cgBounds(info))
    guard matches.count == 1 else { emit(["before": before, "performed": false, "reason": "\(matches.count) AX windows share that frame"], ok: false) }
    let w = matches[0]
    var done = false
    switch action {
    case "fullscreen": done = setBool(w, "AXFullScreen", true)
    case "exit-fullscreen": done = setBool(w, "AXFullScreen", false)
    case "raise":
        NSRunningApplication(processIdentifier: pid)?.activate()
        done = AXUIElementPerformAction(w, kAXRaiseAction as CFString) == .success
    default: usage()
    }
    let actedAtMs = Int(Date().timeIntervalSince1970 * 1000)
    emit(["action": action, "performed": done, "actedAtMs": actedAtMs, "before": before], ok: done)

case "voiceover":
    var report: [String: Any] = ["enabled": NSWorkspace.shared.isVoiceOverEnabled, "atMs": Int(Date().timeIntervalSince1970 * 1000)]
    let running = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.VoiceOver")
    report["pids"] = running.map { Int($0.processIdentifier) }
    var windows: [[String: Any]] = []
    if AXIsProcessTrusted() {
        for app in running {
            let element = AXUIElementCreateApplication(app.processIdentifier)
            let list: [AXUIElement] = attribute(element, kAXWindowsAttribute) ?? []
            for w in list {
                var nodes: [[String: Any]] = []
                tree(w, depth: 0, maxDepth: 6, into: &nodes)
                let texts = nodes.compactMap { ($0["value"] as? String) ?? ($0["label"] as? String) }.filter { !$0.isEmpty }
                windows.append(["title": (attribute(w, kAXTitleAttribute) as String?) ?? "", "frame": rect(frame(w)), "texts": Array(texts.prefix(12))])
            }
        }
    }
    report["windows"] = windows
    // VoiceOver draws its cursor (and caption panel) as its own windows.
    let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]) ?? []
    let pids = Set(running.map { Int($0.processIdentifier) })
    report["screenWindows"] = list.compactMap { info -> [String: Any]? in
        guard let owner = info[kCGWindowOwnerPID as String] as? Int, pids.contains(owner) else { return nil }
        return ["layer": info[kCGWindowLayer as String] as? Int ?? 0, "bounds": rect(cgBounds(info)), "name": info[kCGWindowName as String] as? String ?? ""]
    }
    emit(report)

case "ax-find":
    guard args.count >= 3, let pid = pid_t(args[1]), AXIsProcessTrusted() else { usage() }
    func search(_ element: AXUIElement, depth: Int) -> AXUIElement? {
        guard depth < 40 else { return nil }
        let label = [kAXTitleAttribute, kAXDescriptionAttribute, "AXLabel"].compactMap { attribute(element, $0) as String? }.joined(separator: " ")
        if label.contains(args[2]) { return element }
        let children: [AXUIElement] = attribute(element, kAXChildrenAttribute) ?? []
        for child in children { if let hit = search(child, depth: depth + 1) { return hit } }
        return nil
    }
    guard let hit = search(AXUIElementCreateApplication(pid), depth: 0) else { emit(["pid": pid, "label": args[2], "found": false], ok: false) }
    emit(["pid": pid, "label": args[2], "found": true, "role": (attribute(hit, kAXRoleAttribute) as String?) ?? "", "frame": rect(frame(hit)),
          "pressed": (attribute(hit, kAXValueAttribute) as NSNumber?)?.intValue ?? -1])

case "ax-tree":
    guard args.count >= 2, let pid = pid_t(args[1]), AXIsProcessTrusted() else { usage() }
    var nodes: [[String: Any]] = []
    tree(AXUIElementCreateApplication(pid), depth: 0, maxDepth: args.count > 2 ? Int(args[2]) ?? 30 : 30, into: &nodes)
    emit(["pid": pid, "nodes": nodes])

case "notification":
    guard args.count >= 3, AXIsProcessTrusted() else { usage() }
    let mode = args[1]
    let needle = args[2]
    let timeout = args.count > 3 ? Double(args[3]) ?? 10 : 10
    let deadline = Date(timeIntervalSinceNow: timeout)
    while Date() < deadline {
        for app in NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.notificationcenterui") {
            if let (element, label) = findPressable(AXUIElementCreateApplication(app.processIdentifier), needle) {
                var report: [String: Any] = ["mode": mode, "needle": needle, "found": true, "label": String(label.prefix(160))]
                if mode == "press" {
                    let status = AXUIElementPerformAction(element, kAXPressAction as CFString)
                    report["pressed"] = status == .success
                    report["axStatus"] = status.rawValue
                    report["pressedAtMs"] = Int(Date().timeIntervalSince1970 * 1000)
                    emit(report, ok: status == .success)
                }
                emit(report)
            }
        }
        pause(0.25)
    }
    emit(["mode": mode, "needle": needle, "found": false], ok: false)

case "drag":
    let v = args.dropFirst().compactMap(Double.init)
    guard v.count == 4 else { usage() }
    let from = CGPoint(x: v[0], y: v[1]), to = CGPoint(x: v[2], y: v[3])
    let cursorBefore = CGEvent(source: nil)?.location ?? .zero
    mouse(.mouseMoved, from); pause(0.15)
    mouse(.leftMouseDown, from)
    // A web drag region starts the native drag loop after an IPC round trip.
    pause(0.4)
    for step in 1...20 {
        let t = CGFloat(step) / 20
        mouse(.leftMouseDragged, CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t))
        pause(0.02)
    }
    mouse(.leftMouseUp, to); pause(0.8)
    mouse(.mouseMoved, cursorBefore)
    emit(["from": ["x": v[0], "y": v[1]], "to": ["x": v[2], "y": v[3]]])

case "click":
    let v = args.dropFirst().compactMap(Double.init)
    guard v.count == 2 else { usage() }
    let at = CGPoint(x: v[0], y: v[1])
    mouse(.mouseMoved, at); pause(0.1)
    mouse(.leftMouseDown, at); pause(0.05)
    mouse(.leftMouseUp, at); pause(0.3)
    emit(["x": v[0], "y": v[1]])

case "key":
    guard args.count >= 2, let code = CGKeyCode(args[1]) else { usage() }
    var flags: CGEventFlags = []
    for name in args.dropFirst(2) {
        switch name {
        case "shift": flags.insert(.maskShift)
        case "cmd": flags.insert(.maskCommand)
        case "option": flags.insert(.maskAlternate)
        case "control": flags.insert(.maskControl)
        default: usage()
        }
    }
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)
        event?.flags = flags
        event?.post(tap: .cghidEventTap)
        pause(0.05)
    }
    emit(["keycode": Int(code), "flags": args.dropFirst(2).map { $0 }])

case "pixels-stats":
    guard args.count >= 2, let image = load(args[1]), let px = rgba(image, cropArgument(args.dropFirst(2))) else { usage() }
    var sum = 0.0, sumSquares = 0.0, distinct = Set<UInt32>()
    let count = px.width * px.height
    for i in 0..<count {
        let r = Double(px.bytes[i * 4]), g = Double(px.bytes[i * 4 + 1]), b = Double(px.bytes[i * 4 + 2])
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b
        sum += luma; sumSquares += luma * luma
        if distinct.count < 5000 && i % 7 == 0 {
            distinct.insert(UInt32(px.bytes[i * 4]) << 16 | UInt32(px.bytes[i * 4 + 1]) << 8 | UInt32(px.bytes[i * 4 + 2]))
        }
    }
    let mean = count > 0 ? sum / Double(count) : 0
    emit(["width": px.width, "height": px.height, "meanLuma": mean, "lumaStdDev": count > 0 ? (sumSquares / Double(count) - mean * mean).squareRoot() : 0, "sampledDistinctColors": distinct.count])

case "pixels-diff":
    guard args.count >= 3, let a = load(args[1]), let b = load(args[2]) else { usage() }
    let crop = cropArgument(args.dropFirst(3))
    guard let pa = rgba(a, crop), let pb = rgba(b, crop), pa.width == pb.width, pa.height == pb.height else {
        emit(["comparable": false], ok: false)
    }
    var total = 0.0, changed = 0
    let count = pa.width * pa.height
    for i in 0..<count {
        var delta = 0
        for c in 0..<3 { delta += abs(Int(pa.bytes[i * 4 + c]) - Int(pb.bytes[i * 4 + c])) }
        total += Double(delta) / 3.0
        if delta > 24 { changed += 1 }
    }
    emit(["comparable": true, "width": pa.width, "height": pa.height, "meanAbsDiff": count > 0 ? total / Double(count) : 0,
          "changedFraction": count > 0 ? Double(changed) / Double(count) : 0])

case "ax-switch":
    guard args.count >= 3, AXIsProcessTrusted() else { usage() }
    let bundle = args[1], needle = args[2], doPress = args.count > 3 && args[3] == "press"
    var element: AXUIElement?
    let deadline = Date(timeIntervalSinceNow: 15)
    while element == nil && Date() < deadline {
        for app in NSRunningApplication.runningApplications(withBundleIdentifier: bundle) {
            element = findLabelled(AXUIElementCreateApplication(app.processIdentifier), needle)
            if element != nil { break }
        }
        if element == nil { pause(0.5) }
    }
    guard let control = element else { emit(["bundle": bundle, "label": needle, "found": false], ok: false) }
    let before = switchValue(control)
    var pressed = false
    if doPress {
        pressed = AXUIElementPerformAction(control, kAXPressAction as CFString) == .success
        pause(1.0)
    }
    emit(["bundle": bundle, "label": needle, "found": true, "valueBefore": before ?? -1, "pressed": pressed, "valueAfter": switchValue(control) ?? -1])

case "ax-press":
    guard args.count >= 3, AXIsProcessTrusted() else { usage() }
    let bundle = args[1], needle = args[2]
    var element: AXUIElement?
    let deadline = Date(timeIntervalSinceNow: 15)
    while element == nil && Date() < deadline {
        for app in NSRunningApplication.runningApplications(withBundleIdentifier: bundle) {
            element = findPressableLabelled(AXUIElementCreateApplication(app.processIdentifier), needle)
            if element != nil { break }
        }
        if element == nil { pause(0.5) }
    }
    guard let control = element else { emit(["bundle": bundle, "label": needle, "found": false], ok: false) }
    let ok = AXUIElementPerformAction(control, kAXPressAction as CFString) == .success
    pause(1.0)
    emit(["bundle": bundle, "label": needle, "found": true, "pressed": ok], ok: ok)

case "ax-focused":
    guard args.count >= 2, let pid = pid_t(args[1]), AXIsProcessTrusted() else { usage() }
    let app = AXUIElementCreateApplication(pid)
    guard let focused: AXUIElement = attribute(app, kAXFocusedUIElementAttribute) else { emit(["pid": pid, "found": false], ok: false) }
    let label = [kAXTitleAttribute, kAXDescriptionAttribute, "AXLabel"]
        .compactMap { attribute(focused, $0) as String? }
        .filter { !$0.isEmpty }
        .joined(separator: " | ")
    emit(["pid": pid, "found": true, "role": (attribute(focused, kAXRoleAttribute) as String?) ?? "", "label": label])

case "idle":
    var iterator: io_iterator_t = 0
    var idleSeconds = -1.0
    if IOServiceGetMatchingServices(kIOMainPortDefault, IOServiceMatching("IOHIDSystem"), &iterator) == KERN_SUCCESS {
        let entry = IOIteratorNext(iterator)
        if entry != 0, let value = IORegistryEntryCreateCFProperty(entry, "HIDIdleTime" as CFString, kCFAllocatorDefault, 0)?.takeRetainedValue() as? NSNumber {
            idleSeconds = value.doubleValue / 1_000_000_000
        }
        IOObjectRelease(entry)
        IOObjectRelease(iterator)
    }
    emit(["idleSeconds": idleSeconds], ok: idleSeconds >= 0)

case "displays":
    let screens: [[String: Any]] = NSScreen.screens.map { screen in
        let number = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value ?? 0
        return [
            "displayId": number,
            "frame": rect(screen.frame),
            "visibleFrame": rect(screen.visibleFrame),
            "backingScale": Double(screen.backingScaleFactor),
            "builtin": CGDisplayIsBuiltin(number) != 0,
            "main": CGDisplayIsMain(number) != 0,
        ]
    }
    emit(["screens": screens])

case "display-mode-hold":
    // A device-pixel-ratio change on a Retina-only Mac: the usable 1x mode
    // nearest the current point size, applied for this process only, so
    // macOS restores the previous mode when the process exits, however it
    // exits. Prints one JSON line, then holds the mode for <seconds>.
    guard args.count >= 2, let seconds = Double(args[1]) else { usage() }
    let display = CGMainDisplayID()
    guard let current = CGDisplayCopyDisplayMode(display) else { emit(["applied": false, "reason": "no current mode"], ok: false) }
    let options = [kCGDisplayShowDuplicateLowResolutionModes: kCFBooleanTrue] as CFDictionary
    let oneX = ((CGDisplayCopyAllDisplayModes(display, options) as? [CGDisplayMode]) ?? [])
        .filter { $0.isUsableForDesktopGUI() && $0.pixelWidth == $0.width }
    let distance = { (mode: CGDisplayMode) -> (Int, Double) in
        (abs(mode.width - current.width) + abs(mode.height - current.height), abs(mode.refreshRate - current.refreshRate))
    }
    guard let target = oneX.min(by: { distance($0) < distance($1) }) else { emit(["applied": false, "reason": "no usable 1x mode"], ok: false) }
    let describe = { (mode: CGDisplayMode) -> [String: Any] in
        ["width": mode.width, "height": mode.height, "pixelWidth": mode.pixelWidth, "pixelHeight": mode.pixelHeight, "refreshRate": mode.refreshRate]
    }
    var config: CGDisplayConfigRef?
    CGBeginDisplayConfiguration(&config)
    CGConfigureDisplayWithDisplayMode(config, display, target, nil)
    let status = CGCompleteDisplayConfiguration(config, .forAppOnly)
    let report: [String: Any] = ["applied": status == .success, "status": Int(status.rawValue), "from": describe(current), "to": describe(target), "holdSeconds": seconds]
    let line = (try? JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])) ?? Data("{}".utf8)
    print(String(decoding: line, as: UTF8.self))
    fflush(stdout)
    guard status == .success else { exit(1) }
    pause(seconds)
    exit(0)

default:
    usage()
}
