import AppKit
import SwiftUI

@MainActor
final class SkaldModel: ObservableObject {
    @Published var status = "Checking daemon…"
    @Published var recording = false
    @Published var overlayVisible = true
    private var timer: Timer?
    private var globalHotKey: Any?
    private var localHotKey: Any?

    init() {
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
        globalHotKey = NSEvent.addGlobalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard event.keyCode == 49,
                  event.modifierFlags.contains([.control, .option])
            else { return }
            Task { @MainActor in self?.toggle() }
        }
        localHotKey = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard event.keyCode == 49,
                  event.modifierFlags.contains([.control, .option])
            else { return event }
            Task { @MainActor in self?.toggle() }
            return nil
        }
    }

    func run(_ arguments: [String]) {
        let process = Process()
        process.executableURL = bundledExecutable("skald")
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try? process.run()
        process.waitUntilExit()
        refresh()
    }

    func toggle() { run(["toggle"]) }
    func cancel() { run(["cancel"]) }
    func installDaemon() {
        run(["service", "install"])
        run(["service", "start"])
    }
    func requestAccessibility() {
        let process = Process()
        process.executableURL = bundledExecutable("skald-native")
        process.arguments = ["permissions", "--prompt"]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try? process.run()
    }
    func openConfig() {
        let path = FileManager.default.homeDirectoryForCurrentUser
            .appending(path: "Library/Application Support/Skald/config.toml")
        NSWorkspace.shared.open(path)
    }

    func refresh() {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = bundledExecutable("skald")
        process.arguments = ["status"]
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
            process.waitUntilExit()
            let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            let lower = output.lowercased()
            recording = lower.contains("recording")
            status = process.terminationStatus == 0 ? (recording ? "Recording" : "Ready") : "Daemon unavailable"
        } catch {
            status = "Daemon unavailable"
        }
    }

    private func bundledExecutable(_ name: String) -> URL {
        Bundle.main.bundleURL.appending(path: "Contents/Resources/bin/\(name)")
    }
}

struct MenuContent: View {
    @ObservedObject var model: SkaldModel

    var body: some View {
        Text(model.status)
        Divider()
        Button(model.recording ? "Stop recording" : "Start recording") { model.toggle() }
            .keyboardShortcut(.space, modifiers: [.control, .option])
        Button("Cancel") { model.cancel() }.disabled(!model.recording)
        Toggle("Show overlay", isOn: $model.overlayVisible)
        Divider()
        Button("Install or repair daemon") { model.installDaemon() }
        Button("Grant Accessibility access") { model.requestAccessibility() }
        Button("Open configuration") { model.openConfig() }
        Button("Quit Skald") { NSApplication.shared.terminate(nil) }
    }
}

struct OverlayView: View {
    @ObservedObject var model: SkaldModel

    var body: some View {
        if model.overlayVisible && model.recording {
            HStack(spacing: 10) {
                Circle().fill(.red).frame(width: 10, height: 10)
                Text("Skald is listening…")
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 12)
            .background(.ultraThinMaterial, in: Capsule())
        }
    }
}

@main
struct SkaldApp: App {
    @StateObject private var model = SkaldModel()

    var body: some Scene {
        MenuBarExtra("Skald", systemImage: model.recording ? "mic.fill" : "mic") {
            MenuContent(model: model)
        }
        Window("Skald Overlay", id: "overlay") {
            OverlayView(model: model)
                .frame(minWidth: 260, minHeight: 60)
        }
        .defaultPosition(.bottomTrailing)
        Settings { MenuContent(model: model).padding().frame(width: 320) }
    }
}
