import SwiftUI

// Nội dung xổ ra khi bấm icon SlopShot trên thanh menu.
// Dùng Label (icon + chữ) + .keyboardShortcut (hiện phím tắt canh phải) + Section
// để trông gọn gàng như menu CleanShot, thay vì các dòng text trơn.
struct MenuContent: View {
    @ObservedObject var capturer: ScreenCapturer
    // Đọc cấu hình phím tắt hiện tại → hint trong menu luôn khớp với phím đã
    // rebind (setHotkey publish thay đổi → MenuContent vẽ lại).
    @ObservedObject private var settings = AppSettings.shared

    var body: some View {
        // ── Nhóm chụp / quay ──────────────────────────────────────────────
        Section {
            Button { Task { await capturer.captureRegion() } } label: {
                Label("Capture Area", systemImage: "crop")
            }
            .shortcut(settings.hotkey(for: .captureArea))

            Button { Task { await capturer.captureFullScreen() } } label: {
                Label("Capture Fullscreen", systemImage: "display")
            }
            .shortcut(settings.hotkey(for: .captureFullscreen))

            Button { Task { await capturer.captureScrollingArea() } } label: {
                Label("Capture Scrolling Area", systemImage: "rectangle.and.arrow.up.right.and.arrow.down.left")
            }
            .shortcut(settings.hotkey(for: .captureScrolling))

            Button { Task { await capturer.captureText() } } label: {
                Label("Capture Text (OCR)", systemImage: "text.viewfinder")
            }
            .shortcut(settings.hotkey(for: .captureText))

            Button { Task { await capturer.pickColor() } } label: {
                Label("Pick Color", systemImage: "eyedropper")
            }
            .shortcut(settings.hotkey(for: .pickColor))

            if capturer.isRecording {
                Button { Task { await capturer.stopRecording() } } label: {
                    Label("Stop Recording", systemImage: "stop.circle")
                }
            } else {
                Button { Task { await capturer.recordRegion() } } label: {
                    Label("Record Area", systemImage: "video")
                }
                .shortcut(settings.hotkey(for: .recordArea))
            }
        }

        Divider()

        // ── Nhóm thao tác với ảnh/clip vừa tạo ────────────────────────────
        if let url = capturer.lastSavedURL {
            Section {
                if capturer.canEditLast {
                    Button { capturer.openLastInEditor() } label: {
                        Label("Edit Last Screenshot", systemImage: "pencil.tip.crop.circle")
                    }
                }
                Button { NSWorkspace.shared.open(url) } label: {
                    Label("Open Last File", systemImage: "arrow.up.forward.app")
                }
            }
            Divider()
        }

        // ── History + Settings ────────────────────────────────────────────
        Section {
            Button { capturer.showHistory() } label: {
                Label("Capture History…", systemImage: "clock.arrow.circlepath")
            }
            Button { capturer.showSettings() } label: {
                Label("Settings…", systemImage: "gearshape")
            }
            .keyboardShortcut(",", modifiers: .command)
        }

        Divider()

        // About: panel chuẩn của macOS, tự hiện app icon (logo) + tên + version.
        // activate trước để panel nổi lên trước (app nền .accessory nếu không sẽ ẩn sau).
        Button {
            NSApp.activate(ignoringOtherApps: true)
            NSApp.orderFrontStandardAboutPanel(nil)
        } label: {
            Label("About SlopShot", systemImage: "info.circle")
        }

        Button(role: .destructive) { NSApp.terminate(nil) } label: {
            Label("Quit SlopShot", systemImage: "power")
        }
        .keyboardShortcut("q")
    }
}

private extension View {
    // Gắn keyboardShortcut từ 1 Hotkey (nếu quy đổi được sang KeyEquivalent).
    @ViewBuilder
    func shortcut(_ hk: Hotkey) -> some View {
        if let ke = hk.keyEquivalent {
            keyboardShortcut(ke, modifiers: hk.swiftUIModifiers)
        } else {
            self
        }
    }
}
