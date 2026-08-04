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
//   ở đây:   1) ngưỡng THÍCH ỨNG tính từ chính ảnh (percentile) → nền tối,
//               nền sáng, ảnh game nhiều màu đều ra ngưỡng hợp lý.
//            2) chấm điểm = ĐỘ PHỦ của biên dọc theo cạnh đang kéo, rồi mới trừ
//               điểm theo khoảng cách → biên MẠNH thắng biên "gần mà mờ".
//            3) dò nguyên KHUNG item dưới con trỏ: tìm 4 cạnh ứng viên rồi thử
//               mọi tổ hợp, kiểm tra cả 4 cạnh có phủ đủ không (đúng nghĩa "đây
//               là 1 cái hộp"), chọn hộp KHÍT NHẤT quanh con trỏ.
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

        for info in list {
            // layer 0 = cửa sổ ứng dụng bình thường. Menu/tooltip/overlay ở layer
            // khác — bỏ qua để khỏi khoanh nhầm mấy thứ trong suốt.
            guard (info[kCGWindowLayer as String] as? Int) == 0,
                  (info[kCGWindowOwnerPID as String] as? Int) != myPID,
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
    private let thr: UInt8                          // ngưỡng "đây là biên thật"

    // Danh sách ĐƯỜNG THẲNG dài dựng sẵn lúc build.
    // Segment của cột x nằm ở [vStart[x], vStart[x+1]) — mỗi cái là 1 đoạn
    // [vA, vB] theo hàng. Tương tự hStart/hA/hB cho các đường ngang.
    // Đây là điểm khác cốt lõi so với macshot: ứng viên cạnh phải là 1 ĐƯỜNG
    // THẬT chạy dài, chứ không phải "pixel nào tương phản cũng tính" — nên
    // ảnh/game/artwork nhiều chi tiết không còn làm nhiễu kết quả.
    private let vStart: [Int32], vA: [Int32], vB: [Int32]
    private let hStart: [Int32], hA: [Int32], hB: [Int32]

    private init(w: Int, h: Int, scale: CGFloat,
                 vEdge: UnsafeMutablePointer<UInt8>, hEdge: UnsafeMutablePointer<UInt8>, thr: UInt8,
                 vStart: [Int32], vA: [Int32], vB: [Int32],
                 hStart: [Int32], hA: [Int32], hB: [Int32]) {
        self.w = w; self.h = h; self.scale = scale
        self.vEdge = vEdge; self.hEdge = hEdge; self.thr = thr
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
        var hist = [Int](repeating: 0, count: 256)
        for y in 0..<h {
            let row = y * bpr
            let base = y * w
            for x in 1..<w {
                let d = chanDiff(pixels, row + (x - 1) * 4, row + x * 4)
                vEdge[base + x] = d
                hist[Int(d)] += 1
            }
            guard y >= 1 else { continue }
            let prev = (y - 1) * bpr
            for x in 0..<w {
                let d = chanDiff(pixels, prev + x * 4, row + x * 4)
                hEdge[base + x] = d
                hist[Int(d)] += 1
            }
        }

        // Ngưỡng thích ứng: lấy mốc ~1.5% mẫu mạnh nhất, kẹp trong [16, 64].
        // Màn hình toàn UI phẳng → ngưỡng tụt về 16 (bắt được viền nhạt);
        // màn đầy ảnh/nhiễu → ngưỡng dâng lên, khỏi dính vào chi tiết vụn.
        let total = hist.reduce(0, +)
        var cut = max(1, total * 15 / 1000)
        var thr = 64
        for v in stride(from: 255, through: 1, by: -1) {
            cut -= hist[v]
            if cut <= 0 { thr = v; break }
        }
        thr = min(64, max(16, thr))
        let t = UInt8(thr)

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

        return SnapEngine(w: w, h: h, scale: scale, vEdge: vEdge, hEdge: hEdge, thr: t,
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
    private func vLines(from: Int, stop: Int, row: Int, limit: Int) -> [Int] {
        var out: [Int] = []
        let step = stop > from ? 1 : -1
        var x = from + step * 2
        while out.count < limit, x >= 1, x < w, step > 0 ? x <= stop : x >= stop {
            var hit = false
            for i in Int(vStart[x])..<Int(vStart[x + 1])
            where Int(vA[i]) - 2 <= row && row <= Int(vB[i]) + 2 { hit = true; break }
            if hit { out.append(x); x += step * 3 } else { x += step }  // nhảy qua viền dày vài px
        }
        return out
    }

    /// Các hàng có ĐƯỜNG NGANG đi qua cột `col`.
    private func hLines(from: Int, stop: Int, col: Int, limit: Int) -> [Int] {
        var out: [Int] = []
        let step = stop > from ? 1 : -1
        var y = from + step * 2
        while out.count < limit, y >= 1, y < h, step > 0 ? y <= stop : y >= stop {
            var hit = false
            for i in Int(hStart[y])..<Int(hStart[y + 1])
            where Int(hA[i]) - 2 <= col && col <= Int(hB[i]) + 2 { hit = true; break }
            if hit { out.append(y); y += step * 3 } else { y += step }
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

    /// Khoanh item nhỏ nhất bao quanh `point`, chỉ tìm trong `limit`
    /// (thường là rect của cửa sổ đang rê chuột lên).
    func element(at point: CGPoint, within limit: CGRect) -> CGRect? {
        let x0 = clamp(px(limit.minX), 0, w - 1), x1 = clamp(px(limit.maxX), 1, w)
        let y0 = clamp(px(limit.minY), 0, h - 1), y1 = clamp(px(limit.maxY), 1, h)
        guard x1 - x0 > 16, y1 - y0 > 16 else { return nil }

        let cx = clamp(px(point.x), x0, x1 - 1)
        let cy = clamp(px(point.y), y0, y1 - 1)

        let minSize = max(12, px(16))
        let limit = 20

        // Ứng viên 4 cạnh = các ĐƯỜNG gần nhất theo 4 hướng; kèm mép vùng tìm
        // kiếm (thường là mép cửa sổ) làm phương án chót.
        var lefts   = vLines(from: cx, stop: x0, row: cy, limit: limit)
        var rights  = vLines(from: cx, stop: x1 - 1, row: cy, limit: limit)
        var tops    = hLines(from: cy, stop: y0, col: cx, limit: limit)
        var bottoms = hLines(from: cy, stop: y1 - 1, col: cx, limit: limit)
        if x0 >= 1, !lefts.contains(x0) { lefts.append(x0) }
        if x1 <= w - 1, !rights.contains(x1) { rights.append(x1) }
        if y0 >= 1, !tops.contains(y0) { tops.append(y0) }
        if y1 <= h - 1, !bottoms.contains(y1) { bottoms.append(y1) }
        guard !lefts.isEmpty, !rights.isEmpty, !tops.isEmpty, !bottoms.isEmpty else { return nil }

        // Prefix-sum cho từng cạnh ứng viên → hỏi độ phủ đoạn bất kỳ trong O(1).
        // Không có bước này thì 5×5×5×5 tổ hợp × vài nghìn pixel = giật khi rê chuột.
        let colPre = lefts.map { columnPrefix($0, y0: y0, y1: y1 - 1) }
            + rights.map { columnPrefix($0, y0: y0, y1: y1 - 1) }
        let rowPre = tops.map { rowPrefix($0, x0: x0, x1: x1 - 1) }
            + bottoms.map { rowPrefix($0, x0: x0, x1: x1 - 1) }

        @inline(__always) func cov(_ pre: [Int32], _ lo: Int, _ hi: Int, _ base: Int) -> Float {
            guard hi >= lo else { return 0 }
            return Float(pre[hi - base + 1] - pre[lo - base]) / Float(hi - lo + 1)
        }

        // Các danh sách ứng viên đều xếp theo khoảng cách TĂNG DẦN nên diện tích
        // cũng tăng dần theo chỉ số → gặp tổ hợp đã to hơn hộp tốt nhất là cắt
        // luôn cả nhánh. Nhờ vậy 20⁴ tổ hợp thực tế chỉ duyệt vài trăm cái.
        let minH = max(1, bottoms[0] - tops[0])
        var best: CGRect?
        var bestArea = Int.max
        for (li, L) in lefts.enumerated() {
            if (rights[0] - L) * minH >= bestArea { break }
            for (ri, R) in rights.enumerated() {
                if (R - L) * minH >= bestArea { break }
                guard R - L >= minSize else { continue }
                for (ti, T) in tops.enumerated() {
                    if (R - L) * (bottoms[0] - T) >= bestArea { break }
                    for (bi, B) in bottoms.enumerated() {
                        let area = (R - L) * (B - T)
                        if area >= bestArea { break }
                        guard B - T >= minSize else { continue }

                        let cL = cov(colPre[li], T, B - 1, y0)
                        let cR = cov(colPre[lefts.count + ri], T, B - 1, y0)
                        let cT = cov(rowPre[ti], L, R - 1, x0)
                        let cB = cov(rowPre[tops.count + bi], L, R - 1, x0)
                        // Cả 4 cạnh đều phải "có mặt" gần như trọn vẹn thì mới
                        // đúng là 1 cái hộp — bước này snap-từng-cạnh không có.
                        // Ngưỡng đo trên ảnh thật: khung UI thật (nút, thẻ, panel)
                        // đạt min ≥ 0.85 / mean ≥ 0.91; còn "hộp" ăn may trong
                        // ảnh/artwork chỉ tầm min 0.67 / mean 0.77 → tách bạch rõ.
                        guard min(min(cL, cR), min(cT, cB)) >= 0.80,
                              (cL + cR + cT + cB) / 4 >= 0.86 else { continue }

                        bestArea = area
                        best = CGRect(x: pt(L), y: pt(T), width: pt(R - L), height: pt(B - T))
                    }
                }
            }
        }
        return best
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
