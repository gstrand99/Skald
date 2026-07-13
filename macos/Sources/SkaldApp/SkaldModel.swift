import AppKit
import Foundation

private struct TaggedState: Decodable {
    let state: String
}

private struct ProtocolError: Decodable {
    let message: String
}

private struct DaemonEvent: Decodable {
    let event: String
    let jobState: TaggedState?
    let finalModelState: TaggedState?
    let stable: String?
    let provisional: String?
    let speechActive: Bool?
    let rms: Double?
    let peak: Double?
    let error: ProtocolError?

    enum CodingKeys: String, CodingKey {
        case event
        case jobState = "job_state"
        case finalModelState = "final_model_state"
        case stable, provisional
        case speechActive = "speech_active"
        case rms, peak, error
    }
}

enum HotKeyChoice: String, CaseIterable, Identifiable {
    case controlOptionSpace
    case commandShiftSpace
    case controlOptionD

    var id: String { rawValue }

    var label: String {
        switch self {
        case .controlOptionSpace: "Control–Option–Space"
        case .commandShiftSpace: "Command–Shift–Space"
        case .controlOptionD: "Control–Option–D"
        }
    }

    var keyCode: UInt16 {
        switch self {
        case .controlOptionSpace, .commandShiftSpace: 49
        case .controlOptionD: 2
        }
    }

    var modifiers: NSEvent.ModifierFlags {
        switch self {
        case .controlOptionSpace, .controlOptionD: [.control, .option]
        case .commandShiftSpace: [.command, .shift]
        }
    }
}

@MainActor
final class SkaldModel: ObservableObject {
    @Published private(set) var daemonAvailable = false
    @Published private(set) var jobState = "idle"
    @Published private(set) var finalModelState = "unknown"
    @Published private(set) var stablePreview = ""
    @Published private(set) var provisionalPreview = ""
    @Published private(set) var speechActive = false
    @Published private(set) var rms = 0.0
    @Published private(set) var peak = 0.0
    @Published private(set) var lastError: String?
    @Published private(set) var accessibilityGranted = false
    @Published var overlayVisible: Bool {
        didSet {
            UserDefaults.standard.set(overlayVisible, forKey: "overlayVisible")
            overlayController?.updateVisibility()
        }
    }
    @Published var shortcut: HotKeyChoice {
        didSet {
            UserDefaults.standard.set(shortcut.rawValue, forKey: "globalShortcut")
            installHotKey()
        }
    }

    private var started = false
    private var eventTask: Task<Void, Never>?
    private var eventProcess: Process?
    private var globalHotKey: Any?
    private var localHotKey: Any?
    private var overlayController: OverlayPanelController?

    init() {
        let defaults = UserDefaults.standard
        overlayVisible = defaults.object(forKey: "overlayVisible") as? Bool ?? true
        shortcut = HotKeyChoice(rawValue: defaults.string(forKey: "globalShortcut") ?? "")
            ?? .controlOptionSpace
    }

    var recording: Bool { jobState == "recording" }
    var jobActive: Bool { !["idle", "done", "cancelled", "failed"].contains(jobState) }

    var status: String {
        if !daemonAvailable { return "Daemon unavailable" }
        switch jobState {
        case "recording": return "Recording"
        case "stopping", "transcribing": return "Transcribing"
        case "cleaning": return "Cleaning"
        case "copying", "injecting": return "Inserting"
        default: return lastError == nil ? "Ready" : "Ready — last action failed"
        }
    }

    var statusSymbol: String {
        if !daemonAvailable { return "exclamationmark.triangle" }
        if recording { return "mic.fill" }
        if lastError != nil { return "exclamationmark.circle" }
        return "checkmark.circle"
    }

    var modelStatus: String {
        switch finalModelState {
        case "ready": "Ready"
        case "loading": "Loading"
        case "unloaded": "Available on demand"
        case "failed": "Failed"
        default: "Unknown"
        }
    }

    func start() {
        guard !started else { return }
        started = true
        overlayController = OverlayPanelController(model: self)
        installHotKey()
        refreshPermissionState()
        eventTask = Task { [weak self] in await self?.listenForEvents() }
    }

    func toggle() { run(["toggle"]) }
    func cancel() { run(["cancel"]) }

    func installDaemon() {
        run(["service", "install"], refreshStream: false)
        run(["service", "start"], refreshStream: true)
    }

    func requestAccessibility() {
        runNative(["permissions", "--prompt"])
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            self?.refreshPermissionState()
        }
    }

    func openAccessibilitySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
            NSWorkspace.shared.open(url)
        }
    }

    func openConfig() {
        let path = FileManager.default.homeDirectoryForCurrentUser
            .appending(path: "Library/Application Support/Skald/config.toml")
        NSWorkspace.shared.open(path)
    }

    func refreshPermissionState() {
        refreshPermissions()
    }

    private func run(_ arguments: [String], refreshStream: Bool = false) {
        let process = Process()
        process.executableURL = bundledExecutable("skald")
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            if process.terminationStatus != 0 {
                lastError = "Command failed"
            } else {
                lastError = nil
            }
            if refreshStream { restartEventStream() }
        } catch {
            lastError = error.localizedDescription
        }
    }

    private func runNative(_ arguments: [String]) {
        let process = Process()
        process.executableURL = bundledExecutable("skald-native")
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try? process.run()
    }

    private func refreshPermissions() {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = bundledExecutable("skald-native")
        process.arguments = ["permissions"]
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return }
        process.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        if let object = try? JSONSerialization.jsonObject(with: data) as? [String: Bool] {
            accessibilityGranted = object["accessibility"] ?? false
        }
    }

    private func installHotKey() {
        if let globalHotKey { NSEvent.removeMonitor(globalHotKey) }
        if let localHotKey { NSEvent.removeMonitor(localHotKey) }
        let choice = shortcut
        globalHotKey = NSEvent.addGlobalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard Self.matches(event, choice: choice) else { return }
            Task { @MainActor in self?.toggle() }
        }
        localHotKey = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard Self.matches(event, choice: choice) else { return event }
            Task { @MainActor in self?.toggle() }
            return nil
        }
    }

    nonisolated private static func matches(_ event: NSEvent, choice: HotKeyChoice) -> Bool {
        let mask: NSEvent.ModifierFlags = [.command, .control, .option, .shift]
        return !event.isARepeat
            && event.keyCode == choice.keyCode
            && event.modifierFlags.intersection(mask) == choice.modifiers
    }

    private func restartEventStream() {
        eventProcess?.terminate()
    }

    private func listenForEvents() async {
        while !Task.isCancelled {
            let process = Process()
            let pipe = Pipe()
            process.executableURL = bundledExecutable("skald")
            process.arguments = ["watch", "--json"]
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            eventProcess = process
            do {
                try process.run()
                for try await line in pipe.fileHandleForReading.bytes.lines {
                    receive(line)
                }
            } catch {
                lastError = error.localizedDescription
            }
            if eventProcess === process { eventProcess = nil }
            setDisconnected()
            try? await Task.sleep(for: .seconds(1))
        }
    }

    private func receive(_ line: String) {
        guard let data = line.data(using: .utf8),
              let event = try? JSONDecoder().decode(DaemonEvent.self, from: data)
        else { return }
        daemonAvailable = true
        switch event.event {
        case "state":
            if let state = event.jobState?.state { jobState = state }
            if let model = event.finalModelState?.state { finalModelState = model }
            if !jobActive { clearEphemeralPreview() }
        case "preview":
            stablePreview = event.stable ?? ""
            provisionalPreview = event.provisional ?? ""
            speechActive = event.speechActive ?? false
        case "audio_level":
            rms = event.rms ?? 0
            peak = event.peak ?? 0
        case "error":
            lastError = event.error?.message ?? "Daemon error"
        case "result":
            clearEphemeralPreview()
        default:
            break
        }
        overlayController?.updateVisibility()
    }

    private func setDisconnected() {
        daemonAvailable = false
        jobState = "idle"
        finalModelState = "unknown"
        clearEphemeralPreview()
        overlayController?.updateVisibility()
    }

    private func clearEphemeralPreview() {
        stablePreview = ""
        provisionalPreview = ""
        speechActive = false
        rms = 0
        peak = 0
    }

    private func bundledExecutable(_ name: String) -> URL {
        Bundle.main.bundleURL.appending(path: "Contents/Resources/bin/\(name)")
    }
}
