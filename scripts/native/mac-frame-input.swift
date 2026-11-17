// Test-only native macOS input/geometry driver. Build with swiftc on the Mac host.
import AppKit
import ApplicationServices

let pid = pid_t(CommandLine.arguments[1])!
let application = AXUIElementCreateApplication(pid)
var pressed = false
func window() throws -> AXUIElement {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(application, kAXWindowsAttribute as CFString, &value) == .success,
          let windows = value as? [AXUIElement], let first = windows.first else {
        throw NSError(domain: "No AX window for launched PID", code: 1)
    }
    return first
}
func post(_ type: CGEventType, _ point: CGPoint, clicks: Int64 = 1) {
    let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)!
    event.setIntegerValueField(.mouseEventClickState, value: clicks)
    event.post(tap: .cghidEventTap)
}
while let line = readLine() {
    do {
        let command = try JSONSerialization.jsonObject(with: Data(line.utf8)) as! [String: Any]
        var result: [String: Any] = ["ok": true]
        let point = CGPoint(x: command["x"] as? Double ?? 0, y: command["y"] as? Double ?? 0)
        switch command["action"] as! String {
        case "access":
            result["trusted"] = AXIsProcessTrusted() && CGPreflightPostEventAccess()
        case "activate":
            NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateIgnoringOtherApps])
        case "restore":
            let code = try AXUIElementSetAttributeValue(window(), kAXMinimizedAttribute as CFString, kCFBooleanFalse)
            guard code == .success else { throw NSError(domain: "AX restore", code: Int(code.rawValue)) }
            NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateIgnoringOtherApps])
        case "query":
            let list = CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
            let entry = list.first { ($0[kCGWindowOwnerPID as String] as? Int) == Int(pid) && ($0[kCGWindowLayer as String] as? Int) == 0 }
            if let entry = entry, let bounds = entry[kCGWindowBounds as String] as? [String: Any] {
                result["frame"] = [bounds["X"]!, bounds["Y"]!, bounds["Width"]!, bounds["Height"]!]
                result["window_id"] = entry[kCGWindowNumber as String]
            }
        case "move": post(pressed ? .leftMouseDragged : .mouseMoved, point)
        case "down": pressed = true; post(.leftMouseDown, point, clicks: command["clicks"] as? Int64 ?? 1)
        case "up": post(.leftMouseUp, point, clicks: command["clicks"] as? Int64 ?? 1); pressed = false
        case "escape":
            CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: true)?.post(tap: .cghidEventTap)
            CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: false)?.post(tap: .cghidEventTap)
        default: throw NSError(domain: "Unknown native input command", code: 2)
        }
        let output = try JSONSerialization.data(withJSONObject: result)
        print(String(data: output, encoding: .utf8)!)
    } catch {
        let output = try! JSONSerialization.data(withJSONObject: ["error": String(describing: error)])
        print(String(data: output, encoding: .utf8)!)
    }
    fflush(stdout)
}
