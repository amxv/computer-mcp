import Darwin
import Foundation
import ServiceManagement

final class SingleInstanceLock {
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

func siblingZodexURL() -> URL {
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

func captureLoginShellEnvironment() -> [String: String]? {
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

func setLaunchAtLogin(_ enabled: Bool) throws {
    let service = SMAppService.mainApp
    if enabled {
        if service.status != .enabled {
            try service.register()
        }
    } else if service.status != .notRegistered {
        try service.unregister()
    }
}
