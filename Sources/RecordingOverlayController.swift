import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Lớp phủ FOCUS khi đang quay: tối xung quanh + khoét trong suốt đúng vùng
// đang quay + viền quanh vùng. Cho click xuyên qua (ignoresMouseEvents) để
// vẫn thao tác được app đang quay.
//
// dim + viền vẽ NGOÀI vùng quay → KHÔNG lọt vào video (SCStream chỉ crop
// đúng vùng qua sourceRect; vùng quay ở đây trong suốt nên quay ra nội dung thật).
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class RecordingOverlayController {
    private var window: NSWindow?

    func show(rect: CGRect, on screen: NSScreen) {
        hide()

        let host = NSHostingView(rootView: RecordingFocusView(region: rect))
        host.frame = NSRect(origin: .zero, size: screen.frame.size)

        let win = NSWindow(contentRect: screen.frame, styleMask: .borderless,
                           backing: .buffered, defer: false)
        win.level = .floating
        win.backgroundColor = .clear
        win.isOpaque = false
        win.hasShadow = false
        win.ignoresMouseEvents = true   // click xuyên qua: vẫn dùng được app đang quay
        win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        win.contentView = host
        win.orderFrontRegardless()
        window = win
    }

    func hide() {
        window?.orderOut(nil)
        window = nil
    }
}

/// `region` dùng gốc TRÊN-TRÁI (như rect trả về từ overlay chọn vùng) — trùng
/// luôn với hệ toạ độ của SwiftUI nên khỏi lật gì cả.
private struct RecordingFocusView: View {
    let region: CGRect

    var body: some View {
        GeometryReader { geo in
            ZStack {
                // Tối cả màn, khoét thủng đúng vùng đang quay (even-odd).
                Path { p in
                    p.addRect(CGRect(origin: .zero, size: geo.size))
                    p.addRect(region)
                }
                .fill(.black.opacity(0.35), style: FillStyle(eoFill: true))

                // Viền đỏ = đang quay. Nới ra 1.5px để nét viền nằm NGOÀI khung
                // quay, không lọt vào video.
                Path(region.insetBy(dx: -1.5, dy: -1.5))
                    .stroke(Color(nsColor: .systemRed).opacity(0.9), lineWidth: 2)
            }
            .ignoresSafeArea()
        }
    }
}
