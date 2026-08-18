import AppKit
import Darwin
import Foundation
import ServiceManagement

private struct CommandResult {
    let exitCode: Int32
    let output: String
}

private struct LocalStatus: Decodable {
    struct Runtime: Decodable {
        let runtime_id: String
    }

    let state: String
    let runtime: Runtime?
}

private struct UpgradeEvent: Decodable {
    let schema_version: Int
    let event: String
    let current_version: String
    let target_version: String?
    let update_available: Bool?
    let direction: String?
    let local_state: String?
    let local_blocks_upgrade: Bool?
    let code: String?
    let message: String
}

private final class SingleInstanceLock {
    private let descriptor: Int32

    init?() {
        let path = "/tmp/zodex-menubar-\(getuid()).lock"
        let descriptor = path.withCString {
            Darwin.open($0, O_CREAT | O_RDWR, mode_t(S_IRUSR | S_IWUSR))
        }
        guard descriptor >= 0 else {
            return nil
        }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            Darwin.close(descriptor)
            return nil
        }
        self.descriptor = descriptor
    }

    deinit {
        flock(descriptor, LOCK_UN)
        Darwin.close(descriptor)
    }
}

private final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private static let startFolderKey = "startFolder"

    private let zodexURL: URL
    private let defaults = UserDefaults.standard
    private let menu = NSMenu()
    private var statusItem: NSStatusItem!

    private lazy var startItem = menuItem(
        "Start Zodex",
        action: #selector(startZodex),
        symbol: "play.fill",
        symbolDescription: "Start"
    )
    private lazy var stopItem = menuItem(
        "Stop Zodex",
        action: #selector(stopZodex),
        symbol: "stop.fill",
        symbolDescription: "Stop"
    )
    private lazy var liveboardItem = menuItem(
        "Open Liveboard",
        action: #selector(openLiveboard),
        symbol: "arrow.up.right",
        symbolDescription: "Liveboard"
    )
    private lazy var startFolderItem: NSMenuItem = {
        let item = NSMenuItem(title: "Start Folder: Not set", action: nil, keyEquivalent: "")
        item.image = menuSymbol("folder.fill", description: "Start folder")
        item.isEnabled = false
        return item
    }()
    private lazy var changeFolderItem = menuItem(
        "Set Start Folder…",
        action: #selector(changeStartFolder),
        symbol: "folder.fill",
        symbolDescription: "Start folder"
    )
    private lazy var launchAtLoginItem = menuItem(
        "Launch at Login",
        action: #selector(toggleLaunchAtLogin)
    )
    private lazy var updateItem = menuItem(
        "Check for Updates…",
        action: #selector(updateZodex)
    )
    private lazy var versionItem: NSMenuItem = {
        let version = Bundle.main.object(forInfoDictionaryKey: "ZodexVersion") as? String
        let item = NSMenuItem(title: "Zodex v\(version ?? "unknown")", action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }()
    private lazy var quitItem = menuItem("Quit", action: #selector(quit))

    private var commandInFlight = false
    private var currentRuntimeID: String?
    private var availableUpdateVersion: String?
    private var upgradeCheckInFlight = false
    private var upgradeInFlight = false

    private var liveboardProcess: Process?
    private var liveboardURL: URL?
    private var liveboardRuntimeID: String?
    private var pendingLiveboardOpen = false

    init(zodexURL: URL) {
        self.zodexURL = zodexURL
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            button.image = nil
            button.title = "Z"
            button.font = NSFont.systemFont(ofSize: NSFont.systemFontSize, weight: .semibold)
            button.toolTip = "Zodex"
        }

        menu.delegate = self
        menu.autoenablesItems = false
        menu.addItem(startItem)
        menu.addItem(stopItem)
        menu.addItem(liveboardItem)
        menu.addItem(.separator())
        menu.addItem(startFolderItem)
        menu.addItem(changeFolderItem)
        menu.addItem(.separator())
        menu.addItem(updateItem)
        menu.addItem(launchAtLoginItem)
        menu.addItem(versionItem)
        menu.addItem(.separator())
        menu.addItem(quitItem)
        statusItem.menu = menu

        updateStartFolderItems()
        updateLaunchAtLoginItem()
        disableRuntimeActions()
    }

    func applicationWillTerminate(_ notification: Notification) {
        stopLiveboardHost()
    }

    func menuWillOpen(_ menu: NSMenu) {
        updateStartFolderItems()
        updateLaunchAtLoginItem()
        refreshStatus()
        refreshUpgradeStatus(force: false, reportUpToDate: false)
    }

    private func menuItem(
        _ title: String,
        action: Selector,
        symbol: String? = nil,
        symbolDescription: String? = nil
    ) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        if let symbol {
            item.image = menuSymbol(symbol, description: symbolDescription ?? title)
        }
        return item
    }

    private func menuSymbol(_ name: String, description: String) -> NSImage? {
        let image = NSImage(systemSymbolName: name, accessibilityDescription: description)
        image?.isTemplate = true
        return image
    }

    private var configuredStartFolder: String? {
        defaults.string(forKey: Self.startFolderKey)
    }

    private var validStartFolder: String? {
        guard let path = configuredStartFolder else {
            return nil
        }
        var isDirectory = ObjCBool(false)
        guard FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory), isDirectory.boolValue else {
            return nil
        }
        return path
    }

    private func updateStartFolderItems() {
        if let path = configuredStartFolder {
            let availability = validStartFolder == nil ? " (Unavailable)" : ""
            startFolderItem.title = "Start Folder: \(displayPath(path))\(availability)"
            changeFolderItem.title = "Change Start Folder…"
        } else {
            startFolderItem.title = "Start Folder: Not set"
            changeFolderItem.title = "Set Start Folder…"
        }
    }

    private func updateLaunchAtLoginItem() {
        launchAtLoginItem.state = SMAppService.mainApp.status == .enabled ? .on : .off
    }

    private func displayPath(_ path: String) -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        if path == home {
            return "~"
        }
        let homePrefix = home + "/"
        if path.hasPrefix(homePrefix) {
            return "~/" + path.dropFirst(homePrefix.count)
        }
        return path
    }

    private func refreshStatus() {
        disableRuntimeActions()
        let result = runZodexSync(["local", "status", "--json"])
        guard !commandInFlight else {
            return
        }
        guard result.exitCode == 0,
              let data = result.output.data(using: .utf8),
              let status = try? JSONDecoder().decode(LocalStatus.self, from: data)
        else {
            enableFallbackActions()
            return
        }
        apply(status: status)
    }

    private func apply(status: LocalStatus) {
        currentRuntimeID = status.runtime?.runtime_id

        if liveboardProcess?.isRunning == true,
           (status.state != "running" || liveboardRuntimeID != currentRuntimeID)
        {
            stopLiveboardHost()
        }

        switch status.state {
        case "running":
            startItem.title = "Zodex is Running"
            startItem.isEnabled = false
            stopItem.isEnabled = true
            liveboardItem.isEnabled = true
        case "stale":
            startItem.title = "Start Zodex"
            startItem.isEnabled = false
            stopItem.isEnabled = true
            liveboardItem.isEnabled = false
        case "stopped":
            startItem.title = "Start Zodex"
            startItem.isEnabled = validStartFolder != nil
            stopItem.isEnabled = false
            liveboardItem.isEnabled = false
        case "unconfigured":
            startItem.title = "Start Zodex"
            disableRuntimeActions()
        default:
            startItem.title = "Start Zodex"
            enableFallbackActions()
        }
    }

    private func disableRuntimeActions() {
        startItem.isEnabled = false
        stopItem.isEnabled = false
        liveboardItem.isEnabled = false
    }

    private func enableFallbackActions() {
        startItem.isEnabled = validStartFolder != nil
        stopItem.isEnabled = true
        liveboardItem.isEnabled = true
    }

    @objc private func startZodex() {
        guard let path = validStartFolder else {
            showError("Start folder unavailable", detail: "Choose an existing start folder before starting Zodex.")
            return
        }
        commandInFlight = true
        startItem.title = "Starting Zodex…"
        disableRuntimeActions()
        runZodexStart(path: path) { [weak self] result in
            guard let self else { return }
            self.commandInFlight = false
            if result.exitCode != 0 {
                self.startItem.title = "Start Zodex"
                self.showCommandError("Start Zodex failed", result: result)
            }
            self.refreshStatus()
        }
    }

    @objc private func stopZodex() {
        commandInFlight = true
        startItem.title = "Stopping Zodex…"
        disableRuntimeActions()
        runZodex(["local", "stop"]) { [weak self] result in
            guard let self else { return }
            self.commandInFlight = false
            if result.exitCode == 0 {
                self.stopLiveboardHost()
            } else {
                self.startItem.title = "Zodex is Running"
                self.showCommandError("Stop Zodex failed", result: result)
            }
            self.refreshStatus()
        }
    }

    @objc private func openLiveboard() {
        if liveboardProcess?.isRunning == true {
            if let liveboardURL {
                NSWorkspace.shared.open(liveboardURL)
            } else {
                pendingLiveboardOpen = true
            }
            return
        }

        let process = Process()
        let outputPipe = Pipe()
        process.executableURL = zodexURL
        process.arguments = ["local", "watch", "--no-open"]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = outputPipe
        process.standardError = outputPipe

        liveboardProcess = process
        liveboardURL = nil
        liveboardRuntimeID = currentRuntimeID
        pendingLiveboardOpen = true

        do {
            try process.run()
        } catch {
            liveboardProcess = nil
            liveboardRuntimeID = nil
            pendingLiveboardOpen = false
            showError("Open Liveboard failed", detail: error.localizedDescription)
            return
        }

        let handle = outputPipe.fileHandleForReading
        DispatchQueue.global(qos: .utility).async { [weak self, process] in
            var output = ""
            var discoveredURL: URL?

            while true {
                let data = handle.availableData
                if data.isEmpty {
                    break
                }
                output += String(decoding: data, as: UTF8.self)

                if discoveredURL == nil, let url = Self.liveboardURL(in: output) {
                    discoveredURL = url
                    DispatchQueue.main.async { [weak self, process] in
                        guard let self, self.liveboardProcess === process else {
                            return
                        }
                        self.liveboardURL = url
                        if self.pendingLiveboardOpen {
                            self.pendingLiveboardOpen = false
                            NSWorkspace.shared.open(url)
                        }
                    }
                }
            }

            process.waitUntilExit()
            let exitCode = process.terminationStatus
            DispatchQueue.main.async { [weak self, process] in
                guard let self, self.liveboardProcess === process else {
                    return
                }
                self.liveboardProcess = nil
                self.liveboardURL = nil
                self.liveboardRuntimeID = nil
                self.pendingLiveboardOpen = false
                if exitCode != 0 {
                    let detail = output.trimmingCharacters(in: .whitespacesAndNewlines)
                    self.showError(
                        "Open Liveboard failed",
                        detail: detail.isEmpty ? "zodex local watch exited with status \(exitCode)." : detail
                    )
                }
            }
        }
    }

    private static func liveboardURL(in output: String) -> URL? {
        for line in output.split(whereSeparator: \.isNewline) {
            let text = String(line)
            guard text.hasPrefix("Liveboard: ") else {
                continue
            }
            return URL(string: String(text.dropFirst("Liveboard: ".count)))
        }
        return nil
    }

    private func stopLiveboardHost() {
        guard let process = liveboardProcess else {
            liveboardURL = nil
            liveboardRuntimeID = nil
            pendingLiveboardOpen = false
            return
        }
        liveboardProcess = nil
        liveboardURL = nil
        liveboardRuntimeID = nil
        pendingLiveboardOpen = false
        if process.isRunning {
            kill(process.processIdentifier, SIGINT)
        }
    }

    @objc private func changeStartFolder() {
        let panel = NSOpenPanel()
        panel.title = configuredStartFolder == nil ? "Set Zodex Start Folder" : "Change Zodex Start Folder"
        panel.prompt = "Choose"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true

        if let path = validStartFolder {
            panel.directoryURL = URL(fileURLWithPath: path, isDirectory: true)
        } else {
            panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        }

        NSApp.activate(ignoringOtherApps: true)
        guard panel.runModal() == .OK, let selectedURL = panel.url else {
            return
        }

        let path = selectedURL.standardizedFileURL.resolvingSymlinksInPath().path
        defaults.set(path, forKey: Self.startFolderKey)
        updateStartFolderItems()
        refreshStatus()
    }

    @objc private func toggleLaunchAtLogin() {
        let service = SMAppService.mainApp
        do {
            if service.status == .enabled {
                try service.unregister()
            } else {
                try service.register()
            }
            updateLaunchAtLoginItem()
        } catch {
            updateLaunchAtLoginItem()
            showError("Launch at Login could not be changed", detail: error.localizedDescription)
        }
    }

    @objc private func updateZodex() {
        if let version = availableUpdateVersion {
            beginUpgrade(version: version, stopLocal: false)
        } else {
            refreshUpgradeStatus(force: true, reportUpToDate: true)
        }
    }

    private func refreshUpgradeStatus(force: Bool, reportUpToDate: Bool) {
        guard !upgradeCheckInFlight, !upgradeInFlight else {
            return
        }
        upgradeCheckInFlight = true
        updateItem.title = "Checking for Updates…"
        updateItem.isEnabled = false

        var arguments = ["upgrade", "--check", "--format", "json"]
        if force {
            arguments.append("--refresh")
        }
        runZodex(arguments) { [weak self] result in
            guard let self else { return }
            self.upgradeCheckInFlight = false
            guard result.exitCode == 0,
                  let event = Self.lastUpgradeEvent(named: "check_complete", in: result.output)
            else {
                self.availableUpdateVersion = nil
                self.updateItem.title = "Check for Updates…"
                self.updateItem.isEnabled = true
                if reportUpToDate {
                    let failure = Self.lastUpgradeEvent(named: "failed", in: result.output)
                    self.showError(
                        "Update check failed",
                        detail: failure?.message
                            ?? result.output.trimmingCharacters(in: .whitespacesAndNewlines)
                    )
                }
                return
            }

            self.versionItem.title = "Zodex v\(event.current_version)"
            if event.update_available == true, let target = event.target_version {
                self.availableUpdateVersion = target
                self.updateItem.title = "Update to v\(target)…"
            } else {
                self.availableUpdateVersion = nil
                self.updateItem.title = "Check for Updates…"
                if reportUpToDate {
                    self.showInformation("Zodex is up to date", detail: event.message)
                }
            }
            self.updateItem.isEnabled = true
        }
    }

    private func beginUpgrade(version: String, stopLocal: Bool) {
        guard !upgradeInFlight else {
            return
        }
        upgradeInFlight = true
        updateItem.title = "Preparing Update…"
        updateItem.isEnabled = false

        var arguments = ["upgrade", "--version", version, "--format", "json"]
        if stopLocal {
            arguments.append("--stop-local")
        }
        runUpgradeStreaming(
            arguments: arguments,
            onEvent: { [weak self] event in
                self?.handleUpgradeEvent(event)
            },
            completion: { [weak self] exitCode, stdout, stderr in
                guard let self else { return }
                self.upgradeInFlight = false
                if exitCode == 0 {
                    self.availableUpdateVersion = nil
                    self.updateItem.title = "Check for Updates…"
                    self.updateItem.isEnabled = true
                    return
                }

                let failure = Self.lastUpgradeEvent(named: "failed", in: stdout)
                if failure?.code == "local_running", !stopLocal {
                    self.updateItem.title = "Update to v\(version)…"
                    self.updateItem.isEnabled = true
                    self.confirmStopAndUpdate(version: version)
                    return
                }

                self.updateItem.title = "Update to v\(version)…"
                self.updateItem.isEnabled = true
                let detail = failure?.message
                    ?? stderr.trimmingCharacters(in: .whitespacesAndNewlines)
                self.showError(
                    "Zodex update failed",
                    detail: detail.isEmpty ? "The upgrade command exited with status \(exitCode)." : detail
                )
            }
        )
    }

    private func handleUpgradeEvent(_ event: UpgradeEvent) {
        switch event.event {
        case "stopping_local":
            updateItem.title = "Stopping Zodex…"
        case "downloading", "retrying_download":
            updateItem.title = event.event == "retrying_download" ? "Retrying Download…" : "Downloading Update…"
        case "verifying":
            updateItem.title = "Verifying Update…"
        case "installing":
            updateItem.title = "Installing Update…"
        case "complete":
            updateItem.title = "Updated to v\(event.target_version ?? event.current_version)"
        default:
            break
        }
    }

    private func confirmStopAndUpdate(version: String) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Stop Zodex Local and update?"
        alert.informativeText = "Zodex Local must stop before the operator can be replaced. Active Zodex-owned processes will be stopped, then Zodex will update to v\(version)."
        alert.addButton(withTitle: "Stop and Update")
        alert.addButton(withTitle: "Cancel")
        if alert.runModal() == .alertFirstButtonReturn {
            beginUpgrade(version: version, stopLocal: true)
        }
    }

    @objc private func quit() {
        stopLiveboardHost()
        NSApp.terminate(nil)
    }

    private func runZodex(_ arguments: [String], completion: @escaping (CommandResult) -> Void) {
        let executable = zodexURL
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Self.runCommand(executable: executable, arguments: arguments)
            DispatchQueue.main.async {
                completion(result)
            }
        }
    }

    private func runZodexStart(path: String, completion: @escaping (CommandResult) -> Void) {
        let executable = zodexURL
        DispatchQueue.global(qos: .userInitiated).async {
            guard let environment = captureLoginShellEnvironment() else {
                DispatchQueue.main.async {
                    completion(CommandResult(
                        exitCode: -1,
                        output: "Could not capture the interactive login-shell environment for Start Zodex."
                    ))
                }
                return
            }
            let process = Process()
            let outputPipe = Pipe()
            process.executableURL = executable
            process.arguments = ["local", "start", path]
            process.environment = environment
            process.standardInput = FileHandle.nullDevice
            process.standardOutput = outputPipe
            process.standardError = outputPipe

            let result: CommandResult
            do {
                try process.run()
                let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                result = CommandResult(
                    exitCode: process.terminationStatus,
                    output: String(decoding: data, as: UTF8.self)
                )
            } catch {
                result = CommandResult(exitCode: -1, output: error.localizedDescription)
            }

            DispatchQueue.main.async {
                completion(result)
            }
        }
    }

    private func runZodexSync(_ arguments: [String]) -> CommandResult {
        Self.runCommand(executable: zodexURL, arguments: arguments)
    }

    private static func runCommand(executable: URL, arguments: [String]) -> CommandResult {
        let process = Process()
        let outputPipe = Pipe()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = outputPipe
        process.standardError = outputPipe

        do {
            try process.run()
            let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            return CommandResult(
                exitCode: process.terminationStatus,
                output: String(decoding: data, as: UTF8.self)
            )
        } catch {
            return CommandResult(exitCode: -1, output: error.localizedDescription)
        }
    }

    private func runUpgradeStreaming(
        arguments: [String],
        onEvent: @escaping (UpgradeEvent) -> Void,
        completion: @escaping (Int32, String, String) -> Void
    ) {
        let executable = zodexURL
        let process = Process()
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        do {
            try process.run()
        } catch {
            completion(-1, "", error.localizedDescription)
            return
        }

        DispatchQueue.global(qos: .userInitiated).async {
            let stdoutHandle = stdoutPipe.fileHandleForReading
            var stdout = ""
            var pending = ""
            while true {
                let data = stdoutHandle.availableData
                if data.isEmpty {
                    break
                }
                let chunk = String(decoding: data, as: UTF8.self)
                stdout += chunk
                pending += chunk
                while let newline = pending.firstIndex(of: "\n") {
                    let line = String(pending[..<newline])
                    pending.removeSubrange(...newline)
                    guard let data = line.data(using: .utf8),
                          let event = try? JSONDecoder().decode(UpgradeEvent.self, from: data)
                    else {
                        continue
                    }
                    DispatchQueue.main.async {
                        onEvent(event)
                    }
                }
            }
            process.waitUntilExit()
            let stderr = String(
                decoding: stderrPipe.fileHandleForReading.readDataToEndOfFile(),
                as: UTF8.self
            )
            DispatchQueue.main.async {
                completion(process.terminationStatus, stdout, stderr)
            }
        }
    }

    private static func lastUpgradeEvent(named name: String, in output: String) -> UpgradeEvent? {
        for line in output.split(whereSeparator: \.isNewline).reversed() {
            guard let data = String(line).data(using: .utf8),
                  let event = try? JSONDecoder().decode(UpgradeEvent.self, from: data),
                  event.event == name
            else {
                continue
            }
            return event
        }
        return nil
    }

    private func showCommandError(_ title: String, result: CommandResult) {
        let detail = result.output.trimmingCharacters(in: .whitespacesAndNewlines)
        showError(title, detail: detail.isEmpty ? "Zodex exited with status \(result.exitCode)." : detail)
    }

    private func showError(_ title: String, detail: String) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = title
        alert.informativeText = detail
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }

    private func showInformation(_ title: String, detail: String) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = title
        alert.informativeText = detail
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }
}

private func siblingZodexURL() -> URL {
    let bundle = Bundle.main.bundleURL.standardizedFileURL.resolvingSymlinksInPath()
    if bundle.pathExtension == "app" {
        return bundle.deletingLastPathComponent().appendingPathComponent("zodex", isDirectory: false)
    }
    let executable = URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL.resolvingSymlinksInPath()
    return executable.deletingLastPathComponent().appendingPathComponent("zodex", isDirectory: false)
}

private func loginShellPath() -> String {
    if let entry = getpwuid(getuid()), let shell = entry.pointee.pw_shell {
        let path = String(cString: shell)
        if !path.isEmpty {
            return path
        }
    }
    return "/bin/zsh"
}

private func captureLoginShellEnvironment() -> [String: String]? {
    let process = Process()
    let outputPipe = Pipe()
    process.executableURL = URL(fileURLWithPath: loginShellPath())
    process.arguments = [
        "-lic",
        #"/usr/bin/printf 'ZODEX_ENV_MARKER\0'; /usr/bin/env -0"#,
    ]
    process.standardInput = FileHandle.nullDevice
    process.standardOutput = outputPipe
    process.standardError = FileHandle.nullDevice

    do {
        try process.run()
        let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            return nil
        }

        let marker = Data(Array("ZODEX_ENV_MARKER".utf8) + [0])
        guard let markerRange = data.range(of: marker) else {
            return nil
        }
        var environment: [String: String] = [:]
        for bytes in data[markerRange.upperBound...].split(separator: 0) {
            let entry = String(decoding: bytes, as: UTF8.self)
            guard let separator = entry.firstIndex(of: "=") else {
                continue
            }
            let key = String(entry[..<separator])
            let value = String(entry[entry.index(after: separator)...])
            if !key.isEmpty {
                environment[key] = value
            }
        }
        return environment.isEmpty ? nil : environment
    } catch {
        return nil
    }
}

private func setLaunchAtLogin(_ enabled: Bool) throws {
    let service = SMAppService.mainApp
    if enabled {
        if service.status != .enabled {
            try service.register()
        }
    } else if service.status != .notRegistered {
        try service.unregister()
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
