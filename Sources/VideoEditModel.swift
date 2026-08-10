import Foundation
import AppKit
import CoreGraphics

// ─────────────────────────────────────────────────────────────────────────
// MÔ HÌNH DỮ LIỆU cho Video Editor.
//
// Mọi mốc thời gian trong file này đều là "giây theo clip GỐC" (source time) —
// tức là toạ độ của file .mov vừa quay, TRƯỚC khi cắt/tua nhanh. Timeline dưới
// editor cũng vẽ theo trục này, nên kéo thả một pill ở đâu thì nó nằm đúng ở đó,
// không bị lệch khi thêm/bớt đoạn cắt.
//
// Khi phát/xuất file, ta dựng một AVComposition mới → clock của nó (composition
// time) KHÁC source time. `VideoTimeMap` là cây cầu hai chiều giữa hai trục đó:
//   - phát video: comp → source (để biết lúc này phải zoom/che vùng nào)
//   - bấm vào timeline: source → comp (để tua player tới đúng chỗ)
//
// React-analogy: `VideoSegment` ~ item trong state array; `VideoRenderPlan` ~
// snapshot bất biến của state, đưa xuống "render layer" (Core Image) chạy ở
// thread khác nên không được đụng vào object gốc nữa.
// ─────────────────────────────────────────────────────────────────────────

// MARK: - Loại hiệu ứng

enum VideoEffectKind: String, CaseIterable {
    case cut        // bỏ hẳn đoạn này khỏi clip
    case speed      // tua nhanh/chậm đoạn này
    case freeze     // đứng hình tại 1 mốc trong N giây
    case zoom       // phóng to vào 1 vùng
    case censor     // che (blur / pixelate) 1 vùng
    case text       // chèn chữ lên video

    var label: String {
        switch self {
        case .cut:    return "Cut"
        case .speed:  return "Speed"
        case .freeze: return "Freeze"
        case .zoom:   return "Zoom"
        case .censor: return "Censor"
        case .text:   return "Text"
        }
    }

    var icon: String {
        switch self {
        case .cut:    return "scissors"
        case .speed:  return "speedometer"
        case .freeze: return "snowflake"
        case .zoom:   return "plus.magnifyingglass"
        case .censor: return "eye.slash"
        case .text:   return "textformat"
        }
    }

    /// Màu pill trên timeline (mỗi loại một màu để liếc phát là biết).
    var tint: NSColor {
        switch self {
        case .cut:    return NSColor.systemRed
        case .speed:  return NSColor.systemOrange
        case .freeze: return NSColor.systemTeal
        case .zoom:   return NSColor.systemYellow
        case .censor: return NSColor.systemPurple
        case .text:   return NSColor.systemGreen
        }
    }

    /// Hiệu ứng có "vùng vẽ trên video" hay không (zoom/censor/text thì có).
    var hasRegion: Bool { self == .zoom || self == .censor || self == .text }

    /// Thứ tự lane trên timeline — mỗi loại một hàng riêng cho khỏi chồng nhau.
    var lane: Int {
        switch self {
        case .cut:    return 0
        case .speed:  return 1
        case .freeze: return 1
        case .zoom:   return 2
        case .censor: return 3
        case .text:   return 4
        }
    }

    static let laneCount = 5
}

enum CensorStyle: String, CaseIterable {
    case blur, pixelate
    var label: String { self == .blur ? "Blur" : "Pixelate" }
}

// MARK: - Một đoạn hiệu ứng

/// Dùng CHUNG một class cho cả 6 loại: field nào không liên quan thì bỏ qua.
/// Gộp lại thế này để timeline / undo / inspector chỉ phải xử lý một mảng duy
/// nhất, thay vì 6 mảng gần giống hệt nhau.
final class VideoSegment: Identifiable {
    let id = UUID()
    var kind: VideoEffectKind

    /// Khoảng thời gian trên clip gốc (giây). Với `.freeze`, `start` là mốc
    /// đứng hình còn `end - start` là ĐỘ DÀI GIỮ HÌNH (thời gian nó chiếm trên
    /// clip xuất ra) — vẽ trên timeline vẫn là một pill nhỏ tại chỗ đó.
    var start: Double
    var end: Double

    // ── zoom ──────────────────────────────────────────────────────────────
    /// Hệ số phóng (1 = không zoom). Tâm zoom nằm ở `center`.
    var zoom: CGFloat = 2.0
    /// Tâm zoom, chuẩn hoá 0…1 theo khung hình, GỐC TRÊN-TRÁI.
    var center: CGPoint = CGPoint(x: 0.5, y: 0.5)
    /// Thời gian "phóng vào" / "thu ra" (giây) — để zoom mượt chứ không giật.
    var fade: Double = 0.35

    // ── speed ─────────────────────────────────────────────────────────────
    var speed: Double = 2.0

    // ── censor ────────────────────────────────────────────────────────────
    var censorStyle: CensorStyle = .blur
    /// Độ mạnh 0…1 (map ra bán kính blur / cỡ ô pixel).
    var strength: Double = 0.5

    // ── censor + text: vùng trên khung hình (0…1, gốc TRÊN-TRÁI) ──────────
    var rect: CGRect = CGRect(x: 0.3, y: 0.3, width: 0.4, height: 0.2)

    // ── text ──────────────────────────────────────────────────────────────
    var text: String = "Text"
    /// Cỡ chữ tính theo % chiều cao khung hình → xuất full-res vẫn cân đối.
    var fontScale: Double = 0.08
    var textColor: NSColor = .white
    var hasShadow: Bool = true

    init(kind: VideoEffectKind, start: Double, end: Double) {
        self.kind = kind
        self.start = start
        self.end = end
    }

    var duration: Double { max(0, end - start) }

    /// Độ dài tối thiểu theo từng loại (tránh pill 0 giây bấm không trúng).
    static func minDuration(_ kind: VideoEffectKind) -> Double {
        switch kind {
        case .cut:    return 0.1
        case .speed:  return 0.2
        case .freeze: return 0.2
        default:      return 0.2
        }
    }

    func overlaps(_ other: VideoSegment) -> Bool {
        start < other.end && end > other.start
    }

    /// Bản sao (cho undo — snapshot toàn bộ mảng segment).
    func copy() -> VideoSegment {
        let s = VideoSegment(kind: kind, start: start, end: end)
        s.zoom = zoom; s.center = center; s.fade = fade
        s.speed = speed
        s.censorStyle = censorStyle; s.strength = strength; s.rect = rect
        s.text = text; s.fontScale = fontScale; s.textColor = textColor
        s.hasShadow = hasShadow
        return s
    }

    /// Chữ mô tả ngắn hiện trên pill.
    var badge: String {
        switch kind {
        case .cut:    return "Cut"
        case .speed:  return speed == rint(speed) ? "\(Int(speed))×"
                                                  : String(format: "%.2g×", speed)
        case .freeze: return String(format: "❄︎ %.1fs", duration)
        case .zoom:   return String(format: "%.1f×", zoom)
        case .censor: return censorStyle.label
        case .text:   return text.isEmpty ? "Text" : text
        }
    }
}

// MARK: - Cắt: từ trim + các đoạn cut → danh sách đoạn GIỮ LẠI

enum VideoCuts {
    /// Trả về các khoảng (theo source time) còn sống sau khi trim hai đầu và
    /// khoét các đoạn `cuts` ở giữa. Kết quả đã sắp xếp và không chồng nhau.
    static func keptRanges(trimStart: Double, trimEnd: Double,
                           cuts: [VideoSegment]) -> [(Double, Double)] {
        guard trimEnd > trimStart else { return [] }

        // 1) Kẹp mọi đoạn cut vào trong vùng trim, bỏ đoạn rỗng.
        let clipped = cuts
            .filter { $0.kind == .cut && $0.end > $0.start }
            .map { (max(trimStart, $0.start), min(trimEnd, $0.end)) }
            .filter { $0.0 < $0.1 }
            .sorted { $0.0 < $1.0 }

        // 2) Gộp các cut dính/chồng nhau thành một.
        var merged: [(Double, Double)] = []
        for c in clipped {
            if let last = merged.last, c.0 <= last.1 + 0.001 {
                merged[merged.count - 1] = (last.0, max(last.1, c.1))
            } else {
                merged.append(c)
            }
        }

        // 3) Phần bù của các cut bên trong vùng trim chính là phần giữ lại.
        var kept: [(Double, Double)] = []
        var cursor = trimStart
        for (cs, ce) in merged {
            if cs > cursor + 0.001 { kept.append((cursor, cs)) }
            cursor = max(cursor, ce)
        }
        if cursor < trimEnd - 0.001 { kept.append((cursor, trimEnd)) }
        return kept
    }
}

// MARK: - Bản đồ thời gian: composition ⇄ source

struct VideoTimeMap {
    /// Một mảnh liên tục trên clip xuất ra.
    ///   sourceTime = srcStart + (compTime - compStart) × factor
    /// factor = 1 (bình thường), = tốc độ (speed), ≈ 0 (freeze — nguồn gần như
    /// đứng yên nên cùng một khung hình được lặp suốt thời gian giữ).
    struct Piece {
        let compStart: Double
        let compDuration: Double
        let srcStart: Double
        let srcDuration: Double

        var compEnd: Double { compStart + compDuration }
        var srcEnd: Double { srcStart + srcDuration }
        var factor: Double { compDuration > 0 ? srcDuration / compDuration : 1 }
    }

    let pieces: [Piece]
    var duration: Double { pieces.last.map { $0.compEnd } ?? 0 }

    /// Lát source cực mỏng dùng làm "chỗ dựa" cho freeze: đủ nhỏ để cả đoạn giữ
    /// hình chỉ đọc đúng một khung, nhưng khác 0 để `insertTimeRange` chèn được.
    static let freezeSlice: Double = 1.0 / 600.0

    // ── Dựng bản đồ từ: khoảng giữ lại + đoạn speed + mốc freeze ───────────
    static func build(keptRanges: [(Double, Double)],
                      speeds: [VideoSegment],
                      freezes: [VideoSegment]) -> VideoTimeMap {
        // Chuẩn hoá speed: bỏ đoạn hỏng, sắp xếp, và cắt phần chồng lấn
        // (đoạn nào bắt đầu sau thì bị đẩy lùi — UI cũng đã chặn chồng rồi).
        var cleanSpeeds: [(Double, Double, Double)] = []
        for s in speeds.filter({ $0.kind == .speed && $0.end > $0.start && $0.speed > 0 })
                       .sorted(by: { $0.start < $1.start }) {
            let from = max(s.start, cleanSpeeds.last?.1 ?? -.infinity)
            if s.end > from { cleanSpeeds.append((from, s.end, s.speed)) }
        }

        let stops = freezes.filter { $0.kind == .freeze && $0.duration > 0 }
                           .sorted { $0.start < $1.start }

        var out: [Piece] = []
        var comp: Double = 0

        /// Đẩy một mảnh vào danh sách (bỏ qua mảnh 0 giây).
        func emit(srcStart: Double, srcDuration: Double, compDuration: Double) {
            guard compDuration > 0.0005 else { return }
            out.append(Piece(compStart: comp, compDuration: compDuration,
                             srcStart: srcStart, srcDuration: srcDuration))
            comp += compDuration
        }

        for (rangeStart, rangeEnd) in keptRanges {
            guard rangeEnd > rangeStart else { continue }
            var cursor = rangeStart

            /// Phát phần [cursor, limit] và chẻ nhỏ theo các đoạn speed nằm trong đó.
            func emitUpTo(_ limit: Double) {
                guard limit > cursor else { return }
                for (ss, se, factor) in cleanSpeeds {
                    if se <= cursor { continue }
                    if ss >= limit { break }
                    let a = max(ss, cursor), b = min(se, limit)
                    if a > cursor { emit(srcStart: cursor, srcDuration: a - cursor, compDuration: a - cursor) }
                    if b > a { emit(srcStart: a, srcDuration: b - a, compDuration: (b - a) / factor) }
                    cursor = max(cursor, b)
                }
                if cursor < limit {
                    emit(srcStart: cursor, srcDuration: limit - cursor, compDuration: limit - cursor)
                    cursor = limit
                }
            }

            // Các mốc freeze rơi vào khoảng này → chẻ khoảng ra tại đúng mốc đó.
            for stop in stops where stop.start > rangeStart && stop.start < rangeEnd {
                emitUpTo(stop.start)
                let sliceStart = max(stop.start, cursor)
                let slice = min(freezeSlice, max(0, rangeEnd - sliceStart))
                if slice > 0 {
                    emit(srcStart: sliceStart, srcDuration: slice, compDuration: stop.duration)
                    cursor = sliceStart + slice
                }
            }
            emitUpTo(rangeEnd)
        }

        return VideoTimeMap(pieces: out)
    }

    // ── Đổi trục ──────────────────────────────────────────────────────────

    /// comp → source. Dùng khi render (biết đang ở giây thứ mấy của clip xuất
    /// ra, cần biết nó ứng với giây nào của clip gốc để tra hiệu ứng).
    func sourceTime(comp t: Double) -> Double {
        guard let first = pieces.first else { return 0 }
        if t <= first.compStart { return first.srcStart }
        for p in pieces where t < p.compEnd {
            guard t >= p.compStart else { break }
            return p.srcStart + (t - p.compStart) * p.factor
        }
        let last = pieces[pieces.count - 1]
        return last.srcEnd
    }

    /// source → comp. Dùng khi bấm/kéo trên timeline để tua player.
    /// Mốc nằm trong đoạn đã cắt → nhảy tới đầu đoạn giữ lại kế tiếp.
    func compTime(source t: Double) -> Double {
        guard let first = pieces.first else { return 0 }
        if t <= first.srcStart { return first.compStart }
        for p in pieces {
            if t < p.srcStart { return p.compStart }          // rơi vào chỗ đã cắt
            if t <= p.srcEnd {
                let f = p.factor
                return f > 0 ? p.compStart + (t - p.srcStart) / f : p.compStart
            }
        }
        return duration
    }

    /// Mốc source này có bị cắt bỏ không (dùng để làm mờ pill trên timeline).
    func isDropped(source t: Double) -> Bool {
        !pieces.contains { t >= $0.srcStart - 0.001 && t <= $0.srcEnd + 0.001 }
    }
}

// MARK: - Snapshot bất biến để đưa xuống Core Image

/// Toàn bộ thứ cần để vẽ một khung hình, đóng băng lại tại thời điểm bấm phát/
/// xuất file. Core Image chạy ở thread nền nên KHÔNG được đọc `VideoSegment`
/// (class, main-actor) trực tiếp — nó đọc mấy struct này.
struct VideoRenderPlan {
    struct Zoom {
        let start: Double, end: Double
        let level: CGFloat
        let center: CGPoint          // 0…1, gốc trên-trái
        let fade: Double

        /// Hệ số phóng tại giây `t` (đã ease vào/ra). Ngoài đoạn thì = 1.
        func level(at t: Double) -> CGFloat {
            let dur = end - start
            guard t >= start, t <= end, dur > 0 else { return 1 }
            let f = min(fade, max(0, dur / 2 - 0.001))
            guard f > 0 else { return level }
            let into = t - start, toEnd = end - t
            if into < f { return 1 + (level - 1) * ease(CGFloat(into / f)) }
            if toEnd < f { return 1 + (level - 1) * ease(CGFloat(toEnd / f)) }
            return level
        }

        /// smoothstep: chậm ở hai đầu, nhanh ở giữa.
        private func ease(_ x: CGFloat) -> CGFloat {
            let c = min(max(x, 0), 1)
            return c * c * (3 - 2 * c)
        }
    }

    struct Censor {
        let start: Double, end: Double
        let rect: CGRect             // 0…1, gốc trên-trái
        let style: CensorStyle
        let strength: Double
    }

    struct Text {
        let start: Double, end: Double
        let rect: CGRect             // 0…1, gốc trên-trái
        let image: CGImage?          // chữ đã raster sẵn ở main thread
    }

    let zooms: [Zoom]
    let censors: [Censor]
    let texts: [Text]

    var isEmpty: Bool { zooms.isEmpty && censors.isEmpty && texts.isEmpty }
}

// MARK: - Raster chữ ra ảnh (làm ở main thread, trước khi render)

enum VideoTextRasterizer {
    /// Vẽ `text` vừa khít vào khung `pixelSize` (kích thước vùng trên khung hình
    /// tính bằng pixel). Trả CGImage nền trong suốt để Core Image ghép chồng lên.
    static func image(text: String, pixelSize: CGSize, fontPixels: CGFloat,
                      color: NSColor, shadow: Bool) -> CGImage? {
        let w = max(2, Int(pixelSize.width.rounded()))
        let h = max(2, Int(pixelSize.height.rounded()))
        guard !text.isEmpty else { return nil }

        let font = NSFont.systemFont(ofSize: max(6, fontPixels), weight: .semibold)
        let para = NSMutableParagraphStyle()
        para.alignment = .center
        para.lineBreakMode = .byWordWrapping

        var attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: color,
            .paragraphStyle: para,
        ]
        if shadow {
            let sh = NSShadow()
            sh.shadowColor = NSColor.black.withAlphaComponent(0.75)
            sh.shadowBlurRadius = max(2, fontPixels * 0.12)
            sh.shadowOffset = NSSize(width: 0, height: -max(1, fontPixels * 0.05))
            attrs[.shadow] = sh
        }

        let attributed = NSAttributedString(string: text, attributes: attrs)

        guard let ctx = CGContext(data: nil, width: w, height: h,
                                  bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }

        let nsCtx = NSGraphicsContext(cgContext: ctx, flipped: false)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = nsCtx

        // Canh giữa theo chiều dọc trong khung.
        let box = NSRect(x: 0, y: 0, width: CGFloat(w), height: CGFloat(h))
        let textH = attributed.boundingRect(with: NSSize(width: box.width, height: .greatestFiniteMagnitude),
                                            options: [.usesLineFragmentOrigin]).height
        let y = max(0, (box.height - textH) / 2)
        attributed.draw(with: NSRect(x: 0, y: y, width: box.width, height: min(textH, box.height)),
                        options: [.usesLineFragmentOrigin])

        NSGraphicsContext.restoreGraphicsState()
        return ctx.makeImage()
    }
}
