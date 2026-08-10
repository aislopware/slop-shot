import ScreenCaptureKit
import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Điều phối phiên "chụp cuộn": chụp lại 1 vùng nhiều lần trong lúc user cuộn,
// đưa từng frame cho ScrollStitcher ghép, rồi trả ảnh dài khi bấm Done.
//
//   start() → bật timer chụp ~5fps → mỗi frame: stitcher.add → cập nhật đếm px
//   Done    → tắt timer, trả stitcher.finalImage()
//   Cancel  → tắt timer, trả nil
// ─────────────────────────────────────────────────────────────────────────
// Panel borderless bình thường KHÔNG nhận được phím. Override để thanh điều
// khiển làm "key window" → bấm ⏎ (Done) / Esc (Cancel) chạy mà không cần chuột.
final class KeyPanel: NSPanel {
    override var canBecomeKey: Bool { true }
}

// ─────────────────────────────────────────────────────────────────────────
// Trạng thái của các panel nổi trong phiên chụp cuộn.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class ScrollCaptureModel: ObservableObject {
    @Published var status = "Scroll through the area…"
    @Published var autoScroll = false
    @Published var preview: CGImage?

    var onAutoChange: ((Bool) -> Void)?
    var onDone: (() -> Void)?
    var onCancel: (() -> Void)?
}

// Thanh điều khiển nhỏ: hướng dẫn + số px đã chụp + Auto-scroll / Cancel / Done.
struct ScrollCaptureBar: View {
    @ObservedObject var model: ScrollCaptureModel

    var body: some View {
        HStack(spacing: 10) {
            // Nhãn trạng thái rộng cố định, cắt đuôi gọn nếu quá dài → không bao
            // giờ tràn đè lên nút.
            Text(model.status)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.white)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(width: 130, alignment: .leading)

            Toggle("Auto-scroll", isOn: $model.autoScroll)
                .toggleStyle(.button)

            Spacer(minLength: 0)

            Button("Cancel") { model.onCancel?() }
                .keyboardShortcut(.cancelAction)
            Button("Done") { model.onDone?() }
                .keyboardShortcut(.defaultAction)
        }
        .padding(.horizontal, 14)
        .frame(width: ScrollCaptureBar.width, height: ScrollCaptureBar.height)
        .background(Color(white: 0.13).opacity(0.96), in: RoundedRectangle(cornerRadius: 11))
        .onChange(of: model.autoScroll) { _, on in model.onAutoChange?(on) }
    }

    static let width: CGFloat = 470
    static let height: CGFloat = 44
}

// Pill "Please slow down" ở giữa vùng chụp.
struct ScrollSlowPill: View {
    var body: some View {
        Text("Please slow down")
            .font(.system(size: 14, weight: .semibold))
            .foregroundStyle(.white)
            .padding(.horizontal, 18)
            .padding(.vertical, 11)
            .background(Color(white: 0.1).opacity(0.92), in: Capsule())
    }
}

// Live preview ảnh dài ở mép phải: hàng mới nhất luôn ở đáy.
struct ScrollPreviewPanel: View {
    @ObservedObject var model: ScrollCaptureModel

    var body: some View {
        Group {
            if let cg = model.preview {
                Image(decorative: cg, scale: 1)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
            }
        }
        .padding(6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(white: 0.12).opacity(0.96))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

// Spinner lúc ghép ảnh cuối.
struct ScrollProcessingPanel: View {
    var body: some View {
        VStack(spacing: 14) {
            ProgressView()
                .controlSize(.large)
            Text("Building image…")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.white)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(white: 0.12).opacity(0.96))
        .clipShape(RoundedRectangle(cornerRadius: 14))
    }
}

@MainActor
final class ScrollCaptureController {
    private var panel: NSPanel?
    private var slowPanel: NSPanel?         // pill "Please slow down" giữa vùng
    private var previewPanel: NSPanel?      // live preview ảnh dài ở mép phải
    private var processingPanel: NSPanel?   // spinner "Building image…" lúc Done
    private var captureScreen: NSScreen?    // màn đang chụp (để đặt spinner giữa)
    private var previewBottomY: CGFloat = 0 // đáy panel cố định, mọc lên trên
    private var previewMaxH: CGFloat = 0
    private let previewWc: CGFloat = 160
    private var timer: Timer?

    private let model = ScrollCaptureModel()

    // Tự cuộn (autoscroll).
    private var autoScroll = false
    private var idleCount = 0               // số nhịp KHÔNG có nội dung mới (để biết đã hết trang)
    private var scrollCenterCG: CGPoint = .zero   // tâm vùng theo toạ độ CGEvent (gốc trên-trái)
    private var autoStepPoints: CGFloat = 38      // bước cuộn mỗi nhịp (point); đặt theo cao vùng

    // Đọc offset cuộn THẬT qua Accessibility (bước "xa" như CleanShot).
    private let tracker = ScrollAreaTracker()
    private var axOK = false                 // AX có đọc được offset không (hiện lên bar)
    private var lastAXValue: CGFloat?        // giá trị 0..1 nhịp trước
    private var rangePx: CGFloat = 0         // số px ứng với toàn dải 0..1 (hiệu chỉnh 1 lần)

    private var stitcher: ScrollStitcher?
    private var filter: SCContentFilter?
    private var config: SCStreamConfiguration?
    private var capturing = false               // tránh chụp chồng khi frame trước chưa xong
    private var finished = false                 // đã bấm Done/Cancel → bỏ qua mọi nhịp đến trễ
    private var onFinish: ((CGImage?) -> Void)?
    private var onStop: (() -> Void)?           // gọi NGAY khi Done/Cancel (ẩn viền đỏ liền)

    // Việc ghép ảnh nặng → dồn hết về 1 queue nền nối tiếp, KHÔNG để chạm main
    // thread (nếu không UI sẽ đơ vì mỗi frame phải dò hàng pixel).
    private let stitchQueue = DispatchQueue(label: "com.thanglb.slopshot.scrollstitch")

    // ── Bắt đầu phiên chụp cuộn cho vùng `rect` (points, gốc trên-trái) ─────
    func start(rect: CGRect, screen: NSScreen,
               onStop: @escaping () -> Void,
               onFinish: @escaping (CGImage?) -> Void) async throws {
        self.onFinish = onFinish
        self.onStop = onStop
        self.captureScreen = screen
        self.finished = false

        model.status = "Scroll through the area…"
        model.autoScroll = false
        model.preview = nil
        model.onAutoChange = { [weak self] on in self?.setAuto(on) }
        model.onDone = { [weak self] in self?.tapDone() }
        model.onCancel = { [weak self] in self?.tapCancel() }

        // Tìm display + dựng filter/config CROP đúng vùng (giống ScreenRecorder).
        let content = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: false)
        guard let display = content.displays.first(where: { $0.displayID == screen.displayID })
                ?? content.displays.first else {
            throw NSError(domain: "SlopShot", code: 20,
                          userInfo: [NSLocalizedDescriptionKey: "No display found to capture."])
        }

        let scale = screen.backingScaleFactor
        let pxW = max(Int((rect.width  * scale).rounded()), 1)
        let pxH = max(Int((rect.height * scale).rounded()), 1)

        let cfg = SCStreamConfiguration()
        cfg.sourceRect = rect
        cfg.width  = pxW
        cfg.height = pxH
        cfg.showsCursor = false          // bỏ con trỏ → khỏi gây nhiễu khi dò chồng

        self.config = cfg
        let st = ScrollStitcher(width: pxW)
        st.enablePreview(targetWidth: Int(150 * scale))   // preview ~150pt bề ngang
        self.stitcher = st

        // Tâm vùng theo toạ độ CGEvent (gốc TRÊN-trái của màn chính) để autoscroll.
        let mainH = NSScreen.screens.first?.frame.height ?? screen.frame.height
        scrollCenterCG = CGPoint(
            x: screen.frame.minX + rect.midX,
            y: mainH - (screen.frame.minY + (screen.frame.height - rect.midY)))

        // Bước autoscroll ≈ 38% chiều cao vùng/nhịp (kẹp 90…420pt). Đủ nhanh mà
        // vẫn chừa ~62% chồng lấn để ScrollStitcher ghép chắc, không bị "tooFast".
        autoStepPoints = min(max(rect.height * 0.38, 90), 420)

        // Thử gắn AX vào scroll area tại tâm vùng (có thể chưa ra ngay — sẽ thử
        // lại ở mỗi nhịp nếu chưa gắn được).
        tracker.attach(atGlobalTopLeft: scrollCenterCG)

        // Tạo UI TRƯỚC khi dựng filter (thanh điều khiển + pill + preview).
        showBar(below: rect, on: screen)
        showSlowMessage(over: rect, on: screen)
        showPreviewPanel(on: screen)

        // Loại MỌI cửa sổ của app mình khỏi ảnh chụp → thanh điều khiển & pill
        // "Please slow down" (nằm GIỮA vùng) không bao giờ lọt vào ảnh ghép.
        let content2 = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: false)
        let mine = content2.windows.filter {
            $0.owningApplication?.bundleIdentifier == Bundle.main.bundleIdentifier
        }
        self.filter = SCContentFilter(display: display, excludingWindows: mine)

        startTimer()
    }

    /// Panel nổi dùng chung cho mọi mảnh chrome của phiên chụp cuộn.
    private func floatingPanel<Content: View>(frame: NSRect, key: Bool = false,
                                              content: Content) -> NSPanel {
        let style: NSWindow.StyleMask = [.borderless, .nonactivatingPanel]
        let p = key ? KeyPanel(contentRect: frame, styleMask: style, backing: .buffered, defer: false)
                    : NSPanel(contentRect: frame, styleMask: style, backing: .buffered, defer: false)
        p.level = .floating
        p.backgroundColor = .clear
        p.isOpaque = false
        p.hasShadow = true
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        let host = NSHostingView(rootView: content)
        host.frame = NSRect(origin: .zero, size: frame.size)
        host.autoresizingMask = [.width, .height]
        p.contentView = host
        return p
    }

    // Pill "Please slow down" ở GIỮA vùng chụp (ẩn sẵn alpha 0, hiện khi cuộn nhanh).
    private func showSlowMessage(over rect: CGRect, on screen: NSScreen) {
        let size = NSHostingView(rootView: ScrollSlowPill()).fittingSize
        // Tâm vùng theo toạ độ AppKit toàn cục (gốc dưới-trái).
        let cx = screen.frame.minX + rect.midX
        let cy = screen.frame.minY + (screen.frame.height - rect.midY)
        let frame = NSRect(x: cx - size.width / 2, y: cy - size.height / 2,
                           width: size.width, height: size.height)

        let p = floatingPanel(frame: frame, content: ScrollSlowPill())
        p.alphaValue = 0                 // ẩn nhưng VẪN tồn tại để được loại khỏi ảnh
        p.orderFrontRegardless()
        self.slowPanel = p
    }

    // Panel live preview ở mép phải màn. Bắt đầu NHỎ, mọc lên trên theo lượng
    // đã chụp (đáy cố định), tối đa previewMaxH — giống CleanShot.
    private func showPreviewPanel(on screen: NSScreen) {
        previewMaxH = min(screen.visibleFrame.height * 0.72, 560)
        let margin: CGFloat = 16
        let x = screen.visibleFrame.maxX - previewWc - margin
        previewBottomY = screen.visibleFrame.minY + margin
        let startH: CGFloat = 54                      // cao ban đầu nhỏ
        let frame = NSRect(x: x, y: previewBottomY, width: previewWc, height: startH)

        let p = floatingPanel(frame: frame, content: ScrollPreviewPanel(model: model))
        p.alphaValue = 0                        // hiện khi có frame đầu
        p.orderFrontRegardless()
        self.previewPanel = p
    }

    private func updatePreview(_ cg: CGImage) {
        model.preview = cg
        guard let p = previewPanel else { return }
        // Cao panel = chiều cao ảnh khi vừa khung bề ngang, chặn trên previewMaxH.
        let contentW = previewWc - 12
        let dispH = contentW * CGFloat(cg.height) / CGFloat(max(cg.width, 1))
        let panelH = min(max(dispH + 12, 40), previewMaxH)
        var f = p.frame
        f.size.height = panelH
        f.origin.y = previewBottomY              // giữ đáy, mọc lên trên
        p.setFrame(f, display: true)
        if p.alphaValue == 0 {
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.2
                p.animator().alphaValue = 1
            }
        }
    }

    // ── Autoscroll ───────────────────────────────────────────────────────────
    private func setAuto(_ on: Bool) {
        guard autoScroll != on else { return }
        autoScroll = on
        idleCount = 0
        // Đưa con trỏ vào giữa vùng 1 lần để cú cuộn trúng đúng cửa sổ đích.
        if autoScroll { CGWarpMouseCursorPosition(scrollCenterCG) }
    }

    // Bắn 1 cú cuộn xuống nhẹ vào tâm vùng (gọi sau mỗi frame khi autoscroll bật).
    private func postScroll() {
        if let ev = CGEvent(scrollWheelEvent2Source: nil, units: .pixel,
                            wheelCount: 1, wheel1: Int32(-autoStepPoints), wheel2: 0, wheel3: 0) {
            ev.location = scrollCenterCG
            ev.post(tap: .cghidEventTap)
        }
    }

    // Hiện/ẩn pill "slow down" (mượt bằng alpha).
    private func setSlow(_ show: Bool) {
        guard let p = slowPanel else { return }
        let target: CGFloat = show ? 1 : 0
        guard p.alphaValue != target else { return }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.15
            p.animator().alphaValue = target
        }
    }

    // ── Thanh điều khiển ────────────────────────────────────────────────────
    private func showBar(below rect: CGRect, on screen: NSScreen) {
        let w = ScrollCaptureBar.width, h = ScrollCaptureBar.height
        let gap: CGFloat = 12
        let regionBottomY = screen.frame.minY + (screen.frame.height - rect.maxY)
        let regionTopY    = screen.frame.minY + (screen.frame.height - rect.minY)
        var y = regionBottomY - gap - h
        if y < screen.visibleFrame.minY + 6 { y = regionTopY + gap }
        y = min(max(y, screen.visibleFrame.minY + 6), screen.visibleFrame.maxY - h - 6)
        let cx = screen.frame.minX + rect.midX - w / 2
        let x = min(max(cx, screen.visibleFrame.minX + 6), screen.visibleFrame.maxX - w - 6)

        let p = floatingPanel(frame: NSRect(x: x, y: y, width: w, height: h),
                              key: true, content: ScrollCaptureBar(model: model))
        p.orderFrontRegardless()
        // Kích hoạt app + biến thanh này thành key window → ⏎/Esc chạy được mà
        // không cần chuột (autoscroll vẫn cuộn bằng sự kiện tổng hợp, không phụ
        // thuộc focus). Cuộn tay vẫn ổn vì cuộn đi theo con trỏ, không theo app.
        NSApp.activate(ignoringOtherApps: true)
        p.makeKeyAndOrderFront(nil)
        self.panel = p
    }

    // Timer ~5fps: mỗi nhịp chụp lại vùng rồi đưa cho stitcher.
    private func startTimer() {
        let t = Timer(timeInterval: 0.12, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    private func tick() {
        guard !capturing, let filter, let config, let stitcher else { return }
        capturing = true
        Task { [weak self] in
            let cg = try? await SCScreenshotManager.captureImage(
                contentFilter: filter, configuration: config)
            guard let self else { return }
            guard let cg else { self.capturing = false; return }

            // ── Đọc offset AX (đang ở main actor) ──────────────────────────
            if !self.tracker.isAttached {        // chưa gắn được → thử lại (AX "lười")
                self.tracker.attach(atGlobalTopLeft: self.scrollCenterCG)
            }
            let axVal = self.tracker.value()
            self.axOK = axVal != nil
            let prevAX = self.lastAXValue
            // Nếu đã hiệu chỉnh rangePx → tính độ trượt CHÍNH XÁC từ AX.
            var known: Int? = nil
            if let v = axVal, let last = prevAX, self.rangePx > 0 {
                let shift = (v - last) * self.rangePx
                if shift >= 0 { known = Int(shift.rounded()) }
            }
            let atEnd = (axVal ?? 0) >= 0.999     // AX biết chắc đã chạm đáy trang

            let rowsBefore = stitcher.capturedRows
            self.stitchQueue.async {
                let outcome = stitcher.add(frame: cg, knownShift: known)
                let rows = stitcher.capturedRows
                let dApplied = rows - rowsBefore
                let preview = outcome == .advanced ? stitcher.previewImage() : nil
                DispatchQueue.main.async {
                    // Đã Done/Cancel → bỏ qua nhịp đến trễ (đừng cuộn/đụng UI thêm).
                    guard !self.finished else { self.capturing = false; return }
                    // Hiệu chỉnh rangePx 1 lần: lấy d (px) vừa ghép chia cho Δvalue
                    // → suy ra tổng px của cả dải 0..1. Từ đó AX cho offset chính xác.
                    if self.rangePx == 0, dApplied > 2,
                       let v = axVal, let last = prevAX, v - last > 0.0005 {
                        self.rangePx = CGFloat(dApplied) / (v - last)
                    }
                    self.lastAXValue = axVal

                    self.model.status = "Captured \(rows) px"
                    self.setSlow(outcome == .tooFast)
                    if let preview { self.updatePreview(preview) }
                    if self.autoScroll {
                        if outcome == .idle { self.idleCount += 1 } else { self.idleCount = 0 }
                        // Dừng khi AX báo chạm đáy, hoặc (fallback) 6 nhịp không ra gì.
                        if atEnd || self.idleCount >= 6 {
                            self.autoScroll = false
                            self.model.autoScroll = false
                        } else if outcome != .tooFast {
                            self.postScroll()
                        }
                    }
                    self.capturing = false
                }
            }
        }
    }

    // Spinner "Building image…" ở giữa màn trong lúc ghép ảnh cuối (queue nền).
    private func showProcessing() {
        guard let screen = captureScreen else { return }
        let w: CGFloat = 160, h: CGFloat = 96
        let f = NSRect(x: screen.frame.midX - w / 2, y: screen.frame.midY - h / 2,
                       width: w, height: h)
        let p = floatingPanel(frame: f, content: ScrollProcessingPanel())
        p.orderFrontRegardless()
        self.processingPanel = p
    }

    // ── Kết thúc ────────────────────────────────────────────────────────────
    private func tapDone() {
        finished = true; autoScroll = false      // chặn cuộn thêm từ nhịp đang bay
        timer?.invalidate(); timer = nil
        panel?.orderOut(nil); panel = nil
        slowPanel?.orderOut(nil); slowPanel = nil
        previewPanel?.orderOut(nil); previewPanel = nil
        onStop?(); onStop = nil          // ẩn viền đỏ NGAY, không đợi dựng ảnh
        let cb = onFinish; onFinish = nil
        guard let stitcher else { reset(); cb?(nil); return }
        showProcessing()                 // spinner trong lúc ghép ảnh dài
        // Dựng ảnh cuối trên queue nền (ảnh có thể rất cao) rồi trả về main.
        stitchQueue.async {
            let image = stitcher.finalImage()
            DispatchQueue.main.async {
                self.processingPanel?.orderOut(nil); self.processingPanel = nil
                self.reset()
                cb?(image)
            }
        }
    }

    private func tapCancel() {
        finished = true; autoScroll = false
        timer?.invalidate(); timer = nil
        panel?.orderOut(nil); panel = nil
        slowPanel?.orderOut(nil); slowPanel = nil
        previewPanel?.orderOut(nil); previewPanel = nil
        onStop?(); onStop = nil
        let cb = onFinish; onFinish = nil
        reset()
        cb?(nil)
    }

    private func reset() {
        processingPanel?.orderOut(nil); processingPanel = nil
        tracker.reset()
        lastAXValue = nil; rangePx = 0; axOK = false
        stitcher = nil; filter = nil; config = nil; capturing = false
    }
}
