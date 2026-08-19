import AppKit
import Darwin
import Foundation
import ServiceManagement

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
