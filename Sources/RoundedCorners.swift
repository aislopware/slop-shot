import CoreGraphics

// ═════════════════════════════════════════════════════════════════════════
// GÓC BO — khung chọn bám đúng 1 panel/menu/thẻ bo góc thì 4 góc của hình chữ
// nhật vẫn dính nền phía sau. Ở đây dò từng góc trên CHÍNH ảnh đã cắt và đục
// trong suốt phần nằm ngoài đường cong, giống macOS chụp cửa sổ.
//
// Chỉ nhận 1 góc khi chắc chắn đó là góc bo của item có mép trùng mép ảnh:
//   • pixel ở góc là "nền", và dọc 2 mép ảnh nền chỉ kéo dài 1 đoạn ngắn (bằng
//     bán kính) rồi gặp item, phần còn lại của mép là item liền mạch;
//   • vùng nền ở góc liền 1 khối, có dạng bậc thang thu hẹp dần, đối xứng qua
//     đường chéo — đúng hình 1 góc tròn (tròn hay squircle đều được).
// Kéo tay lệch mép item, góc vuông, nền loang (ảnh nền, chữ) → giữ nguyên.
//
// Viền khử răng cưa của đường cong không bị cắt cứng: mỗi pixel ở rìa được tách
// ngược lại thành "màu item + độ phủ" (p = a·F + (1−a)·B ⇒ a·F = p − (1−a)·B),
// nên ảnh dán lên nền nào cũng mịn.
// ═════════════════════════════════════════════════════════════════════════

enum RoundedCorners {
    /// Chênh lệch tối đa (theo kênh) vẫn coi là cùng màu nền.
    private static let tolerance = 8

    /// `top` vẽ đè lên `backdrop` (cùng kích thước), ra ảnh không trong suốt:
    /// góc đã đục lấy lại nền gốc, nét vẽ thêm trên `top` vẫn giữ.
    static func flatten(_ top: CGImage, over backdrop: CGImage) -> CGImage? {
        guard top.width == backdrop.width, top.height == backdrop.height else { return nil }
        let space = backdrop.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }
            ?? CGColorSpace(name: CGColorSpace.sRGB)!
        guard let ctx = CGContext(data: nil, width: top.width, height: top.height, bitsPerComponent: 8,
                                  bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        let rect = CGRect(x: 0, y: 0, width: top.width, height: top.height)
        ctx.draw(backdrop, in: rect)
        ctx.draw(top, in: rect)
        return ctx.makeImage()
    }

    /// Ảnh với các góc bo đã trong suốt; không góc nào bo thì trả lại đúng `cg`.
    /// `scale` = pixel trên mỗi point, để giới hạn bán kính theo point.
    static func clip(_ cg: CGImage, scale: CGFloat) -> CGImage {
        let w = cg.width, h = cg.height
        let maxRadius = Int((32 * max(scale, 1)).rounded())
        let cap = min(maxRadius, min(w, h) / 2 - 2)
        guard cap >= 3 else { return cg }

        let space = cg.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }
            ?? CGColorSpace(name: CGColorSpace.sRGB)!
        let bpr = w * 4
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: bpr, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let data = ctx.data else { return cg }
        ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
        // Bitmap context xếp hàng 0 = hàng TRÊN cùng của ảnh.
        let px = data.bindMemory(to: UInt8.self, capacity: h * bpr)

        var changed = false
        for (sx, sy) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
            let corner = Corner(px: px, w: w, h: h, sx: sx, sy: sy)
            if let shape = corner.detect(cap: cap) {
                corner.cut(shape)
                changed = true
            }
        }
        guard changed else { return cg }
        return ctx.makeImage() ?? cg
    }

    /// 1 góc của ảnh, đọc trong hệ toạ độ riêng: (0,0) là pixel ở góc, x đi
    /// dọc mép ngang vào trong, y đi dọc mép dọc vào trong.
    private struct Corner {
        let px: UnsafeMutablePointer<UInt8>
        let w: Int, h: Int
        let sx: Int, sy: Int

        @inline(__always) func offset(_ x: Int, _ y: Int) -> Int {
            let ix = sx > 0 ? x : w - 1 - x
            let iy = sy > 0 ? y : h - 1 - y
            return (iy * w + ix) * 4
        }

        @inline(__always) func rgb(_ x: Int, _ y: Int) -> (Int, Int, Int) {
            let o = offset(x, y)
            return (Int(px[o]), Int(px[o + 1]), Int(px[o + 2]))
        }

        @inline(__always) func diff(_ x: Int, _ y: Int, _ b: (Int, Int, Int)) -> Int {
            let p = rgb(x, y)
            return max(abs(p.0 - b.0), max(abs(p.1 - b.1), abs(p.2 - b.2)))
        }

        struct Shape {
            let bg: (Int, Int, Int)
            let side: Int           // cạnh ô vuông chứa góc
            let filled: [Bool]      // [y*side + x] pixel nền phía ngoài đường cong
            let center: Double      // tâm đường cong ≈ (r, r)
            let hEdge: (Int, Int, Int)  // màu mép item trên mép ngang, sau đoạn cong
            let vEdge: (Int, Int, Int)  // …và trên mép dọc
        }

        func detect(cap: Int) -> Shape? {
            let tol = RoundedCorners.tolerance
            let bg = rgb(0, 0)

            // Nền chạy dọc 2 mép bao xa thì gặp item.
            func run(_ along: (Int) -> (Int, Int), limit: Int) -> Int? {
                for k in 0...limit {
                    let (x, y) = along(k)
                    if diff(x, y, bg) > tol { return k }
                }
                return nil
            }
            guard let gx = run({ ($0, 0) }, limit: cap),
                  let gy = run({ (0, $0) }, limit: cap),
                  min(gx, gy) >= 2,
                  abs(gx - gy) <= max(2, max(gx, gy) / 4) else { return nil }

            // Phần còn lại của 2 mép (tới giữa ảnh) phải là item liền mạch: mép
            // ảnh trùng mép item, không phải kéo lệch ra ngoài nền.
            func solid(_ along: (Int) -> (Int, Int), from: Int, to: Int) -> Bool {
                guard to > from else { return false }
                var hit = 0
                for k in from..<to {
                    let (x, y) = along(k)
                    if diff(x, y, bg) > tol { hit += 1 }
                }
                return hit * 100 >= (to - from) * 95
            }
            guard solid({ ($0, 0) }, from: gx, to: w / 2),
                  solid({ (0, $0) }, from: gy, to: h / 2) else { return nil }

            // Loang nền từ góc trong ô vuông cạnh g+3; loang chạm biên ô là nền
            // còn chạy tiếp vào trong, không phải góc bo.
            let g = max(gx, gy)
            let side = g + 3
            var filled = [Bool](repeating: false, count: side * side)
            var stack = [(0, 0)]
            filled[0] = true
            var count = 0
            while let (x, y) = stack.popLast() {
                count += 1
                if x == side - 1 || y == side - 1 { return nil }
                for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
                where nx >= 0 && ny >= 0 && !filled[ny * side + nx] && diff(nx, ny, bg) <= tol {
                    filled[ny * side + nx] = true
                    stack.append((nx, ny))
                }
            }

            // Dạng bậc thang: mỗi hàng là 1 đoạn liền từ mép, ngắn dần.
            var lens: [Int] = []
            var total = 0
            for y in 0..<side {
                var n = 0
                while n < side, filled[y * side + n] { n += 1 }
                if let last = lens.last, n > last { return nil }
                lens.append(n)
                total += n
            }
            guard total == count, lens[0] == gx else { return nil }

            // Đối xứng qua đường chéo, và thực sự CONG: tới nửa bán kính thì
            // hàng nền đã hụt hẳn (góc tròn ≈ 13% r, squircle còn ít hơn; khung
            // lẹm vào trong cung thì nhiều hơn, vẫn là cung).
            var asym = 0
            for y in 0..<side {
                for x in 0..<side where filled[y * side + x] != filled[x * side + y] { asym += 1 }
            }
            guard asym <= max(2, count / 8),
                  lens[g / 2] * 10 <= gx * 6 else { return nil }
            // Góc vát chéo hay chi tiết tam giác trong ảnh: biên là đường THẲNG
            // nối 2 đầu đoạn nền. Cung tròn thì võng vào phía góc, trung bình
            // lệch khỏi dây cung ≥ nửa pixel kể cả khi chỉ thấy 1 khúc cung.
            var sag = 0.0
            for y in 0...gy {
                sag += Double(gx) * (1 - Double(y) / Double(gy)) - Double(lens[y])
            }
            guard sag / Double(gy + 1) >= 0.4 else { return nil }

            // Màu mép item: lấy trên mép thẳng, đủ xa chỗ bắt đầu cong để không
            // dính pixel khử răng cưa.
            let hEdge = rgb(min(gx + g / 2 + 2, w / 2), 0)
            let vEdge = rgb(0, min(gy + g / 2 + 2, h / 2))
            return Shape(bg: bg, side: side, filled: filled, center: Double(g), hEdge: hEdge, vEdge: vEdge)
        }

        /// Đục nền thành trong suốt, tách lại độ phủ ở rìa khử răng cưa.
        func cut(_ s: Shape) {
            let side = s.side, bg = s.bg
            let tol = Double(RoundedCorners.tolerance)

            /// Pháp tuyến hướng vào trong (về tâm đường cong) tại pixel.
            func normal(_ x: Int, _ y: Int) -> (Double, Double) {
                let vx = s.center - Double(x), vy = s.center - Double(y)
                let len = max((vx * vx + vy * vy).squareRoot(), 1e-6)
                return (vx / len, vy / len)
            }

            /// Độ phủ của item tại pixel, coi nó là hỗn hợp nền + màu item ngay
            /// bên trong. nil = pixel không phải hỗn hợp đó (viền tối hơn nền
            /// trong khi ruột sáng hơn nền…), tức là chính item.
            /// `solidBeyond`: pixel đã đi xa khỏi nền ít nhất bằng màu tham chiếu
            /// thì là item đặc, kể cả khi lẫn màu khác (viền + ruột cùng lọt
            /// vào 1 pixel khử răng cưa).
            func blend(_ p: (Int, Int, Int), _ f: (Int, Int, Int), solidBeyond: Bool = false) -> Double? {
                let fb = (Double(f.0 - bg.0), Double(f.1 - bg.1), Double(f.2 - bg.2))
                let len2 = fb.0 * fb.0 + fb.1 * fb.1 + fb.2 * fb.2
                guard len2 > tol * tol else { return nil }
                let pb = (Double(p.0 - bg.0), Double(p.1 - bg.1), Double(p.2 - bg.2))
                let raw = (pb.0 * fb.0 + pb.1 * fb.1 + pb.2 * fb.2) / len2
                if solidBeyond && raw >= 0.95 { return 1 }
                let a = max(0, min(1, raw))
                let residual = max(abs(pb.0 - a * fb.0), max(abs(pb.1 - a * fb.1), abs(pb.2 - a * fb.2)))
                return residual > tol ? nil : a
            }

            func coverage(_ x: Int, _ y: Int) -> Double? {
                let p = rgb(x, y)
                // Đường cong là phần nối của mép thẳng, nên màu mép thẳng là màu
                // tham chiếu đáng tin nhất: viền xám 1px trên card trắng là pixel
                // ĐẶC màu xám, không phải trắng phủ 50% — so với ruột trắng sẽ
                // thành nửa trong suốt, dán lên nền trắng là mất viền.
                if let a = blend(p, x >= y ? s.hEdge : s.vEdge, solidBeyond: true) { return a }

                // Mép đổi màu dọc đường cong (icon gradient…): lấy mẫu khác nền
                // nhiều nhất trong 3 bước vào trong. Tính cả chính pixel: card chỉ
                // có viền, ruột cùng màu nền, thì đi vào trong chỉ gặp lại nền.
                let (vx, vy) = normal(x, y)
                var ref: (Int, Int, Int)?
                var refD = 0
                for k in 0...3 {
                    let rx = x + Int((vx * Double(k)).rounded())
                    let ry = y + Int((vy * Double(k)).rounded())
                    guard rx >= 0, ry >= 0, rx < w, ry < h else { continue }
                    let d = diff(rx, ry, bg)
                    if d > refD { refD = d; ref = rgb(rx, ry) }
                }
                guard let f = ref, Double(refD) > tol else { return 0 }
                return blend(p, f)
            }

            // Nền đã loang: trong suốt, trừ mấy pixel còn ánh màu item (rìa mờ).
            var alpha = [Double?](repeating: nil, count: side * side)
            for y in 0..<side {
                for x in 0..<side where s.filled[y * side + x] {
                    alpha[y * side + x] = diff(x, y, bg) <= 2 ? 0 : (coverage(x, y) ?? 0)
                }
            }
            // Rìa khử răng cưa: đi vào từng lớp, chỉ nhận pixel mà phía NGOÀI nó
            // (về phía góc) đã là nền. Pixel giữa viền tối và ruột sáng có thể
            // trùng màu nền nhưng phía ngoài nó là viền, nên không bị đục lỗ.
            for _ in 0..<2 {
                let prev = alpha
                for y in 0..<side {
                    for x in 0..<side where prev[y * side + x] == nil {
                        let (vx, vy) = normal(x, y)
                        let here = diff(x, y, bg)
                        // Đi vào trong thì pixel rìa phải càng lúc càng khác nền
                        // (đang leo lên màu mép). Khác nền ít hơn pixel ngoài nó
                        // là đã qua đỉnh viền, vào tới ruột item: giữ nguyên.
                        let outward = [(-vx, -vy), (-vx + vy, -vy - vx), (-vx - vy, -vy + vx)].contains { dx, dy in
                            let n = max(abs(dx), abs(dy))
                            guard n > 0.1 else { return false }
                            let ox = x + Int((dx / n).rounded()), oy = y + Int((dy / n).rounded())
                            guard ox >= 0, oy >= 0, ox < side, oy < side, let a = prev[oy * side + ox] else { return false }
                            return a < 0.98 && here + 2 >= diff(ox, oy, bg)
                        }
                        guard outward, let a = coverage(x, y), a < 0.98 else { continue }
                        alpha[y * side + x] = a
                    }
                }
            }

            for y in 0..<side {
                for x in 0..<side {
                    guard let a = alpha[y * side + x] else { continue }
                    write(x, y, a, bg)
                }
            }
        }

        /// Ghi pixel với độ phủ `a`. Premultiplied: a·F = p − (1−a)·B.
        private func write(_ x: Int, _ y: Int, _ a: Double, _ bg: (Int, Int, Int)) {
            let o = offset(x, y)
            guard a > 0.02 else {
                px[o] = 0; px[o + 1] = 0; px[o + 2] = 0; px[o + 3] = 0
                return
            }
            for (c, b) in [bg.0, bg.1, bg.2].enumerated() {
                let v = Double(px[o + c]) - (1 - a) * Double(b)
                px[o + c] = UInt8(max(0, min(255 * a, v)).rounded())
            }
            px[o + 3] = UInt8((255 * a).rounded())
        }
    }
}
