import AppKit
import SwiftUI

struct MenuContent: View {
    @ObservedObject var model: SkaldModel

    var body: some View {
        Label(model.status, systemImage: model.statusSymbol)
        Text("Model: \(model.modelStatus)")
        Text(model.accessibilityGranted ? "Accessibility: Granted" : "Accessibility: Required for paste")
        Divider()
        Button(model.recording ? "Stop recording" : "Start recording") { model.toggle() }
            .disabled(!model.daemonAvailable)
        Button("Cancel") { model.cancel() }.disabled(!model.jobActive)
        Toggle("Show overlay", isOn: $model.overlayVisible)
        Picker("Global shortcut", selection: $model.shortcut) {
            ForEach(HotKeyChoice.allCases) { choice in
                Text(choice.label).tag(choice)
            }
        }
        if !model.shortcutAvailable {
            Text("Shortcut unavailable — choose another combination")
        }
        Divider()
        Button("Install or repair daemon") { model.installDaemon() }
        if !model.accessibilityGranted {
            Button("Grant Accessibility access") { model.requestAccessibility() }
            Button("Open Accessibility settings") { model.openAccessibilitySettings() }
        }
        Button("Open configuration") { model.openConfig() }
        Button("Quit Skald") { NSApplication.shared.terminate(nil) }
            .onAppear { model.refreshPermissionState() }
    }
}

@main
struct SkaldApp: App {
    @StateObject private var model: SkaldModel

    init() {
        let model = SkaldModel()
        _model = StateObject(wrappedValue: model)
        DispatchQueue.main.async { model.start() }
    }

    var body: some Scene {
        MenuBarExtra("Skald", systemImage: model.recording ? "mic.fill" : "mic") {
            MenuContent(model: model)
        }
        Settings {
            MenuContent(model: model)
                .padding()
                .frame(width: 360)
        }
    }
}
