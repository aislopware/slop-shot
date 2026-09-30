import AppKit
import SwiftUI

// Vỏ cửa sổ AppKit, nhúng EditorView (SwiftUI).
// Dùng NSHostingView làm contentView TRỰC TIẾP (không qua contentViewController)
// để SwiftUI phủ kín cả vùng titlebar — tránh việc safe-area của titlebar
// đẩy nội dung làm mất toolbar.
@MainActor
final class EditorWindowController: NSObject, NSWindowDelegate {
    private var window: NSWindow?

    func open(image: NSImage, sourceURL: URL?) {
        window?.close()

        let root = EditorView(image: image, sourceURL: sourceURL,
                              onClose: { [weak self] in self?.window?.close() })
        let host = NSHostingView(rootView: root)

        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1040, height: 740),
            styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
            backing: .buffered, defer: false)
        win.title = "SlopShot — Editor"
        win.titlebarAppearsTransparent = true   // titlebar trong suốt
        win.titleVisibility = .hidden
        // KHÔNG cho kéo cửa sổ bằng nền — nếu bật, kéo trên canvas sẽ di chuyển
        // cả cửa sổ và double-click nền sẽ zoom/thu nhỏ. Vẫn kéo được bằng vùng
        // trống trên toolbar (đó là vùng titlebar).
        win.isMovableByWindowBackground = false
        win.appearance = NSAppearance(named: .darkAqua)  // tông tối như CleanShot
        win.contentView = host                  // SwiftUI phủ toàn bộ, kể cả titlebar
        win.minSize = NSSize(width: 760, height: 500)
        win.delegate = self
        win.isReleasedWhenClosed = false
        window = win

        // App đang là menu-bar (.accessory). Tạm chuyển .regular để có cửa sổ + focus.
        // Ẩn 3 nút đèn giao thông (close/minimize/zoom). Đóng bằng nút "Done".
        win.standardWindowButton(.closeButton)?.isHidden = true
        win.standardWindowButton(.miniaturizeButton)?.isHidden = true
        win.standardWindowButton(.zoomButton)?.isHidden = true

        NSApp.setActivationPolicy(.regular)
        win.center()
        win.presentInFront()
    }

    // Đóng editor đang mở mà KHÔNG lưu (giống CleanShot: bắt đầu chụp mới thì
    // tự "Done" cái cũ). Trả về true nếu vừa đóng một editor.
    @discardableResult
    func dismiss() -> Bool {
        guard window != nil else { return false }
        window?.close()      // windowWillClose lo dọn + trả về .accessory
        return true
    }

    var isOpen: Bool { window != nil }

    func windowWillClose(_ notification: Notification) {
        window = nil
        NSApp.setActivationPolicy(.accessory)
    }
}
