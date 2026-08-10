import AppKit
import SwiftUI

// Vỏ cửa sổ AppKit, nhúng VideoEditorView (SwiftUI) — giống hệt cách
// EditorWindowController bọc EditorView. Mọi state + logic nằm ở VideoEditStore,
// file này chỉ lo: mở cửa sổ, giữ store sống, đóng thì dọn.
//
// Dùng NSHostingView làm contentView TRỰC TIẾP (không qua contentViewController)
// để SwiftUI phủ kín cả vùng titlebar — nếu không, safe-area của titlebar sẽ
// đẩy nội dung xuống làm hụt mất thanh công cụ.

/// Một cú click ghi lại lúc quay — dùng cho Auto Zoom.
struct RecordedClick {
    let time: Double          // giây tính từ đầu clip
    let point: CGPoint        // 0…1 trong vùng quay, gốc TRÊN-TRÁI
}

@MainActor
final class VideoEditorWindowController: NSObject, NSWindowDelegate {
    private var window: NSWindow?
    private var store: VideoEditStore?

    /// Gọi khi xuất file xong (URL file vừa ghi).
    var onDone: ((URL?) -> Void)?

    func open(url: URL, saveFolder: URL, clicks: [RecordedClick] = []) {
        window?.close()

        let store = VideoEditStore(url: url, saveFolder: saveFolder, clicks: clicks)
        store.onDone = { [weak self] out in self?.onDone?(out) }
        self.store = store

        let root = VideoEditorView(store: store,
                                   onClose: { [weak self] in self?.window?.close() })
        let host = NSHostingView(rootView: root)

        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1160, height: 780),
            styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
            backing: .buffered, defer: false)
        win.title = "SlopShot — Video Editor"
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        // KHÔNG kéo cửa sổ bằng nền: kéo trên video/timeline mà cửa sổ chạy theo
        // thì không vẽ được vùng. Vẫn kéo được bằng chỗ trống trên thanh trên cùng.
        win.isMovableByWindowBackground = false
        win.appearance = NSAppearance(named: .darkAqua)
        win.contentView = host
        win.minSize = NSSize(width: 940, height: 640)
        win.delegate = self
        win.isReleasedWhenClosed = false
        window = win

        // Ẩn 3 nút đèn giao thông — đóng bằng nút "Done" cho thống nhất với editor ảnh.
        win.standardWindowButton(.closeButton)?.isHidden = true
        win.standardWindowButton(.miniaturizeButton)?.isHidden = true
        win.standardWindowButton(.zoomButton)?.isHidden = true

        // App đang là menu-bar (.accessory). Tạm chuyển .regular để có cửa sổ + focus.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        win.center()
        win.makeKeyAndOrderFront(nil)
    }

    /// Đóng editor đang mở mà KHÔNG xuất file (bắt đầu chụp mới thì tự "Done"
    /// cái cũ). Trả về true nếu vừa đóng một cửa sổ.
    @discardableResult
    func dismiss() -> Bool {
        guard window != nil else { return false }
        window?.close()      // windowWillClose lo dọn + trả về .accessory
        return true
    }

    var isOpen: Bool { window != nil }

    func windowWillClose(_ notification: Notification) {
        // VideoEditorView.onDisappear cũng gọi tearDown(), nhưng gọi thẳng ở đây
        // cho chắc: player phải nhả file .mov ra ngay lúc đóng.
        store?.tearDown()
        store = nil
        window = nil
        NSApp.setActivationPolicy(.accessory)
    }
}
