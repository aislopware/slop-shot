import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Đồ hoạ dùng chung cho các lớp phủ toàn màn hình (chọn vùng, chọn màu):
// chip nền tối, kính lúp phóng pixel, đọc màu 1 pixel.
//
// Toàn bộ là SwiftUI. Hệ toạ độ của SwiftUI có gốc ở góc TRÊN-trái, trùng với
// hệ toạ độ ảnh CGImage → khỏi phải lật trục như hồi vẽ bằng NSView.
// ─────────────────────────────────────────────────────────────────────────
enum OverlayChrome {

    static let dimAlpha: CGFloat = 0.42
    static let chipFill = Color(.sRGB, red: 0.08, green: 0.09, blue: 0.11, opacity: 0.92)
    static let chipEdge = Color.white.opacity(0.14)
    static let guide    = Color(.sRGB, red: 0.24, green: 0.86, blue: 0.79, opacity: 1)
    static let radius: CGFloat = 8

    // ── Chip ─────────────────────────────────────────────────────────────

    static func chipFont(_ size: CGFloat) -> Font {
        .system(size: size, weight: .semibold).monospacedDigit()
    }

    /// Kích thước chip sẽ chiếm, tính trước để canh vị trí (SwiftUI đặt view
    /// theo TÂM, còn logic canh mép/kẹp trong màn hình thì làm theo góc).
    static func chipSize(_ text: String, fontSize: CGFloat = 12, swatch: Bool = false) -> CGSize {
        let s = text.size(withAttributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: fontSize, weight: .semibold)])
        return CGSize(width: s.width + 16 + (swatch ? fontSize + 5 : 0),
                      height: s.height + 10)
    }

    // ── Đọc pixel ────────────────────────────────────────────────────────

    /// Màu của đúng 1 pixel trong ảnh (toạ độ theo point, gốc trên-trái).
    static func pixelColor(in cg: CGImage, at p: CGPoint, scale: CGFloat) -> NSColor? {
        let x = Int(p.x * scale), y = Int(p.y * scale)
        guard x >= 0, y >= 0, x < cg.width, y < cg.height,
              let crop = cg.cropping(to: CGRect(x: x, y: y, width: 1, height: 1))
        else { return nil }
        // Phải cấp phát riêng: truyền `&mảng` vào CGContext là con trỏ chỉ hợp lệ
        // trong đúng lời gọi đó, dùng tiếp ở c.draw() là chạm bộ nhớ đã hết hạn.
        let px = UnsafeMutablePointer<UInt8>.allocate(capacity: 4)
        px.initialize(repeating: 0, count: 4)
        defer { px.deallocate() }
        guard let c = CGContext(data: px, width: 1, height: 1, bitsPerComponent: 8,
                                bytesPerRow: 4, space: CGColorSpaceCreateDeviceRGB(),
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        c.draw(crop, in: CGRect(x: 0, y: 0, width: 1, height: 1))
        return NSColor(srgbRed: CGFloat(px[0]) / 255, green: CGFloat(px[1]) / 255,
                       blue: CGFloat(px[2]) / 255, alpha: 1)
    }

    static func hex(of color: NSColor) -> String {
        let c = color.usingColorSpace(.sRGB) ?? color
        return String(format: "#%02X%02X%02X",
                      Int((c.redComponent * 255).rounded()),
                      Int((c.greenComponent * 255).rounded()),
                      Int((c.blueComponent * 255).rounded()))
    }

    // ── Kính lúp: chỗ đặt ────────────────────────────────────────────────

    /// Khung kính lúp: lệch khỏi con trỏ, tự lật khi sát mép màn hình.
    ///
    /// `reserveBelow` = khoảng trống cần chừa dưới kính cho nhãn, để khi con trỏ
    /// xuống sát đáy màn hình thì kính tự nhảy lên trên thay vì đè ra ngoài.
    static func loupeBox(cursor: CGPoint, in bounds: CGRect, side: CGFloat,
                         offset: CGFloat = 22, reserveBelow: CGFloat = 32) -> CGRect {
        var box = CGRect(x: cursor.x + offset, y: cursor.y + offset, width: side, height: side)
        if box.maxX > bounds.maxX - 12 { box.origin.x = cursor.x - offset - side }
        if box.maxY > bounds.maxY - reserveBelow { box.origin.y = cursor.y - offset - side }
        box.origin.x = max(12, box.origin.x)
        box.origin.y = max(12, box.origin.y)
        return box
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Chip nền tối bo góc, có thể kèm ô màu nhỏ phía trước.
// ─────────────────────────────────────────────────────────────────────────
struct ChipView: View {
    let text: String
    var fontSize: CGFloat = 12
    var accent = false
    var swatch: Color?

    var body: some View {
        HStack(spacing: 6) {
            if let swatch {
                let dot = fontSize - 1
                RoundedRectangle(cornerRadius: 2)
                    .fill(swatch)
                    .overlay(RoundedRectangle(cornerRadius: 2).stroke(.white.opacity(0.5), lineWidth: 1))
                    .frame(width: dot, height: dot)
            }
            Text(text)
                .font(OverlayChrome.chipFont(fontSize))
                .foregroundStyle(.white)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
        .background(accent ? Color(nsColor: .controlAccentColor).opacity(0.95) : OverlayChrome.chipFill,
                    in: RoundedRectangle(cornerRadius: OverlayChrome.radius))
        .overlay(RoundedRectangle(cornerRadius: OverlayChrome.radius)
            .stroke(OverlayChrome.chipEdge, lineWidth: 1))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Kính lúp: phóng to vùng quanh con trỏ từ ảnh đóng băng, kèm lưới pixel và ô
// đánh dấu đúng pixel đang trỏ. Bên gọi tự đặt chỗ bằng OverlayChrome.loupeBox.
// ─────────────────────────────────────────────────────────────────────────
struct LoupeView: View {
    let image: CGImage
    let cursor: CGPoint          // point, gốc trên-trái
    let scale: CGFloat           // point → pixel
    let side: CGFloat            // bề rộng kính (point)
    let zoom: CGFloat

    /// Số pixel nguồn mỗi cạnh của ô phóng.
    private var srcSide: CGFloat { max(1, (side / zoom * scale).rounded()) }

    /// Góc trên-trái của ô cắt trong ảnh (pixel), đã kẹp trong ảnh.
    private var origin: CGPoint? {
        let maxX = CGFloat(image.width) - srcSide, maxY = CGFloat(image.height) - srcSide
        guard maxX > 0, maxY > 0 else { return nil }
        return CGPoint(x: min(max(0, (cursor.x * scale - srcSide / 2).rounded()), maxX),
                       y: min(max(0, (cursor.y * scale - srcSide / 2).rounded()), maxY))
    }

    var body: some View {
        if let origin,
           let crop = image.cropping(to: CGRect(x: origin.x, y: origin.y,
                                                width: srcSide, height: srcSide)) {
            ZStack {
                Image(decorative: crop, scale: 1)
                    .resizable()
                    .interpolation(.none)          // giữ pixel sắc, không làm mượt
                Canvas { ctx, _ in
                    let step = side / srcSide
                    // Lưới pixel: mỗi ô = 1 pixel thật của màn hình.
                    if step >= 3 {
                        var grid = Path()
                        var k: CGFloat = 1
                        while k < srcSide {
                            grid.move(to: CGPoint(x: k * step, y: 0))
                            grid.addLine(to: CGPoint(x: k * step, y: side))
                            grid.move(to: CGPoint(x: 0, y: k * step))
                            grid.addLine(to: CGPoint(x: side, y: k * step))
                            k += 1
                        }
                        ctx.stroke(grid, with: .color(.white.opacity(0.10)), lineWidth: 0.5)
                    }
                    // Ô đánh dấu đúng pixel dưới con trỏ — viền đen + trắng cho
                    // nổi được trên cả nền sáng lẫn nền tối.
                    let ix = (cursor.x * scale).rounded(.down) - origin.x
                    let iy = (cursor.y * scale).rounded(.down) - origin.y
                    let cell = CGRect(x: ix * step, y: iy * step, width: step, height: step)
                    ctx.stroke(Path(cell.insetBy(dx: -1, dy: -1)),
                               with: .color(.black.opacity(0.9)), lineWidth: 1)
                    ctx.stroke(Path(cell), with: .color(.white), lineWidth: 1)
                }
            }
            .frame(width: side, height: side)
            .background(Color(white: 0.08))
            .clipShape(RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(OverlayChrome.chipEdge, lineWidth: 1))
            .shadow(color: .black.opacity(0.55), radius: 8)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Bảng gợi ý dưới đáy màn hình (2 dòng), dùng chung cho cả hai overlay.
// ─────────────────────────────────────────────────────────────────────────
struct OverlayHint: View {
    let title: String
    let subtitle: String

    var body: some View {
        VStack(spacing: 5) {
            Text(title)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
            Text(subtitle)
                .font(.system(size: 12))
                .foregroundStyle(.white.opacity(0.62))
        }
        .padding(.horizontal, 22)
        .padding(.vertical, 15)
        .background(OverlayChrome.chipFill, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).stroke(OverlayChrome.chipEdge, lineWidth: 1))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// NSHostingView chỉ để VẼ: mọi cú chuột xuyên thẳng qua nó xuống view bắt sự
// kiện nằm dưới. Overlay cần bắt chuột/phím thô (mouseMoved khi app không
// active, cursor rect, warp con trỏ) nên phần đó vẫn là NSView.
// ─────────────────────────────────────────────────────────────────────────
final class PassthroughHostingView<Content: View>: NSHostingView<Content> {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
