// G16 foundation check against the running Threadspace application, through
// public macOS input and accessibility APIs:
//   1. traffic-light buttons exist on the native window;
//   2. a real mouse drag on the toolbar drag region moves the window;
//   3. pressing the native minimize button minimizes it;
//   4. un-minimizing and activating restores focus to it.
// The window and the cursor are put back where they were. Prints one JSON report.
//
//   swiftc -O tests/native/window-check.swift -o <out>/window-check && <out>/window-check <pid>
import AppKit
import ApplicationServices
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

let arguments = Array(CommandLine.arguments.dropFirst())
guard let pid = arguments.first.flatMap(Int32.init) else { fail("usage: window-check <pid> [restoreWidth restoreHeight]") }
let restoreSize: CGSize? = arguments.count >= 3
    ? Double(arguments[1]).flatMap { w in Double(arguments[2]).map { CGSize(width: w, height: $0) } }
    : nil
guard AXIsProcessTrusted() else { fail("accessibility trust required") }
let app = AXUIElementCreateApplication(pid)

func attribute<T>(_ element: AXUIElement, _ name: String) -> T? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
    return value as? T
}

func point(_ element: AXUIElement) -> CGPoint {
    var result = CGPoint.zero
    if let value: AXValue = attribute(element, kAXPositionAttribute) { AXValueGetValue(value, .cgPoint, &result) }
    return result
}

func size(_ element: AXUIElement) -> CGSize {
    var result = CGSize.zero
    if let value: AXValue = attribute(element, kAXSizeAttribute) { AXValueGetValue(value, .cgSize, &result) }
    return result
}

func setSize(_ element: AXUIElement, _ size: CGSize) {
    var size = size
    if let value = AXValueCreate(.cgSize, &size) {
        AXUIElementSetAttributeValue(element, kAXSizeAttribute as CFString, value)
    }
}

func setPosition(_ element: AXUIElement, _ position: CGPoint) {
    var position = position
    if let value = AXValueCreate(.cgPoint, &position) {
        AXUIElementSetAttributeValue(element, kAXPositionAttribute as CFString, value)
    }
}

func pause(_ seconds: Double) { RunLoop.current.run(until: Date(timeIntervalSinceNow: seconds)) }

/// Every synthetic gesture is a single click (click state 1); otherwise two
/// gestures in quick succession count as a double-click, which the drag region
/// reserves for zoom.
func mouse(_ type: CGEventType, _ at: CGPoint) {
    guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .left) else { return }
    event.setIntegerValueField(.mouseEventClickState, value: 1)
    event.post(tap: .cghidEventTap)
}

guard let windows: [AXUIElement] = attribute(app, kAXWindowsAttribute),
      let window = windows.first(where: { (attribute($0, kAXTitleAttribute) as String?) == "Threadspace" })
else { fail("Threadspace window not found") }

var report: [String: Any] = ["pid": pid]
let buttons: [String: String] = [
    "close": kAXCloseButtonAttribute,
    "minimize": kAXMinimizeButtonAttribute,
    "zoom": kAXZoomButtonAttribute,
    "fullScreen": kAXFullScreenButtonAttribute,
]
var present: [String: Bool] = [:]
for (name, key) in buttons { present[name] = (attribute(window, key) as AXUIElement?) != nil }
report["trafficLights"] = present

let cursorBefore = CGEvent(source: nil)?.location ?? .zero
NSRunningApplication(processIdentifier: pid)?.activate()
pause(0.6)
let origin = point(window)
let frameSize = size(window)
report["boundsBefore"] = ["x": origin.x, "y": origin.y, "width": frameSize.width, "height": frameSize.height]
report["screens"] = NSScreen.screens.map { ["frame": NSStringFromRect($0.frame), "visibleFrame": NSStringFromRect($0.visibleFrame)] }
// A window as tall as the visible screen cannot move down; give it room first.
let testSize = CGSize(width: 980, height: 600)
setSize(window, testSize)
pause(0.5)
let start = point(window)
report["boundsBeforeDrag"] = ["x": start.x, "y": start.y, "width": size(window).width, "height": size(window).height]

let delta = CGPoint(x: 140, y: 90)
func drag(from grab: CGPoint) {
    mouse(.mouseMoved, grab)
    pause(0.15)
    mouse(.leftMouseDown, grab)
    // A drag region starts the native drag loop through an IPC round trip;
    // pointer motion before the loop is live does not move the window.
    pause(0.4)
    for step in 1...20 {
        let t = CGFloat(step) / 20
        mouse(.leftMouseDragged, CGPoint(x: grab.x + delta.x * t, y: grab.y + delta.y * t))
        pause(0.02)
    }
    mouse(.leftMouseUp, CGPoint(x: grab.x + delta.x, y: grab.y + delta.y))
    pause(1.2)
}

// Control: ordinary content (empty lower attention/fleet rail) is not a drag region.
drag(from: CGPoint(x: start.x + 150, y: start.y + 560))
let afterControl = point(window)
report["contentDragDelta"] = ["x": afterControl.x - start.x, "y": afterControl.y - start.y]
report["contentDragDoesNotMoveWindow"] = afterControl == start

// Toolbar drag region: right of the traffic lights and wordmark, 26 pt down.
drag(from: CGPoint(x: afterControl.x + 470, y: afterControl.y + 26))
let dragged = point(window)
report["boundsAfterDrag"] = ["x": dragged.x, "y": dragged.y]
report["dragDelta"] = ["x": dragged.x - afterControl.x, "y": dragged.y - afterControl.y]
report["dragRegionMovesWindow"] = dragged.x - afterControl.x > 20 && dragged.y - afterControl.y > 20

if let minimizeButton: AXUIElement = attribute(window, kAXMinimizeButtonAttribute) {
    AXUIElementPerformAction(minimizeButton, kAXPressAction as CFString)
}
pause(1.2)
report["minimizedAfterPress"] = (attribute(window, kAXMinimizedAttribute) as Bool?) ?? false

AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, kCFBooleanFalse)
pause(1.0)
NSRunningApplication(processIdentifier: pid)?.activate()
AXUIElementPerformAction(window, kAXRaiseAction as CFString)
pause(0.8)
report["minimizedAfterRestore"] = (attribute(window, kAXMinimizedAttribute) as Bool?) ?? true
report["mainAfterRestore"] = (attribute(window, kAXMainAttribute) as Bool?) ?? false
report["focusedAfterRestore"] = (attribute(window, kAXFocusedAttribute) as Bool?) ?? false
report["frontmostAfterRestore"] = NSWorkspace.shared.frontmostApplication?.processIdentifier == pid

setPosition(window, origin)
pause(0.3)
setSize(window, restoreSize ?? frameSize)
pause(0.4)
let final = point(window)
report["boundsRestored"] = ["x": final.x, "y": final.y, "width": size(window).width, "height": size(window).height]
CGWarpMouseCursorPosition(cursorBefore)

let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
print(String(decoding: data, as: UTF8.self))
