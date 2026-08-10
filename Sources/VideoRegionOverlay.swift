import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Lớp phủ TRONG SUỐT nằm đè lên player — SwiftUI thuần. Nhiệm vụ duy nhất:
// cho kéo/vẽ VÙNG của đoạn đang chọn ngay trên hình — zoom phóng chỗ nào, che
// chỗ nào, chữ nằm đâu.
//
// Mọi toạ độ ghi vào segment đều CHUẨN HOÁ 0…1 theo khung hình (gốc trên-trái),
// nên kéo ở cửa sổ nhỏ hay to, xuất 480p hay 4K đều ra đúng chỗ đó. Toạ độ của
// SwiftUI cũng gốc trên-trái nên quy đổi chỉ là phép chia.
//
// Zoom lưu (center, level) chứ không lưu rect: vẽ ra rect nào thì quy ngược
// thành "phóng bao nhiêu lần quanh tâm nào" — đúng thứ Core Image cần.
// ─────────────────────────────────────────────────────────────────────────

struct VideoRegionOverlay: View {
    @ObservedObject var store: VideoEditStore

    private enum Mode {
        case idle
        case move(grab: CGPoint, origin: CGRect)
        case corner(fixed: CGPoint)
        case draw(anchor: CGPoint)
    }
    @State private var mode: Mode = .idle

    var body: some View {
        GeometryReader { geo in
            let v = videoRect(in: geo.size)
            if let seg = store.selectedRegion, let norm = current(seg) {
                let r = view(norm, in: v)
                let tint = Color(nsColor: seg.kind.tint)
                let alpha: Double = store.selectedRegionIsLive ? 1 : 0.35

                ZStack(alignment: .topLeading) {
                    // Nền bắt chuột: kéo trên nền trống = vẽ vùng mới.
                    Color.clear.contentShape(Rectangle())

                    // Mờ phần ngoài vùng (chỉ với zoom — cho thấy chỗ sẽ bị cắt).
                    if seg.kind == .zoom {
                        Path { p in p.addRect(v); p.addRect(r) }
                            .fill(.black.opacity(0.30 * alpha), style: FillStyle(eoFill: true))
                    }

                    if seg.kind == .censor {
                        Path(r).fill(tint.opacity(0.18 * alpha))
                        Path(r).stroke(tint.opacity(alpha),
                                       style: StrokeStyle(lineWidth: 2, dash: [5, 3]))
                    } else {
                        Path(r).stroke(tint.opacity(alpha), lineWidth: 2)
                    }

                    ForEach(Array(corners(r).enumerated()), id: \.offset) { _, c in
                        Rectangle()
                            .fill(tint.opacity(alpha))
                            .frame(width: 8, height: 8)
                            .offset(x: c.x - 4, y: c.y - 4)
                    }

                    // Nhắc nhẹ khi playhead chưa nằm trong đoạn → sửa vùng thì
                    // không thấy gì đổi trên hình, dễ tưởng là hỏng.
                    if !store.selectedRegionIsLive {
                        Text("Playhead is outside this \(seg.kind.label.lowercased()) — move it to preview")
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(.white.opacity(0.75))
                            .offset(x: v.minX + 8, y: v.minY + 8)
                    }
                }
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { g in
                            if case .idle = mode { begin(g.startLocation, seg: seg, rect: r) }
                            update(g.location, seg: seg, in: v)
                        }
                        .onEnded { _ in
                            if case .idle = mode { return }
                            mode = .idle
                            store.touch()
                        }
                )
                .onHover { $0 ? NSCursor.crosshair.set() : NSCursor.arrow.set() }
            } else {
                // Không có đoạn vùng nào đang chọn → không nhận chuột, click
                // xuyên xuống player như thường.
                Color.clear.allowsHitTesting(false)
            }
        }
    }

    // MARK: - Quy đổi toạ độ

    /// Vùng video thật sự hiển thị (aspect fit) trong khung được cấp.
    private func videoRect(in size: CGSize) -> CGRect {
        let s = store.sourceSize
        guard s.width > 0, s.height > 0 else { return CGRect(origin: .zero, size: size) }
        let fit = min(size.width / s.width, size.height / s.height)
        let w = s.width * fit, h = s.height * fit
        return CGRect(x: (size.width - w) / 2, y: (size.height - h) / 2, width: w, height: h)
    }

    private func view(_ norm: CGRect, in v: CGRect) -> CGRect {
        CGRect(x: v.minX + norm.minX * v.width, y: v.minY + norm.minY * v.height,
               width: norm.width * v.width, height: norm.height * v.height)
    }
    private func norm(_ r: CGRect, in v: CGRect) -> CGRect {
        guard v.width > 0, v.height > 0 else { return .zero }
        return CGRect(x: (r.minX - v.minX) / v.width, y: (r.minY - v.minY) / v.height,
                      width: r.width / v.width, height: r.height / v.height)
    }
    private func corners(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY),
         CGPoint(x: r.minX, y: r.maxY), CGPoint(x: r.maxX, y: r.maxY)]
    }

    /// Vùng (chuẩn hoá) hiện đang hiển thị cho đoạn đang chọn.
    private func current(_ seg: VideoSegment) -> CGRect? {
        switch seg.kind {
        case .zoom:
            let side = 1 / max(seg.zoom, 1.001)
            let half = side / 2
            let cx = min(max(seg.center.x, half), 1 - half)
            let cy = min(max(seg.center.y, half), 1 - half)
            return CGRect(x: cx - half, y: cy - half, width: side, height: side)
        case .censor, .text:
            return seg.rect
        default:
            return nil
        }
    }

    /// Ghi vùng mới vào đoạn (zoom thì quy ra center + level).
    private func apply(_ r: CGRect, to seg: VideoSegment) {
        let c = CGRect(x: min(max(r.minX, 0), 1), y: min(max(r.minY, 0), 1),
                       width: min(max(r.width, 0.03), 1), height: min(max(r.height, 0.03), 1))
        switch seg.kind {
        case .zoom:
            seg.center = CGPoint(x: c.midX, y: c.midY)
            // Vẽ vùng nhỏ → phóng to nhiều. Lấy cạnh LỚN hơn để cả vùng vẽ đều
            // lọt vào khung, rồi kẹp trong 1.2…5×.
            seg.zoom = min(max(1 / max(max(c.width, c.height), 0.0001), 1.2), 5)
        case .censor, .text:
            seg.rect = c
        default:
            break
        }
    }

    // MARK: - Kéo thả

    private func begin(_ p: CGPoint, seg: VideoSegment, rect r: CGRect) {
        store.pushUndo()
        let cs = corners(r)
        if let i = cs.firstIndex(where: { abs($0.x - p.x) <= 7 && abs($0.y - p.y) <= 7 }) {
            mode = .corner(fixed: cs[3 - i])      // góc đối diện đứng yên khi kéo
        } else if r.contains(p) {
            mode = .move(grab: CGPoint(x: p.x - r.minX, y: p.y - r.minY),
                         origin: seg.kind == .zoom ? (current(seg) ?? .zero) : seg.rect)
        } else {
            mode = .draw(anchor: p)               // kéo trên nền → vẽ vùng mới
        }
    }

    private func update(_ p: CGPoint, seg: VideoSegment, in v: CGRect) {
        switch mode {
        case .move(let grab, let origin):
            var r = origin
            r.origin = CGPoint(x: (p.x - grab.x - v.minX) / max(v.width, 1),
                               y: (p.y - grab.y - v.minY) / max(v.height, 1))
            r.origin.x = min(max(r.origin.x, 0), 1 - r.width)
            r.origin.y = min(max(r.origin.y, 0), 1 - r.height)
            apply(r, to: seg)
        case .corner(let fixed):
            apply(norm(between(fixed, p), in: v), to: seg)
        case .draw(let anchor):
            apply(norm(between(anchor, p), in: v), to: seg)
        case .idle:
            return
        }
        store.touch(live: true)
    }

    private func between(_ a: CGPoint, _ b: CGPoint) -> CGRect {
        CGRect(x: min(a.x, b.x), y: min(a.y, b.y),
               width: max(abs(b.x - a.x), 8), height: max(abs(b.y - a.y), 8))
    }
}
