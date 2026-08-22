import AppKit
import Darwin
import Foundation
import ServiceManagement

if let copyIndex = CommandLine.arguments.firstIndex(of: "--copy-liveboard-link") {
    let tail = CommandLine.arguments.suffix(from: copyIndex + 1)
    guard let agentFlag = tail.firstIndex(of: "--agent"),
          tail.index(after: agentFlag) < tail.endIndex
    else {
        fputs("missing --agent for focused Liveboard copy\n", stderr)
        exit(2)
    }
    let agentID = tail[tail.index(after: agentFlag)]
    do {
        try copyFocusedLiveboardLinkAndShowHUD(agentID: agentID)
        exit(0)
    } catch {
        fputs("failed to copy focused Liveboard link\n", stderr)
        exit(1)
    }
}

if CommandLine.arguments.contains("--register-login-item") {
    do {
        try setLaunchAtLogin(true)
        print("enabled")
        exit(0)
    } catch {
        fputs("failed to enable Zodex menu bar launch at login: \(error)\n", stderr)
        exit(1)
    }
}

if CommandLine.arguments.contains("--login-item-status") {
    print(SMAppService.mainApp.status == .enabled ? "enabled" : "disabled")
    exit(0)
}

if CommandLine.arguments.contains("--unregister-login-item") {
    do {
        try setLaunchAtLogin(false)
        print("disabled")
        exit(0)
    } catch {
        fputs("failed to disable Zodex menu bar launch at login: \(error)\n", stderr)
        exit(1)
    }
}

private let singleInstanceLock = SingleInstanceLock()
if singleInstanceLock == nil {
    exit(0)
}

private let app = NSApplication.shared
private let delegate = AppDelegate(zodexURL: siblingZodexURL())
app.setActivationPolicy(.accessory)
app.delegate = delegate
app.run()
