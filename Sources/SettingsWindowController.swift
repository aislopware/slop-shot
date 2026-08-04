import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Cửa sổ Settings: TabView 3 tab (Destination · Shortcuts · About).
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class SettingsWindowController {
    private var window: NSWindow?

    func show(settings: AppSettings) {
        if let window {
            NSApp.activate(ignoringOtherApps: true)
            window.makeKeyAndOrderFront(nil)
            return
        }
        let host = NSHostingController(rootView: SettingsView(settings: settings))
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 460, height: 360),
                           styleMask: [.titled, .closable],
                           backing: .buffered, defer: false)
        win.title = "Settings"
        win.contentViewController = host
        win.isReleasedWhenClosed = false
        win.center()

        NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: win, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.window = nil }
        }
        NSApp.activate(ignoringOtherApps: true)
        win.makeKeyAndOrderFront(nil)
        self.window = win
    }
}

private struct SettingsView: View {
    @ObservedObject var settings: AppSettings

    var body: some View {
        TabView {
            DestinationTab(settings: settings)
                .tabItem { Label("Destination", systemImage: "folder") }
            ShortcutsTab(settings: settings)
                .tabItem { Label("Shortcuts", systemImage: "keyboard") }
            AboutTab()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 460, height: 360)
    }
}

// ── Tab Destination ────────────────────────────────────────────────────────
private struct DestinationTab: View {
    @ObservedObject var settings: AppSettings

    var body: some View {
        Form {
            Section("Startup") {
                Toggle("Launch SlopShot at login", isOn: Binding(
                    get: { settings.launchAtLogin },
                    set: { settings.setLaunchAtLogin($0) }
                ))
            }

            Section("Save location") {
                HStack {
                    Image(systemName: "folder.fill").foregroundStyle(.secondary)
                    Text(settings.saveFolderDisplay)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Choose…", action: chooseFolder)
                }
            }

            Section("Selection") {
                Toggle("Snap to window & item edges", isOn: $settings.snapToEdges)
                Text("Hover to outline the window or item under the cursor, then click to grab it. Hold ⌥ while dragging for a free selection.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("After capture") {
                Toggle("Also copy to clipboard", isOn: $settings.copyToClipboard)
                Toggle("Show preview thumbnail", isOn: $settings.showThumbnail)
            }
            Section {
                Text("Captures are kept temporarily until you click Save on the preview — then they're written to the folder above.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Image format") {
                Picker("Format", selection: $settings.imageFormat) {
                    ForEach(AppSettings.ImageFormat.allCases) { f in
                        Text(f.label).tag(f)
                    }
                }
                .pickerStyle(.menu)
                .frame(maxWidth: 160)
            }
        }
        .formStyle(.grouped)
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.directoryURL = settings.saveFolderURL
        if panel.runModal() == .OK, let url = panel.url {
            settings.saveFolderPath = url.path
        }
    }
}

// ── Tab Shortcuts (bind lại được) ───────────────────────────────────────────
private struct ShortcutsTab: View {
    @ObservedObject var settings: AppSettings

    var body: some View {
        Form {
            Section("Global shortcuts") {
                ForEach(ShortcutAction.allCases) { action in
                    HStack {
                        Text(action.title)
                        Spacer()
                        // Bấm vào ô → gõ tổ hợp mới để gán.
                        ShortcutRecorder(display: settings.hotkey(for: action).display) { hk in
                            settings.setHotkey(hk, for: action)
                        }
                        .frame(width: 110, height: 22)
                        Button { settings.resetHotkey(for: action) } label: {
                            Image(systemName: "arrow.uturn.backward")
                        }
                        .buttonStyle(.borderless)
                        .help("Reset to default")
                    }
                }
            }
            Text("Click a shortcut, then press the new key combo (needs at least one of ⌃⌥⇧⌘). Press ⎋ to cancel.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Ô ghi phím tắt: bấm vào để "lắng nghe", gõ tổ hợp → trả về Hotkey.
// Bọc 1 NSView tự vẽ (AppKit) vì SwiftIU thuần khó bắt phím thô + modifier.
// ─────────────────────────────────────────────────────────────────────────
private struct ShortcutRecorder: NSViewRepresentable {
    let display: String
    let onCapture: (Hotkey) -> Void

    func makeNSView(context: Context) -> RecorderView {
        let v = RecorderView()
        v.onCapture = onCapture
        v.displayText = display
        return v
    }
    func updateNSView(_ v: RecorderView, context: Context) {
        v.onCapture = onCapture
        if !v.recording { v.displayText = display; v.needsDisplay = true }
    }
}

private final class RecorderView: NSView {
    var displayText = ""
    var onCapture: ((Hotkey) -> Void)?
    var recording = false

    override var acceptsFirstResponder: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        let bg = recording ? NSColor.controlAccentColor.withAlphaComponent(0.18)
                           : NSColor.controlBackgroundColor
        bg.setFill()
        let path = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 5, yRadius: 5)
        path.fill()
        (recording ? NSColor.controlAccentColor : NSColor.separatorColor).setStroke()
        path.lineWidth = 1; path.stroke()

        let text = recording ? "Type shortcut…" : (displayText.isEmpty ? "—" : displayText)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 12, weight: .medium),
            .foregroundColor: recording ? NSColor.controlAccentColor : NSColor.labelColor,
        ]
        let size = text.size(withAttributes: attrs)
        text.draw(at: CGPoint(x: (bounds.width - size.width) / 2,
                              y: (bounds.height - size.height) / 2), withAttributes: attrs)
    }

    override func mouseDown(with event: NSEvent) {
        recording = true
        window?.makeFirstResponder(self)
        needsDisplay = true
    }

    override func keyDown(with event: NSEvent) {
        guard recording else { super.keyDown(with: event); return }
        if event.keyCode == 53 {                 // ⎋ → huỷ
            recording = false; needsDisplay = true; return
        }
        let mods = Hotkey.carbonModifiers(from: event.modifierFlags)
        guard mods != 0 else { NSSound.beep(); return }   // bắt buộc có modifier
        recording = false
        onCapture?(Hotkey(keyCode: UInt32(event.keyCode), modifiers: mods))
        needsDisplay = true
    }

    override func resignFirstResponder() -> Bool {
        recording = false; needsDisplay = true
        return true
    }
}

// ── Tab About ───────────────────────────────────────────────────────────────
private struct AboutTab: View {
    var body: some View {
        VStack(spacing: 10) {
            // App icon thật (logo S trong khung ngắm) thay cho SF Symbol.
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 84, height: 84)
            Text("SlopShot").font(.title2).bold()
            if let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String {
                Text("Version \(v)").font(.caption).foregroundStyle(.secondary)
            }
            Text("A CleanShot-style capture tool, built native on macOS.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 300)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
