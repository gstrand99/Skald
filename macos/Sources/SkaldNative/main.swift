import AppKit
import ApplicationServices
import Foundation

enum NativeError: Error, CustomStringConvertible {
    case usage
    case accessibilityDenied
    case clipboardFailed

    var description: String {
        switch self {
        case .usage: "usage: skald-native target|clipboard-read|clipboard-write|paste|permissions"
        case .accessibilityDenied: "Accessibility permission is required for safe paste"
        case .clipboardFailed: "macOS pasteboard operation failed"
        }
    }
}

func accessibilityTrusted(prompt: Bool) -> Bool {
    let options = ["AXTrustedCheckOptionPrompt": prompt] as CFDictionary
    return AXIsProcessTrustedWithOptions(options)
}

func run() throws {
    guard let command = CommandLine.arguments.dropFirst().first else { throw NativeError.usage }
    switch command {
    case "target":
        guard let app = NSWorkspace.shared.frontmostApplication else { return }
        let object: [String: Any] = [
            "pid": app.processIdentifier,
            "bundle_id": app.bundleIdentifier ?? "",
        ]
        let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0a]))
    case "clipboard-read":
        if let value = NSPasteboard.general.string(forType: .string) {
            FileHandle.standardOutput.write(Data(value.utf8))
        }
    case "clipboard-write":
        let data = FileHandle.standardInput.readDataToEndOfFile()
        guard let value = String(data: data, encoding: .utf8) else { throw NativeError.clipboardFailed }
        NSPasteboard.general.clearContents()
        guard NSPasteboard.general.setString(value, forType: .string) else {
            throw NativeError.clipboardFailed
        }
    case "paste":
        guard accessibilityTrusted(prompt: false) else { throw NativeError.accessibilityDenied }
        guard
            let down = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: true),
            let up = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: false)
        else { throw NativeError.accessibilityDenied }
        down.flags = .maskCommand
        up.flags = .maskCommand
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
    case "permissions":
        let prompt = CommandLine.arguments.contains("--prompt")
        let value = ["accessibility": accessibilityTrusted(prompt: prompt)]
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0a]))
    default:
        throw NativeError.usage
    }
}

do {
    try run()
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
