import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Thanh điều khiển khi ĐANG QUAY — bố cục giống CleanShot X:
//   [■ stop]  0:05  │  ⏸ pause   ↺ restart   🗑 discard   ≡ kéo
// Kéo chỗ trống / icon ≡ để di chuyển thanh (isMovableByWindowBackground).
//
// Panel borderless + nonactivating: bấm nút trên thanh KHÔNG kéo app đang quay
// mất focus. Nội dung là SwiftUI, panel co giãn theo `fittingSize` của nó nên
// bật/tắt nút mic là bề rộng tự khớp, không phải cộng trừ toạ độ bằng tay.
// ─────────────────────────────────────────────────────────────────────────

/// State của thanh, tách ra để SwiftUI theo dõi được (đồng hồ, pause, mic).
@MainActor
final class RecordingBarModel: ObservableObject {
    @Published var seconds = 0
    @Published var paused = false
    @Published var micMuted = false
    var micEnabled = false

    var onStop: (() -> Void)?
    var onPauseToggle: ((_ paused: Bool) -> Void)?
    var onRestart: (() -> Void)?
    var onDiscard: (() -> Void)?
    var onMicToggle: ((_ muted: Bool) -> Void)?

    private var timer: Timer?

    var timecode: String { String(format: "%d:%02d", seconds / 60, seconds % 60) }

    /// Về 0:00 và bỏ trạng thái tạm dừng (chỉ đổi hiển thị — người gọi tự lo
    /// phần bảo recorder chạy tiếp, nên KHÔNG bắn onPauseToggle ở đây).
    func reset() {
        seconds = 0
        paused = false
    }

    func startTimer() {
        stopTimer()
        let t = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.paused else { return }   // pause thì đồng hồ đứng
                self.seconds += 1
            }
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    func stopTimer() {
        timer?.invalidate()
        timer = nil
    }
}

@MainActor
final class RecordingBarController {
    private var panel: NSPanel?
    private let model = RecordingBarModel()

    // Có đang thu mic hay không — đặt TRƯỚC show(). Không thu thì nút mic ẩn
    // luôn, chứ không hiện ra một nút bấm vào chẳng để làm gì.
    var micEnabled: Bool {
        get { model.micEnabled }
        set { model.micEnabled = newValue }
    }

    var onStop: (() -> Void)? {
        get { model.onStop } set { model.onStop = newValue }
    }
    var onPauseToggle: ((_ paused: Bool) -> Void)? {
        get { model.onPauseToggle } set { model.onPauseToggle = newValue }
    }
    var onRestart: (() -> Void)? {
        get { model.onRestart } set { model.onRestart = newValue }
    }
    var onDiscard: (() -> Void)? {
        get { model.onDiscard } set { model.onDiscard = newValue }
    }
    var onMicToggle: ((_ muted: Bool) -> Void)? {
        get { model.onMicToggle } set { model.onMicToggle = newValue }
    }

    func show(below rect: CGRect, on screen: NSScreen) {
        hide()
        model.paused = false
        model.seconds = 0
        model.micMuted = false

        // Hỏi SwiftUI xem thanh cần rộng bao nhiêu rồi mới đặt panel.
        let host = NSHostingView(rootView: RecordingBarView(model: model))
        let size = host.fittingSize
        let w = size.width, h = size.height

        // Đặt thanh ngay DƯỚI viền vùng quay (rect: gốc trên-trái của màn hình).
        // Đổi sang toạ độ AppKit (gốc dưới-trái) để định vị panel.
        let gap: CGFloat = 12
        let regionBottomY = screen.frame.minY + (screen.frame.height - rect.maxY)
        let regionTopY    = screen.frame.minY + (screen.frame.height - rect.minY)

        var y = regionBottomY - gap - h          // mặc định: ngay dưới vùng quay
        if y < screen.visibleFrame.minY + 6 {     // sát đáy → lật lên trên vùng quay
            y = regionTopY + gap
        }
        y = min(max(y, screen.visibleFrame.minY + 6), screen.visibleFrame.maxY - h - 6)

        let cx = screen.frame.minX + rect.midX - w / 2
        let x = min(max(cx, screen.visibleFrame.minX + 6), screen.visibleFrame.maxX - w - 6)
        let frame = NSRect(x: x, y: y, width: w, height: h)

        let p = NSPanel(contentRect: frame,
                        styleMask: [.borderless, .nonactivatingPanel],
                        backing: .buffered, defer: false)
        p.level = .floating
        p.backgroundColor = .clear
        p.isOpaque = false
        p.hasShadow = true
        p.isMovableByWindowBackground = true   // kéo nền để di chuyển thanh
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        p.contentView = host
        p.orderFrontRegardless()

        self.panel = p
        model.startTimer()
    }

    func resetTimer() { model.reset() }

    func hide() {
        model.stopTimer()
        panel?.orderOut(nil)
        panel = nil
    }
}

private struct RecordingBarView: View {
    @ObservedObject var model: RecordingBarModel

    var body: some View {
        HStack(spacing: 8) {
            // ■ Stop — ô vuông đỏ bo nhẹ.
            iconButton("stop.fill", "Stop & save", tint: Color(nsColor: .systemRed), size: 22) {
                model.onStop?()
            }

            Text(model.timecode)
                .font(.system(size: 13, weight: .semibold).monospacedDigit())
                .foregroundStyle(.white)
                .frame(width: 40, alignment: .leading)

            Rectangle().fill(.white.opacity(0.18)).frame(width: 1, height: 20)

            // ⏸ Pause / ▶ Resume (toggle).
            iconButton(model.paused ? "play.fill" : "pause.fill",
                       model.paused ? "Resume" : "Pause") {
                model.paused.toggle()
                model.onPauseToggle?(model.paused)
            }

            iconButton("arrow.counterclockwise", "Restart") {
                model.reset()                 // quay lại từ đầu → đồng hồ về 0:00
                model.onRestart?()
            }
            iconButton("trash", "Discard (don't save)") { model.onDiscard?() }

            // 🎤 Tắt/bật mic giữa chừng (chỉ hiện khi lần quay này CÓ thu mic).
            if model.micEnabled {
                iconButton(model.micMuted ? "mic.slash.fill" : "mic.fill",
                           model.micMuted ? "Unmute microphone" : "Mute microphone",
                           tint: model.micMuted ? Color(nsColor: .systemRed) : .white) {
                    model.micMuted.toggle()
                    model.onMicToggle?(model.micMuted)
                }
            }

            // ≡ Tay kéo (chỉ để nhìn — kéo nền là di chuyển được rồi).
            Image(systemName: "line.3.horizontal")
                .font(.system(size: 12, weight: .bold))
                .foregroundStyle(.white.opacity(0.35))
        }
        .padding(.horizontal, 10)
        .frame(height: 38)
        .background(Color(white: 0.13).opacity(0.96), in: RoundedRectangle(cornerRadius: 11))
    }

    private func iconButton(_ symbol: String, _ tip: String,
                            tint: Color = .white, size: CGFloat = 24,
                            action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: size * 0.55, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: size, height: size)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(tip)
    }
}
