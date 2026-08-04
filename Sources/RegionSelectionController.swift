import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Tiện ích: lấy displayID (số định danh màn hình) từ 1 NSScreen.
// Cần nó để ghép NSScreen (AppKit) với SCDisplay (ScreenCaptureKit).
// ─────────────────────────────────────────────────────────────────────────
extension NSScreen {
    var displayID: CGDirectDisplayID {
        let key = NSDeviceDescriptionKey("NSScreenNumber")
        return (deviceDescription[key] as? CGDirectDisplayID) ?? 0
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Cửa sổ overlay.
//
// QUAN TRỌNG — .nonactivatingPanel: panel kiểu này nhận được phím (Esc) mà
// KHÔNG cần kích hoạt app SlopShot. Trước đây dùng NSWindow thường + gọi
// NSApp.activate(ignoringOtherApps:) → app đang dùng bị mất "active", nên
// Telegram/Preview… tự đóng chế độ xem ảnh phóng to ngay lúc bấm phím tắt.
// Bỏ activate + dùng nonactivatingPanel là hết.
// ─────────────────────────────────────────────────────────────────────────
final class OverlayPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
}

// ─────────────────────────────────────────────────────────────────────────
// View nền: chỉ để "dán" ảnh đóng băng vào layer (rẻ hơn vẽ lại ảnh full màn
// hình mỗi lần rê chuột). SelectionView nằm ĐÈ lên trên nó và khoét lỗ để lộ
// đúng vùng chọn với độ sáng gốc.
// ─────────────────────────────────────────────────────────────────────────
final class FrozenBackdropView: NSView {
    override var isFlipped: Bool { true }
}

// ─────────────────────────────────────────────────────────────────────────
// View vẽ lớp tối mờ + khung chọn + gợi ý snap, và bắt sự kiện chuột.
// ─────────────────────────────────────────────────────────────────────────
final class SelectionView: NSView {
    // Callback trả kết quả ra ngoài (giống props onSelected / onCancel).
    var onSelected: ((CGRect) -> Void)?   // rect theo points, gốc trên-trái màn hình
    var onCancel: (() -> Void)?

    var scaleFactor: CGFloat = 2           // để hiện kích thước theo pixel
    var hintText: String = "Drag to capture"   // gợi ý hiện giữa màn khi chưa kéo

    // Ảnh đóng băng của màn hình (dùng cho kính lúp). nil = chế độ cũ, overlay trong suốt.
    var frozen: CGImage?
    // Hai lớp bắt dính: hình học cửa sổ + phân tích pixel.
    var windows = WindowSnapper.empty
    var snapEnabled = true
    var snap: SnapEngine? { didSet { refreshHover(); needsDisplay = true } }

    private var startPoint: NSPoint?
    private var currentRect: CGRect = .zero
    private var dragging = false
    private var interacted = false          // đã kéo/bấm lần nào chưa (để ẩn hint)
    private var hover: SnapTarget?          // khung item đang được gợi ý dưới con trỏ
    private var cursor: NSPoint = .zero
    private var freeMode = false            // giữ ⌥ = tắt bắt dính
    private var guideXs: [CGFloat] = []     // đường gióng khi cạnh bị hút
    private var guideYs: [CGFloat] = []

    private let dragThreshold: CGFloat = 4  // xê dưới mức này vẫn tính là "bấm", không phải "kéo"
    private let snapRadius: CGFloat = 9     // bán kính hút (points) — macshot chỉ 4

    // Lật trục y: gốc toạ độ về góc TRÊN-trái, khớp với ảnh CGImage → đỡ phải convert.
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    // Con trỏ hình chữ thập như mọi tool chụp màn hình.
    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .crosshair)
    }

    // Cần tracking area thì mouseMoved mới được gọi (để dò item dưới con trỏ).
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(
            rect: .zero, options: [.activeAlways, .inVisibleRect, .mouseMoved, .mouseEnteredAndExited],
            owner: self, userInfo: nil))
    }

    // ── Vẽ ───────────────────────────────────────────────────────────────
    //
    // Bảng màu & khoảng cách gom về một chỗ cho dễ chỉnh.
    override func draw(_ dirtyRect: NSRect) {
        // 1. Phủ tối toàn bộ màn hình. Không có ảnh đóng băng (overlay trong suốt,
        //    nền là màn hình sống) thì phủ nhạt hơn cho đỡ chói.
        NSColor(srgbRed: 0.02, green: 0.02, blue: 0.04,
                alpha: frozen == nil ? 0.34 : OverlayChrome.dimAlpha).setFill()
        bounds.fill()

        if !currentRect.isEmpty {
            punch(currentRect)
            drawGuides(around: currentRect)
            strokeSelection(currentRect)
            drawHandles(currentRect)
            drawBadge(sizeText(of: currentRect), for: currentRect, accent: false)
        } else if let hover {
            // Chưa kéo: khoanh sẵn item dưới con trỏ, bấm 1 phát là chụp đúng nó.
            punch(hover.rect)
            NSColor.controlAccentColor.withAlphaComponent(0.12).setFill()
            hover.rect.fill()
            strokeHover(hover.rect)
            let kind = hover.isWindow ? "Window" : "Item"
            drawBadge("\(kind)   \(sizeText(of: hover.rect))", for: hover.rect, accent: true)
        } else {
            drawCrosshair()
        }

        if !interacted { drawHint() }
        if frozen != nil, currentRect.isEmpty || dragging { drawLoupe() }
    }

    /// "Khoét" vùng chọn cho trong suốt (lộ ảnh đóng băng / màn hình thật bên dưới).
    private func punch(_ rect: CGRect) {
        NSColor.clear.setFill()
        rect.fill(using: .copy)
    }

    /// Viền vùng đang kéo: 1 nét trắng nằm SÁT NGOÀI rect (nên không ăn vào vùng
    /// sẽ chụp), kèm 1 nét đen mờ bao ngoài nữa để vẫn thấy rõ trên nền trắng.
    private func strokeSelection(_ rect: CGRect) {
        NSColor(white: 0, alpha: 0.45).setStroke()
        let shadow = NSBezierPath(rect: rect.insetBy(dx: -1.5, dy: -1.5))
        shadow.lineWidth = 1
        shadow.stroke()

        NSColor.white.setStroke()
        let line = NSBezierPath(rect: rect.insetBy(dx: -0.5, dy: -0.5))
        line.lineWidth = 1
        line.stroke()
    }

    private func strokeHover(_ rect: CGRect) {
        NSColor.controlAccentColor.setStroke()
        let line = NSBezierPath(rect: rect.insetBy(dx: -1, dy: -1))
        line.lineWidth = 2
        line.stroke()
    }

    /// Tay nắm kiểu ngoặc góc (⌐ ¬ L ⌐) + thanh nhỏ giữa cạnh, vẽ NẰM TRONG khung
    /// nên không che nội dung xung quanh. Gọn và hiện đại hơn 8 chấm tròn.
    private func drawHandles(_ rect: CGRect) {
        let t: CGFloat = 3                                       // độ dày
        let arm = min(22, max(8, min(rect.width, rect.height) / 3))
        guard rect.width > t * 2, rect.height > t * 2 else { return }

        var bars: [CGRect] = []
        // (x góc, y góc, hướng x, hướng y)
        let corners: [(CGFloat, CGFloat, CGFloat, CGFloat)] = [
            (rect.minX, rect.minY,  1,  1), (rect.maxX, rect.minY, -1,  1),
            (rect.minX, rect.maxY,  1, -1), (rect.maxX, rect.maxY, -1, -1),
        ]
        for (cx, cy, sx, sy) in corners {
            bars.append(CGRect(x: sx > 0 ? cx : cx - arm, y: sy > 0 ? cy : cy - t,
                               width: arm, height: t))
            bars.append(CGRect(x: sx > 0 ? cx : cx - t, y: sy > 0 ? cy : cy - arm,
                               width: t, height: arm))
        }

        // Thanh giữa cạnh — chỉ vẽ khi khung đủ rộng, kẻo dính vào ngoặc góc.
        let bar = min(18, max(10, min(rect.width, rect.height) / 4))
        if rect.width > arm * 2 + bar + 12 {
            bars.append(CGRect(x: rect.midX - bar / 2, y: rect.minY, width: bar, height: t))
            bars.append(CGRect(x: rect.midX - bar / 2, y: rect.maxY - t, width: bar, height: t))
        }
        if rect.height > arm * 2 + bar + 12 {
            bars.append(CGRect(x: rect.minX, y: rect.midY - bar / 2, width: t, height: bar))
            bars.append(CGRect(x: rect.maxX - t, y: rect.midY - bar / 2, width: t, height: bar))
        }

        // Viền tối bao quanh trước, rồi mới tô trắng lên: nếu không, tay nắm trắng
        // nằm trên nền sáng (thanh công cụ, trang web nền trắng…) là mất hút.
        NSColor(white: 0, alpha: 0.4).setFill()
        for b in bars {
            NSBezierPath(roundedRect: b.insetBy(dx: -1, dy: -1), xRadius: 2, yRadius: 2).fill()
        }
        NSColor.white.setFill()
        for b in bars {
            NSBezierPath(roundedRect: b, xRadius: 1.5, yRadius: 1.5).fill()
        }
    }

    private func sizeText(of rect: CGRect) -> String {
        let w = Int((rect.width * scaleFactor).rounded())
        let h = Int((rect.height * scaleFactor).rounded())
        return "\(w) × \(h)"
    }

    /// Nhãn của khung: mặc định nằm ngay trên góc trái, hết chỗ thì tụt xuống
    /// dưới, chật nữa thì chui vào trong khung.
    private func drawBadge(_ text: String, for rect: CGRect, accent: Bool) {
        let size = text.size(withAttributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .semibold)])
        let w = size.width + 16, h = size.height + 10
        var origin = CGPoint(x: rect.minX, y: rect.minY - h - 8)
        if origin.y < 6 {
            origin.y = rect.maxY + 8
            if origin.y + h > bounds.maxY - 6 { origin.y = rect.minY + 8 }
        }
        origin.x = max(6, min(origin.x, bounds.maxX - w - 6))
        OverlayChrome.drawChip(text, at: origin, accent: accent)
    }

    /// Đường gióng ở cạnh vừa bị hút. Chỉ kéo dài ra 2 phía NGOÀI khung (không
    /// cắt ngang vùng chọn) và mờ dần ở đầu mút — đỡ rối hơn kẻ hết màn hình.
    private func drawGuides(around rect: CGRect) {
        guard !guideXs.isEmpty || !guideYs.isEmpty else { return }
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let ext: CGFloat = 130
        let color = OverlayChrome.guide

        func fade(_ from: CGPoint, _ to: CGPoint) {
            guard let grad = NSGradient(colors: [color.withAlphaComponent(0.85),
                                                 color.withAlphaComponent(0)],
                                        atLocations: [0, 1],
                                        colorSpace: .sRGB) else { return }
            let horizontal = abs(to.x - from.x) > abs(to.y - from.y)
            let thickness: CGFloat = 1
            let r = horizontal
                ? CGRect(x: min(from.x, to.x), y: from.y - thickness / 2,
                         width: abs(to.x - from.x), height: thickness)
                : CGRect(x: from.x - thickness / 2, y: min(from.y, to.y),
                         width: thickness, height: abs(to.y - from.y))
            ctx.saveGState()
            NSBezierPath(rect: r).addClip()
            grad.draw(from: from, to: to, options: [])
            ctx.restoreGState()
        }

        for x in guideXs {
            fade(CGPoint(x: x, y: rect.minY), CGPoint(x: x, y: max(0, rect.minY - ext)))
            fade(CGPoint(x: x, y: rect.maxY), CGPoint(x: x, y: min(bounds.maxY, rect.maxY + ext)))
        }
        for y in guideYs {
            fade(CGPoint(x: rect.minX, y: y), CGPoint(x: max(0, rect.minX - ext), y: y))
            fade(CGPoint(x: rect.maxX, y: y), CGPoint(x: min(bounds.maxX, rect.maxX + ext), y: y))
        }
    }

    /// Hai đường mảnh chạy qua con trỏ khi chưa có gì được khoanh — giúp gióng
    /// mắt trước lúc bấm chuột.
    private func drawCrosshair() {
        NSColor(white: 1, alpha: 0.20).setStroke()
        let p = NSBezierPath()
        p.lineWidth = 1
        p.move(to: CGPoint(x: cursor.x + 0.5, y: 0))
        p.line(to: CGPoint(x: cursor.x + 0.5, y: bounds.maxY))
        p.move(to: CGPoint(x: 0, y: cursor.y + 0.5))
        p.line(to: CGPoint(x: bounds.maxX, y: cursor.y + 0.5))
        p.stroke()
    }

    /// Bảng gợi ý dưới đáy màn hình, chỉ hiện khi user chưa bắt đầu thao tác.
    /// (Để giữa màn hình thì kính lúp hay đè lên nó.)
    private func drawHint() {
        let title: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 15, weight: .semibold),
            .foregroundColor: NSColor.white,
        ]
        let sub: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 12, weight: .regular),
            .foregroundColor: NSColor(white: 1, alpha: 0.62),
        ]
        let line2 = snapEnabled
            ? "click a highlighted area  ·  ⌥ free select  ·  esc to cancel"
            : "⌥ free select  ·  esc to cancel"

        let s1 = hintText.size(withAttributes: title)
        let s2 = line2.size(withAttributes: sub)
        let padX: CGFloat = 22, padY: CGFloat = 15, gap: CGFloat = 5
        let boxW = max(s1.width, s2.width) + padX * 2
        let boxH = s1.height + gap + s2.height + padY * 2
        let box = CGRect(x: (bounds.width - boxW) / 2,
                         y: bounds.maxY - boxH - 96,   // view lật trục: maxY = đáy màn hình
                         width: boxW, height: boxH)

        let shape = NSBezierPath(roundedRect: box, xRadius: 14, yRadius: 14)
        OverlayChrome.chipFill.setFill()
        shape.fill()
        OverlayChrome.chipEdge.setStroke()
        shape.lineWidth = 1
        shape.stroke()

        hintText.draw(at: CGPoint(x: box.midX - s1.width / 2, y: box.minY + padY),
                      withAttributes: title)
        line2.draw(at: CGPoint(x: box.midX - s2.width / 2, y: box.minY + padY + s1.height + gap),
                   withAttributes: sub)
    }

    // Kính lúp: phóng to vùng quanh con trỏ từ ảnh đóng băng → chọn tới từng pixel.
    private func drawLoupe() {
        guard let cg = frozen,
              let box = OverlayChrome.drawLoupe(image: cg, cursor: cursor, scale: scaleFactor,
                                                in: bounds, side: 128, zoom: 8, reserveBelow: 44)
        else { return }

        // Chip dưới kính: mã màu + toạ độ pixel.
        let color = OverlayChrome.pixelColor(in: cg, at: cursor, scale: scaleFactor)
        let text = "\(Int(cursor.x * scaleFactor)), \(Int(cursor.y * scaleFactor))"
        let label = color.map { "\(OverlayChrome.hex(of: $0))  \(text)" } ?? text
        let w = OverlayChrome.chipWidth(label, fontSize: 11, swatch: color != nil)
        OverlayChrome.drawChip(label, at: CGPoint(x: box.midX - w / 2, y: box.maxY + 6),
                               fontSize: 11, swatch: color)
    }

    // ── Bắt dính ─────────────────────────────────────────────────────────

    /// Đặt vị trí con trỏ ban đầu (lúc overlay vừa mở, chưa có mouseMoved nào)
    /// để khung gợi ý hiện ngay dưới chuột thay vì nằm ở góc màn hình.
    func primeCursor(_ p: NSPoint) {
        cursor = p
        refreshHover()
    }

    /// Dò item dưới con trỏ: ưu tiên khung do phân tích pixel tìm ra, không có
    /// thì lấy nguyên cửa sổ.
    private func refreshHover() {
        guard snapEnabled, !freeMode, !dragging else {
            if hover != nil { hover = nil; needsDisplay = true }
            return
        }
        let win = windows.window(at: cursor)
        var found: SnapTarget?
        if let el = snap?.element(at: cursor, within: win ?? bounds) {
            // Khung dò được gần trùng cửa sổ → lấy hẳn số đo cửa sổ cho chuẩn.
            if let win, abs(el.minX - win.minX) < 4, abs(el.minY - win.minY) < 4,
               abs(el.maxX - win.maxX) < 4, abs(el.maxY - win.maxY) < 4 {
                found = SnapTarget(rect: win, isWindow: true)
            } else {
                found = SnapTarget(rect: el, isWindow: false)
            }
        } else if let win {
            found = SnapTarget(rect: win, isWindow: true)
        }
        if found != hover { hover = found; needsDisplay = true }
    }

    /// Hút 4 cạnh của khung đang kéo về biên gần nhất: cạnh cửa sổ (chính xác
    /// tuyệt đối) trước, rồi mới tới biên dò từ pixel.
    private func snapped(_ r: CGRect) -> CGRect {
        guideXs = []; guideYs = []
        guard snapEnabled, !freeMode else { return r }

        var minX = r.minX, maxX = r.maxX, minY = r.minY, maxY = r.maxY

        func snapLine(_ v: CGFloat, geo: [CGFloat], image: () -> CGFloat?) -> (CGFloat, Bool) {
            if let g = geo.min(by: { abs($0 - v) < abs($1 - v) }), abs(g - v) <= snapRadius {
                return (g, true)
            }
            if let i = image() { return (i, true) }
            return (v, false)
        }

        let (nx0, h0) = snapLine(minX, geo: windows.edgeXs) {
            snap?.snapX(near: minX, from: minY, to: maxY, radius: snapRadius)
        }
        let (nx1, h1) = snapLine(maxX, geo: windows.edgeXs) {
            snap?.snapX(near: maxX, from: minY, to: maxY, radius: snapRadius)
        }
        let (ny0, h2) = snapLine(minY, geo: windows.edgeYs) {
            snap?.snapY(near: minY, from: minX, to: maxX, radius: snapRadius)
        }
        let (ny1, h3) = snapLine(maxY, geo: windows.edgeYs) {
            snap?.snapY(near: maxY, from: minX, to: maxX, radius: snapRadius)
        }
        if h0 { guideXs.append(nx0) }
        if h1 { guideXs.append(nx1) }
        if h2 { guideYs.append(ny0) }
        if h3 { guideYs.append(ny1) }
        minX = nx0; maxX = nx1; minY = ny0; maxY = ny1

        guard maxX - minX >= 1, maxY - minY >= 1 else { return r }
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    // ── Sự kiện chuột / phím ─────────────────────────────────────────────
    override func mouseMoved(with event: NSEvent) {
        cursor = convert(event.locationInWindow, from: nil)
        freeMode = event.modifierFlags.contains(.option)
        refreshHover()
        if frozen != nil { needsDisplay = true }   // kính lúp bám theo con trỏ
    }

    override func flagsChanged(with event: NSEvent) {
        let free = event.modifierFlags.contains(.option)
        guard free != freeMode else { return }
        freeMode = free
        if dragging, let start = startPoint {
            currentRect = rect(from: start, to: cursor)
        }
        refreshHover()
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        startPoint = convert(event.locationInWindow, from: nil)
        cursor = startPoint ?? .zero
        currentRect = .zero
        dragging = false
        interacted = true
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start = startPoint else { return }
        cursor = convert(event.locationInWindow, from: nil)
        freeMode = event.modifierFlags.contains(.option)
        // Bấm chuột bao giờ cũng xê vài pixel (trackpad càng rõ). Chỉ tính là KÉO
        // khi vượt ngưỡng — chưa vượt thì giữ nguyên `hover`, để thả ra vẫn chụp
        // được đúng cửa sổ đang khoanh thay vì bị huỷ.
        if !dragging {
            guard hypot(cursor.x - start.x, cursor.y - start.y) >= dragThreshold else { return }
            dragging = true
            hover = nil
        }
        currentRect = rect(from: start, to: cursor)
        needsDisplay = true
    }

    // Chuẩn hoá để kéo theo hướng nào cũng ra rect dương, rồi cho bắt dính.
    private func rect(from start: NSPoint, to p: NSPoint) -> CGRect {
        let raw = CGRect(x: min(start.x, p.x), y: min(start.y, p.y),
                         width: abs(p.x - start.x), height: abs(p.y - start.y))
        guard raw.width >= 3, raw.height >= 3 else { return raw }
        return snapped(raw)
    }

    override func mouseUp(with event: NSEvent) {
        if dragging, currentRect.width >= 5, currentRect.height >= 5 {
            onSelected?(currentRect)
        } else if let hover {
            // Bấm 1 phát (không kéo) lên vùng đang được khoanh → chụp đúng vùng đó.
            onSelected?(hover.rect)
        } else {
            onCancel?()          // bấm hụt vào chỗ trống = huỷ, như trước
        }
    }

    override func rightMouseDown(with event: NSEvent) { onCancel?() }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { onCancel?() }  // 53 = phím ESC
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Controller: mở overlay trên 1 màn hình, chờ user chọn, gọi completion.
// completion trả về rect (points, gốc trên-trái của màn hình đó) hoặc nil nếu huỷ.
//
// `frozen` = ảnh chụp sẵn của màn hình đó (đóng băng khung hình). Có nó thì:
//   • overlay hiện đúng nội dung lúc bấm phím tắt (app dưới có đổi gì cũng kệ)
//   • bật được kính lúp + bắt dính theo biên ảnh
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class RegionSelectionController {
    private var window: OverlayPanel?

    func begin(on screen: NSScreen, frozen: CGImage? = nil, completion: @escaping (CGRect?) -> Void) {
        let size = screen.frame.size
        let view = SelectionView(frame: NSRect(origin: .zero, size: size))
        // Tỉ lệ point→pixel lấy từ chính ảnh đóng băng (khớp với ảnh sẽ cắt ra),
        // không có ảnh thì mới dùng backingScaleFactor.
        view.scaleFactor = frozen.map { CGFloat($0.width) / max(size.width, 1) }
            ?? screen.backingScaleFactor
        view.autoresizingMask = [.width, .height]
        view.frozen = frozen
        view.snapEnabled = AppSettings.shared.snapToEdges
        view.hintText = view.snapEnabled
            ? "Drag to capture · click a highlighted area · ⌥ free · esc"
            : "Drag to capture · esc to cancel"
        // Lấy danh sách cửa sổ TRƯỚC khi overlay hiện lên (khỏi dính chính mình).
        if view.snapEnabled {
            view.windows = WindowSnapper.snapshot(on: screen)
            // Chuột đang ở đâu → khoanh sẵn ngay chỗ đó (mouseMoved chưa bắn lần nào).
            let m = NSEvent.mouseLocation
            view.primeCursor(NSPoint(x: m.x - screen.frame.minX, y: screen.frame.maxY - m.y))
        }

        // completion chỉ được phép chạy ĐÚNG 1 lần. Nếu không, các sự kiện dồn
        // nhau (Esc lúc đang kéo chuột, Esc nhấn 2 lần, Esc sát lúc thả chuột)
        // có thể bắn callback 2 lần → withCheckedContinuation resume 2 lần →
        // fatalError "continuation misuse" → CẢ APP TẮT. Guard 1-lần ở đây.
        var finished = false
        let finishOnce: (CGRect?) -> Void = { [weak self] rect in
            guard !finished else { return }
            finished = true
            self?.cleanup()
            completion(rect)
        }

        // Nền = ảnh đóng băng, dán thẳng vào layer (GPU lo phần vẽ lại).
        let backdrop = FrozenBackdropView(frame: NSRect(origin: .zero, size: size))
        backdrop.wantsLayer = true
        backdrop.layer?.contentsGravity = .resize
        backdrop.layer?.contentsScale = screen.backingScaleFactor
        backdrop.layer?.contents = frozen
        backdrop.addSubview(view)

        let win = OverlayPanel(contentRect: screen.frame,
                               styleMask: [.borderless, .nonactivatingPanel],
                               backing: .buffered,
                               defer: false)
        win.level = .screenSaver          // nằm trên cả menu bar
        win.backgroundColor = .clear
        win.isOpaque = false
        win.hasShadow = false
        win.animationBehavior = .none     // hiện tức thì: ảnh đóng băng phải trùng khít màn hình
        win.hidesOnDeactivate = false
        win.acceptsMouseMovedEvents = true
        win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        win.contentView = backdrop

        view.onSelected = { rect in finishOnce(rect) }
        view.onCancel = { finishOnce(nil) }

        if frozen == nil {
            // Không có ảnh đóng băng (fallback): hiện ở alpha 0 rồi fade nhanh,
            // nếu order-front thẳng ở alpha 1 sẽ lộ 1 frame ĐEN trước khi view vẽ.
            win.alphaValue = 0
            win.makeKeyAndOrderFront(nil)
            win.displayIfNeeded()
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.1
                win.animator().alphaValue = 1
            }
        } else {
            win.makeKeyAndOrderFront(nil)
        }
        // KHÔNG gọi NSApp.activate: app đang dùng giữ nguyên trạng thái, nên
        // Telegram/Preview… không tự thoát chế độ xem ảnh phóng to nữa.
        win.makeFirstResponder(view)
        self.window = win

        // Phân tích biên ảnh ở luồng nền (~20-40ms). Trong lúc chờ, snap vẫn
        // chạy được bằng hình học cửa sổ.
        if let frozen, view.snapEnabled {
            let scale = CGFloat(frozen.width) / max(size.width, 1)
            Task { [weak view] in
                let engine = await Task.detached(priority: .userInitiated) {
                    SnapEngine.build(from: frozen, scale: scale)
                }.value
                view?.snap = engine
            }
        }
    }

    private func cleanup() {
        window?.orderOut(nil)
        window = nil
    }
}
