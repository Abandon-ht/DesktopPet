// Standalone P0-05 test fixture; does not move windows or request AX permission.
import AppKit
import ApplicationServices

func log(_ event: String, _ fields: [String: Any] = [:]) {
    var row = fields
    row["event"] = event
    row["time"] = Date().timeIntervalSince1970
    let data = try! JSONSerialization.data(withJSONObject: row, options: [.sortedKeys])
    FileHandle.standardOutput.write(data + Data([10]))
}

func attribute(_ element: AXUIElement, _ name: String) -> (AXError, CFTypeRef?) {
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    return (error, value)
}

func geometry(_ window: AXUIElement) -> [Double]? {
    let (pe, position) = attribute(window, kAXPositionAttribute)
    let (se, size) = attribute(window, kAXSizeAttribute)
    guard pe == .success, se == .success, let position, let size,
          CFGetTypeID(position) == AXValueGetTypeID(),
          CFGetTypeID(size) == AXValueGetTypeID() else {
        log("geometry_unavailable", ["position_error": pe.rawValue, "size_error": se.rawValue])
        return nil
    }
    var point = CGPoint.zero
    var extent = CGSize.zero
    guard AXValueGetValue(position as! AXValue, .cgPoint, &point),
          AXValueGetValue(size as! AXValue, .cgSize, &extent) else { return nil }
    return [point.x, point.y, extent.width, extent.height]
}

final class Observation {
    let window: AXUIElement
    let initial: [Double]
    var moved = 0
    var changedPosition = false
    var detached = false
    init(window: AXUIElement, initial: [Double]) {
        self.window = window
        self.initial = initial
    }
    func receive(_ notification: String) {
        if notification == kAXUIElementDestroyedNotification {
            detached = true
            log("detached", ["reason": "target_destroyed"])
            return
        }
        let frame = geometry(window)
        if notification == kAXMovedNotification {
            moved += 1
            if let frame, frame[0] != initial[0] || frame[1] != initial[1] {
                changedPosition = true
            }
        }
        var fields: [String: Any] = ["notification": notification]
        if let frame { fields["frame"] = frame }
        log("ax_notification", fields)
        if notification == kAXWindowMiniaturizedNotification {
            detached = true
            log("detached", ["reason": "target_minimized"])
        }
    }
}

func run() -> Int32 {
    let args = Array(CommandLine.arguments.dropFirst())
    guard args.count <= 2 else {
        log("usage", ["command": "ax-observer [BUNDLE_ID=com.apple.Terminal] [SECONDS=120]"])
        return 1
    }
    let bundle = args.first ?? "com.apple.Terminal"
    let seconds = args.count == 2 ? Double(args[1]) : 120
    guard let seconds, seconds.isFinite, (1...600).contains(seconds) else {
        log("invalid_duration")
        return 1
    }
    let trusted = AXIsProcessTrusted()
    log("permission", ["ax_trusted": trusted, "prompt_requested": false])
    guard trusted else {
        log("unavailable", ["reason": "AX_permission_required", "fallback": "screen_snap"])
        return 2
    }
    let targets = NSRunningApplication.runningApplications(withBundleIdentifier: bundle)
    guard targets.count == 1, let app = targets.first else {
        log("unavailable", ["reason": "expected_one_running_target", "count": targets.count])
        return 1
    }
    let element = AXUIElementCreateApplication(app.processIdentifier)
    let timeoutError = AXUIElementSetMessagingTimeout(element, 1)
    guard timeoutError == .success else {
        log("unavailable", ["reason": "messaging_timeout", "error": timeoutError.rawValue])
        return 1
    }
    let (error, value) = attribute(element, kAXFocusedWindowAttribute)
    guard error == .success, let value, CFGetTypeID(value) == AXUIElementGetTypeID() else {
        log("unavailable", ["reason": "no_focused_target_window", "error": error.rawValue])
        return 1
    }
    let window = value as! AXUIElement
    guard let initial = geometry(window) else { return 1 }
    let state = Observation(window: window, initial: initial)
    var observer: AXObserver?
    let created = AXObserverCreate(app.processIdentifier, { _, _, notification, context in
        guard let context else { return }
        Unmanaged<Observation>.fromOpaque(context).takeUnretainedValue()
            .receive(notification as String)
    }, &observer)
    guard created == .success, let observer else {
        log("unavailable", ["reason": "observer_create", "error": created.rawValue])
        return 1
    }
    // Keep state alive until all notifications are removed and the run loop source is detached.
    let context = Unmanaged.passRetained(state)
    var subscribed: [String] = []
    let source = AXObserverGetRunLoopSource(observer)
    defer {
        CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, .defaultMode)
        for name in subscribed {
            AXObserverRemoveNotification(observer, window, name as CFString)
        }
        context.release()
    }
    for name in [kAXMovedNotification, kAXResizedNotification,
                 kAXUIElementDestroyedNotification, kAXWindowMiniaturizedNotification] {
        let result = AXObserverAddNotification(observer, window, name as CFString, context.toOpaque())
        log("subscription", ["notification": name, "error": result.rawValue])
        if result == .success { subscribed.append(name) }
    }
    guard subscribed.contains(kAXMovedNotification) else {
        log("unavailable", ["reason": "move_notification_unsupported"])
        return 1
    }
    CFRunLoopAddSource(CFRunLoopGetCurrent(), source, .defaultMode)
    log("observing", ["pid": app.processIdentifier, "bundle_id": bundle,
                       "frame": initial, "seconds": seconds,
                       "coordinate_system": "AX_global_top_left_logical_points"])
    let start = ProcessInfo.processInfo.systemUptime
    var permissionLost = false
    while ProcessInfo.processInfo.systemUptime - start < seconds && !state.detached {
        CFRunLoopRunInMode(.defaultMode, 0.25, false)
        if app.isTerminated {
            log("detached", ["reason": "target_terminated"])
            break
        }
        if !AXIsProcessTrusted() {
            permissionLost = true
            log("detached", ["reason": "permission_revoked", "fallback": "screen_snap"])
            break
        }
    }
    let passed = state.moved > 0 && state.changedPosition && !permissionLost
    log("summary", ["move_notifications": state.moved,
                    "position_changed": state.changedPosition, "movement_check_passed": passed])
    return passed ? 0 : 3
}

exit(run())
