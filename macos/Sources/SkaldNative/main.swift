import AppKit
import ApplicationServices
import Foundation
import UserNotifications

private let protocolVersion = 1
private let maximumTextBytes = 16 * 1024 * 1024

private final class DeliveryResult: @unchecked Sendable {
    private let lock = NSLock()
    private var error: Error?

    func set(error: Error?) {
        lock.withLock { self.error = error }
    }

    var succeeded: Bool {
        lock.withLock { error == nil }
    }
}

enum NativeError: Error, CustomStringConvertible {
    case usage
    case accessibilityDenied
    case clipboardFailed
    case notificationFailed
    case invalidRequest(String)

    var description: String {
        switch self {
        case .usage: "usage: skald-native target|clipboard-read|clipboard-write|paste|permissions|broker"
        case .accessibilityDenied: "Accessibility permission is required for safe paste"
        case .clipboardFailed: "macOS pasteboard operation failed"
        case .notificationFailed: "macOS notification delivery failed"
        case let .invalidRequest(message): message
        }
    }

    var code: String {
        switch self {
        case .usage, .invalidRequest: "invalid_request"
        case .accessibilityDenied: "accessibility_denied"
        case .clipboardFailed: "clipboard_failed"
        case .notificationFailed: "notification_failed"
        }
    }
}

func accessibilityTrusted(prompt: Bool) -> Bool {
    let options = ["AXTrustedCheckOptionPrompt": prompt] as CFDictionary
    return AXIsProcessTrustedWithOptions(options)
}

func activeTarget() -> [String: Any]? {
    guard let app = NSWorkspace.shared.frontmostApplication else { return nil }
    return [
        "pid": app.processIdentifier,
        "bundle_id": app.bundleIdentifier ?? "",
    ]
}

func readClipboard() -> String {
    NSPasteboard.general.string(forType: .string) ?? ""
}

func writeClipboard(_ value: String) throws {
    guard value.utf8.count <= maximumTextBytes else {
        throw NativeError.invalidRequest("clipboard text exceeds the broker limit")
    }
    NSPasteboard.general.clearContents()
    guard NSPasteboard.general.setString(value, forType: .string) else {
        throw NativeError.clipboardFailed
    }
}

func paste() throws {
    guard accessibilityTrusted(prompt: false) else { throw NativeError.accessibilityDenied }
    guard
        let down = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: true),
        let up = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: false)
    else { throw NativeError.accessibilityDenied }
    down.flags = .maskCommand
    up.flags = .maskCommand
    down.post(tap: .cghidEventTap)
    up.post(tap: .cghidEventTap)
}

func notify(summary: String, body: String) throws {
    guard summary.utf8.count <= 256, body.utf8.count <= 4_096 else {
        throw NativeError.invalidRequest("notification fields exceed the broker limit")
    }
    let content = UNMutableNotificationContent()
    content.title = summary
    content.body = body
    let request = UNNotificationRequest(
        identifier: UUID().uuidString,
        content: content,
        trigger: nil
    )
    let semaphore = DispatchSemaphore(value: 0)
    let result = DeliveryResult()
    UNUserNotificationCenter.current().add(request) { error in
        result.set(error: error)
        semaphore.signal()
    }
    guard semaphore.wait(timeout: .now() + 2) == .success, result.succeeded else {
        throw NativeError.notificationFailed
    }
}

func writeJSON(_ object: [String: Any]) throws {
    let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0a]))
}

func brokerResponse(id: String, values: [String: Any] = [:]) -> [String: Any] {
    var response = values
    response["protocol_version"] = protocolVersion
    response["request_id"] = id
    response["ok"] = true
    return response
}

func brokerError(id: String, error: Error) -> [String: Any] {
    let native = error as? NativeError
    return [
        "protocol_version": protocolVersion,
        "request_id": id,
        "ok": false,
        "error": [
            "code": native?.code ?? "native_error",
            "message": native?.description ?? "native broker operation failed",
        ],
    ]
}

func validatedBrokerRequest(_ data: Data) throws -> [String: Any] {
    guard data.count <= maximumTextBytes + 4_096,
          let request = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          request["protocol_version"] as? Int == protocolVersion,
          let id = request["request_id"] as? String,
          !id.isEmpty,
          id.utf8.count <= 128,
          request["operation"] is String
    else { throw NativeError.invalidRequest("invalid broker request") }
    return request
}

func handleBrokerRequest(_ request: [String: Any]) throws -> [String: Any] {
    let id = request["request_id"] as! String
    let operation = request["operation"] as! String
    let commonKeys: Set<String> = ["protocol_version", "request_id", "operation"]
    switch operation {
    case "clipboard_read":
        guard Set(request.keys).isSubset(of: commonKeys) else {
            throw NativeError.invalidRequest("unexpected clipboard_read field")
        }
        return brokerResponse(id: id, values: ["text": readClipboard()])
    case "clipboard_write":
        guard Set(request.keys).isSubset(of: commonKeys.union(["text"])),
              let text = request["text"] as? String
        else { throw NativeError.invalidRequest("clipboard_write requires text") }
        try writeClipboard(text)
        return brokerResponse(id: id)
    case "target":
        guard Set(request.keys).isSubset(of: commonKeys) else {
            throw NativeError.invalidRequest("unexpected target field")
        }
        let target: Any = activeTarget().map { $0 as Any } ?? NSNull()
        return brokerResponse(id: id, values: ["target": target])
    case "paste":
        guard Set(request.keys).isSubset(of: commonKeys) else {
            throw NativeError.invalidRequest("unexpected paste field")
        }
        try paste()
        return brokerResponse(id: id)
    case "permissions":
        guard Set(request.keys).isSubset(of: commonKeys) else {
            throw NativeError.invalidRequest("unexpected permissions field")
        }
        return brokerResponse(id: id, values: ["accessibility": accessibilityTrusted(prompt: false)])
    case "notify":
        guard Set(request.keys).isSubset(of: commonKeys.union(["summary", "body"])),
              let summary = request["summary"] as? String,
              let body = request["body"] as? String
        else { throw NativeError.invalidRequest("notify requires summary and body") }
        try notify(summary: summary, body: body)
        return brokerResponse(id: id)
    default:
        throw NativeError.invalidRequest("unknown broker operation")
    }
}

func runBroker() throws {
    while let line = readLine(strippingNewline: true) {
        guard let data = line.data(using: .utf8) else { continue }
        var requestID = "invalid"
        do {
            let request = try validatedBrokerRequest(data)
            requestID = request["request_id"] as! String
            try writeJSON(handleBrokerRequest(request))
        } catch {
            try writeJSON(brokerError(id: requestID, error: error))
        }
    }
}

func run() throws {
    guard let command = CommandLine.arguments.dropFirst().first else { throw NativeError.usage }
    switch command {
    case "broker":
        try runBroker()
    case "target":
        if let target = activeTarget() { try writeJSON(target) }
    case "clipboard-read":
        FileHandle.standardOutput.write(Data(readClipboard().utf8))
    case "clipboard-write":
        let data = FileHandle.standardInput.readDataToEndOfFile()
        guard let value = String(data: data, encoding: .utf8) else { throw NativeError.clipboardFailed }
        try writeClipboard(value)
    case "paste":
        try paste()
    case "permissions":
        let prompt = CommandLine.arguments.contains("--prompt")
        try writeJSON(["accessibility": accessibilityTrusted(prompt: prompt)])
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
