import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Hiện vòng tròn "nảy ra" tại mỗi cú click chuột khi đang quay (giống CleanShot)
// → người xem video thấy rõ bạn bấm ở đâu.
//
// Bắt click bằng GLOBAL monitor (thấy click ở app khác — đúng cái mình quay).
// Mỗi click tạo 1 cửa sổ nhỏ trong suốt, vẽ vòng tròn animate rồi tự huỷ.
// Cửa sổ này NẰM TRONG vùng quay nên ĐƯỢC ghi vào video (khác dim/viền ở ngoài).
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class ClickEffectController {
    private var monitor: Any?
    private var region: CGRect = .zero   // vùng quay (toạ độ AppKit toàn cục)

    // NHẬT KÝ CLICK: mỗi cú bấm ghi lại "giây thứ mấy của clip" + "bấm chỗ nào
    // trong khung hình". Video Editor lấy đúng danh sách này để làm Auto Zoom —
    // quay xong bấm một nút là có sẵn các nhịp phóng to vào chỗ vừa thao tác.
    private(set) var clicks: [RecordedClick] = []
    private var startedAt: Date?
    private var pausedAt: Date?
    private var pausedTotal: TimeInterval = 0

    // Bắt đầu lắng nghe click, chỉ hiện hiệu ứng trong `region`.
    func start(in region: CGRect) {
        stop()
        self.region = region
        clicks = []
        startedAt = Date()
        pausedAt = nil
        pausedTotal = 0
        monitor = NSEvent.addGlobalMonitorForEvents(
            matching: [.leftMouseDown, .rightMouseDown]
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                let p = NSEvent.mouseLocation               // gốc dưới-trái, toàn cục
                if NSPointInRect(p, self.region) {
                    self.ripple(at: p)
                    self.log(click: p)
                }
            }
        }
    }

    // Quay đang tạm dừng thì đồng hồ của clip cũng đứng → trừ đúng khoảng đó ra,
    // nếu không mốc zoom sẽ trôi dần khỏi chỗ thật sự bấm.
    func setPaused(_ paused: Bool) {
        guard startedAt != nil else { return }
        if paused {
            if pausedAt == nil { pausedAt = Date() }
        } else if let since = pausedAt {
            pausedTotal += Date().timeIntervalSince(since)
            pausedAt = nil
        }
    }

    // Dừng lắng nghe nhưng GIỮ nhật ký lại — editor mở sau khi stop() mới đọc.
    func stop() {
        if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        pausedAt = nil
    }

    private func log(click p: NSPoint) {
        guard let startedAt, region.width > 0, region.height > 0 else { return }
        let t = Date().timeIntervalSince(startedAt) - pausedTotal
        guard t >= 0 else { return }
        // Chuẩn hoá về 0…1 theo khung hình, GỐC TRÊN-TRÁI (giống toạ độ video).
        let x = (p.x - region.minX) / region.width
        let y = 1 - (p.y - region.minY) / region.height
        clicks.append(RecordedClick(time: t, point: CGPoint(x: x, y: y)))
    }

    // Vẽ 1 vòng tròn xanh phình to + mờ dần tại điểm click (giống CleanShot).
    private func ripple(at point: NSPoint) {
        let size: CGFloat = 56
        let win = NSWindow(contentRect: NSRect(x: point.x - size / 2, y: point.y - size / 2,
                                               width: size, height: size),
                           styleMask: .borderless, backing: .buffered, defer: false)
        win.level = .screenSaver           // trên mọi thứ → chắc chắn vào khung quay
        win.backgroundColor = .clear
        win.isOpaque = false
        win.hasShadow = false
        win.ignoresMouseEvents = true
        win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        win.contentView = NSHostingView(rootView: RippleView())
        win.orderFrontRegardless()

        // Huỷ cửa sổ sau khi animate xong (giữ ref qua closure để không bị thu sớm).
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            win.orderOut(nil)
        }
    }
}

/// Vòng tròn phình từ 0.3→1.0 + mờ dần về 0 trong 0.45s, tự chạy khi hiện ra.
private struct RippleView: View {
    @State private var grown = false

    var body: some View {
        Circle()
            .fill(Color(nsColor: .systemBlue).opacity(0.22))
            .overlay {
                Circle().stroke(Color(nsColor: .systemBlue).opacity(0.95), lineWidth: 2.5)
            }
            .padding(3)
            .scaleEffect(grown ? 1 : 0.3)
            .opacity(grown ? 0 : 1)
            .task {
                // Đổi state trong .task (sau khi đã vẽ khung đầu) — đặt ngay ở
                // onAppear thì SwiftUI gộp vào lần vẽ đầu và mất animation.
                withAnimation(.easeOut(duration: 0.45)) { grown = true }
            }
    }
}
