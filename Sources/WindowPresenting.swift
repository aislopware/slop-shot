import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Đưa một cửa sổ lên TRƯỚC MẶT khi app đang không active.
//
// Từ macOS 14, activate là "xin", không phải "ra lệnh": app khác đang active
// (terminal, trình duyệt…) mà người dùng chưa bấm gì vào SlopShot thì macOS từ
// chối. Mở cửa sổ ngay sau một phím tắt (OCR, editor) là đúng ca đó: cửa sổ có
// tạo ra, nằm trên màn hình, nhưng SAU app đang dùng — nhìn như "không hiện".
// orderFrontRegardless cũng không vượt qua được app active.
//
// Nên: mở ở level .floating (nổi trên cửa sổ thường của mọi app), xin activate,
// và khi app thật sự active (người dùng bấm vào cửa sổ) thì hạ về .normal để nó
// cư xử như cửa sổ bình thường, không đè mãi lên app khác.
// ─────────────────────────────────────────────────────────────────────────
extension NSWindow {
    @MainActor
    func presentInFront() {
        NSApp.activate()
        if NSApp.isActive {
            makeKeyAndOrderFront(nil)
            return
        }
        level = .floating
        makeKeyAndOrderFront(nil)
        orderFrontRegardless()

        var token: NSObjectProtocol?
        token = NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.level = .normal
                token.map(NotificationCenter.default.removeObserver)
                token = nil
            }
        }
    }
}
