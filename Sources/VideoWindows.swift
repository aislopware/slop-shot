import SwiftUI
import AVKit
import AVFoundation

// ─────────────────────────────────────────────────────────────────────────
// "Quick Look" cho clip vừa quay: một màn xem nhanh ngay trong app, không có
// nút sửa gì cả. Muốn cắt/zoom/blur/xuất file thì bấm ✂️ để mở Video Editor
// (VideoEditorView) — trước đây chỗ này còn một cửa sổ Trim riêng, giờ đã gộp
// hết vào editor để khỏi có hai giao diện làm gần giống nhau.
//
// Ở đây dùng `VideoPlayer` của SwiftUI (không phải AVPlayerView bọc lại): màn
// này CẦN thanh điều khiển mặc định của hệ thống, mà đó đúng là thứ VideoPlayer
// cho sẵn. (Trong Video Editor thì ngược lại — cần mặt phát trần nên vẫn phải
// mượn AVPlayerView.)
//
// App chạy nền (.accessory); mở cửa sổ → tạm .regular để có focus, đóng → .accessory.
// ─────────────────────────────────────────────────────────────────────────

@MainActor
final class VideoPlayerWindowController: NSObject, NSWindowDelegate {
    private var window: NSWindow?
    private var player: AVPlayer?

    func open(url: URL) {
        window?.close()

        let player = AVPlayer(url: url)
        let host = NSHostingView(rootView: QuickLookView(player: player))

        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 720, height: 460),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered, defer: false)
        win.title = "Quick Look — \(url.lastPathComponent)"
        win.appearance = NSAppearance(named: .darkAqua)
        win.contentView = host
        win.minSize = NSSize(width: 420, height: 280)
        win.delegate = self
        win.isReleasedWhenClosed = false
        self.window = win
        self.player = player

        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        win.center()
        win.makeKeyAndOrderFront(nil)
        player.play()
    }

    // Đóng màn Quick Look (gọi khi bắt đầu chụp/quay mới).
    @discardableResult
    func dismiss() -> Bool {
        guard window != nil else { return false }
        window?.close()      // windowWillClose lo dọn + trả .accessory
        return true
    }

    func windowWillClose(_ notification: Notification) {
        player?.pause()
        player = nil
        window = nil
        NSApp.setActivationPolicy(.accessory)
    }
}

private struct QuickLookView: View {
    let player: AVPlayer

    var body: some View {
        VideoPlayer(player: player)
            .background(.black)
            .ignoresSafeArea()
    }
}
