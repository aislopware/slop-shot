import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Đồ hoạ dùng chung cho các lớp phủ toàn màn hình (chọn vùng, chọn màu):
// chip nền tối, kính lúp phóng pixel, đọc màu 1 pixel.
//
// Cả hai overlay đều vẽ trên view LẬT TRỤC Y (gốc góc trên-trái, khớp toạ độ
// ảnh CGImage), nên mọi hàm ở đây đều giả định đang ở trong ngữ cảnh đó.
// ─────────────────────────────────────────────────────────────────────────
enum OverlayChrome {

    static let dimAlpha: CGFloat = 0.42
    static let chipFill = NSColor(srgbRed: 0.08, green: 0.09, blue: 0.11, alpha: 0.92)
    static let chipEdge = NSColor(white: 1, alpha: 0.14)
    static let guide    = NSColor(srgbRed: 0.24, green: 0.86, blue: 0.79, alpha: 1)
    static let radius: CGFloat = 8

    // ── Chip ─────────────────────────────────────────────────────────────

    static func chipFont(_ size: CGFloat) -> NSFont {
        .monospacedDigitSystemFont(ofSize: size, weight: .semibold)
    }

    /// Bề rộng chip sẽ chiếm, tính trước để canh giữa được.
    static func chipWidth(_ text: String, fontSize: CGFloat = 12, swatch: Bool = false) -> CGFloat {
        text.size(withAttributes: [.font: chipFont(fontSize)]).width + 16
            + (swatch ? fontSize + 5 : 0)
    }

    /// Chip nền tối bo góc, có thể kèm ô màu nhỏ phía trước.
    @discardableResult
    static func drawChip(_ text: String, at origin: CGPoint, fontSize: CGFloat = 12,
                         accent: Bool = false, swatch: NSColor? = nil) -> CGRect {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: chipFont(fontSize), .foregroundColor: NSColor.white,
        ]
        let size = text.size(withAttributes: attrs)
        let padX: CGFloat = 8, padY: CGFloat = 5
        let dot: CGFloat = swatch == nil ? 0 : fontSize - 1
        let gap: CGFloat = swatch == nil ? 0 : 6
        let box = CGRect(x: origin.x, y: origin.y,
                         width: size.width + padX * 2 + dot + gap,
                         height: size.height + padY * 2)

        let shape = NSBezierPath(roundedRect: box, xRadius: radius, yRadius: radius)
        (accent ? NSColor.controlAccentColor.withAlphaComponent(0.95) : chipFill).setFill()
        shape.fill()
        chipEdge.setStroke()
        shape.lineWidth = 1
        shape.stroke()

        var textX = box.minX + padX
        if let swatch {
            let sq = CGRect(x: textX, y: box.midY - dot / 2, width: dot, height: dot)
            swatch.setFill()
            NSBezierPath(roundedRect: sq, xRadius: 2, yRadius: 2).fill()
            NSColor(white: 1, alpha: 0.5).setStroke()
            NSBezierPath(roundedRect: sq, xRadius: 2, yRadius: 2).stroke()
            textX += dot + gap
        }
        text.draw(at: CGPoint(x: textX, y: box.minY + padY), withAttributes: attrs)
        return box
    }

    // ── Kính lúp ─────────────────────────────────────────────────────────

    /// Phóng to vùng quanh con trỏ từ ảnh đóng băng, kèm lưới pixel và ô đánh
    /// dấu đúng pixel đang trỏ. Trả về khung đã vẽ để bên gọi đặt nhãn quanh nó
    /// (nil = không vẽ được, vd. ảnh nhỏ hơn cửa sổ phóng).
    ///
    /// `reserveBelow` = khoảng trống cần chừa dưới kính cho nhãn, để khi con trỏ
    /// xuống sát đáy màn hình thì kính tự nhảy lên trên thay vì đè ra ngoài.
    @discardableResult
    static func drawLoupe(image cg: CGImage, cursor: NSPoint, scale: CGFloat,
                          in bounds: CGRect, side: CGFloat, zoom: CGFloat,
                          offset: CGFloat = 22, reserveBelow: CGFloat = 32) -> CGRect? {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return nil }
        let srcSide = max(1, (side / zoom * scale).rounded())
        let maxX = CGFloat(cg.width) - srcSide, maxY = CGFloat(cg.height) - srcSide
        guard maxX > 0, maxY > 0 else { return nil }
        let srcX = min(max(0, (cursor.x * scale - srcSide / 2).rounded()), maxX)
        let srcY = min(max(0, (cursor.y * scale - srcSide / 2).rounded()), maxY)
        guard let crop = cg.cropping(to: CGRect(x: srcX, y: srcY, width: srcSide, height: srcSide))
        else { return nil }

        // Đặt lệch khỏi con trỏ, tự lật khi sát mép màn hình.
        var box = CGRect(x: cursor.x + offset, y: cursor.y + offset, width: side, height: side)
        if box.maxX > bounds.maxX - 12 { box.origin.x = cursor.x - offset - side }
        if box.maxY > bounds.maxY - reserveBelow { box.origin.y = cursor.y - offset - side }
        box.origin.x = max(12, box.origin.x)
        box.origin.y = max(12, box.origin.y)
        let shape = NSBezierPath(roundedRect: box, xRadius: 14, yRadius: 14)

        // Bóng đổ cho kính nổi hẳn khỏi nền.
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: 16, color: NSColor(white: 0, alpha: 0.55).cgColor)
        NSColor(white: 0.08, alpha: 1).setFill()
        shape.fill()
        ctx.restoreGState()

        ctx.saveGState()
        shape.addClip()
        // Vẽ ảnh trong view đã lật trục → phải lật ngược lại đúng trong ô này.
        ctx.translateBy(x: 0, y: box.minY + box.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.interpolationQuality = .none          // giữ pixel sắc, không làm mượt
        ctx.draw(crop, in: box)
        ctx.restoreGState()

        ctx.saveGState()
        shape.addClip()
        // Lưới pixel: mỗi ô = 1 pixel thật của màn hình. srcSide pixel nguồn trải
        // đều trên `side` point → mỗi pixel rộng `step` point.
        let step = side / srcSide
        if step >= 3 {
            NSColor(white: 1, alpha: 0.10).setStroke()
            let grid = NSBezierPath()
            grid.lineWidth = 0.5
            var k: CGFloat = 1
            while k < srcSide {
                let x = box.minX + k * step, y = box.minY + k * step
                grid.move(to: CGPoint(x: x, y: box.minY)); grid.line(to: CGPoint(x: x, y: box.maxY))
                grid.move(to: CGPoint(x: box.minX, y: y)); grid.line(to: CGPoint(x: box.maxX, y: y))
                k += 1
            }
            grid.stroke()
        }
        // Ô vuông đánh dấu đúng pixel dưới con trỏ (viền đen + trắng cho nổi trên mọi nền).
        let ix = (cursor.x * scale).rounded(.down) - srcX
        let iy = (cursor.y * scale).rounded(.down) - srcY
        let cell = CGRect(x: box.minX + ix * step, y: box.minY + iy * step,
                          width: step, height: step)
        NSColor(white: 0, alpha: 0.9).setStroke()
        let outer = NSBezierPath(rect: cell.insetBy(dx: -1, dy: -1)); outer.lineWidth = 1; outer.stroke()
        NSColor.white.setStroke()
        let inner = NSBezierPath(rect: cell); inner.lineWidth = 1; inner.stroke()
        ctx.restoreGState()

        chipEdge.setStroke()
        shape.lineWidth = 1
        shape.stroke()
        return box
    }

    // ── Đọc pixel ────────────────────────────────────────────────────────

    /// Màu của đúng 1 pixel trong ảnh (toạ độ theo point của view, gốc trên-trái).
    static func pixelColor(in cg: CGImage, at p: NSPoint, scale: CGFloat) -> NSColor? {
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
}
