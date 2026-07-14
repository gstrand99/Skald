import AppKit
import SwiftUI

private final class FocusSafePanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class OverlayPanelController {
    private let panel: FocusSafePanel
    private unowned let model: SkaldModel

    init(model: SkaldModel) {
        self.model = model
        panel = FocusSafePanel(
            contentRect: NSRect(x: 0, y: 0, width: 560, height: 112),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .floating
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
        panel.contentView = NSHostingView(rootView: OverlayView(model: model))
        positionPanel()
    }

    func updateVisibility() {
        if model.overlayVisible && model.jobActive && model.daemonAvailable {
            positionPanel()
            panel.orderFrontRegardless()
        } else {
            panel.orderOut(nil)
        }
    }

    private func positionPanel() {
        guard let screen = NSScreen.main ?? NSScreen.screens.first else { return }
        let frame = screen.visibleFrame
        let origin = NSPoint(
            x: frame.midX - panel.frame.width / 2,
            y: frame.minY + 28
        )
        panel.setFrameOrigin(origin)
    }
}

struct OverlayView: View {
    @ObservedObject var model: SkaldModel

    var body: some View {
        HStack(spacing: 14) {
            if model.recording {
                AudioMeter(level: max(model.rms * 12, model.peak * 5))
            } else {
                ProgressView().controlSize(.small)
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(model.status)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                preview
                    .font(.system(size: 15))
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 18))
        .padding(8)
    }

    @ViewBuilder
    private var preview: some View {
        if model.stablePreview.isEmpty && model.provisionalPreview.isEmpty {
            Text(model.recording ? (model.speechActive ? "Listening…" : "Waiting for speech…") : "Processing…")
                .foregroundStyle(.primary)
        } else {
            (Text(model.stablePreview) + Text(model.provisionalPreview.isEmpty ? "" : " \(model.provisionalPreview)")
                .foregroundColor(.secondary))
                .foregroundStyle(.primary)
        }
    }
}

private struct AudioMeter: View {
    let level: Double

    var body: some View {
        HStack(alignment: .center, spacing: 2) {
            ForEach(0..<7, id: \.self) { index in
                Capsule()
                    .fill(index == 6 ? Color.red : Color.accentColor)
                    .frame(width: 3, height: height(for: index))
            }
        }
        .frame(width: 34, height: 34)
    }

    private func height(for index: Int) -> CGFloat {
        let clamped = min(max(level, 0), 1)
        let shape = 1 - abs(Double(index - 3)) * 0.12
        return 5 + 25 * clamped * shape
    }
}
