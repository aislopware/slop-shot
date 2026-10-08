import AppKit

// ═════════════════════════════════════════════════════════════════════════
// SNAP ENGINE — "bắt dính" khung chọn vào biên THẬT trên màn hình.
//
// Ý tưởng học từ macshot (github.com/sw33tLie/macshot, file BoundarySnapIndex)
// nhưng làm lại cho ngon hơn:
//
//   macshot: chỉ dò biên KHI ĐANG kéo cạnh, bán kính 4pt, ngưỡng contrast cứng
//            (28), và luôn chọn biên GẦN NHẤT → hay dính vào 1 vệt mờ sát cạnh
//            biên thật; nền tối/ảnh nhiễu thì dò trượt. Không có "khoanh nguyên
//            item", chỉ snap từng cạnh rời rạc.
//
//   ở đây:   1) ngưỡng thấp, cố định; nhiễu lọc bằng cách chỉ nhận ĐƯỜNG thẳng
//               dài làm ứng viên cạnh.
//            2) chấm điểm = ĐỘ PHỦ của biên dọc theo cạnh đang kéo, rồi mới trừ
//               điểm theo khoảng cách → biên MẠNH thắng biên "gần mà mờ".
//            3) dò nguyên KHUNG item dưới con trỏ: tìm 4 cạnh ứng viên rồi thử
//               mọi tổ hợp, kiểm tra cả 4 cạnh có phủ đủ không (đúng nghĩa "đây
//               là 1 cái hộp", chịu được góc bo), rồi xếp các hộp lồng nhau
//               thành chuỗi nhỏ → lớn để cuộn chuột chọn cấp.
//            4) mọi phép đo độ phủ chạy trên prefix-sum → rê chuột vẫn mượt.
//
// Toạ độ: mọi API công khai nhận/trả POINT, gốc TRÊN-TRÁI của màn hình đang
// chọn (khớp với SelectionView vì view đó isFlipped = true).
// Bên trong quy hết về PIXEL của ảnh đóng băng.
//
// Quy ước "biên" (boundary): biên dọc b nằm GIỮA cột b-1 và cột b, nên hộp có
// biên trái L và biên phải R gồm đúng các cột L…R-1 (rộng R-L pixel).
// ═════════════════════════════════════════════════════════════════════════

/// Một gợi ý vùng chọn khi rê chuột (points, gốc trên-trái màn hình).
struct SnapTarget: Equatable {
    let rect: CGRect
    let isWindow: Bool      // true = nguyên 1 cửa sổ, false = 1 item bên trong
}

// ─────────────────────────────────────────────────────────────────────────
// Lớp 1: hình học cửa sổ — lấy từ CGWindowList nên CHÍNH XÁC TUYỆT ĐỐI
// (không phải đoán từ pixel). Dùng để: rê chuột lên cửa sổ nào là khoanh đúng
// cửa sổ đó, và để các cạnh cửa sổ trở thành "đường ray" hút khung chọn.
// ─────────────────────────────────────────────────────────────────────────
struct WindowSnapper {
    /// Rect các cửa sổ đang hiện, theo z-order (trước → sau), toạ độ point
    /// gốc trên-trái của màn hình đang chọn.
    private let rects: [CGRect]
    let edgeXs: [CGFloat]
    let edgeYs: [CGFloat]

    static let empty = WindowSnapper(rects: [], edgeXs: [], edgeYs: [])

    /// Chụp lại danh sách cửa sổ đang hiện trên `screen` (bỏ qua cửa sổ của chính app).
    static func snapshot(on screen: NSScreen) -> WindowSnapper {
        let list = CGWindowListCopyWindowInfo(
            [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []

        // CGWindow dùng hệ toạ độ "global display", gốc TRÊN-TRÁI của màn hình
        // chính, y đi xuống. Đổi về gốc trên-trái của màn hình đang chọn.
        let mainTop = NSScreen.screens.first?.frame.maxY ?? screen.frame.maxY
        let originX = screen.frame.minX
        let originY = mainTop - screen.frame.maxY
        let localBounds = CGRect(origin: .zero, size: screen.frame.size)
        let myPID = Int(ProcessInfo.processInfo.processIdentifier)

        var rects: [CGRect] = []
        var xs = Set<CGFloat>(), ys = Set<CGFloat>()

        // App thường (có Dock icon). Cửa sổ của Dock, thanh menu, Notification
        // Center… cũng nằm ở layer > 0 nhưng không thuộc app thường.
        var regular: [Int: Bool] = [:]
        func isRegular(_ pid: Int) -> Bool {
            if let known = regular[pid] { return known }
            let r = NSRunningApplication(processIdentifier: pid_t(pid))?.activationPolicy == .regular
            regular[pid] = r
            return r
        }

        for info in list {
            // layer 0 = cửa sổ ứng dụng bình thường. Cửa sổ nổi của chính app
            // (trình xem ảnh của Telegram ở layer 101, palette…) cũng tính, vì nó
            // che cửa sổ chính bên dưới. Từ mức screen saver trở lên là của hệ thống.
            guard let layer = info[kCGWindowLayer as String] as? Int,
                  let pid = info[kCGWindowOwnerPID as String] as? Int, pid != myPID,
                  layer == 0 || (layer > 0 && layer < Int(CGWindowLevelForKey(.screenSaverWindow)) && isRegular(pid)),
                  (info[kCGWindowAlpha as String] as? Double ?? 1) > 0.05,
                  let b = info[kCGWindowBounds as String] as? [String: CGFloat]
            else { continue }

            let r = CGRect(x: (b["X"] ?? 0) - originX, y: (b["Y"] ?? 0) - originY,
                           width: b["Width"] ?? 0, height: b["Height"] ?? 0)
                .intersection(localBounds)
            guard r.width >= 24, r.height >= 24 else { continue }

            rects.append(r)
            xs.insert(r.minX); xs.insert(r.maxX)
            ys.insert(r.minY); ys.insert(r.maxY)
        }
        return WindowSnapper(rects: rects, edgeXs: xs.sorted(), edgeYs: ys.sorted())
    }

    /// Cửa sổ TRÊN CÙNG chứa điểm p (danh sách CGWindowList đã sắp theo z-order).
    func window(at p: CGPoint) -> CGRect? {
        rects.first { $0.contains(p) }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Lớp 2: phân tích PIXEL của ảnh đóng băng để tìm biên & khung item.
// Build 1 lần cho mỗi lần mở overlay, chạy ngoài main thread (~20-40ms cho
// màn 5K), sau đó mọi truy vấn đều rẻ.
// ─────────────────────────────────────────────────────────────────────────
final class SnapEngine: @unchecked Sendable {
    private let w: Int
    private let h: Int
    private let scale: CGFloat                    // pixel trên mỗi point
    private let vEdge: UnsafeMutablePointer<UInt8>  // [y*w + x] biên DỌC giữa cột x-1 và x
    private let hEdge: UnsafeMutablePointer<UInt8>  // [y*w + x] biên NGANG giữa hàng y-1 và y
    // Ngưỡng "đây là biên thật": CỐ ĐỊNH, đủ thấp để bắt viền 1px nhạt
    // (#e5e5e5 trên nền trắng, #2f3336 trên nền đen). Không nâng theo độ "nhiễu"
    // của ảnh: màn nhiều chữ/ảnh sẽ đẩy ngưỡng lên và làm mất sạch viền UI thật,
    // trong khi nhiễu đã bị lọc ở bước gom đường (chỉ giữ ĐƯỜNG thẳng dài).
    private static let edgeThreshold: UInt8 = 16
    private var thr: UInt8 { Self.edgeThreshold }

    // Danh sách ĐƯỜNG THẲNG dài dựng sẵn lúc build.
    // Segment của cột x nằm ở [vStart[x], vStart[x+1]) — mỗi cái là 1 đoạn
    // [vA, vB] theo hàng. Tương tự hStart/hA/hB cho các đường ngang.
    // Đây là điểm khác cốt lõi so với macshot: ứng viên cạnh phải là 1 ĐƯỜNG
    // THẬT chạy dài, chứ không phải "pixel nào tương phản cũng tính" — nên
    // ảnh/game/artwork nhiều chi tiết không còn làm nhiễu kết quả.
    private let vStart: [Int32], vA: [Int32], vB: [Int32]
    private let hStart: [Int32], hA: [Int32], hB: [Int32]

    private init(w: Int, h: Int, scale: CGFloat,
                 vEdge: UnsafeMutablePointer<UInt8>, hEdge: UnsafeMutablePointer<UInt8>,
                 vStart: [Int32], vA: [Int32], vB: [Int32],
                 hStart: [Int32], hA: [Int32], hB: [Int32]) {
        self.w = w; self.h = h; self.scale = scale
        self.vEdge = vEdge; self.hEdge = hEdge
        self.vStart = vStart; self.vA = vA; self.vB = vB
        self.hStart = hStart; self.hA = hA; self.hB = hB
    }

    deinit {
        vEdge.deallocate()
        hEdge.deallocate()
    }

    // ── Build ────────────────────────────────────────────────────────────
    /// `scale` = pixel ảnh / point màn hình (thường là backingScaleFactor).
    /// An toàn khi gọi ngoài main thread.
    static func build(from cg: CGImage, scale: CGFloat) -> SnapEngine? {
        let w = cg.width, h = cg.height
        guard w >= 8, h >= 8, scale > 0, w * h <= 40_000_000 else { return nil }

        // Vẽ lại vào buffer RGBA8 để đọc kênh màu chắc chắn đúng thứ tự.
        // Bitmap context của CoreGraphics xếp bộ nhớ TỪ TRÊN XUỐNG: hàng 0 của
        // buffer = hàng trên cùng của ảnh — khớp luôn với view isFlipped, nên
        // KHÔNG được lật thêm lần nữa (lật là toạ độ soi gương hết).
        let bpr = w * 4
        let pixels = UnsafeMutablePointer<UInt8>.allocate(capacity: h * bpr)
        defer { pixels.deallocate() }
        pixels.initialize(repeating: 0, count: h * bpr)
        guard let ctx = CGContext(data: pixels, width: w, height: h,
                                  bitsPerComponent: 8, bytesPerRow: bpr,
                                  space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))

        let vEdge = UnsafeMutablePointer<UInt8>.allocate(capacity: w * h)
        let hEdge = UnsafeMutablePointer<UInt8>.allocate(capacity: w * h)
        vEdge.initialize(repeating: 0, count: w * h)
        hEdge.initialize(repeating: 0, count: w * h)

        // Độ mạnh biên = chênh lệch LỚN NHẤT trong 3 kênh R/G/B. Nhạy hơn kiểu
        // "khoảng cách euclid chia 3" của macshot với mấy đường viền 1px nhạt.
        for y in 0..<h {
            let row = y * bpr
            let base = y * w
            for x in 1..<w {
                vEdge[base + x] = chanDiff(pixels, row + (x - 1) * 4, row + x * 4)
            }
            guard y >= 1 else { continue }
            let prev = (y - 1) * bpr
            for x in 0..<w {
                hEdge[base + x] = chanDiff(pixels, prev + x * 4, row + x * 4)
            }
        }
        let t = edgeThreshold

        // Gom các đoạn biên liên tục thành "đường". Cho hở tối đa 2 pixel (khử
        // răng cưa hay làm đứt vệt), và chỉ giữ đoạn dài ≥ ~20pt.
        let minLine = max(12, Int((20 * scale).rounded()))
        var vStart = [Int32](repeating: 0, count: w + 1)
        var vA: [Int32] = [], vB: [Int32] = []
        vA.reserveCapacity(4096); vB.reserveCapacity(4096)
        for x in 0..<w {
            vStart[x] = Int32(vA.count)
            guard x >= 1 else { continue }
            var runStart = -1, lastOn = -1
            for y in 0..<h {
                if vEdge[y * w + x] >= t {
                    if runStart < 0 { runStart = y }
                    lastOn = y
                } else if runStart >= 0, y - lastOn > 2 {
                    if lastOn - runStart + 1 >= minLine { vA.append(Int32(runStart)); vB.append(Int32(lastOn)) }
                    runStart = -1
                }
            }
            if runStart >= 0, lastOn - runStart + 1 >= minLine {
                vA.append(Int32(runStart)); vB.append(Int32(lastOn))
            }
        }
        vStart[w] = Int32(vA.count)

        var hStart = [Int32](repeating: 0, count: h + 1)
        var hA: [Int32] = [], hB: [Int32] = []
        hA.reserveCapacity(4096); hB.reserveCapacity(4096)
        for y in 0..<h {
            hStart[y] = Int32(hA.count)
            guard y >= 1 else { continue }
            let base = y * w
            var runStart = -1, lastOn = -1
            for x in 0..<w {
                if hEdge[base + x] >= t {
                    if runStart < 0 { runStart = x }
                    lastOn = x
                } else if runStart >= 0, x - lastOn > 2 {
                    if lastOn - runStart + 1 >= minLine { hA.append(Int32(runStart)); hB.append(Int32(lastOn)) }
                    runStart = -1
                }
            }
            if runStart >= 0, lastOn - runStart + 1 >= minLine {
                hA.append(Int32(runStart)); hB.append(Int32(lastOn))
            }
        }
        hStart[h] = Int32(hA.count)

        return SnapEngine(w: w, h: h, scale: scale, vEdge: vEdge, hEdge: hEdge,
                          vStart: vStart, vA: vA, vB: vB, hStart: hStart, hA: hA, hB: hB)
    }

    @inline(__always)
    private static func chanDiff(_ p: UnsafeMutablePointer<UInt8>, _ a: Int, _ b: Int) -> UInt8 {
        let dr = abs(Int(p[a])     - Int(p[b]))
        let dg = abs(Int(p[a + 1]) - Int(p[b + 1]))
        let db = abs(Int(p[a + 2]) - Int(p[b + 2]))
        return UInt8(min(255, max(dr, max(dg, db))))
    }

    // ── Đổi toạ độ ───────────────────────────────────────────────────────
    @inline(__always) private func px(_ v: CGFloat) -> Int { Int((v * scale).rounded()) }
    @inline(__always) private func pt(_ v: Int) -> CGFloat { CGFloat(v) / scale }
    @inline(__always) private func clamp(_ v: Int, _ lo: Int, _ hi: Int) -> Int { min(hi, max(lo, v)) }

    // ── Đo đạc cơ bản ────────────────────────────────────────────────────

    /// Các cột có ĐƯỜNG DỌC đi ngang qua hàng `row`, quét từ `from` về phía
    /// `stop`, gần nhất trước. Trả tối đa `limit` cột.
    /// `span`: thêm cả các đường phủ ≥ nửa đoạn này dù không đi qua `row` —
    /// mép ảnh hoà vào nền ở đúng chỗ con trỏ thì vẫn bắt được nhờ phần còn lại.
    private func vLines(from: Int, stop: Int, row: Int, span: ClosedRange<Int>? = nil, limit: Int) -> [Int] {
        Self.lines(vStart, vA, vB, count: w, from: from, stop: stop, across: row, span: span, limit: limit)
    }

    /// Các hàng có ĐƯỜNG NGANG đi qua cột `col`.
    private func hLines(from: Int, stop: Int, col: Int, span: ClosedRange<Int>? = nil, limit: Int) -> [Int] {
        Self.lines(hStart, hA, hB, count: h, from: from, stop: stop, across: col, span: span, limit: limit)
    }

    private static func lines(_ start: [Int32], _ a: [Int32], _ b: [Int32], count: Int,
                              from: Int, stop: Int, across p: Int, span: ClosedRange<Int>?,
                              limit: Int) -> [Int] {
        /// Độ dài đường ở cột/hàng `i` nếu nó đi qua `p` hoặc phủ ≥ nửa `span`
        /// (0 = không tính).
        func reach(_ i: Int) -> Int {
            guard i >= 1, i < count else { return 0 }
            var through = 0, inSpan = 0, total = 0
            for k in Int(start[i])..<Int(start[i + 1]) {
                let lo = Int(a[k]), hi = Int(b[k])
                total += hi - lo + 1
                if lo - 2 <= p && p <= hi + 2 { through = hi - lo + 1 }
                if let span { inSpan += max(0, min(hi, span.upperBound) - max(lo, span.lowerBound) + 1) }
            }
            if through > 0 { return total }
            if let span, inSpan * 2 >= span.count { return total }
            return 0
        }
        var out: [Int] = []
        let step = stop > from ? 1 : -1
        var i = from + step * 2
        while out.count < limit, i >= 1, i < count, step > 0 ? i <= stop : i >= stop {
            var bestLen = reach(i)
            guard bestLen > 0 else { i += step; continue }
            // Viền dày / viền kép chiếm vài px liền nhau: chỉ lấy 1 đường cho cả
            // cụm, và lấy đường DÀI nhất — đường đầu tiên gặp có thể chỉ là 1
            // mẩu ngắn sát bên mép thật.
            var best = i
            for j in [i + step, i + 2 * step] {
                let len = reach(j)
                if len > bestLen { best = j; bestLen = len }
            }
            out.append(best)
            i += step * 3
        }
        return out
    }

    /// Tỉ lệ pixel "đủ mạnh" trên biên dọc x, quét từ hàng y0 tới y1 (0…1).
    private func vCoverage(x: Int, y0: Int, y1: Int) -> Float {
        guard x >= 1, x < w, y1 >= y0 else { return 0 }
        var hit = 0
        var i = y0 * w + x
        for _ in y0...y1 {
            if vEdge[i] >= thr { hit += 1 }
            i += w
        }
        return Float(hit) / Float(y1 - y0 + 1)
    }

    private func hCoverage(y: Int, x0: Int, x1: Int) -> Float {
        guard y >= 1, y < h, x1 >= x0 else { return 0 }
        var hit = 0
        let base = y * w
        for x in x0...x1 where hEdge[base + x] >= thr { hit += 1 }
        return Float(hit) / Float(x1 - x0 + 1)
    }

    // ═════════════════════════════════════════════════════════════════════
    // A. SNAP TỪNG CẠNH khi đang kéo khung
    // ═════════════════════════════════════════════════════════════════════

    /// Tìm biên dọc "đáng dính" gần `x` nhất, chấm điểm theo độ phủ trên đoạn
    /// [y0, y1] (chính là chiều cao khung đang kéo). Trả nil nếu không có gì đáng.
    func snapX(near x: CGFloat, from y0: CGFloat, to y1: CGFloat, radius: CGFloat) -> CGFloat? {
        let center = clamp(px(x), 0, w)
        let r = max(1, px(radius))
        var a = clamp(px(min(y0, y1)), 0, h - 1)
        var b = clamp(px(max(y0, y1)) - 1, 0, h - 1)
        if b < a { swap(&a, &b) }
        if b - a < 4 { b = min(h - 1, a + 4) }          // khung còn quá bé → nới đoạn đo

        var best: Int?, bestScore: Float = 0
        for cand in max(1, center - r)...min(w - 1, center + r) {
            let cov = vCoverage(x: cand, y0: a, y1: b)
            guard cov >= 0.55 else { continue }
            // Phủ nhiều thắng; ở gần chỉ là điểm cộng nhỏ → biên MẠNH luôn thắng
            // biên "gần mà mờ" (đúng chỗ macshot hay dính sai).
            let score = cov - 0.25 * Float(abs(cand - center)) / Float(r)
            if score > bestScore { bestScore = score; best = cand }
        }
        return best.map(pt)
    }

    /// Tương tự cho biên ngang.
    func snapY(near y: CGFloat, from x0: CGFloat, to x1: CGFloat, radius: CGFloat) -> CGFloat? {
        let center = clamp(px(y), 0, h)
        let r = max(1, px(radius))
        var a = clamp(px(min(x0, x1)), 0, w - 1)
        var b = clamp(px(max(x0, x1)) - 1, 0, w - 1)
        if b < a { swap(&a, &b) }
        if b - a < 4 { b = min(w - 1, a + 4) }

        var best: Int?, bestScore: Float = 0
        for cand in max(1, center - r)...min(h - 1, center + r) {
            let cov = hCoverage(y: cand, x0: a, x1: b)
            guard cov >= 0.55 else { continue }
            let score = cov - 0.25 * Float(abs(cand - center)) / Float(r)
            if score > bestScore { bestScore = score; best = cand }
        }
        return best.map(pt)
    }

    // ═════════════════════════════════════════════════════════════════════
    // B. DÒ NGUYÊN KHUNG ITEM dưới con trỏ (thứ macshot chưa có)
    // ═════════════════════════════════════════════════════════════════════

    /// Các khung lồng nhau bao quanh `point`, từ nhỏ tới lớn (nút → thẻ → cột…),
    /// chỉ tìm trong `limit` (thường là rect của cửa sổ đang rê chuột lên).
    /// Không trả về khung trùng nguyên `limit` — cấp đó người gọi tự thêm.
    func elements(at point: CGPoint, within limit: CGRect) -> [CGRect] {
        let x0 = clamp(px(limit.minX), 0, w - 1), x1 = clamp(px(limit.maxX), 1, w)
        let y0 = clamp(px(limit.minY), 0, h - 1), y1 = clamp(px(limit.maxY), 1, h)
        guard x1 - x0 > 16, y1 - y0 > 16 else { return [] }

        let cx = clamp(px(point.x), x0, x1 - 1)
        let cy = clamp(px(point.y), y0, y1 - 1)

        let minSize = max(12, px(16))
        let limit = 20

        // Ứng viên 4 cạnh = các ĐƯỜNG gần nhất theo 4 hướng, cộng mép vùng tìm
        // kiếm. Mép vùng tìm là ranh giới đã biết (mép cửa sổ) nên luôn tính là
        // cạnh thật — kể cả khi nó trùng mép màn hình, nơi không có pixel nào để đo.
        var lefts   = vLines(from: cx, stop: x0, row: cy, limit: limit)
        var rights  = vLines(from: cx, stop: x1 - 1, row: cy, limit: limit)
        var tops    = hLines(from: cy, stop: y0, col: cx, limit: limit)
        var bottoms = hLines(from: cy, stop: y1 - 1, col: cx, limit: limit)
        // Dò lại, nhận thêm đường phủ ≥ nửa khoảng giữa 2 cạnh gần nhất phía
        // vuông góc (xem `span` ở vLines).
        let xSpan = (lefts.first ?? x0)...((rights.first ?? x1) - 1)
        let ySpan = (tops.first ?? y0)...((bottoms.first ?? y1) - 1)
        lefts   = vLines(from: cx, stop: x0, row: cy, span: ySpan, limit: limit)
        rights  = vLines(from: cx, stop: x1 - 1, row: cy, span: ySpan, limit: limit)
        tops    = hLines(from: cy, stop: y0, col: cx, span: xSpan, limit: limit)
        bottoms = hLines(from: cy, stop: y1 - 1, col: cx, span: xSpan, limit: limit)
        if !lefts.contains(x0) { lefts.append(x0) }
        if !rights.contains(x1) { rights.append(x1) }
        if !tops.contains(y0) { tops.append(y0) }
        if !bottoms.contains(y1) { bottoms.append(y1) }

        // Prefix-sum cho từng cạnh ứng viên → hỏi độ phủ đoạn bất kỳ trong O(1).
        let colPre = (lefts + rights).map { columnPrefix($0, y0: y0, y1: y1 - 1) }
        let rowPre = (tops + bottoms).map { rowPrefix($0, x0: x0, x1: x1 - 1) }

        // Góc bo: ảnh trong bong bóng chat, thẻ, panel… gần như đều bo góc, nên
        // cạnh thẳng chỉ bắt đầu cách góc 1 đoạn đúng bằng bán kính. Đo khoảng
        // hụt đó ở CẢ HAI cạnh gặp nhau tại góc: góc bo thật thì hai khoảng hụt
        // bằng nhau (cùng bán kính), góc vuông thì cùng ≈ 0. Hộp "ăn gian" lấy
        // nhầm 1 đường bên ngoài item thì lệch ở CẢ 2 góc của cạnh đó → loại.
        // Cho phép lệch 1 góc: nhãn giờ, nút nổi… hay nằm sát 1 góc của ảnh.
        let cap = px(24)
        let large = px(160)
        let vCands = lefts + rights, hCands = tops + bottoms
        let gapV = vCands.map { x in
            hCands.enumerated().map { hi, y in
                hi < tops.count ? vGap(x: x, from: y, step: 1, cap: cap) : vGap(x: x, from: y - 1, step: -1, cap: cap)
            }
        }
        let gapH = hCands.map { y in
            vCands.enumerated().map { vi, x in
                vi < lefts.count ? hGap(y: y, from: x, step: 1, cap: cap) : hGap(y: y, from: x - 1, step: -1, cap: cap)
            }
        }
        /// nil = không thành góc (1 cạnh không chạm tới), false = có nhưng 2 cạnh lệch bán kính.
        @inline(__always) func corner(_ gv: Int, _ gh: Int, vFixed: Bool, hFixed: Bool) -> Bool? {
            // Mép cửa sổ thẳng tắp: đường bên trong mà chạm mép thì chạm sát.
            if vFixed { return gh <= px(2) ? true : nil }
            if hFixed { return gv <= px(2) ? true : nil }
            guard gv <= cap, gh <= cap else { return nil }
            return abs(gv - gh) <= max(px(3), max(gv, gh) / 3)
        }
        // Độ phủ trên phần THẲNG của cạnh (bỏ 2 đầu cong); phần thẳng quá ngắn
        // so với cạnh thì không coi là cạnh.
        @inline(__always) func cov(_ pre: [Int32], _ lo: Int, _ hi: Int, _ base: Int, _ g0: Int, _ g1: Int) -> Float {
            let a = lo + g0, b = hi - g1
            guard (b - a + 1) * 4 >= hi - lo + 1 else { return 0 }
            return Float(pre[b - base + 1] - pre[a - base]) / Float(b - a + 1)
        }

        var boxes: [(l: Int, t: Int, r: Int, b: Int, score: Float)] = []
        for (ti, T) in tops.enumerated() {
            let bj0 = tops.count
            for (bi, B) in bottoms.enumerated() where B - T >= minSize {
                let bj = bj0 + bi
                for (li, L) in lefts.enumerated() {
                    for (ri, R) in rights.enumerated() where R - L >= minSize {
                        let rj = lefts.count + ri
                        let fl = L == x0, fr = R == x1, ft = T == y0, fb = B == y1
                        if fl, fr, ft, fb { continue }
                        let corners = [corner(gapV[li][ti], gapH[ti][li], vFixed: fl, hFixed: ft),
                                       corner(gapV[rj][ti], gapH[ti][rj], vFixed: fr, hFixed: ft),
                                       corner(gapV[li][bj], gapH[bj][li], vFixed: fl, hFixed: fb),
                                       corner(gapV[rj][bj], gapH[bj][rj], vFixed: fr, hFixed: fb)]
                        let missing = corners.filter { $0 == nil }.count
                        let skewed = corners.filter { $0 == false }.count
                        // Ảnh to trên nền tối (trình xem ảnh của Telegram, Preview…):
                        // chỗ ảnh cũng tối thì viền ảnh hoà vào nền, 1 góc và 1-2
                        // cạnh gần đó mất hẳn. Cho phép thiếu 1 góc nếu hộp đủ to
                        // và 3 góc còn lại khớp nhau, đổi lại phải đạt ngưỡng phủ
                        // riêng bên dưới. Hộp nhỏ thì vẫn đòi đủ 4 góc.
                        let partial = missing == 1 && skewed == 0 && B - T >= large && R - L >= large
                        guard missing == 0 && skewed <= 1 || partial else { continue }
                        let cL = fl ? 1 : cov(colPre[li], T, B - 1, y0, gapV[li][ti], gapV[li][bj])
                        let cR = fr ? 1 : cov(colPre[rj], T, B - 1, y0, gapV[rj][ti], gapV[rj][bj])
                        let cT = ft ? 1 : cov(rowPre[ti], L, R - 1, x0, gapH[ti][li], gapH[ti][rj])
                        let cB = fb ? 1 : cov(rowPre[bj], L, R - 1, x0, gapH[bj][li], gapH[bj][rj])
                        // Cả 4 cạnh đều phải "có mặt" gần như trọn vẹn thì mới
                        // đúng là 1 cái hộp — bước này snap-từng-cạnh không có.
                        // Ngưỡng đo trên ảnh thật: khung UI thật (nút, thẻ, panel)
                        // đạt min ≥ 0.85 / mean ≥ 0.91; còn "hộp" ăn may trong
                        // ảnh/artwork chỉ tầm min 0.67 / mean 0.77 → tách bạch rõ.
                        let low = min(min(cL, cR), min(cT, cB)), mean = (cL + cR + cT + cB) / 4
                        guard partial ? low >= 0.50 && mean >= 0.78 : low >= 0.80 && mean >= 0.86,
                              !isStack(l: L, t: T, r: R, b: B) else { continue }
                        boxes.append((L, T, R, B, mean - 0.05 * Float(skewed) - (partial ? 0.15 : 0)))
                    }
                }
            }
        }

        // Hai hộp CẮT CHÉO nhau (chồng một phần, không cái nào chứa cái kia) thì
        // không thể cùng là item thật — thường 1 cái là hộp ăn may ghép từ mép
        // chữ thẳng hàng với viền thẻ. Giữ cái có viền rõ hơn.
        let slack = px(1)
        func contains(_ a: (l: Int, t: Int, r: Int, b: Int, score: Float),
                      _ b: (l: Int, t: Int, r: Int, b: Int, score: Float)) -> Bool {
            a.l <= b.l + slack && a.t <= b.t + slack && a.r >= b.r - slack && a.b >= b.b - slack
        }
        var kept: [(l: Int, t: Int, r: Int, b: Int, score: Float)] = []
        for box in boxes.sorted(by: { $0.score > $1.score })
        where kept.allSatisfy({ contains($0, box) || contains(box, $0) }) {
            kept.append(box)
        }
        boxes = kept

        // Xếp thành 1 chuỗi lồng nhau. Hộp sau chỉ nhỉnh hơn hộp trước bằng 1
        // dải mỏng ở 1-2 phía (thanh tiêu đề / chân của thẻ), hoặc 1-2px quanh
        // (viền kép) thì THAY luôn hộp trước — vẫn là cùng 1 item. Còn lề đều
        // quanh (ảnh nằm trong bong bóng chat) là 2 item lồng nhau, giữ cả hai.
        boxes.sort { ($0.r - $0.l) * ($0.b - $0.t) < ($1.r - $1.l) * ($1.b - $1.t) }
        var chain: [(l: Int, t: Int, r: Int, b: Int, score: Float)] = []
        var anchor = (w: 0, h: 0)       // cỡ hộp ĐẦU TIÊN của item đang xét, để thay liên tiếp không trôi dần ra item to hơn
        for box in boxes {
            let bw = box.r - box.l, bh = box.b - box.t
            guard let last = chain.last else { chain.append(box); anchor = (bw, bh); continue }
            guard contains(box, last) else { continue }
            let grow = [last.l - box.l, last.t - box.t, box.r - last.r, box.b - last.b]
            let shared = grow.filter { $0 <= slack }.count
            if bw - anchor.w <= max(px(6), anchor.w / 8), bh - anchor.h <= max(px(6), anchor.h / 8),
               shared >= 2 || grow.max()! <= px(2) {
                chain[chain.count - 1] = box
            } else {
                chain.append(box)
                anchor = (bw, bh)
            }
        }
        return chain.map { CGRect(x: pt($0.l), y: pt($0.t), width: pt($0.r - $0.l), height: pt($0.b - $0.t)) }
    }

    /// Hộp bị 1 đường chạy gần hết bề ngang (hoặc dọc) cắt ngang khúc giữa, và
    /// đường đó đậm ngang ngửa chính viền hộp → CHỒNG nhiều item cùng loại (2
    /// dòng của 1 danh sách, 2 cột…), không phải 1 item. Đường cắt nhạt hơn hẳn
    /// viền (lưới trong 1 tấm ảnh) hoặc sát mép (thanh tiêu đề) thì vẫn là 1 item.
    private func isStack(l: Int, t: Int, r: Int, b: Int) -> Bool {
        let hh = b - t, ww = r - l
        let rowRef = min(rowStrength(t, l, r - 1), rowStrength(b, l, r - 1))
        for y in (t + hh * 3 / 10)...(b - hh * 3 / 10)
        where spans(hStart, hA, hB, at: y, from: l, to: r - 1) && rowStrength(y, l, r - 1) * 5 >= rowRef * 3 {
            return true
        }
        let colRef = min(colStrength(l, t, b - 1), colStrength(r, t, b - 1))
        for x in (l + ww * 3 / 10)...(r - ww * 3 / 10)
        where spans(vStart, vA, vB, at: x, from: t, to: b - 1) && colStrength(x, t, b - 1) * 5 >= colRef * 3 {
            return true
        }
        return false
    }

    /// Độ đậm trung bình của biên ngang `y` trên [x0, x1]. Mép màn hình (không
    /// có pixel để đo) coi như đậm tối đa.
    private func rowStrength(_ y: Int, _ x0: Int, _ x1: Int) -> Int {
        guard y >= 1, y < h else { return 255 }
        var sum = 0
        for x in x0...x1 { sum += Int(hEdge[y * w + x]) }
        return sum / (x1 - x0 + 1)
    }

    private func colStrength(_ x: Int, _ y0: Int, _ y1: Int) -> Int {
        guard x >= 1, x < w else { return 255 }
        var sum = 0
        for y in y0...y1 { sum += Int(vEdge[y * w + x]) }
        return sum / (y1 - y0 + 1)
    }

    /// Các đoạn đường ở hàng/cột `i` phủ ≥ 90% khoảng [lo, hi].
    private func spans(_ start: [Int32], _ a: [Int32], _ b: [Int32], at i: Int, from lo: Int, to hi: Int) -> Bool {
        guard i >= 1, i + 1 < start.count, hi > lo else { return false }
        var covered = 0
        for k in Int(start[i])..<Int(start[i + 1]) {
            let s = max(lo, Int(a[k])), e = min(hi, Int(b[k]))
            if e >= s { covered += e - s + 1 }
        }
        return covered * 10 >= (hi - lo + 1) * 9
    }

    /// Từ góc đi dọc biên DỌC `x` theo `step`, bao nhiêu pixel mới gặp biên
    /// (góc vuông ≈ 0, góc bo ≈ bán kính). Không gặp trong `cap` → cap + 1.
    private func vGap(x: Int, from y: Int, step: Int, cap: Int) -> Int {
        guard x >= 1, x < w else { return cap + 1 }
        var y = y
        for k in 0...cap {
            guard y >= 0, y < h else { break }
            if vEdge[y * w + x] >= thr { return k }
            y += step
        }
        return cap + 1
    }

    private func hGap(y: Int, from x: Int, step: Int, cap: Int) -> Int {
        guard y >= 1, y < h else { return cap + 1 }
        var x = x
        for k in 0...cap {
            guard x >= 0, x < w else { break }
            if hEdge[y * w + x] >= thr { return k }
            x += step
        }
        return cap + 1
    }

    private func columnPrefix(_ x: Int, y0: Int, y1: Int) -> [Int32] {
        var pre = [Int32](repeating: 0, count: y1 - y0 + 2)
        guard x >= 1, x < w else { return pre }
        var acc: Int32 = 0
        for (k, y) in (y0...y1).enumerated() {
            if vEdge[y * w + x] >= thr { acc += 1 }
            pre[k + 1] = acc
        }
        return pre
    }

    private func rowPrefix(_ y: Int, x0: Int, x1: Int) -> [Int32] {
        var pre = [Int32](repeating: 0, count: x1 - x0 + 2)
        guard y >= 1, y < h else { return pre }
        var acc: Int32 = 0
        let base = y * w
        for (k, x) in (x0...x1).enumerated() {
            if hEdge[base + x] >= thr { acc += 1 }
            pre[k + 1] = acc
        }
        return pre
    }
}
