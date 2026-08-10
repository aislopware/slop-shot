import SwiftUI

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
//
// Phần NHÌN THẤY là SwiftUI (ColorPickerOverlay); ColorPickerEventView chỉ còn
// nhiệm vụ bắt chuột/phím thô và đẩy vào model.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class ColorPickerModel: ObservableObject {
    @Published var cursor: CGPoint = .zero
    @Published var color: NSColor?
    @Published var format: AppSettings.ColorFormat = .hex

    var frozen: CGImage?
    var scaleFactor: CGFloat = 2
}

struct ColorPickerOverlay: View {
    @ObservedObject var model: ColorPickerModel

    var body: some View {
        GeometryReader { geo in
            let bounds = CGRect(origin: .zero, size: geo.size)
            // Kính lúp to hơn và phóng mạnh hơn lúc chọn vùng: ở đây cần trúng
            // ĐÚNG 1 pixel, không phải canh khung.
            let side: CGFloat = 152
            let box = OverlayChrome.loupeBox(cursor: model.cursor, in: bounds, side: side,
                                             offset: 26, reserveBelow: 96)
            ZStack(alignment: .topLeading) {
                Color.clear

                // Gợi ý nằm DƯỚI cùng trong thứ tự vẽ: nó ở cố định đáy màn, còn
                // kính lúp bám con trỏ nên hai cái chồng nhau là chuyện thường —
                // lúc đó phải thấy màu, không phải thấy hướng dẫn.
                OverlayHint(title: "Click to copy the color",
                            subtitle: "← → change format  ·  arrow keys with ⇧ nudge by 1px  ·  esc to cancel")
                    .padding(.bottom, 96)
                    .frame(width: geo.size.width, height: geo.size.height, alignment: .bottom)

                if let cg = model.frozen {
                    LoupeView(image: cg, cursor: model.cursor, scale: model.scaleFactor,
                              side: side, zoom: 12)
                        .position(x: box.midX, y: box.midY)
                    readout(below: box, in: bounds)
                }
            }
        }
        .ignoresSafeArea()
    }

    /// Bảng dưới kính lúp: ô màu to + chuỗi màu theo định dạng đang chọn + toạ độ.
    @ViewBuilder
    private func readout(below box: CGRect, in bounds: CGRect) -> some View {
        if let color = model.color {
            let value = model.format.string(from: color)
            let meta = "\(model.format.shortLabel)  ·  "
                + "\(Int(model.cursor.x * model.scaleFactor)), \(Int(model.cursor.y * model.scaleFactor))"
            let size = panelSize(value: value, meta: meta)

            // Canh giữa dưới kính, kẹp trong màn hình.
            let x = min(max(8, box.midX - size.width / 2), bounds.maxX - size.width - 8)
            let yBelow = box.maxY + 8
            let y = yBelow + size.height > bounds.maxY - 8 ? box.minY - size.height - 8 : yBelow

            HStack(spacing: 10) {
                // Ô màu: viền sáng bên trong để phân biệt được cả màu đen tuyền
                // lẫn trắng tinh.
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color(nsColor: color))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(.white.opacity(0.35), lineWidth: 1))
                    .frame(width: 30, height: 30)
                VStack(alignment: .leading, spacing: 2) {
                    Text(value)
                        .font(.system(size: 13, weight: .semibold).monospacedDigit())
                        .foregroundStyle(.white)
                    Text(meta)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(.white.opacity(0.55))
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 9)
            .background(OverlayChrome.chipFill, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(OverlayChrome.chipEdge, lineWidth: 1))
            .position(x: x + size.width / 2, y: y + size.height / 2)
        }
    }

    /// Đo trước bảng để canh vị trí (SwiftUI đặt view theo tâm).
    private func panelSize(value: String, meta: String) -> CGSize {
        let vs = value.size(withAttributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 13, weight: .semibold)])
        let ms = meta.size(withAttributes: [
            .font: NSFont.systemFont(ofSize: 10, weight: .medium)])
        return CGSize(width: 10 * 2 + 30 + 10 + max(vs.width, ms.width),
                      height: 9 * 2 + vs.height + 2 + ms.height)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// View bắt sự kiện (trong suốt, nằm đè lên phần vẽ).
// ─────────────────────────────────────────────────────────────────────────
final class ColorPickerEventView: NSView {
    var onPick: ((NSColor, CGPoint) -> Void)?
    var onCancel: (() -> Void)?
    var onFormatChange: ((AppSettings.ColorFormat) -> Void)?

    let model: ColorPickerModel

    init(frame: NSRect, model: ColorPickerModel) {
        self.model = model
        super.init(frame: frame)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) chưa dùng tới") }

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
        model.cursor = p
        refreshColor()
    }

    private func refreshColor() {
        guard let cg = model.frozen else { return }
        model.color = OverlayChrome.pixelColor(in: cg, at: model.cursor, scale: model.scaleFactor)
    }

    // ── Sự kiện ──────────────────────────────────────────────────────────
    override func mouseMoved(with event: NSEvent) {
        model.cursor = convert(event.locationInWindow, from: nil)
        refreshColor()
    }

    // Phải nhận mouseDown thì mouseUp mới về đúng view này; tiện thể cho phép
    // giữ chuột rê chậm để canh pixel rồi mới thả ra chốt màu.
    override func mouseDown(with event: NSEvent) { mouseMoved(with: event) }
    override func mouseDragged(with event: NSEvent) { mouseMoved(with: event) }

    override func mouseUp(with event: NSEvent) {
        guard let color = model.color else { onCancel?(); return }
        onPick?(color, pixelPoint())
    }

    override func rightMouseDown(with event: NSEvent) { onCancel?() }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 53:                       // esc
            onCancel?()
        case 36, 49:                   // ↩ / space = chốt màu, cho ai thích dùng bàn phím
            if let color = model.color { onPick?(color, pixelPoint()) }
        case 123, 124, 125, 126:       // ← → ↓ ↑
            handleArrow(event)
        default:
            break
        }
    }

    private func pixelPoint() -> CGPoint {
        CGPoint(x: (model.cursor.x * model.scaleFactor).rounded(.down),
                y: (model.cursor.y * model.scaleFactor).rounded(.down))
    }

    /// ← → đổi định dạng; giữ ⇧ thì 4 phím mũi tên nhích con trỏ đúng 1 pixel
    /// (rê tay khó trúng pixel mong muốn ở chỗ chuyển màu gắt).
    private func handleArrow(_ event: NSEvent) {
        let all = AppSettings.ColorFormat.allCases
        if !event.modifierFlags.contains(.shift), event.keyCode == 123 || event.keyCode == 124 {
            guard let i = all.firstIndex(of: model.format) else { return }
            let next = event.keyCode == 124 ? (i + 1) % all.count : (i - 1 + all.count) % all.count
            model.format = all[next]
            onFormatChange?(model.format)
            return
        }

        // Nhích 1 pixel ảnh = 1/scale point, và phải dời cả con trỏ thật của hệ
        // thống, nếu không lần mouseMoved kế tiếp sẽ kéo tuột về chỗ cũ.
        let stepPt = 1 / model.scaleFactor
        var p = model.cursor
        switch event.keyCode {
        case 123: p.x -= stepPt
        case 124: p.x += stepPt
        case 125: p.y += stepPt
        default:  p.y -= stepPt
        }
        p.x = min(max(0, p.x), bounds.maxX - stepPt)
        p.y = min(max(0, p.y), bounds.maxY - stepPt)
        model.cursor = p
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
        let model = ColorPickerModel()
        model.frozen = frozen
        model.scaleFactor = CGFloat(frozen.width) / max(size.width, 1)
        model.format = AppSettings.shared.colorFormat

        let view = ColorPickerEventView(frame: NSRect(origin: .zero, size: size), model: model)
        view.autoresizingMask = [.width, .height]
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

        let chrome = PassthroughHostingView(rootView: ColorPickerOverlay(model: model))
        chrome.frame = NSRect(origin: .zero, size: size)
        chrome.autoresizingMask = [.width, .height]
        backdrop.addSubview(chrome)
        backdrop.addSubview(view)          // lớp bắt sự kiện nằm trên cùng

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
