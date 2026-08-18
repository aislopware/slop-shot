import AppKit
import ImageIO
import UniformTypeIdentifiers

// ─────────────────────────────────────────────────────────────────────────
// Một ảnh = một DÃY FRAME. Ảnh tĩnh chỉ là dãy 1 frame, nên mọi chỗ trong
// editor xài chung một kiểu, khỏi phải tách nhánh "tĩnh hay động".
//
// Ba nguồn frame gặp trong kho sticker:
//   • GIF / APNG / WebP động → CGImageSource trả nhiều frame, có sẵn delay.
//   • SPRITE SHEET ngang     → 1 ảnh, các frame xếp cạnh nhau (Zalo đóng gói
//                              animation kiểu này: 3250×130 = 25 frame 130×130).
//                              Không có thông tin delay nên đặt cứng 12fps.
//   • Ảnh thường             → 1 frame.
// ─────────────────────────────────────────────────────────────────────────
struct FrameSequence {
    var frames: [NSImage]
    var delays: [Double]      // giây, cùng số phần tử với frames

    var isAnimated: Bool { frames.count > 1 }
    var duration: Double { delays.reduce(0, +) }
    var still: NSImage? { frames.first }
    var size: NSSize { frames.first?.size ?? .zero }

    init(frames: [NSImage], delays: [Double]) {
        self.frames = frames
        // Thiếu delay thì bù, thừa thì cắt — chỗ khác cứ zip thoải mái.
        self.delays = frames.indices.map { $0 < delays.count ? delays[$0] : 0.1 }
    }

    init(_ still: NSImage) {
        self.init(frames: [still], delays: [0])
    }

    /// Frame đang chiếu ở giây thứ `t`, lặp vô hạn.
    func frame(at t: Double) -> NSImage? {
        guard frames.count > 1, duration > 0 else { return frames.first }
        var x = t.truncatingRemainder(dividingBy: duration)
        if x < 0 { x += duration }
        for (i, d) in delays.enumerated() {
            if x < d { return frames[i] }
            x -= d
        }
        return frames.last
    }
}

// ── Đọc frame từ file ────────────────────────────────────────────────────
extension FrameSequence {
    /// Sprite sheet không mang thông tin tốc độ; 12fps là mức Zalo chạy nhìn khớp nhất.
    static let spriteFPS = 12.0

    /// Đọc TOÀN BỘ frame. Tốn RAM (một GIF 360px 50 frame ≈ 26MB) nên chỉ gọi
    /// lúc thật sự chèn sticker vào ảnh, đừng gọi khi vẽ lưới chọn sticker.
    static func load(_ url: URL) -> FrameSequence? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil)
        else { return NSImage(contentsOf: url).map(FrameSequence.init) }
        // Định dạng lạ ImageIO không đọc nổi → để NSImage tự lo.
        return load(source: src) ?? NSImage(contentsOf: url).map(FrameSequence.init)
    }

    static func load(data: Data) -> FrameSequence? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil)
        else { return NSImage(data: data).map(FrameSequence.init) }
        return load(source: src) ?? NSImage(data: data).map(FrameSequence.init)
    }

    private static func load(source src: CGImageSource) -> FrameSequence? {
        let n = CGImageSourceGetCount(src)
        guard n > 0 else { return nil }
        if n > 1 {
            var frames: [NSImage] = [], delays: [Double] = []
            for i in 0..<n {
                guard let cg = CGImageSourceCreateImageAtIndex(src, i, nil) else { continue }
                frames.append(ns(cg))
                delays.append(delay(src, i))
            }
            guard !frames.isEmpty else { return nil }
            return FrameSequence(frames: frames, delays: delays)
        }

        guard let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
        return slice(cg) ?? FrameSequence(ns(cg))
    }

    /// File này có nhiều hơn 1 frame không — trả lời mà KHÔNG giải nén frame nào.
    static func isAnimated(_ url: URL) -> Bool {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return false }
        if CGImageSourceGetCount(src) > 1 { return true }
        // Sprite sheet: nhìn kích thước trong metadata là đủ, khỏi giải nén.
        guard let p = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
              let w = p[kCGImagePropertyPixelWidth] as? Int,
              let h = p[kCGImagePropertyPixelHeight] as? Int
        else { return false }
        return isSpriteSheet(width: w, height: h)
    }

    /// Ảnh đại diện để vẽ lưới chọn sticker: chỉ frame đầu, thu nhỏ sẵn cho nhẹ cache.
    static func thumbnail(_ url: URL, maxEdge: Int = 160) -> NSImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(src) > 0,
              var cg = CGImageSourceCreateImageAtIndex(src, 0, nil)
        else { return NSImage(contentsOf: url) }
        if let first = firstCell(of: cg) { cg = first }
        return ns(downscale(cg, maxEdge: maxEdge) ?? cg)
    }

    // ── Sprite sheet ───────────────────────────────────────────────────────
    // Chỉ nhận khi ngang chia hết cho cao và ≥3 lần, để không cắt nhầm ảnh
    // panorama/banner bình thường.
    static func isSpriteSheet(width w: Int, height h: Int) -> Bool {
        h > 0 && w >= h * 3 && w % h == 0
    }

    /// Ô vuông đầu tiên của sprite sheet (nil nếu không phải sprite sheet).
    static func firstCell(of cg: CGImage) -> CGImage? {
        guard isSpriteSheet(width: cg.width, height: cg.height) else { return nil }
        return cg.cropping(to: CGRect(x: 0, y: 0, width: cg.height, height: cg.height))
    }

    /// Cắt sprite sheet thành dãy frame (nil nếu không phải sprite sheet).
    static func slice(_ cg: CGImage) -> FrameSequence? {
        guard isSpriteSheet(width: cg.width, height: cg.height) else { return nil }
        let side = cg.height
        let frames = (0..<(cg.width / side)).compactMap {
            cg.cropping(to: CGRect(x: $0 * side, y: 0, width: side, height: side)).map(ns)
        }
        guard frames.count > 1 else { return nil }
        return FrameSequence(frames: frames,
                             delays: Array(repeating: 1 / spriteFPS, count: frames.count))
    }

    // ── Helper ─────────────────────────────────────────────────────────────
    static func ns(_ cg: CGImage) -> NSImage {
        NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
    }

    /// Delay của frame `i`. Mỗi định dạng cất ở một dictionary riêng nên phải dò cả lượt.
    private static func delay(_ src: CGImageSource, _ i: Int) -> Double {
        guard let props = CGImageSourceCopyPropertiesAtIndex(src, i, nil) as? [CFString: Any]
        else { return 0.1 }
        let slots: [(CFString, CFString, CFString)] = [
            (kCGImagePropertyGIFDictionary,
             kCGImagePropertyGIFUnclampedDelayTime, kCGImagePropertyGIFDelayTime),
            (kCGImagePropertyPNGDictionary,
             kCGImagePropertyAPNGUnclampedDelayTime, kCGImagePropertyAPNGDelayTime),
            (kCGImagePropertyHEICSDictionary,
             kCGImagePropertyHEICSUnclampedDelayTime, kCGImagePropertyHEICSDelayTime),
            (kCGImagePropertyWebPDictionary,
             kCGImagePropertyWebPUnclampedDelayTime, kCGImagePropertyWebPDelayTime),
        ]
        for (dict, unclamped, clamped) in slots {
            guard let d = props[dict] as? [CFString: Any] else { continue }
            // Unclamped là giá trị THẬT trong file; bản clamped đã bị ImageIO nâng
            // lên tối thiểu 0.1s. Ưu tiên bản thật để animation không bị chậm lại.
            if let v = d[unclamped] as? Double, v > 0 { return v }
            if let v = d[clamped] as? Double, v > 0 { return v }
        }
        return 0.1
    }

    /// Thu nhỏ cho cạnh dài không quá `maxEdge` (nil nếu vốn đã đủ nhỏ).
    static func downscale(_ cg: CGImage, maxEdge: Int) -> CGImage? {
        let long = max(cg.width, cg.height)
        guard long > maxEdge, long > 0 else { return nil }
        let k = Double(maxEdge) / Double(long)
        let w = max(Int((Double(cg.width) * k).rounded()), 1)
        let h = max(Int((Double(cg.height) * k).rounded()), 1)
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        ctx.interpolationQuality = .high
        ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
        return ctx.makeImage()
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Ghi GIF động.
//
// Ghi THẲNG từng frame vào destination thay vì gom cả mảng CGImage rồi mới
// mã hoá: ảnh chụp màn hình cỡ 1600px, 50 frame là 500MB RAM nếu giữ hết.
// ─────────────────────────────────────────────────────────────────────────
final class GIFWriter {
    private let buffer = NSMutableData()
    private let dest: CGImageDestination

    /// Cạnh dài tối đa của GIF xuất ra. Ảnh chụp Retina 3000px mà xuất nguyên cỡ
    /// thì một GIF 50 frame lên tới hàng trăm MB — không app chat nào nhận.
    static let maxEdge: CGFloat = 1600

    init?(frameCount: Int) {
        guard let d = CGImageDestinationCreateWithData(
            buffer, UTType.gif.identifier as CFString, max(frameCount, 1), nil)
        else { return nil }
        dest = d
        CGImageDestinationSetProperties(dest, [
            kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFLoopCount: 0],  // 0 = lặp mãi
        ] as CFDictionary)
    }

    func add(_ cg: CGImage, delay: Double) {
        // GIF lưu delay theo đơn vị 1/100s. Dưới 0.02s là nhiều trình xem tự coi
        // như "không đặt" rồi nhảy về 0.1s → animation chậm hẳn lại.
        let d = max(delay, 0.02)
        CGImageDestinationAddImage(dest, cg, [
            kCGImagePropertyGIFDictionary: [
                kCGImagePropertyGIFUnclampedDelayTime: d,
                kCGImagePropertyGIFDelayTime: d,
            ],
        ] as CFDictionary)
    }

    func finish() -> Data? {
        CGImageDestinationFinalize(dest) ? (buffer as Data) : nil
    }
}
