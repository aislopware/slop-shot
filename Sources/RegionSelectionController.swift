import SwiftUI

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
// hình mỗi lần rê chuột). Lớp SwiftUI nằm ĐÈ lên trên nó và khoét lỗ để lộ
// đúng vùng chọn với độ sáng gốc.
// ─────────────────────────────────────────────────────────────────────────
final class FrozenBackdropView: NSView {
    override var isFlipped: Bool { true }
}

// ─────────────────────────────────────────────────────────────────────────
// Trạng thái của lớp phủ chọn vùng. View bắt sự kiện ghi vào đây, SwiftUI đọc ra.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class SelectionModel: ObservableObject {
    @Published var currentRect: CGRect = .zero
    @Published var hover: SnapTarget?
    @Published var cursor: CGPoint = .zero
    @Published var dragging = false
    @Published var interacted = false        // đã kéo/bấm lần nào chưa (để ẩn hint)
    @Published var guideXs: [CGFloat] = []   // đường gióng khi cạnh bị hút
    @Published var guideYs: [CGFloat] = []

    var frozen: CGImage?                     // nil = overlay trong suốt, không có kính lúp
    var scaleFactor: CGFloat = 2             // để hiện kích thước theo pixel
    var hintText = "Drag to capture"
    var snapEnabled = true
}

// ─────────────────────────────────────────────────────────────────────────
// Toàn bộ phần nhìn thấy của lớp phủ chọn vùng.
// ─────────────────────────────────────────────────────────────────────────
struct SelectionOverlay: View {
    @ObservedObject var model: SelectionModel

    private let loupeSide: CGFloat = 128

    var body: some View {
        GeometryReader { geo in
            let bounds = CGRect(origin: .zero, size: geo.size)
            ZStack(alignment: .topLeading) {
                Canvas { ctx, size in paint(&ctx, size: size) }

                if !model.interacted {
                    OverlayHint(title: model.hintText, subtitle: hintSubtitle)
                        .padding(.bottom, 96)
                        .frame(width: geo.size.width, height: geo.size.height, alignment: .bottom)
                }

                badge(in: bounds)
                loupe(in: bounds)
            }
        }
        .ignoresSafeArea()
    }

    private var hintSubtitle: String {
        model.snapEnabled
            ? "click a highlighted area  ·  ⌥ free select  ·  esc to cancel"
            : "⌥ free select  ·  esc to cancel"
    }

    // ── Vẽ nền: phủ tối, khoét lỗ, viền, tay nắm, đường gióng, chữ thập ──
    private func paint(_ ctx: inout GraphicsContext, size: CGSize) {
        let bounds = CGRect(origin: .zero, size: size)

        // 1. Phủ tối toàn bộ màn hình, khoét thủng đúng vùng đang khoanh. Không
        //    có ảnh đóng băng (overlay trong suốt, nền là màn hình sống) thì phủ
        //    nhạt hơn cho đỡ chói.
        let hole = model.currentRect.isEmpty ? model.hover?.rect : model.currentRect
        var dim = Path(bounds)
        if let hole { dim.addRect(hole) }
        ctx.fill(dim,
                 with: .color(Color(.sRGB, red: 0.02, green: 0.02, blue: 0.04,
                                    opacity: model.frozen == nil ? 0.34 : OverlayChrome.dimAlpha)),
                 style: FillStyle(eoFill: true))

        if !model.currentRect.isEmpty {
            let r = model.currentRect
            drawGuides(&ctx, around: r, in: bounds)
            strokeSelection(&ctx, r)
            drawHandles(&ctx, r)
        } else if let hover = model.hover {
            // Chưa kéo: khoanh sẵn item dưới con trỏ, bấm 1 phát là chụp đúng nó.
            ctx.fill(Path(hover.rect), with: .color(Color(nsColor: .controlAccentColor).opacity(0.12)))
            ctx.stroke(Path(hover.rect.insetBy(dx: -1, dy: -1)),
                       with: .color(Color(nsColor: .controlAccentColor)), lineWidth: 2)
        } else {
            drawCrosshair(&ctx, in: bounds)
        }
    }

    /// Viền vùng đang kéo: 1 nét trắng nằm SÁT NGOÀI rect (nên không ăn vào vùng
    /// sẽ chụp), kèm 1 nét đen mờ bao ngoài nữa để vẫn thấy rõ trên nền trắng.
    private func strokeSelection(_ ctx: inout GraphicsContext, _ rect: CGRect) {
        ctx.stroke(Path(rect.insetBy(dx: -1.5, dy: -1.5)),
                   with: .color(.black.opacity(0.45)), lineWidth: 1)
        ctx.stroke(Path(rect.insetBy(dx: -0.5, dy: -0.5)),
                   with: .color(.white), lineWidth: 1)
    }

    /// Tay nắm kiểu ngoặc góc (⌐ ¬ L ⌐) + thanh nhỏ giữa cạnh, vẽ NẰM TRONG khung
    /// nên không che nội dung xung quanh. Gọn và hiện đại hơn 8 chấm tròn.
    private func drawHandles(_ ctx: inout GraphicsContext, _ rect: CGRect) {
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
        var halo = Path()
        var fill = Path()
        for b in bars {
            halo.addRoundedRect(in: b.insetBy(dx: -1, dy: -1),
                                cornerSize: CGSize(width: 2, height: 2))
            fill.addRoundedRect(in: b, cornerSize: CGSize(width: 1.5, height: 1.5))
        }
        ctx.fill(halo, with: .color(.black.opacity(0.4)))
        ctx.fill(fill, with: .color(.white))
    }

    /// Đường gióng ở cạnh vừa bị hút. Chỉ kéo dài ra 2 phía NGOÀI khung (không
    /// cắt ngang vùng chọn) và mờ dần ở đầu mút — đỡ rối hơn kẻ hết màn hình.
    private func drawGuides(_ ctx: inout GraphicsContext, around rect: CGRect, in bounds: CGRect) {
        guard !model.guideXs.isEmpty || !model.guideYs.isEmpty else { return }
        let ext: CGFloat = 130
        let stops = Gradient(colors: [OverlayChrome.guide.opacity(0.85),
                                      OverlayChrome.guide.opacity(0)])

        func fade(_ from: CGPoint, _ to: CGPoint) {
            let horizontal = abs(to.x - from.x) > abs(to.y - from.y)
            let thickness: CGFloat = 1
            let r = horizontal
                ? CGRect(x: min(from.x, to.x), y: from.y - thickness / 2,
                         width: abs(to.x - from.x), height: thickness)
                : CGRect(x: from.x - thickness / 2, y: min(from.y, to.y),
                         width: thickness, height: abs(to.y - from.y))
            ctx.fill(Path(r), with: .linearGradient(stops, startPoint: from, endPoint: to))
        }

        for x in model.guideXs {
            fade(CGPoint(x: x, y: rect.minY), CGPoint(x: x, y: max(0, rect.minY - ext)))
            fade(CGPoint(x: x, y: rect.maxY), CGPoint(x: x, y: min(bounds.maxY, rect.maxY + ext)))
        }
        for y in model.guideYs {
            fade(CGPoint(x: rect.minX, y: y), CGPoint(x: max(0, rect.minX - ext), y: y))
            fade(CGPoint(x: rect.maxX, y: y), CGPoint(x: min(bounds.maxX, rect.maxX + ext), y: y))
        }
    }

    /// Hai đường mảnh chạy qua con trỏ khi chưa có gì được khoanh — giúp gióng
    /// mắt trước lúc bấm chuột.
    private func drawCrosshair(_ ctx: inout GraphicsContext, in bounds: CGRect) {
        var p = Path()
        p.move(to: CGPoint(x: model.cursor.x + 0.5, y: 0))
        p.addLine(to: CGPoint(x: model.cursor.x + 0.5, y: bounds.maxY))
        p.move(to: CGPoint(x: 0, y: model.cursor.y + 0.5))
        p.addLine(to: CGPoint(x: bounds.maxX, y: model.cursor.y + 0.5))
        ctx.stroke(p, with: .color(.white.opacity(0.20)), lineWidth: 1)
    }

    // ── Nhãn kích thước ──────────────────────────────────────────────────

    /// Nhãn của khung: mặc định nằm ngay trên góc trái, hết chỗ thì tụt xuống
    /// dưới, chật nữa thì chui vào trong khung.
    @ViewBuilder
    private func badge(in bounds: CGRect) -> some View {
        if !model.currentRect.isEmpty {
            chip(sizeText(of: model.currentRect), for: model.currentRect, accent: false, in: bounds)
        } else if let hover = model.hover {
            let kind = hover.isWindow ? "Window" : "Item"
            chip("\(kind)   \(sizeText(of: hover.rect))", for: hover.rect, accent: true, in: bounds)
        }
    }

    private func chip(_ text: String, for rect: CGRect, accent: Bool, in bounds: CGRect) -> some View {
        let size = OverlayChrome.chipSize(text)
        var origin = CGPoint(x: rect.minX, y: rect.minY - size.height - 8)
        if origin.y < 6 {
            origin.y = rect.maxY + 8
            if origin.y + size.height > bounds.maxY - 6 { origin.y = rect.minY + 8 }
        }
        origin.x = max(6, min(origin.x, bounds.maxX - size.width - 6))
        return ChipView(text: text, accent: accent)
            .position(x: origin.x + size.width / 2, y: origin.y + size.height / 2)
    }

    private func sizeText(of rect: CGRect) -> String {
        let w = Int((rect.width * model.scaleFactor).rounded())
        let h = Int((rect.height * model.scaleFactor).rounded())
        return "\(w) × \(h)"
    }

    // ── Kính lúp ─────────────────────────────────────────────────────────

    /// Kính lúp phóng vùng quanh con trỏ từ ảnh đóng băng → chọn tới từng pixel.
    /// Chỉ hiện khi chưa khoanh gì, hoặc đang kéo.
    @ViewBuilder
    private func loupe(in bounds: CGRect) -> some View {
        if let cg = model.frozen, model.currentRect.isEmpty || model.dragging {
            let box = OverlayChrome.loupeBox(cursor: model.cursor, in: bounds,
                                             side: loupeSide, reserveBelow: 44)
            let color = OverlayChrome.pixelColor(in: cg, at: model.cursor, scale: model.scaleFactor)
            let coords = "\(Int(model.cursor.x * model.scaleFactor)), "
                + "\(Int(model.cursor.y * model.scaleFactor))"
            let label = color.map { "\(OverlayChrome.hex(of: $0))  \(coords)" } ?? coords
            let chipH = OverlayChrome.chipSize(label, fontSize: 11, swatch: color != nil).height

            LoupeView(image: cg, cursor: model.cursor, scale: model.scaleFactor,
                      side: loupeSide, zoom: 8)
                .position(x: box.midX, y: box.midY)

            // Chip dưới kính: mã màu + toạ độ pixel.
            ChipView(text: label, fontSize: 11, swatch: color.map { Color(nsColor: $0) })
                .position(x: box.midX, y: box.maxY + 6 + chipH / 2)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// View bắt sự kiện chuột/phím + lo phần bắt dính. Trong suốt, nằm đè lên lớp vẽ.
// ─────────────────────────────────────────────────────────────────────────
final class SelectionEventView: NSView {
    // Callback trả kết quả ra ngoài (giống props onSelected / onCancel).
    var onSelected: ((CGRect) -> Void)?   // rect theo points, gốc trên-trái màn hình
    var onCancel: (() -> Void)?

    let model: SelectionModel

    // Hai lớp bắt dính: hình học cửa sổ + phân tích pixel.
    var windows = WindowSnapper.empty
    var snap: SnapEngine? { didSet { refreshHover() } }

    private var startPoint: NSPoint?
    private var freeMode = false            // giữ ⌥ = tắt bắt dính

    private let dragThreshold: CGFloat = 4  // xê dưới mức này vẫn tính là "bấm", không phải "kéo"
    private let snapRadius: CGFloat = 9     // bán kính hút (points) — macshot chỉ 4

    init(frame: NSRect, model: SelectionModel) {
        self.model = model
        super.init(frame: frame)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) chưa dùng tới") }

    // Lật trục y: gốc toạ độ về góc TRÊN-trái, khớp với SwiftUI và ảnh CGImage.
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

    // ── Bắt dính ─────────────────────────────────────────────────────────

    /// Đặt vị trí con trỏ ban đầu (lúc overlay vừa mở, chưa có mouseMoved nào)
    /// để khung gợi ý hiện ngay dưới chuột thay vì nằm ở góc màn hình.
    func primeCursor(_ p: NSPoint) {
        model.cursor = p
        refreshHover()
    }

    /// Dò item dưới con trỏ: ưu tiên khung do phân tích pixel tìm ra, không có
    /// thì lấy nguyên cửa sổ.
    private func refreshHover() {
        guard model.snapEnabled, !freeMode, !model.dragging else {
            if model.hover != nil { model.hover = nil }
            return
        }
        let win = windows.window(at: model.cursor)
        var found: SnapTarget?
        if let el = snap?.element(at: model.cursor, within: win ?? bounds) {
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
        if found != model.hover { model.hover = found }
    }

    /// Hút 4 cạnh của khung đang kéo về biên gần nhất: cạnh cửa sổ (chính xác
    /// tuyệt đối) trước, rồi mới tới biên dò từ pixel.
    private func snapped(_ r: CGRect) -> CGRect {
        var gx: [CGFloat] = [], gy: [CGFloat] = []
        defer { model.guideXs = gx; model.guideYs = gy }
        guard model.snapEnabled, !freeMode else { return r }

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
        if h0 { gx.append(nx0) }
        if h1 { gx.append(nx1) }
        if h2 { gy.append(ny0) }
        if h3 { gy.append(ny1) }
        minX = nx0; maxX = nx1; minY = ny0; maxY = ny1

        guard maxX - minX >= 1, maxY - minY >= 1 else { return r }
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    // ── Sự kiện chuột / phím ─────────────────────────────────────────────
    override func mouseMoved(with event: NSEvent) {
        model.cursor = convert(event.locationInWindow, from: nil)
        freeMode = event.modifierFlags.contains(.option)
        refreshHover()
    }

    override func flagsChanged(with event: NSEvent) {
        let free = event.modifierFlags.contains(.option)
        guard free != freeMode else { return }
        freeMode = free
        if model.dragging, let start = startPoint {
            model.currentRect = rect(from: start, to: model.cursor)
        }
        refreshHover()
    }

    override func mouseDown(with event: NSEvent) {
        startPoint = convert(event.locationInWindow, from: nil)
        model.cursor = startPoint ?? .zero
        model.currentRect = .zero
        model.dragging = false
        model.interacted = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start = startPoint else { return }
        model.cursor = convert(event.locationInWindow, from: nil)
        freeMode = event.modifierFlags.contains(.option)
        // Bấm chuột bao giờ cũng xê vài pixel (trackpad càng rõ). Chỉ tính là KÉO
        // khi vượt ngưỡng — chưa vượt thì giữ nguyên `hover`, để thả ra vẫn chụp
        // được đúng cửa sổ đang khoanh thay vì bị huỷ.
        if !model.dragging {
            guard hypot(model.cursor.x - start.x, model.cursor.y - start.y) >= dragThreshold
            else { return }
            model.dragging = true
            model.hover = nil
        }
        model.currentRect = rect(from: start, to: model.cursor)
    }

    // Chuẩn hoá để kéo theo hướng nào cũng ra rect dương, rồi cho bắt dính.
    private func rect(from start: NSPoint, to p: CGPoint) -> CGRect {
        let raw = CGRect(x: min(start.x, p.x), y: min(start.y, p.y),
                         width: abs(p.x - start.x), height: abs(p.y - start.y))
        guard raw.width >= 3, raw.height >= 3 else { return raw }
        return snapped(raw)
    }

    override func mouseUp(with event: NSEvent) {
        if model.dragging, model.currentRect.width >= 5, model.currentRect.height >= 5 {
            onSelected?(model.currentRect)
        } else if let hover = model.hover {
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
        let model = SelectionModel()
        // Tỉ lệ point→pixel lấy từ chính ảnh đóng băng (khớp với ảnh sẽ cắt ra),
        // không có ảnh thì mới dùng backingScaleFactor.
        model.scaleFactor = frozen.map { CGFloat($0.width) / max(size.width, 1) }
            ?? screen.backingScaleFactor
        model.frozen = frozen
        model.snapEnabled = AppSettings.shared.snapToEdges
        model.hintText = model.snapEnabled
            ? "Drag to capture · click a highlighted area · ⌥ free · esc"
            : "Drag to capture · esc to cancel"

        let view = SelectionEventView(frame: NSRect(origin: .zero, size: size), model: model)
        view.autoresizingMask = [.width, .height]
        // Lấy danh sách cửa sổ TRƯỚC khi overlay hiện lên (khỏi dính chính mình).
        if model.snapEnabled {
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

        let chrome = PassthroughHostingView(rootView: SelectionOverlay(model: model))
        chrome.frame = NSRect(origin: .zero, size: size)
        chrome.autoresizingMask = [.width, .height]
        backdrop.addSubview(chrome)
        backdrop.addSubview(view)          // lớp bắt sự kiện nằm trên cùng

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
        if let frozen, model.snapEnabled {
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
