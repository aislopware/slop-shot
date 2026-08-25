import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Cửa sổ Settings: TabView 5 tab (General · Capture · Privacy · Shortcuts · About).
// Bề ngang phải đủ chứa CẢ 5 tab: hụt một chút là macOS gấp phần thừa vào nút
// ">>", muốn sang tab khác phải mở menu — thà rộng thêm 150px.
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
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
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
            GeneralTab(settings: settings)
                .tabItem { Label("General", systemImage: "gearshape") }
            CaptureTab(settings: settings)
                .tabItem { Label("Capture", systemImage: "camera.viewfinder") }
            PrivacyTab(settings: settings)
                .tabItem { Label("Privacy", systemImage: "hand.raised") }
            ShortcutsTab(settings: settings)
                .tabItem { Label("Shortcuts", systemImage: "keyboard") }
            AboutTab()
                .tabItem { Label("About", systemImage: "info.circle") }
        }
        .frame(width: 640, height: 400)
    }
}

// ── Tab General ────────────────────────────────────────────────────────────
// Tab cũ tên "Destination" nhưng nhét cả startup, selection, color picker,
// after-capture, privacy, image format vào — tìm một cái toggle phải đọc hết
// bảy khối. Tách theo việc: General (app + file), Capture (lúc chụp), Privacy.
private struct GeneralTab: View {
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

// ── Tab Capture ────────────────────────────────────────────────────────────
private struct CaptureTab: View {
    @ObservedObject var settings: AppSettings

    var body: some View {
        Form {
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

            Section("Color picker") {
                Picker("Copy color as", selection: $settings.colorFormat) {
                    ForEach(AppSettings.ColorFormat.allCases) { fmt in
                        Text(fmt.label).tag(fmt)
                    }
                }
                Text("Press ← → while picking to switch format without leaving the screen.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

// ── Tab Privacy ────────────────────────────────────────────────────────────
private struct PrivacyTab: View {
    @ObservedObject var settings: AppSettings

    var body: some View {
        Form {
            PermissionsSection()

            Section("Sensitive data") {
                Toggle("Scan captures for sensitive data", isOn: $settings.redactScanOnOpen)
                Text("The editor counts emails, phone numbers, card numbers with their CVV, expiry and cardholder name, API tokens, labelled passwords and ID numbers it can see, and offers to cover them. Nothing is covered until you press Redact.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Search") {
                Toggle("Index text inside screenshots", isOn: $settings.indexCaptureText)
                Text("Reads each screenshot in the background so History can be searched by what's written in the image.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                Label("Both run on-device with Apple's Vision framework. No image, and no text read out of one, ever leaves your Mac.",
                      systemImage: "lock.laptopcomputer")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Danh sách quyền hệ thống.
//
// macOS chỉ hỏi xin quyền MỘT lần cho mỗi bản app. Lỡ bấm Deny thì từ đó app
// câm lặng — chụp ra ảnh trắng, Auto scroll bấm không nhúc nhích — mà không
// có chỗ nào trong app để biết là do quyền, càng không có đường bật lại.
// Khối này là chỗ đó: nhìn phát biết cái nào tắt, bấm một nút là sang thẳng
// đúng trang trong System Settings.
// ─────────────────────────────────────────────────────────────────────────
private struct PermissionsSection: View {
    /// Không hỏi được "quyền vừa đổi" bằng notification nào cả — người dùng gạt
    /// công tắc ở app khác. Nên: hỏi lại mỗi 2s lúc cửa sổ này mở, và hỏi ngay
    /// khi họ bấm quay lại SlopShot (đường đi thường gặp nhất).
    private let tick = Timer.publish(every: 2, on: .main, in: .common).autoconnect()
    @State private var granted: [SystemPermission: Bool] = [:]

    var body: some View {
        Section("Permissions") {
            ForEach(SystemPermission.allCases) { p in
                row(p)
            }
        }
        .onAppear(perform: refresh)
        .onReceive(tick) { _ in refresh() }
        .onReceive(NotificationCenter.default.publisher(
            for: NSApplication.didBecomeActiveNotification)) { _ in refresh() }
    }

    @ViewBuilder
    private func row(_ p: SystemPermission) -> some View {
        let ok = granted[p] ?? p.isGranted
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Image(systemName: p.icon)
                    .foregroundStyle(.secondary)
                    .frame(width: 18)
                VStack(alignment: .leading, spacing: 1) {
                    Text(p.title)
                    Text(p.purpose).font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                Label(ok ? "Granted" : "Not granted",
                      systemImage: ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                    .font(.caption).bold()
                    .foregroundStyle(ok ? Color.green : Color.orange)
                    .labelStyle(.titleAndIcon)
                Button("Open…") { p.openSystemSettings() }
            }
            // Screen Recording bật xong vẫn chưa dùng được: macOS chỉ cấp cho
            // tiến trình MỚI. Không nói ra thì người ta bật rồi chụp tiếp, vẫn
            // trắng, và tưởng cái danh sách này nói dối.
            if p.needsRelaunch, !ok {
                Text("Turn it on, then quit and reopen SlopShot — macOS only hands this permission to a freshly launched app.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
    }

    private func refresh() {
        for p in SystemPermission.allCases { granted[p] = p.isGranted }
    }
}

// ── Tab Shortcuts (bind lại được) ───────────────────────────────────────────
private struct ShortcutsTab: View {
    @ObservedObject var settings: AppSettings
    /// Ô nào đang lắng nghe phím. Giữ ở ĐÂY (không phải trong từng ô) để bấm
    /// sang ô khác thì ô cũ tự nhả ra — chỉ một bộ bắt phím sống tại một thời điểm.
    @State private var recordingAction: ShortcutAction?

    var body: some View {
        Form {
            Section("Global shortcuts") {
                ForEach(ShortcutAction.allCases) { action in
                    HStack {
                        Text(action.title)
                        Spacer()
                        // Bấm vào ô → gõ tổ hợp mới để gán.
                        ShortcutRecorder(
                            display: settings.hotkey(for: action).display,
                            isRecording: Binding(
                                get: { recordingAction == action },
                                set: { recordingAction = $0 ? action : nil })
                        ) { hk in
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
//
// Hình hài là SwiftUI; riêng phần BẮT PHÍM phải mượn NSEvent monitor: SwiftUI
// không có cách nào đọc phím thô kèm modifier rồi NUỐT luôn phím đó (trả nil)
// — mà nuốt là bắt buộc, không thì gõ ⌘W lúc đang gán sẽ đóng cửa sổ.
// ─────────────────────────────────────────────────────────────────────────
private struct ShortcutRecorder: View {
    let display: String
    @Binding var isRecording: Bool
    let onCapture: (Hotkey) -> Void

    @State private var monitor: Any?

    var body: some View {
        Button { isRecording = true } label: {
            Text(isRecording ? "Type shortcut…" : (display.isEmpty ? "—" : display))
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(isRecording ? Color.accentColor : Color.primary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(isRecording ? Color.accentColor.opacity(0.18)
                                        : Color(nsColor: .controlBackgroundColor),
                            in: RoundedRectangle(cornerRadius: 5))
                .overlay {
                    RoundedRectangle(cornerRadius: 5)
                        .stroke(isRecording ? Color.accentColor : Color(nsColor: .separatorColor),
                                lineWidth: 1)
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onChange(of: isRecording) { _, on in on ? listen() : deafen() }
        .onDisappear { deafen() }
    }

    private func listen() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            let code = event.keyCode
            let mods = Hotkey.carbonModifiers(from: event.modifierFlags)
            MainActor.assumeIsolated {
                if code == 53 {                       // ⎋ → huỷ
                    isRecording = false
                } else if mods == 0 {
                    NSSound.beep()                    // bắt buộc có modifier
                } else {
                    onCapture(Hotkey(keyCode: UInt32(code), modifiers: mods))
                    isRecording = false
                }
            }
            return nil                                // nuốt phím, không cho lọt ra ngoài
        }
    }

    private func deafen() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
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
