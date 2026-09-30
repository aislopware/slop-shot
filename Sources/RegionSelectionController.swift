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

    // Kéo xong KHÔNG chụp ngay: khung ở lại để kéo di chuyển / kéo cạnh chỉnh cỡ,
    // rồi mới bấm nút xác nhận (hoặc ↩ / double-click) — kiểu macshot.
    @Published var adjusting = false
    @Published var hoverButton: ActionButton?

    var frozen: CGImage?                     // nil = overlay trong suốt, không có kính lúp
    var scaleFactor: CGFloat = 2             // để hiện kích thước theo pixel
    var hintText = "Drag to capture"
    var toolTitle = "Capture Area"
    /// Thả chuột là chốt luôn, không qua bước chỉnh khung (editor inline lo tiếp).
    var skipAdjust = false
    /// Đang vẽ ngay trên vùng chọn: ẩn chip kích thước (thanh công cụ nằm đó).
    @Published var inlineEditing = false
    var toolIcon = "camera.viewfinder"
    var snapEnabled = true
    var confirmTitle = "Capture"
    var confirmIcon = "camera.fill"

    enum ActionButton { case confirm, cancel }

    static let barHeight: CGFloat = 32
    private static let barFont = NSFont.systemFont(ofSize: 13, weight: .semibold)

    /// Chỗ đặt hai nút dưới khung (hết chỗ thì lên trên, chật nữa thì chui vào
    /// trong). Lớp vẽ và lớp bắt chuột dùng CHUNG hàm này nên luôn trùng khít.
    func buttonFrames(in bounds: CGRect) -> (confirm: CGRect, cancel: CGRect) {
        let r = currentRect
        let h = Self.barHeight, gap: CGFloat = 8
        let titleW = (confirmTitle as NSString).size(withAttributes: [.font: Self.barFont]).width
        let confirmW = ceil(titleW) + 16 + 6 + 28      // icon + khoảng + đệm 2 bên
        let cancelW = h
        let total = cancelW + 6 + confirmW

        var y = r.maxY + gap
        if y + h > bounds.maxY - 6 {
            y = r.minY - gap - h
            if y < 6 { y = r.maxY - gap - h }
        }
        var x = r.maxX - total
        x = max(6, min(x, bounds.maxX - total - 6))
        let cancel = CGRect(x: x, y: y, width: cancelW, height: h)
        let confirm = CGRect(x: cancel.maxX + 6, y: y, width: confirmW, height: h)
        return (confirm, cancel)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Toàn bộ phần nhìn thấy của lớp phủ chọn vùng.
// ─────────────────────────────────────────────────────────────────────────
struct SelectionOverlay: View {
    @ObservedObject var model: SelectionModel
    /// Lớp tối + chữ mờ dần vào trên nền ảnh đóng băng (ảnh thì hiện tức thì —
    /// nó trùng khít màn hình nên người dùng không thấy "cửa sổ mới" bật lên).
    @State private var appeared = false

    private let loupeSide: CGFloat = 128

    var body: some View {
        GeometryReader { geo in
            let bounds = CGRect(origin: .zero, size: geo.size)
            ZStack(alignment: .topLeading) {
                Canvas { ctx, size in paint(&ctx, size: size) }

                // Luôn dựng sẵn, chỉ đổi độ mờ → ẩn/hiện mượt thay vì biến mất cái bụp.
                OverlayHint(title: model.hintText, subtitle: hintSubtitle)
                    .padding(.bottom, 96)
                    .frame(width: geo.size.width, height: geo.size.height, alignment: .bottom)
                    .opacity(model.interacted ? 0 : 1)
                    .animation(Theme.quick, value: model.interacted)

                ToolBadge(title: model.toolTitle, icon: model.toolIcon, cursor: model.cursor)
                badge(in: bounds)
                let showBar = model.adjusting && !model.dragging
                ZStack(alignment: .topLeading) { actionBar(in: bounds) }
                    .opacity(showBar ? 1 : 0)
                    .scaleEffect(showBar ? 1 : 0.96)
                    .animation(Theme.quick, value: showBar)
                loupe(in: bounds)
            }
        }
        .ignoresSafeArea()
        .opacity(appeared ? 1 : 0)
        .onAppear { withAnimation(.easeOut(duration: 0.12)) { appeared = true } }
    }

    // Hai nút cạnh khung: ✕ huỷ, ✓ chụp / bắt đầu quay. Chỉ để NHÌN — chuột
    // do SelectionEventView bắt (lớp SwiftUI này không nhận sự kiện).
    @ViewBuilder
    private func actionBar(in bounds: CGRect) -> some View {
        let f = model.buttonFrames(in: bounds)
        let cancelHot = model.hoverButton == .cancel
        let confirmHot = model.hoverButton == .confirm

        Image(systemName: "xmark")
            .font(.system(size: 13, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: f.cancel.width, height: f.cancel.height)
            .background(OverlayChrome.chipFill.opacity(cancelHot ? 1 : 0.92),
                        in: RoundedRectangle(cornerRadius: OverlayChrome.radius))
            .overlay(RoundedRectangle(cornerRadius: OverlayChrome.radius)
                .stroke(cancelHot ? Color.white.opacity(0.5) : OverlayChrome.chipEdge, lineWidth: 1))
            .animation(Theme.quick, value: cancelHot)
            .position(x: f.cancel.midX, y: f.cancel.midY)

        HStack(spacing: 6) {
            Image(systemName: model.confirmIcon)
            Text(model.confirmTitle)
        }
        .font(.system(size: 13, weight: .semibold))
        .foregroundStyle(.white)
        .frame(width: f.confirm.width, height: f.confirm.height)
        .background(Color(nsColor: .controlAccentColor).opacity(confirmHot ? 1 : 0.88),
                    in: RoundedRectangle(cornerRadius: OverlayChrome.radius))
        .overlay(RoundedRectangle(cornerRadius: OverlayChrome.radius)
            .stroke(Color.white.opacity(confirmHot ? 0.6 : 0.25), lineWidth: 1))
        .scaleEffect(confirmHot ? 1.03 : 1)
        .animation(Theme.quick, value: confirmHot)
        .position(x: f.confirm.midX, y: f.confirm.midY)
    }

    private var hintSubtitle: String {
        model.snapEnabled
            ? "click a highlighted area  ·  ⌥ free select  ·  ⇧ square  ·  hold space to move  ·  F full screen  ·  esc to cancel"
            : "⇧ square  ·  hold space to move  ·  F full screen  ·  esc to cancel"
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
        if model.inlineEditing {
            EmptyView()
        } else if !model.currentRect.isEmpty {
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
    private var squareMode = false          // giữ ⇧ lúc kéo = khung vuông
    private var spaceMove = false           // giữ Space lúc kéo = dời cả khung đang kéo
    private var lastDragPoint: CGPoint?

    /// Đang làm gì với khung ở bước chỉnh: kéo cả khung, hay kéo cạnh/góc nào.
    private enum Grab { case move, resize(Edges), button(SelectionModel.ActionButton) }
    private struct Edges: OptionSet {
        let rawValue: Int
        static let left = Edges(rawValue: 1), right = Edges(rawValue: 2)
        static let top = Edges(rawValue: 4), bottom = Edges(rawValue: 8)
    }
    private var grab: Grab?
    private var grabRect: CGRect = .zero    // khung lúc bắt đầu kéo/chỉnh
    private var previousRect: CGRect = .zero  // bấm hụt ra ngoài thì trả lại khung này
    private var startPointForGrab: CGPoint?   // chỗ bấm chuột lúc bắt đầu dời khung

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

    // Con trỏ hình chữ thập như mọi tool chụp màn hình. Ở bước chỉnh khung thì
    // con trỏ đổi theo chỗ đang trỏ (mouseMoved tự set), không dùng cursor rect.
    override func resetCursorRects() {
        if !model.adjusting { addCursorRect(bounds, cursor: .crosshair) }
    }

    // ── Bước chỉnh khung ─────────────────────────────────────────────────

    private func enterAdjust(_ rect: CGRect) {
        if model.skipAdjust {
            model.currentRect = rect
            model.hover = nil
            model.dragging = false
            model.guideXs = []; model.guideYs = []
            onSelected?(rect)
            return
        }
        model.currentRect = rect
        model.hover = nil
        model.dragging = false
        model.guideXs = []; model.guideYs = []
        model.adjusting = true
        window?.invalidateCursorRects(for: self)
        updateAdjustCursor()
    }

    /// Cạnh/góc nào đang nằm dưới con trỏ (vùng bắt rộng hơn nét vẽ cho dễ trúng).
    private func edges(at p: CGPoint) -> Edges {
        let r = model.currentRect
        let tol: CGFloat = 8
        guard p.x >= r.minX - tol, p.x <= r.maxX + tol,
              p.y >= r.minY - tol, p.y <= r.maxY + tol else { return [] }
        var e: Edges = []
        if abs(p.x - r.minX) <= tol { e.insert(.left) }
        else if abs(p.x - r.maxX) <= tol { e.insert(.right) }
        if abs(p.y - r.minY) <= tol { e.insert(.top) }
        else if abs(p.y - r.maxY) <= tol { e.insert(.bottom) }
        return e
    }

    private func button(at p: CGPoint) -> SelectionModel.ActionButton? {
        let f = model.buttonFrames(in: bounds)
        if f.confirm.contains(p) { return .confirm }
        if f.cancel.contains(p) { return .cancel }
        return nil
    }

    private func updateAdjustCursor() {
        let p = model.cursor
        let hot = button(at: p)
        if hot != model.hoverButton { model.hoverButton = hot }
        if hot != nil { NSCursor.pointingHand.set(); return }
        let e = edges(at: p)
        let pos: NSCursor.FrameResizePosition?
        switch e {
        case [.left, .top]:     pos = .topLeft
        case [.right, .top]:    pos = .topRight
        case [.left, .bottom]:  pos = .bottomLeft
        case [.right, .bottom]: pos = .bottomRight
        case [.left]:           pos = .left
        case [.right]:          pos = .right
        case [.top]:            pos = .top
        case [.bottom]:         pos = .bottom
        default:                pos = nil
        }
        if let pos {
            NSCursor.frameResize(position: pos, directions: .all).set()
        } else if model.currentRect.contains(p) {
            NSCursor.openHand.set()
        } else {
            NSCursor.crosshair.set()
        }
    }

    /// Khung mới khi đang kéo cạnh/góc: chỉ các cạnh đang nắm là đổi (và được
    /// hút), cạnh còn lại đứng yên. Kéo quá cạnh đối diện thì lật khung.
    private func resized(_ e: Edges, to p: CGPoint) -> CGRect {
        let o = grabRect
        var x0 = o.minX, x1 = o.maxX, y0 = o.minY, y1 = o.maxY
        if e.contains(.left) { x0 = p.x }
        if e.contains(.right) { x1 = p.x }
        if e.contains(.top) { y0 = p.y }
        if e.contains(.bottom) { y1 = p.y }
        var r = CGRect(x: min(x0, x1), y: min(y0, y1), width: abs(x1 - x0), height: abs(y1 - y0))
            .intersection(bounds)
        guard r.width >= 3, r.height >= 3 else { return r }
        let s = snapped(r)
        // Giữ nguyên cạnh không nắm; chỉ nhận phần hút của cạnh đang kéo.
        let fixedX = !(e.contains(.left) || e.contains(.right))
        let fixedY = !(e.contains(.top) || e.contains(.bottom))
        r = CGRect(x: fixedX ? r.minX : s.minX, y: fixedY ? r.minY : s.minY,
                   width: fixedX ? r.width : s.width, height: fixedY ? r.height : s.height)
        if fixedX { model.guideXs = [] }
        if fixedY { model.guideYs = [] }
        return r
    }

    private func moved(by dx: CGFloat, _ dy: CGFloat) -> CGRect {
        var r = grabRect.offsetBy(dx: dx, dy: dy)
        r.origin.x = max(bounds.minX, min(r.origin.x, bounds.maxX - r.width))
        r.origin.y = max(bounds.minY, min(r.origin.y, bounds.maxY - r.height))
        return r
    }

    private func confirm() {
        let r = model.currentRect
        guard r.width >= 5, r.height >= 5 else { return }
        onSelected?(r)
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
        if model.adjusting { updateAdjustCursor(); return }
        refreshHover()
    }

    override func flagsChanged(with event: NSEvent) {
        let free = event.modifierFlags.contains(.option)
        let square = event.modifierFlags.contains(.shift)
        guard free != freeMode || square != squareMode else { return }
        freeMode = free
        squareMode = square
        if model.dragging, let start = startPoint {
            model.currentRect = rect(from: start, to: model.cursor)
        }
        refreshHover()
    }

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if model.adjusting {
            model.cursor = p
            grabRect = model.currentRect
            if let b = button(at: p) {
                grab = .button(b)
                return
            }
            let e = edges(at: p)
            if !e.isEmpty {
                grab = .resize(e)
                model.dragging = true           // hiện kính lúp để canh cạnh tới từng pixel
                return
            }
            if model.currentRect.contains(p) {
                if event.clickCount >= 2 { confirm(); return }
                grab = .move
                startPointForGrab = p
                NSCursor.closedHand.set()
                return
            }
            // Bấm ra ngoài khung = khoanh lại từ đầu (bấm hụt thì trả khung cũ).
            previousRect = model.currentRect
            model.adjusting = false
            model.hoverButton = nil
            window?.invalidateCursorRects(for: self)
            NSCursor.crosshair.set()
        }
        grab = nil
        startPoint = p
        lastDragPoint = p
        model.cursor = startPoint ?? .zero
        model.currentRect = .zero
        model.dragging = false
        model.interacted = true
    }

    override func mouseDragged(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        freeMode = event.modifierFlags.contains(.option)
        switch grab {
        case .move:
            model.cursor = p
            // Toạ độ bấm ban đầu nằm trong grabRect; lệch bao nhiêu thì dời bấy nhiêu.
            let start = startPointForGrab ?? p
            model.currentRect = moved(by: p.x - start.x, p.y - start.y)
            return
        case .resize(let e):
            model.cursor = p
            model.currentRect = resized(e, to: p)
            return
        case .button(let b):
            model.cursor = p
            model.hoverButton = button(at: p) == b ? b : nil
            return
        case nil:
            break
        }
        guard var start = startPoint else { return }
        squareMode = event.modifierFlags.contains(.shift)
        // Giữ Space: cả khung đang kéo trượt theo chuột (điểm neo đi cùng), buông
        // ra thì kéo tiếp cỡ như cũ — giống macOS ⌘⇧4.
        if spaceMove, model.dragging, let last = lastDragPoint {
            start.x += p.x - last.x
            start.y += p.y - last.y
            startPoint = start
        }
        lastDragPoint = p
        model.cursor = p
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
    // Giữ ⇧ = khung vuông (cạnh dài hơn quyết định), không bắt dính vì hút
    // cạnh sẽ làm méo tỉ lệ.
    private func rect(from start: NSPoint, to p: CGPoint) -> CGRect {
        var end = p
        if squareMode {
            let side = max(abs(p.x - start.x), abs(p.y - start.y))
            end = CGPoint(x: start.x + (p.x >= start.x ? side : -side),
                          y: start.y + (p.y >= start.y ? side : -side))
        }
        let raw = CGRect(x: min(start.x, end.x), y: min(start.y, end.y),
                         width: abs(end.x - start.x), height: abs(end.y - start.y))
        guard raw.width >= 3, raw.height >= 3 else { return raw }
        if squareMode {
            model.guideXs = []; model.guideYs = []
            return raw
        }
        return snapped(raw).intersection(bounds)
    }

    override func mouseUp(with event: NSEvent) {
        if let g = grab {
            grab = nil
            startPointForGrab = nil
            model.dragging = false
            model.guideXs = []; model.guideYs = []
            if case .button(let b) = g {
                let p = convert(event.locationInWindow, from: nil)
                guard button(at: p) == b else { return }   // kéo ra ngoài nút = thôi
                if b == .confirm { confirm() } else { onCancel?() }
                return
            }
            // Chỉnh cỡ tới mức dẹp lép → trả lại khung trước đó.
            if model.currentRect.width < 5 || model.currentRect.height < 5 {
                model.currentRect = grabRect
            }
            updateAdjustCursor()
            return
        }
        startPoint = nil
        lastDragPoint = nil
        spaceMove = false
        if model.dragging, model.currentRect.width >= 5, model.currentRect.height >= 5 {
            enterAdjust(model.currentRect)
        } else if let hover = model.hover {
            // Bấm 1 phát (không kéo) lên vùng đang được khoanh → khoanh đúng vùng đó.
            enterAdjust(hover.rect)
        } else if previousRect.width >= 5 {
            enterAdjust(previousRect)   // bấm hụt ra ngoài khung đang chỉnh → giữ khung cũ
        } else {
            onCancel?()          // bấm hụt vào chỗ trống = huỷ, như trước
        }
        previousRect = .zero
    }

    override func rightMouseDown(with event: NSEvent) { onCancel?() }

    override func keyUp(with event: NSEvent) {
        if event.keyCode == 49 { spaceMove = false }
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 53:                                  // ⎋ = thoát hẳn, không lùi từng bước
            onCancel?()
        case 36, 76:                              // ↩ / enter = chụp khung đang chỉnh
            if model.adjusting { confirm() }
        case 49:                                  // Space (giữ) = dời khung đang kéo
            if startPoint != nil { spaceMove = true }
        case 3 where grab == nil && !model.dragging:  // F = cả màn hình
            startPoint = nil
            model.interacted = true
            enterAdjust(bounds)
        case 123, 124, 125, 126 where model.adjusting:
            // Mũi tên dời khung 1pt, giữ ⇧ thì 10pt.
            let step: CGFloat = event.modifierFlags.contains(.shift) ? 10 : 1
            let dx: CGFloat = event.keyCode == 123 ? -step : event.keyCode == 124 ? step : 0
            let dy: CGFloat = event.keyCode == 126 ? -step : event.keyCode == 125 ? step : 0
            grabRect = model.currentRect
            model.currentRect = moved(by: dx, dy)
        default:
            break
        }
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

    private var escMonitor: Any?
    private var cancelCurrent: (() -> Void)?
    private var model: SelectionModel?
    private weak var eventView: SelectionEventView?

    /// Đóng lớp phủ đang mở (nếu có), coi như huỷ.
    func cancel() { cancelCurrent?() }

    /// Chọn xong (kiểu inline): gỡ phần bắt chuột/phím của bước chọn, lớp phủ
    /// vẫn nằm đó chờ editor.
    private func detachSelection() {
        if let escMonitor { NSEvent.removeMonitor(escMonitor) }
        escMonitor = nil
        eventView?.removeFromSuperview()
        cancelCurrent = { [weak self] in self?.cleanup(fade: true) }
    }

    /// Đắp editor lên vùng vừa chọn (sau `begin(editInline: true)`). completion
    /// nhận ảnh đã vẽ + việc cần làm, hoặc nil nếu huỷ; lớp phủ tự đóng.
    func showInlineEditor(image: NSImage, rect: CGRect,
                          completion: @escaping ((NSImage, InlineEditHost.Action)?) -> Void) {
        guard let win = window, let backdrop = win.contentView else { completion(nil); return }
        var done = false
        let finish: ((NSImage, InlineEditHost.Action)?) -> Void = { [weak self] result in
            guard !done else { return }
            done = true
            self?.cleanup(fade: result == nil)
            completion(result)
        }
        model?.inlineEditing = true
        let host = InlineEditHost(rect: rect) { img, action in finish((img, action)) }
        let editor = EditorView(image: image, sourceURL: nil, onClose: { finish(nil) }, inline: host)
        let hv = NSHostingView(rootView: editor)
        hv.frame = backdrop.bounds
        hv.autoresizingMask = [.width, .height]
        backdrop.addSubview(hv)
        win.makeFirstResponder(hv)
        cancelCurrent = { finish(nil) }
    }

    /// `confirmTitle` / `confirmIcon`: chữ trên nút xác nhận ở bước chỉnh khung
    /// ("Capture", "Start Recording"…).
    func begin(on screen: NSScreen, frozen: CGImage? = nil,
               tool: String = "Capture Area", toolIcon: String = "camera.viewfinder",
               confirmTitle: String = "Capture", confirmIcon: String = "camera.fill",
               editInline: Bool = false,
               completion: @escaping (CGRect?) -> Void) {
        // Phiên cũ còn mở (phím tắt bấm dồn) → huỷ nó trước, không để lại một
        // lớp phủ mồ côi che màn hình mà không ai đóng.
        cancelCurrent?()
        let size = screen.frame.size
        let model = SelectionModel()
        // Tỉ lệ point→pixel lấy từ chính ảnh đóng băng (khớp với ảnh sẽ cắt ra),
        // không có ảnh thì mới dùng backingScaleFactor.
        model.scaleFactor = frozen.map { CGFloat($0.width) / max(size.width, 1) }
            ?? screen.backingScaleFactor
        model.frozen = frozen
        model.snapEnabled = AppSettings.shared.snapToEdges
        model.confirmTitle = confirmTitle
        model.toolTitle = tool
        model.toolIcon = toolIcon
        model.confirmIcon = confirmIcon
        model.hintText = model.snapEnabled
            ? "Drag to capture · click a highlighted area · ⌥ free · esc"
            : "Drag to capture · esc to cancel"

        model.skipAdjust = editInline
        let view = SelectionEventView(frame: NSRect(origin: .zero, size: size), model: model)
        self.model = model
        self.eventView = view
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
            // editInline: chọn xong thì GIỮ lớp phủ — showInlineEditor sẽ đắp
            // editor lên đúng cửa sổ này, không đóng-mở lại nên không nháy.
            if editInline, rect != nil { self?.detachSelection() } else { self?.cleanup(fade: rect == nil) }
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
        win.appearance = NSAppearance(named: .darkAqua)   // HUD nền tối cố định → chữ/nút hệ thống cũng phải tông tối, kể cả khi máy để Light
        win.isOpaque = false
        win.hasShadow = false
        win.animationBehavior = .none     // hiện tức thì: ảnh đóng băng phải trùng khít màn hình
        win.hidesOnDeactivate = false
        win.acceptsMouseMovedEvents = true
        win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        win.contentView = backdrop

        view.onSelected = { rect in finishOnce(rect) }
        view.onCancel = { finishOnce(nil) }
        cancelCurrent = { finishOnce(nil) }

        // Lưới an toàn cho ⎋: có lúc view mất first responder (bấm trúng lớp
        // khác, panel khác thành key…) và keyDown không tới nữa — ⎋ vẫn phải
        // thoát được, không thì người dùng kẹt dưới một lớp phủ kín màn hình.
        escMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            guard event.keyCode == 53 else { return event }
            MainActor.assumeIsolated { finishOnce(nil) }
            return nil
        }

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

    /// `fade`: chỉ khi HUỶ. Chọn xong thì phải biến mất ngay — luồng sau (quay,
    /// chụp cuộn) chụp màn hình sống, lớp phủ đang mờ dần sẽ lọt vào hình.
    private func cleanup(fade: Bool = false) {
        if let escMonitor { NSEvent.removeMonitor(escMonitor) }
        escMonitor = nil
        cancelCurrent = nil
        if let win = window { OverlayChrome.close(win, fade: fade) }
        window = nil
        model = nil
    }
}
