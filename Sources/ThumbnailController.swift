import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Preview card nổi kiểu CleanShot X:
//  - bình thường: chỉ hiện ảnh thu nhỏ (đúng tỷ lệ, bo góc, đổ bóng)
//  - khi rê chuột: làm tối ảnh + hiện 2 nút Copy/Save ở giữa + 4 nút tròn 4 góc
//  - click ảnh = mở editor; kéo ra ngoài = lôi file sang app khác
//
// Kéo-ra-ngoài dùng `.onDrag` (SwiftUI tự lo phần NSDraggingSource), chia sẻ
// dùng `ShareLink` — cả hai đều là đồ có sẵn, khỏi tự dựng lại.
// ─────────────────────────────────────────────────────────────────────────

/// `.onHover` của SwiftUI chỉ bắn khi app đang ACTIVE. Thẻ preview thì nổi trên
/// app khác trong lúc SlopShot chạy nền → rê chuột vào sẽ chẳng hiện nút nào.
/// Nên phần "chuột vào/ra" phải tự gắn tracking area `.activeAlways`.
@MainActor
final class HoverModel: ObservableObject {
    @Published var hovering = false
}

private final class HoverHostingView<Content: View>: NSHostingView<Content> {
    var onHover: ((Bool) -> Void)?

    required init(rootView: Content) { super.init(rootView: rootView) }
    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach { removeTrackingArea($0) }
        addTrackingArea(NSTrackingArea(rect: bounds,
                                       options: [.mouseEnteredAndExited, .activeAlways],
                                       owner: self, userInfo: nil))
    }
    override func mouseEntered(with event: NSEvent) { onHover?(true) }
    override func mouseExited(with event: NSEvent) { onHover?(false) }
}

/// Thẻ preview. Chế độ video đổi 2 nút góc: Pin → 👁 Quick Look, ✏️ → ✂️ Trim.
private struct ThumbnailCard: View {
    @ObservedObject var hover: HoverModel
    let image: NSImage
    let fileURL: URL
    let isVideo: Bool

    var onClick: () -> Void        // ✏️ edit ảnh / 👁 Quick Look video
    var onCopy: () -> Void
    var onSave: () -> Void
    var onTrim: (() -> Void)?
    var onClose: () -> Void
    var onPin: () -> Void

    static let pad: CGFloat = 14   // lề chừa cho bóng đổ
    static let card = CGSize(width: 210, height: 150)
    private let radius: CGFloat = 13

    private var hovering: Bool { hover.hovering }
    @State private var pinned = false
    /// Nút vừa bấm xong → đổi thành icon tick một nhịp trước khi thẻ trượt đi.
    @State private var ticked: String?

    var body: some View {
        ZStack {
            // Ảnh: aspect-FILL trong thẻ (phủ kín, cắt mép thừa → không méo).
            Color(white: 0.1)
            Image(nsImage: image)
                .resizable()
                .aspectRatio(contentMode: .fill)
        }
        .frame(width: Self.card.width, height: Self.card.height)
        .clipShape(RoundedRectangle(cornerRadius: radius))
        .overlay {
            RoundedRectangle(cornerRadius: radius).stroke(.white.opacity(0.22), lineWidth: 1)
        }
        .overlay { if isVideo && !hovering { playBadge } }
        .overlay { if hovering { controls } }
        .shadow(color: .black.opacity(0.5), radius: 10, y: 6)
        .padding(Self.pad)
        .contentShape(RoundedRectangle(cornerRadius: radius))
        .onTapGesture { onClick() }
        .onDrag {                       // kéo ra ngoài = lôi file sang app khác
            NSItemProvider(contentsOf: fileURL) ?? NSItemProvider()
        } preview: {
            Image(nsImage: image).resizable().aspectRatio(contentMode: .fill)
                .frame(width: Self.card.width, height: Self.card.height)
                .clipShape(RoundedRectangle(cornerRadius: radius))
        }
    }

    /// Nút ▶ tròn ở giữa cho biết đây là clip quay được.
    private var playBadge: some View {
        Image(systemName: "play.fill")
            .font(.system(size: 15, weight: .bold))
            .foregroundStyle(.white.opacity(0.95))
            .frame(width: 44, height: 44)
            .background(.black.opacity(0.45), in: Circle())
    }

    /// Lớp tối + 4 nút góc + 2 capsule giữa — chỉ hiện khi rê chuột vào.
    private var controls: some View {
        ZStack {
            Color.black.opacity(0.42)

            VStack(spacing: 6) {
                capsule("Copy", tick: ticked == "copy") { fire("copy", onCopy) }
                capsule("Save", tick: ticked == "save") { fire("save", onSave) }
            }

            VStack {
                HStack {
                    circle("xmark", "Discard") { haptic(); onClose() }
                    Spacer()
                    if isVideo {
                        circle("eye", "Quick Look") { haptic(); onClick() }
                    } else {
                        circle(pinned ? "pin.fill" : "pin", "Pin") {
                            haptic(); pinned.toggle(); onPin()
                        }
                    }
                }
                Spacer()
                HStack {
                    if isVideo {
                        circle("scissors", "Trim") { haptic(); onTrim?(); onClose() }
                    } else {
                        circle("pencil.tip.crop.circle", "Edit") { haptic(); onClick(); onClose() }
                    }
                    Spacer()
                    ShareLink(item: fileURL) {
                        circleLabel("square.and.arrow.up")
                    }
                    .buttonStyle(.plain)
                    .help("Share")
                }
            }
            .padding(6)
        }
        .clipShape(RoundedRectangle(cornerRadius: radius))
    }

    // Copy/Save: hiện tick một nhịp rồi để thẻ trượt đi.
    private func fire(_ key: String, _ action: @escaping () -> Void) {
        haptic()
        action()
        ticked = key
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) { onClose() }
    }

    private func capsule(_ title: String, tick: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Group {
                if tick {
                    Image(systemName: "checkmark.circle.fill").font(.system(size: 15, weight: .semibold))
                } else {
                    Text(title).font(.system(size: 12, weight: .semibold))
                }
            }
            .foregroundStyle(.black.opacity(0.85))
            .frame(width: 68, height: 23)
            .background(.white.opacity(0.92), in: Capsule())
        }
        .buttonStyle(.plain)
    }

    private func circle(_ symbol: String, _ tip: String,
                        _ action: @escaping () -> Void) -> some View {
        Button(action: action) { circleLabel(symbol) }
            .buttonStyle(.plain)
            .help(tip)
    }

    private func circleLabel(_ symbol: String) -> some View {
        Image(systemName: symbol)
            .font(.system(size: 10, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: 24, height: 24)
            .background(.black.opacity(0.55), in: Circle())
            .contentShape(Circle())
    }

    // Rung nhẹ (chỉ trackpad Force Touch). React không có khái niệm này 😄
    private func haptic() {
        NSHapticFeedbackManager.defaultPerformer.perform(.levelChange, performanceTime: .now)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Controller: hiện/ẩn panel preview nổi ở góc dưới-trái màn hình.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class ThumbnailController {
    private var panel: NSPanel?
    private var dismissTask: Task<Void, Never>?
    private var pinned = false

    func show(image: NSImage,
              fileURL: URL,
              isVideo: Bool = false,
              onEdit: @escaping () -> Void,
              onCopy: @escaping () -> Void,
              onSave: @escaping () -> Void,
              onTrim: (() -> Void)? = nil) {
        hide()   // dọn cái cũ trước
        pinned = false

        // Card CHỮ NHẬT cố định 210×150. Ảnh aspect-fill bên trong.
        let pad = ThumbnailCard.pad
        let size = NSSize(width: ThumbnailCard.card.width + pad * 2,
                          height: ThumbnailCard.card.height + pad * 2)

        guard let screen = NSScreen.main else { return }
        let margin: CGFloat = 24
        let origin = NSPoint(x: screen.visibleFrame.minX + margin,
                             y: screen.visibleFrame.minY + margin)   // góc dưới-trái
        let frame = NSRect(origin: origin, size: size)

        let hover = HoverModel()
        let card = ThumbnailCard(
            hover: hover,
            image: image, fileURL: fileURL, isVideo: isVideo,
            onClick: onEdit, onCopy: onCopy, onSave: onSave, onTrim: onTrim,
            onClose: { [weak self] in self?.dismiss(slide: true) },
            onPin: { [weak self] in
                guard let self else { return }
                self.pinned.toggle()
                if self.pinned { self.dismissTask?.cancel() }
            })

        let host = HoverHostingView(rootView: card)
        host.onHover = { [weak self] inside in
            guard let self else { return }
            hover.hovering = inside
            if inside { self.dismissTask?.cancel() }        // rê vào: giữ lại
            else if !self.pinned { self.scheduleDismiss(after: 1.5) }  // rê ra: ẩn (trừ khi ghim)
        }

        let p = NSPanel(contentRect: frame,
                        styleMask: [.borderless, .nonactivatingPanel],
                        backing: .buffered, defer: false)
        p.level = .floating
        p.backgroundColor = .clear
        p.isOpaque = false
        p.hasShadow = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        p.contentView = host

        // Trượt VÀO từ mép trái (đối xứng với lúc đóng trượt ra) + fade.
        var startFrame = frame
        startFrame.origin.x -= frame.width + 40   // bắt đầu ngoài mép trái
        p.setFrame(startFrame, display: false)
        p.alphaValue = 0
        p.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.32
            ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
            p.animator().setFrame(frame, display: true)
            p.animator().alphaValue = 1
        }
        self.panel = p

        scheduleDismiss(after: 8)   // rê chuột vào / ghim sẽ huỷ hẹn ẩn
    }

    func hide() {
        dismissTask?.cancel()
        dismissTask = nil
        panel?.orderOut(nil)
        panel = nil
    }

    // Trượt panel sang trái khỏi màn rồi ẩn hẳn (dùng cho Copy / Edit / Discard).
    func dismiss(slide: Bool) {
        dismissTask?.cancel(); dismissTask = nil
        guard slide, let p = panel else { hide(); return }
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.28
            ctx.timingFunction = CAMediaTimingFunction(name: .easeIn)
            var f = p.frame
            f.origin.x -= f.width + 40   // ra hẳn ngoài mép trái
            p.animator().setFrame(f, display: true)
            p.animator().alphaValue = 0
        }, completionHandler: { [weak self] in
            MainActor.assumeIsolated { self?.hide() }   // completion chạy trên main
        })
    }

    private func scheduleDismiss(after seconds: Double) {
        dismissTask?.cancel()
        dismissTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self?.panel?.animator().alphaValue = 0
            try? await Task.sleep(nanoseconds: 300_000_000)
            guard !Task.isCancelled else { return }
            self?.hide()
        }
    }
}
