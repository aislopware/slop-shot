import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Lớp phủ "hút màu": rê chuột tới đâu đọc màu pixel tới đó, bấm 1 phát là chép
// mã màu vào clipboard.
//
// Khác lớp phủ chọn vùng ở chỗ KHÔNG phủ tối: đang đi soi màu mà bôi đen màn
// hình thì nhìn màu nào cũng sai. Ảnh đóng băng hiện nguyên bản, chỉ có kính
// lúp + bảng thông tin nổi lên trên.
//
// Màu đọc từ ẢNH ĐÓNG BĂNG chứ không phải từ màn hình sống, nên kể cả app phía
// dưới có vẽ lại thì màu vẫn đúng khoảnh khắc bấm phím tắt.
// ─────────────────────────────────────────────────────────────────────────
final class ColorPickerView: NSView {
    var onPick: ((NSColor, CGPoint) -> Void)?
    var onCancel: (() -> Void)?
    var onFormatChange: ((AppSettings.ColorFormat) -> Void)?

    var frozen: CGImage?
    var scaleFactor: CGFloat = 2
    var format: AppSettings.ColorFormat = .hex { didSet { needsDisplay = true } }

    private var cursor: NSPoint = .zero
    private var color: NSColor?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .crosshair)
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(
            rect: .zero, options: [.activeAlways, .inVisibleRect, .mouseMoved, .mouseEnteredAndExited],
            owner: self, userInfo: nil))
    }

    /// Đặt vị trí con trỏ lúc overlay vừa mở (chưa có mouseMoved nào bắn ra).
    func primeCursor(_ p: NSPoint) {
        cursor = p
        refreshColor()
    }

    private func refreshColor() {
        guard let cg = frozen else { return }
        color = OverlayChrome.pixelColor(in: cg, at: cursor, scale: scaleFactor)
        needsDisplay = true
    }

    // ── Vẽ ───────────────────────────────────────────────────────────────
    override func draw(_ dirtyRect: NSRect) {
        guard let cg = frozen else { return }

        // Gợi ý vẽ TRƯỚC: nó nằm cố định dưới đáy, còn kính lúp bám con trỏ nên
        // hai cái chồng nhau là chuyện thường — lúc đó phải thấy màu, không phải
        // thấy hướng dẫn.
        drawHint()

        // Kính lúp to hơn và phóng mạnh hơn lúc chọn vùng: ở đây cần trúng ĐÚNG
        // 1 pixel, không phải canh khung.
        let box = OverlayChrome.drawLoupe(image: cg, cursor: cursor, scale: scaleFactor,
                                          in: bounds, side: 152, zoom: 12,
                                          offset: 26, reserveBelow: 96)
        if let box { drawReadout(below: box) }
    }

    /// Bảng dưới kính lúp: ô màu to + chuỗi màu theo định dạng đang chọn + toạ độ.
    private func drawReadout(below box: CGRect) {
        guard let color else { return }
        let value = format.string(from: color)

        let valueAttrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 13, weight: .semibold),
            .foregroundColor: NSColor.white,
        ]
        let metaAttrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 10, weight: .medium),
            .foregroundColor: NSColor(white: 1, alpha: 0.55),
        ]
        let meta = "\(format.shortLabel)  ·  \(Int(cursor.x * scaleFactor)), \(Int(cursor.y * scaleFactor))"
        let vs = value.size(withAttributes: valueAttrs)
        let ms = meta.size(withAttributes: metaAttrs)

        let swatch: CGFloat = 30, padX: CGFloat = 10, padY: CGFloat = 9, gap: CGFloat = 10
        let textW = max(vs.width, ms.width)
        let w = padX * 2 + swatch + gap + textW
        let h = padY * 2 + vs.height + 2 + ms.height

        // Canh giữa dưới kính, kẹp trong màn hình.
        var origin = CGPoint(x: box.midX - w / 2, y: box.maxY + 8)
        origin.x = min(max(8, origin.x), bounds.maxX - w - 8)
        if origin.y + h > bounds.maxY - 8 { origin.y = box.minY - h - 8 }
        let panel = CGRect(x: origin.x, y: origin.y, width: w, height: h)

        let shape = NSBezierPath(roundedRect: panel, xRadius: 12, yRadius: 12)
        OverlayChrome.chipFill.setFill()
        shape.fill()
        OverlayChrome.chipEdge.setStroke()
        shape.lineWidth = 1
        shape.stroke()

        // Ô màu: viền sáng bên trong để phân biệt được cả màu đen tuyền lẫn trắng tinh.
        let sq = CGRect(x: panel.minX + padX, y: panel.midY - swatch / 2,
                        width: swatch, height: swatch)
        let sqPath = NSBezierPath(roundedRect: sq, xRadius: 6, yRadius: 6)
        color.setFill()
        sqPath.fill()
        NSColor(white: 1, alpha: 0.35).setStroke()
        sqPath.lineWidth = 1
        sqPath.stroke()

        let textX = sq.maxX + gap
        value.draw(at: CGPoint(x: textX, y: panel.minY + padY), withAttributes: valueAttrs)
        meta.draw(at: CGPoint(x: textX, y: panel.minY + padY + vs.height + 2), withAttributes: metaAttrs)
    }

    private func drawHint() {
        let title: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 15, weight: .semibold),
            .foregroundColor: NSColor.white,
        ]
        let sub: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 12, weight: .regular),
            .foregroundColor: NSColor(white: 1, alpha: 0.62),
        ]
        let line1 = "Click to copy the color"
        let line2 = "← → change format  ·  arrow keys with ⇧ nudge by 1px  ·  esc to cancel"

        let s1 = line1.size(withAttributes: title)
        let s2 = line2.size(withAttributes: sub)
        let padX: CGFloat = 22, padY: CGFloat = 15, gap: CGFloat = 5
        let boxW = max(s1.width, s2.width) + padX * 2
        let boxH = s1.height + gap + s2.height + padY * 2
        let box = CGRect(x: (bounds.width - boxW) / 2, y: bounds.maxY - boxH - 96,
                         width: boxW, height: boxH)

        let shape = NSBezierPath(roundedRect: box, xRadius: 14, yRadius: 14)
        OverlayChrome.chipFill.setFill()
        shape.fill()
        OverlayChrome.chipEdge.setStroke()
        shape.lineWidth = 1
        shape.stroke()

        line1.draw(at: CGPoint(x: box.midX - s1.width / 2, y: box.minY + padY), withAttributes: title)
        line2.draw(at: CGPoint(x: box.midX - s2.width / 2, y: box.minY + padY + s1.height + gap),
                   withAttributes: sub)
    }

    // ── Sự kiện ──────────────────────────────────────────────────────────
    override func mouseMoved(with event: NSEvent) {
        cursor = convert(event.locationInWindow, from: nil)
        refreshColor()
    }

    // Phải nhận mouseDown thì mouseUp mới về đúng view này; tiện thể cho phép
    // giữ chuột rê chậm để canh pixel rồi mới thả ra chốt màu.
    override func mouseDown(with event: NSEvent) { mouseMoved(with: event) }
    override func mouseDragged(with event: NSEvent) { mouseMoved(with: event) }

    override func mouseUp(with event: NSEvent) {
        guard let color else { onCancel?(); return }
        onPick?(color, CGPoint(x: (cursor.x * scaleFactor).rounded(.down),
                               y: (cursor.y * scaleFactor).rounded(.down)))
    }

    override func rightMouseDown(with event: NSEvent) { onCancel?() }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 53:                       // esc
            onCancel?()
        case 36, 49:                   // ↩ / space = chốt màu, cho ai thích dùng bàn phím
            if let color {
                onPick?(color, CGPoint(x: (cursor.x * scaleFactor).rounded(.down),
                                       y: (cursor.y * scaleFactor).rounded(.down)))
            }
        case 123, 124, 125, 126:       // ← → ↓ ↑
            handleArrow(event)
        default:
            break
        }
    }

    /// ← → đổi định dạng; giữ ⇧ thì 4 phím mũi tên nhích con trỏ đúng 1 pixel
    /// (rê tay khó trúng pixel mong muốn ở chỗ chuyển màu gắt).
    private func handleArrow(_ event: NSEvent) {
        let all = AppSettings.ColorFormat.allCases
        if !event.modifierFlags.contains(.shift), event.keyCode == 123 || event.keyCode == 124 {
            guard let i = all.firstIndex(of: format) else { return }
            let next = event.keyCode == 124 ? (i + 1) % all.count : (i - 1 + all.count) % all.count
            format = all[next]
            onFormatChange?(format)
            return
        }

        // Nhích 1 pixel ảnh = 1/scale point, và phải dời cả con trỏ thật của hệ
        // thống, nếu không lần mouseMoved kế tiếp sẽ kéo tuột về chỗ cũ.
        let stepPt = 1 / scaleFactor
        var p = cursor
        switch event.keyCode {
        case 123: p.x -= stepPt
        case 124: p.x += stepPt
        case 125: p.y += stepPt
        default:  p.y -= stepPt
        }
        p.x = min(max(0, p.x), bounds.maxX - stepPt)
        p.y = min(max(0, p.y), bounds.maxY - stepPt)
        cursor = p
        refreshColor()

        if let screen = window?.screen {
            // CGWarpMouseCursorPosition ăn toạ độ CG toàn cục (gốc trên-trái màn chính).
            let mainTop = (NSScreen.screens.first?.frame.maxY ?? screen.frame.maxY)
            CGWarpMouseCursorPosition(CGPoint(x: screen.frame.minX + p.x,
                                              y: mainTop - screen.frame.maxY + p.y))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Controller: mở overlay hút màu trên 1 màn hình, chờ user bấm, gọi completion.
// completion trả về màu đã chọn, hoặc nil nếu huỷ.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class ColorPickerController {
    private var window: OverlayPanel?

    func begin(on screen: NSScreen, frozen: CGImage,
               completion: @escaping (NSColor?) -> Void) {
        let size = screen.frame.size
        let view = ColorPickerView(frame: NSRect(origin: .zero, size: size))
        view.autoresizingMask = [.width, .height]
        view.frozen = frozen
        view.scaleFactor = CGFloat(frozen.width) / max(size.width, 1)
        view.format = AppSettings.shared.colorFormat
        let m = NSEvent.mouseLocation
        view.primeCursor(NSPoint(x: m.x - screen.frame.minX, y: screen.frame.maxY - m.y))

        // Giống bên chọn vùng: completion chỉ được chạy đúng 1 lần, không thì
        // continuation bị resume 2 lần → fatalError → tắt cả app.
        var finished = false
        let finishOnce: (NSColor?) -> Void = { [weak self] color in
            guard !finished else { return }
            finished = true
            self?.cleanup()
            completion(color)
        }

        let backdrop = FrozenBackdropView(frame: NSRect(origin: .zero, size: size))
        backdrop.wantsLayer = true
        backdrop.layer?.contentsGravity = .resize
        backdrop.layer?.contentsScale = screen.backingScaleFactor
        backdrop.layer?.contents = frozen
        backdrop.addSubview(view)

        let win = OverlayPanel(contentRect: screen.frame,
                               styleMask: [.borderless, .nonactivatingPanel],
                               backing: .buffered, defer: false)
        win.level = .screenSaver
        win.backgroundColor = .clear
        win.isOpaque = false
        win.hasShadow = false
        win.animationBehavior = .none
        win.hidesOnDeactivate = false
        win.acceptsMouseMovedEvents = true
        win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        win.contentView = backdrop

        view.onPick = { color, _ in finishOnce(color) }
        view.onCancel = { finishOnce(nil) }
        // Đổi định dạng lúc đang soi thì nhớ luôn cho lần sau.
        view.onFormatChange = { AppSettings.shared.colorFormat = $0 }

        win.makeKeyAndOrderFront(nil)
        win.makeFirstResponder(view)
        self.window = win
    }

    private func cleanup() {
        window?.orderOut(nil)
        window = nil
    }
}
