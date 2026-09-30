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
                .onHover { hovering = $0 }
        }
    }
}

extension ButtonStyle where Self == HUDButtonStyle {
    static var hud: HUDButtonStyle { HUDButtonStyle() }
    static func hud(radius: CGFloat = Theme.Radius.control, selected: Bool = false) -> HUDButtonStyle {
        HUDButtonStyle(radius: radius, selected: selected)
    }
}
