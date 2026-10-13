import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Quy chuẩn giao diện dùng chung: bo góc, viền, nền kính mờ, kiểu nút.
//
// Trước đây mỗi màn hình tự chọn số: bo góc có tới hơn 10 cỡ (2…14), nền tối
// 3 kiểu, viền 4 độ sáng — đặt cạnh nhau là thấy "mỗi cái một nhà". Mọi thứ
// mới nên lấy số từ đây; muốn đổi cả app thì đổi ở đây.
// ─────────────────────────────────────────────────────────────────────────
enum Theme {
    enum Radius {
        /// Ô nhỏ: swatch màu, ảnh thu nhỏ trong danh sách, phím tắt.
        static let small: CGFloat = 6
        /// Nút, ô công cụ.
        static let control: CGFloat = 8
        /// Thanh công cụ, chip lớn, thẻ nổi.
        static let bar: CGFloat = 12
        /// Bảng lớn: gợi ý, kính lúp, thẻ thumbnail.
        static let panel: CGFloat = 14
    }

    /// Viền mảnh quanh mọi bề mặt tối — tách khỏi nền phía sau dù sáng hay tối.
    static let edge = Color.white.opacity(0.14)
    /// Nền của nút khi rê chuột / khi bấm.
    static let hoverFill = Color.white.opacity(0.10)
    static let pressFill = Color.white.opacity(0.18)
    /// Mặt nền đặc cho vùng làm việc của editor (không cần trong suốt).
    static let surface = Color(white: 0.11)
    static let surfaceRaised = Color(white: 0.13)

    static let quick = Animation.easeOut(duration: 0.14)
    static let appear = Animation.spring(response: 0.28, dampingFraction: 0.86)
}

// ─────────────────────────────────────────────────────────────────────────
// Nền kính mờ kiểu macOS. `.behindWindow` làm mờ cả những gì nằm SAU cửa sổ
// (app khác, desktop) — cần cho các panel nổi như thanh quay, thumbnail. Trong
// overlay có ảnh đóng băng thì `.withinWindow` là đủ (ảnh nằm ngay trong cửa sổ).
// ─────────────────────────────────────────────────────────────────────────
struct VisualEffect: NSViewRepresentable {
    var material: NSVisualEffectView.Material = .hudWindow
    var blending: NSVisualEffectView.BlendingMode = .behindWindow

    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = material
        v.blendingMode = blending
        v.state = .active                    // mờ cả khi app không active (app nền luôn thế)
        return v
    }

    func updateNSView(_ v: NSVisualEffectView, context: Context) {
        v.material = material
        v.blendingMode = blending
    }
}

extension View {
    /// Nền HUD: kính mờ tối + lớp phủ tối nhẹ cho chữ trắng luôn đọc được + viền mảnh.
    func hudBackground(radius: CGFloat = Theme.Radius.bar,
                       blending: NSVisualEffectView.BlendingMode = .behindWindow) -> some View {
        background(
            ZStack {
                VisualEffect(material: .hudWindow, blending: blending)
                Color.black.opacity(0.28)
            }
            .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        )
        .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous)
            .strokeBorder(Theme.edge, lineWidth: 1))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Nút icon trên nền tối: sáng lên khi rê chuột, lún nhẹ khi bấm. Trước đây
// hầu hết nút đứng im dù rê hay bấm — cảm giác như ảnh chụp giao diện.
// ─────────────────────────────────────────────────────────────────────────
struct HUDButtonStyle: ButtonStyle {
    var radius: CGFloat = Theme.Radius.control
    /// Nút đang ở trạng thái "chọn" (công cụ đang dùng…) đã có nền riêng thì
    /// không phủ thêm nền hover lên nữa.
    var selected = false

    func makeBody(configuration: Configuration) -> some View {
        HUDButtonBody(configuration: configuration, radius: radius, selected: selected)
    }

    private struct HUDButtonBody: View {
        let configuration: Configuration
        let radius: CGFloat
        let selected: Bool
        @State private var hovering = false
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            configuration.label
                .background(
                    RoundedRectangle(cornerRadius: radius, style: .continuous)
                        .fill(selected || !enabled ? Color.clear
                              : configuration.isPressed ? Theme.pressFill
                              : hovering ? Theme.hoverFill : .clear))
                .scaleEffect(configuration.isPressed ? 0.94 : 1)
                .animation(Theme.quick, value: hovering)
                .animation(Theme.quick, value: configuration.isPressed)
                .onHoverAlways { hovering = $0 }
        }
    }
}

extension ButtonStyle where Self == HUDButtonStyle {
    static var hud: HUDButtonStyle { HUDButtonStyle() }
    static func hud(radius: CGFloat = Theme.Radius.control, selected: Bool = false) -> HUDButtonStyle {
        HUDButtonStyle(radius: radius, selected: selected)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// `.onHover` và `.help` của SwiftUI chỉ chạy khi app đang ACTIVE. Editor inline
// nằm trong lớp phủ nonactivating (app vẫn nền) → rê chuột không sáng nút, không
// hiện tooltip; tooltip hệ thống còn nằm dưới level .screenSaver của lớp phủ.
// Nên hover tự gắn tracking area `.activeAlways`, tooltip tự vẽ bằng SwiftUI.
// ─────────────────────────────────────────────────────────────────────────
private struct HoverTracker: NSViewRepresentable {
    let onChange: (Bool) -> Void

    func makeNSView(context: Context) -> TrackingView { TrackingView() }
    func updateNSView(_ v: TrackingView, context: Context) { v.onChange = onChange }

    final class TrackingView: NSView {
        var onChange: ((Bool) -> Void)?

        override func updateTrackingAreas() {
            super.updateTrackingAreas()
            trackingAreas.forEach(removeTrackingArea)
            addTrackingArea(NSTrackingArea(rect: .zero,
                                           options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                           owner: self, userInfo: nil))
        }
        override func mouseEntered(with event: NSEvent) { onChange?(true) }
        override func mouseExited(with event: NSEvent) { onChange?(false) }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
    }
}

private struct HUDTooltip: ViewModifier {
    let text: String
    @State private var hovering = false
    @State private var shown = false

    func body(content: Content) -> some View {
        content
            .onHoverAlways { hovering = $0 }
            .simultaneousGesture(TapGesture().onEnded { hovering = false })
            .overlay(alignment: .bottom) {
                if shown {
                    Text(text)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .hudBackground(radius: Theme.Radius.small, blending: .withinWindow)
                        .fixedSize()
                        .alignmentGuide(.bottom) { $0[.top] - 8 }
                        .allowsHitTesting(false)
                        .transition(.opacity)
                }
            }
            .zIndex(shown ? 1 : 0)
            .task(id: hovering) {
                guard hovering else { shown = false; return }
                try? await Task.sleep(for: .milliseconds(450))
                guard !Task.isCancelled else { return }
                withAnimation(Theme.quick) { shown = true }
            }
            .onDisappear { hovering = false }
    }
}

extension View {
    /// `.onHover` chạy cả khi app không active.
    func onHoverAlways(_ action: @escaping (Bool) -> Void) -> some View {
        background(HoverTracker(onChange: action))
    }

    /// Tooltip nổi ngay dưới nút, hiện cả trong lớp phủ inline (xem HoverTracker).
    func tooltip(_ text: String) -> some View {
        modifier(HUDTooltip(text: text))
    }
}
