import SwiftUI
import AppKit
import UniformTypeIdentifiers

// ─────────────────────────────────────────────────────────────────────────
// Data model. Points are NORMALIZED (0...1) relative to the image, so they
// stay correct when the board is resized (responsive) and when exported full-res.
// ─────────────────────────────────────────────────────────────────────────
enum Tool: String, CaseIterable, Identifiable {
    // .image KHÔNG có nút trên toolbar — chỉ sinh ra khi paste ảnh (⌘V).
    case select, rect, ellipse, line, arrow, highlight, pen, text, counter, image
    var id: String { rawValue }

    var icon: String {
        switch self {
        case .select:    return "cursorarrow"
        case .rect:      return "rectangle"
        case .ellipse:   return "circle"
        case .line:      return "line.diagonal"
        case .arrow:     return "arrow.up.right"
        case .highlight: return "highlighter"
        case .pen:       return "pencil.tip"
        case .text:      return "textformat"
        case .counter:   return "1.circle"
        case .image:     return "photo"
        }
    }
    var label: String {
        switch self {
        case .select:    return "Select"
        case .rect:      return "Rectangle"
        case .ellipse:   return "Ellipse"
        case .line:      return "Line"
        case .arrow:     return "Arrow"
        case .highlight: return "Highlight"
        case .pen:       return "Pen"
        case .text:      return "Text"
        case .counter:   return "Counter"
        case .image:     return "Image"
        }
    }
    var drawsOnDrag: Bool {
        switch self {
        case .select, .text, .counter, .image: return false
        default: return true
        }
    }
    var placesOnTap: Bool { self == .text || self == .counter }
}

struct Annotation: Identifiable {
    let id = UUID()
    var tool: Tool
    var color: Color
    var lineWidth: CGFloat   // fraction of board width (scales with size)
    var points: [CGPoint]    // normalized 0...1
    var text: String = ""
    var number: Int = 0
    var image: NSImage? = nil   // chỉ dùng cho tool .image (ảnh dán vào)
    var flipH: Bool = false     // lật ngang
    var flipV: Bool = false     // lật dọc
    var rotation: Int = 0       // số lần xoay 90° theo chiều kim đồng hồ (0...3)
}

struct NamedColor: Identifiable {
    let id = UUID()
    let name: String
    let color: Color
}

// ─────────────────────────────────────────────────────────────────────────
// Editor: image + Canvas annotation layer — CleanShot-style layout.
// ─────────────────────────────────────────────────────────────────────────
struct EditorView: View {
    let sourceURL: URL?
    var onClose: (() -> Void)? = nil

    // Ảnh nền để @State được vì flip/rotate sẽ thay nó bằng ảnh đã biến đổi.
    @State private var image: NSImage
    @State private var zoom: CGFloat = 1               // 1 = vừa khung (fit); >1 phóng to
    @State private var zoomBase: CGFloat = 1           // zoom lúc bắt đầu pinch (neo cử chỉ)
    @State private var annotations: [Annotation] = []
    @State private var redoStack: [Annotation] = []   // các nét đã undo, chờ redo
    @State private var clearedBackup: [Annotation]?   // ảnh chụp annotation lúc bấm Clear (để Undo khôi phục cả loạt)
    @State private var keyMonitor: Any?               // theo dõi ⌘Z/⌘⇧Z/⌘V khi editor mở
    @State private var dragStartNorm: CGPoint?        // điểm trước đó khi kéo bằng Select
    @State private var dragTargetID: UUID?            // layer đang bị kéo
    @State private var selectedID: UUID?              // layer đang được chọn (hiện handle)
    @State private var resizeCorner: Int?             // góc đang kéo resize: 0=TL 1=TR 2=BR 3=BL
    @State private var current: Annotation?
    @State private var tool: Tool = .arrow
    @State private var color: Color = .red
    @State private var lineWidth: CGFloat = 0.005
    @State private var editingID: UUID?
    @State private var status: String = ""
    @State private var showColorPopover = false
    @State private var showWidthPopover = false
    @State private var showStickerPopover = false
    @FocusState private var textFocused: Bool

    init(image: NSImage, sourceURL: URL?, onClose: (() -> Void)? = nil) {
        _image = State(initialValue: image)
        self.sourceURL = sourceURL
        self.onClose = onClose
    }

    private let barHeight: CGFloat = 52

    private let palette: [NamedColor] = [
        .init(name: "Red", color: .red),
        .init(name: "Orange", color: .orange),
        .init(name: "Yellow", color: .yellow),
        .init(name: "Green", color: .green),
        .init(name: "Blue", color: .blue),
        .init(name: "Purple", color: .purple),
        .init(name: "Pink", color: .pink),
        .init(name: "White", color: .white),
        .init(name: "Black", color: .black),
    ]
    private let widths: [(String, CGFloat)] = [
        ("Thin", 0.003), ("Normal", 0.005), ("Bold", 0.008), ("Heavy", 0.012),
    ]

    private var nextCounter: Int {
        (annotations.filter { $0.tool == .counter }.map { $0.number }.max() ?? 0) + 1
    }

    // ── Layout: toolbar OVERLAYS the top so it's never clipped ─────────────
    var body: some View {
        ZStack(alignment: .top) {
            VStack(spacing: 0) {
                Color.clear.frame(height: barHeight)
                boardArea
                bottomBar
            }
            topBar
        }
        .frame(minWidth: 720, minHeight: 480)
        .background(Color(white: 0.11))
        .ignoresSafeArea()
        .onAppear { installKeyMonitor() }
        .onDisappear { removeKeyMonitor() }
    }

    // ── ⌘Z undo / ⌘⇧Z redo cho annotation ─────────────────────────────────
    // Local monitor chạy TRƯỚC khi event vào responder chain. Nếu đang gõ chữ
    // trong ô Text (editingID != nil) thì trả lại event để hệ thống undo VĂN BẢN
    // (qua Edit menu). Ngược lại tự undo/redo nét vẽ rồi "nuốt" event (return nil).
    private func installKeyMonitor() {
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            guard event.modifierFlags.contains(.command) else { return event }
            let key = event.charactersIgnoringModifiers?.lowercased()
            // ⌘V: đang gõ chữ → để hệ thống paste VĂN BẢN; ngược lại thử dán ẢNH.
            if key == "v" {
                if editingID != nil { return event }
                return pasteImage() ? nil : event
            }
            // ⌘Z / ⌘⇧Z: undo/redo nét vẽ (đang gõ chữ thì để text tự undo).
            if key == "z" {
                if editingID != nil { return event }
                if event.modifierFlags.contains(.shift) { redo() } else { undo() }
                return nil
            }
            // ⌘+ / ⌘- / ⌘0: zoom (không phải thao tác text nên chạy cả khi đang gõ).
            if key == "=" || key == "+" { zoomIn();  return nil }
            if key == "-" || key == "_" { zoomOut(); return nil }
            if key == "0"               { zoomFit(); return nil }
            return event
        }
    }

    // Đọc ảnh từ clipboard, đặt thành 1 layer ở giữa canvas (~50% bề ngang),
    // giữ đúng tỉ lệ ảnh gốc. Trả về false nếu clipboard không có ảnh.
    private func pasteImage() -> Bool {
        guard let img = Self.imageFromClipboard() else { return false }
        placeImage(img, widthFraction: 0.5, note: "Pasted image")
        return true
    }

    // Đặt 1 ảnh thành layer .image ở giữa canvas, giữ đúng tỉ lệ ảnh gốc.
    // Dùng chung cho ⌘V (ảnh clipboard) và cho sticker.
    private func placeImage(_ img: NSImage, widthFraction: CGFloat, note: String) {
        let baseW = max(image.size.width, 1), baseH = max(image.size.height, 1)
        let imgW = max(img.size.width, 1), imgH = max(img.size.height, 1)
        // Toạ độ normalized tính theo ảnh nền → phải quy đổi để ảnh dán không bị méo.
        var nw = widthFraction
        var nh = (nw * baseW) * (imgH / imgW) / baseH
        if nh > 0.8 { let k = 0.8 / nh; nw *= k; nh *= k }   // ảnh quá cao → thu cho vừa khung
        let topLeft = CGPoint(x: 0.5 - nw / 2, y: 0.5 - nh / 2)
        let bottomRight = CGPoint(x: 0.5 + nw / 2, y: 0.5 + nh / 2)
        let a = Annotation(tool: .image, color: .clear, lineWidth: 0,
                           points: [topLeft, bottomRight], image: img)
        annotations.append(a)
        redoStack.removeAll(); clearedBackup = nil
        tool = .select        // chuyển sang Select để kéo chỉnh vị trí ngay
        selectedID = a.id     // chọn sẵn → hiện 4 handle, resize được luôn
        editingID = nil
        status = note
    }

    // Lấy ẢNH THẬT từ clipboard, xử lý cả khi copy FILE từ Finder.
    private static func imageFromClipboard() -> NSImage? {
        let pb = NSPasteboard.general
        // 1. Copy file ảnh từ Finder → clipboard chứa file URL → nạp NỘI DUNG file
        //    (NSImage(pasteboard:) ở đây chỉ trả icon của file, nên phải tự đọc).
        if let urls = pb.readObjects(forClasses: [NSURL.self],
              options: [.urlReadingContentsConformToTypes: [UTType.image.identifier]]) as? [URL],
           let url = urls.first, let img = NSImage(contentsOf: url) {
            return img
        }
        // 2. Bitmap thô trên clipboard (copy từ Preview/trình duyệt…).
        for type in [NSPasteboard.PasteboardType.png, .tiff] {
            if let data = pb.data(forType: type), let img = NSImage(data: data) {
                return img
            }
        }
        // 3. Phương án cuối.
        return NSImage(pasteboard: pb)
    }

    private func removeKeyMonitor() {
        if let m = keyMonitor { NSEvent.removeMonitor(m); keyMonitor = nil }
    }

    private func undo() {
        // Vừa bấm Clear (canvas trống) → Undo khôi phục NGUYÊN loạt vừa xóa.
        if annotations.isEmpty, let backup = clearedBackup {
            annotations = backup
            clearedBackup = nil
            status = ""
            return
        }
        guard !annotations.isEmpty else { return }
        redoStack.append(annotations.removeLast())
    }

    private func redo() {
        guard !redoStack.isEmpty else { return }
        annotations.append(redoStack.removeLast())
    }

    // Xóa SẠCH annotation trong 1 lần (đỡ phải Undo từng nét). Lưu lại loạt vừa
    // xóa vào clearedBackup → Undo (⌘Z) khôi phục nguyên trạng.
    private func clearAll() {
        guard !annotations.isEmpty else { return }
        let n = annotations.count
        clearedBackup = annotations
        annotations = []
        redoStack = []
        selectedID = nil
        editingID = nil
        current = nil
        status = "Cleared \(n) annotation\(n == 1 ? "" : "s") — Undo (⌘Z) to restore"
    }

    // ── Responsive board: image scales to fit, rồi nhân thêm `zoom` ─────────
    // Bọc trong ScrollView để khi phóng to (zoom>1) hoặc ảnh rất dài (scroll
    // capture) thì cuộn xem được. Trên macOS, ScrollView cuộn bằng trackpad/
    // bánh xe — KHÔNG cướp click-drag → cử chỉ VẼ vẫn hoạt động bình thường.
    private var boardArea: some View {
        GeometryReader { geo in
            let base = fittedSize(in: geo.size)        // kích thước ở mức "vừa khung"
            let size = CGSize(width: base.width * zoom, height: base.height * zoom)
            ScrollView([.horizontal, .vertical]) {
                // alignment .topLeading: ô nhập chữ & text vẽ ra dùng CÙNG gốc toạ độ
                // (góc trên-trái) → gõ xong chữ KHÔNG nhảy chỗ nữa.
                ZStack(alignment: .topLeading) {
                    Image(nsImage: image)
                        .resizable().interpolation(.high)
                        .frame(width: size.width, height: size.height)

                    Canvas { ctx, _ in
                        for a in annotations where a.id != editingID {
                            Self.draw(a, size: size, in: &ctx)
                        }
                        if let c = current { Self.draw(c, size: size, in: &ctx) }
                        // Khung + 4 handle góc khi đang chọn ảnh (chỉ ở tool Select).
                        if tool == .select, let a = selectedImage {
                            Self.drawHandles(a, size: size, in: &ctx)
                        }
                    }
                    .frame(width: size.width, height: size.height)

                    if let id = editingID, let idx = annotations.firstIndex(where: { $0.id == id }) {
                        TextField("Text…", text: $annotations[idx].text)
                            .textFieldStyle(.plain)
                            .font(.system(size: 0.022 * size.width, weight: .semibold))
                            .foregroundStyle(annotations[idx].color)
                            .frame(width: 240, alignment: .leading)
                            .focused($textFocused)
                            .offset(x: annotations[idx].points[0].x * size.width,
                                    y: annotations[idx].points[0].y * size.height)
                            .onSubmit { editingID = nil }
                    }
                }
                .frame(width: size.width, height: size.height, alignment: .topLeading)
                .shadow(color: .black.opacity(0.5), radius: 16, y: 6)
                .contentShape(Rectangle())
                .gesture(drawGesture(size))
                .simultaneousGesture(tapGesture(size))
                .padding(28)
                // Nhỏ hơn viewport → căn giữa; lớn hơn → ScrollView tự cho cuộn.
                .frame(minWidth: geo.size.width, minHeight: geo.size.height, alignment: .center)
            }
            .background(Color(white: 0.11))
            // Pinch ở BẤT KỲ đâu trong khung (kể cả vùng xám quanh ảnh) đều zoom.
            // Đặt trên ScrollView (có background phủ kín) nên hit-test cả viewport,
            // không chỉ riêng vùng ảnh.
            .simultaneousGesture(magnifyGesture)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(white: 0.11))
    }

    private func fittedSize(in avail: CGSize) -> CGSize {
        let pad: CGFloat = 28
        let w = max(avail.width - pad * 2, 50), h = max(avail.height - pad * 2, 50)
        let iw = max(image.size.width, 1), ih = max(image.size.height, 1)
        let factor = min(w / iw, h / ih)
        return CGSize(width: iw * factor, height: ih * factor)
    }

    // ── Top toolbar ───────────────────────────────────────────────────────
    // Vùng trống (Spacer) chính là vùng titlebar → vẫn kéo cửa sổ được ở đó.
    private var topBar: some View {
        HStack(spacing: 10) {
            toolbarTools
            colorControl
            widthControl
            stickerControl
            undoButton
            clearButton
            imageTransformControls   // luôn hiện: có chọn ảnh→đổi ảnh đó, không→đổi ảnh nền
            Spacer(minLength: 8)
            Button("Save as…") { saveAs() }
                .controlSize(.large)
                .buttonBorderShape(.roundedRectangle(radius: 4))
            Button("Done") { done() }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .buttonBorderShape(.roundedRectangle(radius: 4))
                .keyboardShortcut(.defaultAction)
        }
        .padding(.horizontal, 14)
        .frame(height: barHeight)
        .frame(maxWidth: .infinity)
        .background(Color(white: 0.13))
        .overlay(alignment: .bottom) {   // hairline ngăn với canvas
            Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
        }
    }

    // Các tool gom thành 1 pill, chia khối bằng vạch ngăn cho dễ nhìn.
    private var toolbarTools: some View {
        HStack(spacing: 3) {
            toolButton(.select)
            divider
            toolButton(.rect); toolButton(.ellipse); toolButton(.line); toolButton(.arrow)
            divider
            toolButton(.highlight); toolButton(.pen)
            divider
            toolButton(.text); toolButton(.counter)
        }
        .padding(5)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).stroke(.white.opacity(0.08), lineWidth: 1))
    }

    private var divider: some View {
        Rectangle().fill(.white.opacity(0.12))
            .frame(width: 1, height: 22).padding(.horizontal, 2)
    }

    private func toolButton(_ t: Tool) -> some View {
        Button { haptic(); tool = t; editingID = nil } label: {
            Image(systemName: t.icon)
                .font(.system(size: 14, weight: .medium))
                .frame(width: 30, height: 28)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(tool == t ? .white : Color(white: 0.80))
        .background(RoundedRectangle(cornerRadius: 7)
                        .fill(tool == t ? Color.accentColor : .clear))
        .help(t.label)
    }

    // ── Color: swatch tròn → popover lưới màu ──────────────────────────────
    private var colorControl: some View {
        Button { showColorPopover.toggle() } label: {
            Circle().fill(color)
                .frame(width: 18, height: 18)
                .overlay(Circle().stroke(.white.opacity(0.55), lineWidth: 1.5))
                .padding(8)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .help("Color")
        .popover(isPresented: $showColorPopover, arrowEdge: .bottom) {
            let cols = Array(repeating: GridItem(.fixed(26), spacing: 10), count: 5)
            LazyVGrid(columns: cols, spacing: 10) {
                ForEach(palette) { c in
                    Button { color = c.color; showColorPopover = false } label: {
                        Circle().fill(c.color)
                            .frame(width: 24, height: 24)
                            .overlay(Circle().stroke(
                                .white.opacity(c.color == color ? 0.95 : 0.25),
                                lineWidth: c.color == color ? 2.5 : 1))
                    }
                    .buttonStyle(.plain)
                    .help(c.name)
                }
            }
            .padding(14)
        }
    }

    // ── Width: icon → popover xem trước độ dày nét ─────────────────────────
    private var widthControl: some View {
        Button { showWidthPopover.toggle() } label: {
            Image(systemName: "lineweight")
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 30, height: 30)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .help("Stroke width")
        .popover(isPresented: $showWidthPopover, arrowEdge: .bottom) {
            VStack(spacing: 2) {
                ForEach(widths, id: \.0) { name, w in
                    Button { lineWidth = w; showWidthPopover = false } label: {
                        HStack(spacing: 12) {
                            RoundedRectangle(cornerRadius: 3)
                                .fill(lineWidth == w ? Color.accentColor : Color(white: 0.8))
                                .frame(width: 64, height: max(w * 600, 1.5))
                            Text(name).font(.system(size: 13))
                            Spacer()
                            if lineWidth == w {
                                Image(systemName: "checkmark").font(.system(size: 11, weight: .bold))
                            }
                        }
                        .padding(.horizontal, 12).padding(.vertical, 8)
                        .frame(width: 200)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(8)
        }
    }

    // ── Sticker: icon mặt cười → popover chọn bộ + sticker ─────────────────
    // Chèn xong là 1 layer .image bình thường: kéo, resize góc, lật, xoay, Undo.
    private var stickerControl: some View {
        Button { showStickerPopover.toggle() } label: {
            Image(systemName: "face.smiling")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 30, height: 30)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .help("Stickers")
        .popover(isPresented: $showStickerPopover, arrowEdge: .bottom) {
            StickerPicker { img, name in
                haptic()
                placeImage(img, widthFraction: 0.28, note: "Added sticker “\(name)”")
                showStickerPopover = false
            }
        }
    }

    // Bộ nút lật/xoay. Áp dụng cho ảnh đang chọn, hoặc ảnh nền nếu không chọn gì.
    private var imageTransformControls: some View {
        HStack(spacing: 3) {
            transformButton("arrow.left.and.right.righttriangle.left.righttriangle.right",
                            "Flip horizontal") { doFlip(horizontal: true) }
            transformButton("arrow.up.and.down.righttriangle.up.righttriangle.down",
                            "Flip vertical") { doFlip(horizontal: false) }
            divider
            transformButton("rotate.left", "Rotate left") { doRotate(clockwise: false) }
            transformButton("rotate.right", "Rotate right") { doRotate(clockwise: true) }
        }
        .padding(5)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).stroke(.white.opacity(0.08), lineWidth: 1))
    }

    private func transformButton(_ symbol: String, _ tip: String,
                                 _ action: @escaping () -> Void) -> some View {
        Button { haptic(); action() } label: {
            Image(systemName: symbol)
                .font(.system(size: 14, weight: .medium))
                .frame(width: 30, height: 28)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(Color(white: 0.85))
        .help(tip)
    }

    private var undoButton: some View {
        Button { undo() } label: {
            Image(systemName: "arrow.uturn.backward")
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 32, height: 30)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .disabled(annotations.isEmpty)
        .help("Undo")
    }

    // Xóa sạch annotation 1 phát (Undo khôi phục lại được).
    private var clearButton: some View {
        Button { clearAll() } label: {
            Image(systemName: "trash")
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 32, height: 30)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .disabled(annotations.isEmpty)
        .help("Clear all annotations")
    }

    // ── Bottom bar ─────────────────────────────────────────────────────────
    private var bottomBar: some View {
        HStack(spacing: 10) {
            Text(status.isEmpty
                 ? "\(annotations.count) annotation\(annotations.count == 1 ? "" : "s")"
                 : status)
                .font(.caption)
                .foregroundStyle(Color(white: 0.55))
            Spacer()
            zoomControls
            iconButton("square.and.arrow.up", "Share") { share() }
            iconButton("doc.on.doc", "Copy") { copyToClipboard() }
        }
        .padding(.horizontal, 16)
        .frame(height: 44)
        .background(Color(white: 0.13))
        .overlay(alignment: .top) {   // hairline ngăn với canvas
            Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
        }
    }

    // ── Zoom: −  [phần trăm, bấm để Fit]  + ────────────────────────────────
    // % tính theo mức "vừa khung" (zoom=1 ⇒ 100%). ⌘+ / ⌘- / ⌘0 cũng chạy.
    private var zoomControls: some View {
        HStack(spacing: 1) {
            zoomButton("minus") { zoomOut() }
            Button { zoomFit() } label: {
                Text("\(Int((zoom * 100).rounded()))%")
                    .font(.system(size: 11, weight: .medium).monospacedDigit())
                    .foregroundStyle(Color(white: 0.85))
                    .frame(width: 42)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Fit to window (⌘0)")
            zoomButton("plus") { zoomIn() }
        }
        .padding(.horizontal, 3).padding(.vertical, 2)
        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 5))
    }

    private func zoomButton(_ symbol: String, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 24, height: 24)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func zoomIn()  { zoom = min(zoom * 1.25, 16) }
    private func zoomOut() { zoom = max(zoom / 1.25, 0.25) }
    private func zoomFit() { zoom = 1 }

    // Pinch trackpad để zoom. magnification bắt đầu từ 1 mỗi cử chỉ → nhân với
    // zoom lúc bắt đầu (zoomBase) rồi kẹp. Là simultaneousGesture nên KHÔNG đụng
    // cử chỉ vẽ (vẽ = 1 ngón kéo; pinch = 2 ngón).
    private var magnifyGesture: some Gesture {
        MagnifyGesture()
            .onChanged { v in
                zoom = min(max(zoomBase * v.magnification, 0.25), 16)
            }
            .onEnded { _ in zoomBase = zoom }
    }

    private func iconButton(_ symbol: String, _ tip: String,
                            _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 34, height: 30)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 4))
        .help(tip)
    }

    // ── Gestures (convert to normalized coords using the live board size) ──
    private func norm(_ p: CGPoint, _ s: CGSize) -> CGPoint {
        CGPoint(x: p.x / s.width, y: p.y / s.height)
    }

    // Tìm layer trên cùng chứa điểm p (duyệt ngược = từ trên xuống dưới).
    private func hitTest(_ p: CGPoint) -> UUID? {
        for a in annotations.reversed() {
            guard let r = Self.boundingRect(a) else { continue }
            if r.insetBy(dx: -0.012, dy: -0.012).contains(p) { return a.id }
        }
        return nil
    }

    private static func boundingRect(_ a: Annotation) -> CGRect? {
        guard let f = a.points.first else { return nil }
        var minX = f.x, minY = f.y, maxX = f.x, maxY = f.y
        for pt in a.points {
            minX = min(minX, pt.x); minY = min(minY, pt.y)
            maxX = max(maxX, pt.x); maxY = max(maxY, pt.y)
        }
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    // Layer ảnh đang được chọn (nil nếu không có / không phải ảnh).
    private var selectedImage: Annotation? {
        guard let id = selectedID,
              let a = annotations.first(where: { $0.id == id }), a.tool == .image
        else { return nil }
        return a
    }

    // 4 góc của 1 annotation theo thứ tự TL, TR, BR, BL (normalized).
    private static func corners(_ a: Annotation) -> [CGPoint] {
        guard let r = boundingRect(a) else { return [] }
        return [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY),
                CGPoint(x: r.maxX, y: r.maxY), CGPoint(x: r.minX, y: r.maxY)]
    }

    // Bấm gần góc nào (trả index) — tol theo normalized (~ vài chục px).
    private func nearCorner(_ p: CGPoint, of a: Annotation) -> Int? {
        let tol: CGFloat = 0.02
        for (i, c) in Self.corners(a).enumerated() {
            if abs(p.x - c.x) < tol && abs(p.y - c.y) < tol { return i }
        }
        return nil
    }

    // Vẽ khung chọn + ô vuông trắng ở 4 góc.
    private static func drawHandles(_ a: Annotation, size s: CGSize,
                                    in ctx: inout GraphicsContext) {
        guard let r = boundingRect(a) else { return }
        let box = CGRect(x: r.minX * s.width, y: r.minY * s.height,
                         width: r.width * s.width, height: r.height * s.height)
        ctx.stroke(Path(box), with: .color(.white.opacity(0.9)),
                   style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
        let hs: CGFloat = 9
        for c in corners(a) {
            let center = CGPoint(x: c.x * s.width, y: c.y * s.height)
            let sq = CGRect(x: center.x - hs / 2, y: center.y - hs / 2, width: hs, height: hs)
            ctx.fill(Path(roundedRect: sq, cornerRadius: 2), with: .color(.white))
            ctx.stroke(Path(roundedRect: sq, cornerRadius: 2),
                       with: .color(.accentColor), lineWidth: 1.5)
        }
    }

    // ── Hành động transform: ảnh đang chọn, hoặc ảnh nền nếu không chọn ─────
    // Index của ảnh layer đang chọn (nil nếu không chọn ảnh nào).
    private var selectedImageIndex: Int? {
        guard let id = selectedID else { return nil }
        guard let i = annotations.firstIndex(where: { $0.id == id }),
              annotations[i].tool == .image else { return nil }
        return i
    }

    private func doFlip(horizontal: Bool) {
        if let i = selectedImageIndex {
            if horizontal { annotations[i].flipH.toggle() } else { annotations[i].flipV.toggle() }
        } else {
            flipBase(horizontal: horizontal)
        }
    }

    private func doRotate(clockwise: Bool) {
        if let i = selectedImageIndex {
            annotations[i].rotation = (annotations[i].rotation + (clockwise ? 1 : 3)) % 4
        } else {
            rotateBase(clockwise: clockwise)
        }
    }

    // Lật ảnh NỀN + lật theo toạ độ của mọi annotation để chúng dính đúng chỗ.
    private func flipBase(horizontal: Bool) {
        guard let cg = ImageOps.cg(image),
              let out = ImageOps.flip(cg, horizontal: horizontal) else { return }
        image = ImageOps.ns(out, scale: image.size.width / CGFloat(cg.width))
        for i in annotations.indices {
            annotations[i].points = annotations[i].points.map {
                horizontal ? CGPoint(x: 1 - $0.x, y: $0.y) : CGPoint(x: $0.x, y: 1 - $0.y)
            }
            if annotations[i].tool == .image {
                if horizontal { annotations[i].flipH.toggle() } else { annotations[i].flipV.toggle() }
            }
        }
    }

    // Xoay ảnh NỀN 90° + xoay toạ độ annotation tương ứng (normalized).
    private func rotateBase(clockwise: Bool) {
        guard let cg = ImageOps.cg(image),
              let out = ImageOps.rotate90(cg, clockwise: clockwise) else { return }
        image = ImageOps.ns(out, scale: image.size.width / CGFloat(cg.width))
        for i in annotations.indices {
            annotations[i].points = annotations[i].points.map {
                clockwise ? CGPoint(x: 1 - $0.y, y: $0.x) : CGPoint(x: $0.y, y: 1 - $0.x)
            }
            if annotations[i].tool == .image {
                annotations[i].rotation = (annotations[i].rotation + (clockwise ? 1 : 3)) % 4
            }
        }
    }

    // Kéo 1 góc về điểm p. points[0]=TL, points[1]=BR. Góc còn lại neo cố định.
    private func resize(_ a: inout Annotation, corner: Int, to p: CGPoint) {
        guard a.points.count >= 2 else { return }
        switch corner {
        case 0: a.points[0] = p                                   // TL
        case 1: a.points[1].x = p.x; a.points[0].y = p.y          // TR
        case 2: a.points[1] = p                                   // BR
        case 3: a.points[0].x = p.x; a.points[1].y = p.y          // BL
        default: break
        }
    }

    private func drawGesture(_ size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 4)
            .onChanged { v in
                // Select: kéo để DI CHUYỂN, hoặc kéo GÓC để resize.
                if tool == .select {
                    let p = norm(v.location, size)
                    if dragStartNorm == nil {
                        let start = norm(v.startLocation, size)
                        dragStartNorm = start
                        // Đang chọn ảnh & bấm trúng 1 góc → vào chế độ resize.
                        if let img = selectedImage, let corner = nearCorner(start, of: img) {
                            resizeCorner = corner
                            dragTargetID = img.id
                        } else {
                            resizeCorner = nil
                            let hit = hitTest(start)
                            dragTargetID = hit
                            selectedID = hit   // kéo trúng layer nào thì chọn layer đó
                        }
                    }
                    if let id = dragTargetID,
                       let idx = annotations.firstIndex(where: { $0.id == id }) {
                        if let corner = resizeCorner {
                            resize(&annotations[idx], corner: corner, to: p)
                        } else if let prev = dragStartNorm {
                            let dx = p.x - prev.x, dy = p.y - prev.y
                            annotations[idx].points = annotations[idx].points
                                .map { CGPoint(x: $0.x + dx, y: $0.y + dy) }
                        }
                    }
                    dragStartNorm = p
                    return
                }
                guard tool.drawsOnDrag else { return }
                let p = norm(v.location, size)
                if current == nil {
                    current = Annotation(tool: tool, color: color, lineWidth: lineWidth,
                                         points: [norm(v.startLocation, size), p])
                } else if tool == .pen {
                    current?.points.append(p)
                } else {
                    current?.points[1] = p
                }
            }
            .onEnded { _ in
                if tool == .select {
                    dragStartNorm = nil; dragTargetID = nil; resizeCorner = nil; return
                }
                // Bỏ nét quá ngắn (lỡ kéo nhẹ) → hết "chấm rác". Pen luôn giữ.
                if let c = current, c.tool == .pen || Self.isBigEnough(c) {
                    annotations.append(c)
                    redoStack.removeAll(); clearedBackup = nil   // vẽ nét mới → bỏ lịch sử redo/khôi phục
                }
                current = nil
            }
    }

    private static func isBigEnough(_ a: Annotation) -> Bool {
        guard let s = a.points.first, let e = a.points.last else { return false }
        return abs(e.x - s.x) > 0.004 || abs(e.y - s.y) > 0.004
    }

    private func tapGesture(_ size: CGSize) -> some Gesture {
        SpatialTapGesture()
            .onEnded { v in
                let p = norm(v.location, size)
                // Select: bấm trúng layer thì chọn, bấm chỗ trống thì bỏ chọn.
                if tool == .select { selectedID = hitTest(p); return }
                guard tool.placesOnTap else { return }
                switch tool {
                case .counter:
                    annotations.append(Annotation(tool: .counter, color: color,
                                                  lineWidth: lineWidth, points: [p],
                                                  number: nextCounter))
                    redoStack.removeAll(); clearedBackup = nil
                case .text:
                    let a = Annotation(tool: .text, color: color, lineWidth: lineWidth, points: [p])
                    annotations.append(a)
                    redoStack.removeAll(); clearedBackup = nil
                    editingID = a.id
                    textFocused = true
                default: break
                }
            }
    }

    // ── Draw one annotation, scaling normalized coords to `size` ───────────
    private static func draw(_ a: Annotation, size s: CGSize, in ctx: inout GraphicsContext) {
        func P(_ n: CGPoint) -> CGPoint { CGPoint(x: n.x * s.width, y: n.y * s.height) }
        guard let n0 = a.points.first else { return }
        let start = P(n0)
        let end = P(a.points.last ?? n0)
        let lw = max(a.lineWidth * s.width, 1)

        switch a.tool {
        case .select:
            break
        case .image:
            // Vẽ ảnh dán vào, fit khung [start, end], có áp dụng lật/xoay.
            if let img = a.image {
                let r = rect(start, end)
                let odd = a.rotation % 2 != 0
                // Xoay 90°/270° thì bề ngang↔cao đổi chỗ → vẽ trong hệ đã xoay
                // bằng kích thước hoán đổi để lấp đúng khung trên màn hình.
                let dw = odd ? r.height : r.width
                let dh = odd ? r.width : r.height
                ctx.drawLayer { layer in
                    layer.translateBy(x: r.midX, y: r.midY)
                    layer.rotate(by: .degrees(Double(a.rotation) * 90))
                    layer.scaleBy(x: a.flipH ? -1 : 1, y: a.flipV ? -1 : 1)
                    // .high như ảnh nền: sticker/ảnh dán thường nhỏ hơn khung vẽ
                    // nhiều lần, để nội suy mặc định là nhoè hẳn.
                    layer.draw(Image(nsImage: img).interpolation(.high),
                               in: CGRect(x: -dw / 2, y: -dh / 2, width: dw, height: dh))
                }
            }
        case .rect:
            ctx.stroke(Path(roundedRect: rect(start, end), cornerRadius: lw),
                       with: .color(a.color), lineWidth: lw)
        case .ellipse:
            ctx.stroke(Path(ellipseIn: rect(start, end)),
                       with: .color(a.color), lineWidth: lw)
        case .line:
            var p = Path(); p.move(to: start); p.addLine(to: end)
            ctx.stroke(p, with: .color(a.color),
                       style: StrokeStyle(lineWidth: lw, lineCap: .round))
        case .highlight:
            var p = Path(); p.move(to: start); p.addLine(to: end)
            ctx.stroke(p, with: .color(a.color.opacity(0.35)),
                       style: StrokeStyle(lineWidth: 0.03 * s.width, lineCap: .round))
        case .pen:
            var p = Path(); p.move(to: start)
            for pt in a.points.dropFirst() { p.addLine(to: P(pt)) }
            ctx.stroke(p, with: .color(a.color),
                       style: StrokeStyle(lineWidth: lw, lineCap: .round, lineJoin: .round))
        case .arrow:
            drawArrow(from: start, to: end, color: a.color, lineWidth: lw,
                      headLen: 0.02 * s.width + lw * 2, in: &ctx)
        case .text:
            guard !a.text.isEmpty else { return }
            ctx.draw(Text(a.text)
                        .font(.system(size: 0.022 * s.width, weight: .semibold))
                        .foregroundColor(a.color),
                     at: start, anchor: .topLeading)
        case .counter:
            let r = 0.013 * s.width
            let circle = Path(ellipseIn: CGRect(x: start.x - r, y: start.y - r,
                                                width: r * 2, height: r * 2))
            ctx.fill(circle, with: .color(a.color))
            ctx.draw(Text("\(a.number)")
                        .font(.system(size: r * 1.2, weight: .bold))
                        .foregroundColor(.white),
                     at: start, anchor: .center)
        }
    }

    private static func rect(_ s: CGPoint, _ e: CGPoint) -> CGRect {
        CGRect(x: min(s.x, e.x), y: min(s.y, e.y),
               width: abs(e.x - s.x), height: abs(e.y - s.y))
    }

    private static func drawArrow(from s: CGPoint, to e: CGPoint, color: Color,
                                  lineWidth lw: CGFloat, headLen: CGFloat,
                                  in ctx: inout GraphicsContext) {
        var line = Path(); line.move(to: s); line.addLine(to: e)
        ctx.stroke(line, with: .color(color),
                   style: StrokeStyle(lineWidth: lw, lineCap: .round))

        let angle = atan2(e.y - s.y, e.x - s.x)
        let spread = CGFloat.pi / 7
        var head = Path()
        head.move(to: e)
        head.addLine(to: CGPoint(x: e.x + cos(angle + .pi - spread) * headLen,
                                 y: e.y + sin(angle + .pi - spread) * headLen))
        head.move(to: e)
        head.addLine(to: CGPoint(x: e.x + cos(angle + .pi + spread) * headLen,
                                 y: e.y + sin(angle + .pi + spread) * headLen))
        ctx.stroke(head, with: .color(color),
                   style: StrokeStyle(lineWidth: lw, lineCap: .round))
    }

    // ── Export at full resolution ──────────────────────────────────────────
    @MainActor
    private func renderImage() -> NSImage? {
        editingID = nil
        let px = image.size
        let content = ZStack {
            Image(nsImage: image).resizable().interpolation(.high)
                .frame(width: px.width, height: px.height)
            Canvas { ctx, _ in
                for a in annotations { Self.draw(a, size: px, in: &ctx) }
            }
            .frame(width: px.width, height: px.height)
        }
        .frame(width: px.width, height: px.height)

        let renderer = ImageRenderer(content: content)
        renderer.scale = 1
        return renderer.nsImage
    }

    private func pngData(_ img: NSImage) -> Data? {
        encode(img, as: .png)
    }

    // Mã hoá NSImage sang định dạng tuỳ chọn (PNG/JPEG/TIFF/BMP/GIF).
    private func encode(_ img: NSImage, as type: NSBitmapImageRep.FileType) -> Data? {
        guard let tiff = img.tiffRepresentation,
              let rep = NSBitmapImageRep(data: tiff) else { return nil }
        let props: [NSBitmapImageRep.PropertyKey: Any] =
            type == .jpeg ? [.compressionFactor: 0.9] : [:]
        return rep.representation(using: type, properties: props)
    }


    // Rung nhẹ (trackpad Force Touch) cho mỗi thao tác → cảm giác "ăn nút".
    private func haptic() {
        NSHapticFeedbackManager.defaultPerformer.perform(.levelChange, performanceTime: .now)
    }

    private func copyToClipboard() {
        haptic()
        guard let img = renderImage() else { return }
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.writeObjects([img])
        status = "Copied"
    }

    @MainActor
    private func share() {
        haptic()
        guard let img = renderImage(),
              let view = NSApp.keyWindow?.contentView else { return }
        let picker = NSSharingServicePicker(items: [img])
        picker.show(relativeTo: .zero, of: view, preferredEdge: .minY)
    }

    private func done() {
        copyToClipboard()
        onClose?()
    }

    private func saveAs() {
        haptic()
        let base = sourceURL?.deletingPathExtension().lastPathComponent ?? "SlopShot edited"
        let panel = NSSavePanel()
        // Tự thêm dropdown chọn định dạng (NSSavePanel không tự hiện popup này).
        let accessory = SaveFormatAccessory(baseName: base)
        panel.accessoryView = accessory.makeAccessory(for: panel)
        panel.nameFieldStringValue = "\(base).png"

        guard panel.runModal() == .OK, let url = panel.url else { return }

        guard let img = renderImage(),
              let data = encode(img, as: accessory.selectedFileType) else {
            status = "Export failed"; return
        }
        do {
            try data.write(to: url)
            status = "Saved \(url.lastPathComponent)"
        } catch {
            status = "Save failed"
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tiện ích lật/xoay ảnh ở mức CGImage (giữ nguyên số pixel → không mất nét).
// ─────────────────────────────────────────────────────────────────────────
enum ImageOps {
    // NSImage → CGImage để xử lý bằng CoreGraphics.
    static func cg(_ img: NSImage) -> CGImage? {
        var rect = CGRect(origin: .zero, size: img.size)
        return img.cgImage(forProposedRect: &rect, context: nil, hints: nil)
    }

    // CGImage → NSImage, đặt lại size (points) để layout/zoom giữ đúng tỉ lệ.
    static func ns(_ cg: CGImage, scale: CGFloat) -> NSImage {
        NSImage(cgImage: cg, size: NSSize(width: CGFloat(cg.width) * scale,
                                          height: CGFloat(cg.height) * scale))
    }

    private static func context(_ w: Int, _ h: Int) -> CGContext? {
        CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                  space: CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    }

    static func flip(_ cg: CGImage, horizontal: Bool) -> CGImage? {
        let w = cg.width, h = cg.height
        guard let ctx = context(w, h) else { return nil }
        if horizontal { ctx.translateBy(x: CGFloat(w), y: 0); ctx.scaleBy(x: -1, y: 1) }
        else          { ctx.translateBy(x: 0, y: CGFloat(h)); ctx.scaleBy(x: 1, y: -1) }
        ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
        return ctx.makeImage()
    }

    static func rotate90(_ cg: CGImage, clockwise: Bool) -> CGImage? {
        let w = cg.width, h = cg.height
        // Ảnh mới hoán đổi bề ngang ↔ cao.
        guard let ctx = context(h, w) else { return nil }
        ctx.translateBy(x: CGFloat(h) / 2, y: CGFloat(w) / 2)
        ctx.rotate(by: clockwise ? -.pi / 2 : .pi / 2)   // context y-up: âm = thuận chiều KĐH
        ctx.draw(cg, in: CGRect(x: -CGFloat(w) / 2, y: -CGFloat(h) / 2,
                                width: CGFloat(w), height: CGFloat(h)))
        return ctx.makeImage()
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Dropdown chọn định dạng cho NSSavePanel (panel không tự hiện cái này).
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class SaveFormatAccessory: NSObject {
    private let formats: [(title: String, type: NSBitmapImageRep.FileType, ut: UTType)] = [
        ("PNG", .png, .png),
        ("JPEG", .jpeg, .jpeg),
        ("TIFF", .tiff, .tiff),
        ("BMP", .bmp, .bmp),
        ("GIF", .gif, .gif),
    ]
    private let baseName: String
    private weak var panel: NSSavePanel?
    /// Chỉ số định dạng đang chọn. Là ObservableObject nên Picker bên dưới
    /// đọc/ghi thẳng vào đây, khỏi cần target-action.
    private let choice = Choice()

    @MainActor
    final class Choice: ObservableObject {
        @Published var index = 0
    }

    init(baseName: String) {
        self.baseName = baseName
        super.init()
    }

    var selectedFileType: NSBitmapImageRep.FileType {
        formats[min(max(choice.index, 0), formats.count - 1)].type
    }

    // NSSavePanel.accessoryView đòi một NSView → bọc SwiftUI qua NSHostingView.
    func makeAccessory(for panel: NSSavePanel) -> NSView {
        self.panel = panel
        let titles = formats.map { $0.title }
        let host = NSHostingView(rootView: SaveFormatPicker(
            titles: titles, choice: choice,
            onChange: { [weak self] in self?.applyExtension() }))
        host.frame = NSRect(origin: .zero, size: host.fittingSize)
        applyExtension()
        return host
    }

    // Đổi đuôi file trong ô tên theo định dạng đang chọn.
    private func applyExtension() {
        guard let panel = panel else { return }
        let f = formats[min(max(choice.index, 0), formats.count - 1)]
        panel.allowedContentTypes = [f.ut]
        let ext = f.ut.preferredFilenameExtension ?? f.title.lowercased()
        panel.nameFieldStringValue = "\(baseName).\(ext)"
    }
}

private struct SaveFormatPicker: View {
    let titles: [String]
    @ObservedObject var choice: SaveFormatAccessory.Choice
    let onChange: () -> Void

    var body: some View {
        Picker("Format:", selection: $choice.index) {
            ForEach(Array(titles.enumerated()), id: \.offset) { i, t in
                Text(t).tag(i)
            }
        }
        .pickerStyle(.menu)
        .fixedSize()
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .onChange(of: choice.index) { _, _ in onChange() }
    }
}
