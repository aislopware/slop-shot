import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// Timeline của Video Editor — SwiftUI thuần.
//
//   ┌──────────────────────────────────────────────────────────┐
//   │ ▐ filmstrip: ảnh nhỏ + 2 tay nắm trim + playhead        ▌ │
//   ├──────────────────────────────────────────────────────────┤
//   │ lane 0  Cut     ▓▓▓▓                                     │
//   │ lane 1  Speed   ░░░░2×░░       ❄︎                        │
//   │ lane 2  Zoom          ▒▒▒2.4×▒▒                          │
//   │ lane 3  Censor    ▓▓Blur▓▓                               │
//   │ lane 4  Text                  ▒▒Hello▒▒                  │
//   └──────────────────────────────────────────────────────────┘
//
// Trục hoành là THỜI GIAN CLIP GỐC (source time) — kéo pill tới đâu thì hiệu
// ứng nằm đúng chỗ đó của bản quay, kể cả khi đã cắt bớt đoạn khác. Playhead
// do store quy đổi từ đồng hồ player (comp time) sang trục này.
//
// Chọn công cụ trên thanh trên → kéo trong vùng lane để TẠO đoạn mới.
// Không chọn công cụ → kéo pill để dời, kéo mép để co giãn, click nền để tua.
//
// Toạ độ SwiftUI gốc TRÊN-TRÁI nên khớp luôn với cách vẽ ở đây (y tăng xuống).
// ─────────────────────────────────────────────────────────────────────────

struct VideoTimeline: View {
    @ObservedObject var store: VideoEditStore

    // ── kích thước ────────────────────────────────────────────────────────
    private let inset: CGFloat = 12          // chừa chỗ cho tay nắm trim
    private let stripTop: CGFloat = 4
    private let stripH: CGFloat = 56
    private let laneH: CGFloat = 18
    private let laneGap: CGFloat = 4
    private var lanesTop: CGFloat { stripTop + stripH + 8 }

    static let height: CGFloat =
        4 + 56 + 8 + CGFloat(VideoEffectKind.laneCount) * (18 + 4) + 4

    // ── trạng thái kéo ────────────────────────────────────────────────────
    private enum Mode {
        case idle
        case trimStart, trimEnd, scrub
        case create(VideoEffectKind, anchor: Double)
        case move(UUID, grab: Double)
        case resizeLeft(UUID), resizeRight(UUID)
    }
    @State private var mode: Mode = .idle
    @State private var didSnapshot = false
    /// Vùng đang kéo để tạo đoạn mới (vẽ mờ cho thấy trước).
    @State private var ghost: (kind: VideoEffectKind, a: Double, b: Double)?

    var body: some View {
        GeometryReader { geo in
            let w = max(geo.size.width - inset * 2, 1)
            ZStack(alignment: .topLeading) {
                Color(white: 0.11)
                if store.duration > 0 {
                    filmstrip(w)
                    lanes(w)
                    segments(w)
                    ghostPill(w)
                    playhead(w)
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { v in onDrag(v, width: w) }
                    .onEnded { v in onDrop(v, width: w) }
            )
            // Cầm công cụ thì con trỏ đổi thành chữ thập — biết là sắp vẽ ra đoạn.
            .onHover { inside in
                if inside, store.activeTool != nil { NSCursor.crosshair.set() }
                else { NSCursor.arrow.set() }
            }
        }
        .frame(height: Self.height)
    }

    // MARK: - Quy đổi toạ độ

    private func x(_ t: Double, _ w: CGFloat) -> CGFloat {
        guard store.duration > 0 else { return inset }
        return inset + CGFloat(t / store.duration) * w
    }
    private func time(_ px: CGFloat, _ w: CGFloat) -> Double {
        guard store.duration > 0, w > 0 else { return 0 }
        return min(max(Double((px - inset) / w) * store.duration, 0), store.duration)
    }
    private func laneTop(_ lane: Int) -> CGFloat {
        lanesTop + CGFloat(lane) * (laneH + laneGap)
    }
    /// Khung của một pill: (x, width). Freeze gần như 0 giây trên clip gốc →
    /// cho nó bề rộng tối thiểu để còn bấm trúng.
    private func pill(_ seg: VideoSegment, _ w: CGFloat) -> (CGFloat, CGFloat) {
        let a = x(seg.start, w)
        let b = seg.kind == .freeze ? a + 26 : max(x(seg.end, w), a + 8)
        return (a, b - a)
    }

    // MARK: - Filmstrip

    private func filmstrip(_ w: CGFloat) -> some View {
        let sx = x(store.trimStart, w), ex = x(store.trimEnd, w)
        return ZStack(alignment: .topLeading) {
            ZStack(alignment: .topLeading) {
                Color(white: 0.07)
                HStack(spacing: 0) {
                    ForEach(Array(store.thumbnails.enumerated()), id: \.offset) { _, img in
                        Image(nsImage: img)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                            .frame(width: w / CGFloat(max(store.thumbnails.count, 1)), height: stripH)
                            .clipped()
                    }
                }
                // Ngoài vùng trim → tối hẳn.
                Color.black.opacity(0.62).frame(width: max(sx - inset, 0))
                Color.black.opacity(0.62).frame(width: max(inset + w - ex, 0)).offset(x: ex - inset)
                // Đoạn đã cut → phủ đỏ ngay trên filmstrip cho dễ thấy.
                ForEach(store.segments.filter { $0.kind == .cut }) { seg in
                    Color(nsColor: .systemRed).opacity(0.28)
                        .frame(width: max(x(seg.end, w) - x(seg.start, w), 2))
                        .offset(x: x(seg.start, w) - inset)
                }
            }
            .frame(width: w, height: stripH)
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .offset(x: inset, y: stripTop)

            // Khung + 2 tay nắm trim.
            Rectangle()
                .stroke(Color(nsColor: .systemYellow), lineWidth: 2)
                .frame(width: max(ex - sx, 1), height: stripH)
                .offset(x: sx, y: stripTop)
            ForEach([sx, ex], id: \.self) { hx in
                trimHandle.offset(x: hx - 5, y: stripTop - 3)
            }
        }
    }

    private var trimHandle: some View {
        RoundedRectangle(cornerRadius: 3)
            .fill(Color(nsColor: .systemYellow))
            .frame(width: 10, height: stripH + 6)
            .overlay {
                HStack(spacing: 3) {
                    Capsule().frame(width: 1, height: 12)
                    Capsule().frame(width: 1, height: 12)
                }
                .foregroundStyle(.black.opacity(0.45))
            }
    }

    // MARK: - Lane

    private func lanes(_ w: CGFloat) -> some View {
        ForEach(0..<VideoEffectKind.laneCount, id: \.self) { lane in
            let lit = store.activeTool?.lane == lane
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 4)
                    .fill(.white.opacity(lit ? 0.10 : 0.04))
                    .frame(width: w, height: laneH)
                    .offset(x: inset)
                Text(laneName(lane))
                    .font(.system(size: 9, weight: .medium))
                    .foregroundStyle(Color(white: 0.42))
                    .offset(x: 3)
            }
            .frame(height: laneH, alignment: .leading)
            .offset(y: laneTop(lane))
        }
    }

    /// Lane 1 chung cho Speed + Freeze nên gọi tên theo lane, không theo kind.
    private func laneName(_ lane: Int) -> String {
        VideoEffectKind.allCases.first { $0.lane == lane }.map { $0.lane == 1 ? "Speed" : $0.label } ?? ""
    }

    // MARK: - Pill

    private func segments(_ w: CGFloat) -> some View {
        ForEach(store.segments) { seg in
            let (px, pw) = pill(seg, w)
            let on = seg.id == store.selectedID
            let tint = Color(nsColor: seg.kind.tint)
            RoundedRectangle(cornerRadius: 4)
                .fill(tint.opacity(on ? 0.85 : 0.6))
                .overlay {
                    RoundedRectangle(cornerRadius: 4)
                        .stroke(on ? .white : tint, lineWidth: on ? 1.5 : 1)
                }
                .overlay {
                    Text(seg.badge)
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .padding(.horizontal, 3)
                }
                .frame(width: pw, height: laneH)
                .offset(x: px, y: laneTop(seg.kind.lane))
                .contextMenu { menu(for: seg) }
        }
        // ID của segment không đổi khi sửa field bên trong → ép vẽ lại theo revision.
        .id(store.revision)
    }

    @ViewBuilder
    private func menu(for seg: VideoSegment) -> some View {
        if seg.kind == .speed {
            ForEach([0.25, 0.5, 0.75, 2.0, 3.0, 5.0, 10.0], id: \.self) { f in
                Button {
                    store.pushUndo(); seg.speed = f; store.touch()
                } label: {
                    Text(f == f.rounded() ? "\(Int(f))×" : "\(f)×")
                    if abs(seg.speed - f) < 0.001 { Image(systemName: "checkmark") }
                }
            }
            Divider()
        }
        if seg.kind == .censor {
            ForEach(CensorStyle.allCases, id: \.self) { style in
                Button {
                    store.pushUndo(); seg.censorStyle = style; store.touch()
                } label: {
                    Text(style.label)
                    if seg.censorStyle == style { Image(systemName: "checkmark") }
                }
            }
            Divider()
        }
        Button("Delete \(seg.kind.label)", role: .destructive) { store.delete(seg.id) }
    }

    /// Vệt mờ trong lúc kéo tạo đoạn mới.
    @ViewBuilder
    private func ghostPill(_ w: CGFloat) -> some View {
        if let g = ghost {
            let a = x(min(g.a, g.b), w), b = x(max(g.a, g.b), w)
            RoundedRectangle(cornerRadius: 4)
                .fill(Color(nsColor: g.kind.tint).opacity(0.35))
                .overlay {
                    RoundedRectangle(cornerRadius: 4)
                        .stroke(Color(nsColor: g.kind.tint), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                }
                .frame(width: max(b - a, 2), height: laneH)
                .offset(x: a, y: laneTop(g.kind.lane))
        }
    }

    // MARK: - Playhead

    private func playhead(_ w: CGFloat) -> some View {
        let px = x(store.playheadSource, w)
        return ZStack(alignment: .topLeading) {
            Rectangle().fill(.white).frame(width: 2)
            Path { p in                       // đầu playhead hình tam giác nhỏ
                p.move(to: CGPoint(x: 0, y: 0))
                p.addLine(to: CGPoint(x: 10, y: 0))
                p.addLine(to: CGPoint(x: 5, y: 7))
                p.closeSubpath()
            }
            .fill(.white)
            .frame(width: 10, height: 7)
            .offset(x: -4)
        }
        .frame(width: 2)
        .offset(x: px - 1)
        .allowsHitTesting(false)
    }

    // MARK: - Kéo thả

    private func onDrag(_ v: DragGesture.Value, width w: CGFloat) {
        if case .idle = mode { begin(at: v.startLocation, width: w) }
        update(to: v.location, width: w)
    }

    private func begin(at p: CGPoint, width w: CGFloat) {
        didSnapshot = false
        guard store.duration > 0 else { return }

        // 1) Vùng filmstrip: tay nắm trim, hoặc tua.
        if p.y <= stripTop + stripH + 4 {
            let sx = x(store.trimStart, w), ex = x(store.trimEnd, w)
            if abs(p.x - sx) <= 8 { mode = .trimStart; return }
            if abs(p.x - ex) <= 8 { mode = .trimEnd; return }
            mode = .scrub
            store.seek(source: time(p.x, w))
            return
        }

        // 2) Vùng lane: đang cầm công cụ → tạo mới.
        if let tool = store.activeTool {
            let t = time(p.x, w)
            if tool == .freeze {
                snapshotOnce()
                store.addSegment(kind: .freeze, start: t, end: t + 1.0)
                mode = .idle
            } else {
                mode = .create(tool, anchor: t)
                ghost = (tool, t, t)
            }
            return
        }

        // 3) Không cầm công cụ → chọn / dời / co giãn pill.
        if let hit = segment(at: p, width: w) {
            let (px, pw) = pill(hit, w)
            store.selectedID = hit.id
            if hit.kind != .freeze && abs(p.x - px) <= 5 { mode = .resizeLeft(hit.id) }
            else if hit.kind != .freeze && abs(p.x - (px + pw)) <= 5 { mode = .resizeRight(hit.id) }
            else { mode = .move(hit.id, grab: time(p.x, w) - hit.start) }
            return
        }

        store.selectedID = nil
        mode = .scrub
        store.seek(source: time(p.x, w))
    }

    private func update(to p: CGPoint, width w: CGFloat) {
        let t = time(p.x, w)
        switch mode {
        case .trimStart:
            store.trimStart = min(t, store.trimEnd - 0.15)
            store.seek(source: store.trimStart)
        case .trimEnd:
            store.trimEnd = max(t, store.trimStart + 0.15)
            store.seek(source: store.trimEnd)
        case .scrub:
            store.seek(source: t)
        case .create(let kind, let anchor):
            ghost = (kind, anchor, t)
        case .move(let id, let grab):
            guard let seg = store.segments.first(where: { $0.id == id }) else { return }
            snapshotOnce()
            let len = seg.duration
            seg.start = min(max(t - grab, 0), store.duration - len)
            seg.end = seg.start + len
            store.touch(live: true)
        case .resizeLeft(let id):
            guard let seg = store.segments.first(where: { $0.id == id }) else { return }
            snapshotOnce()
            seg.start = min(max(t, 0), seg.end - VideoSegment.minDuration(seg.kind))
            store.touch(live: true)
        case .resizeRight(let id):
            guard let seg = store.segments.first(where: { $0.id == id }) else { return }
            snapshotOnce()
            seg.end = max(min(t, store.duration), seg.start + VideoSegment.minDuration(seg.kind))
            store.touch(live: true)
        case .idle:
            break
        }
    }

    private func onDrop(_ v: DragGesture.Value, width w: CGFloat) {
        switch mode {
        case .create(let kind, let anchor):
            let t = time(v.location.x, w)
            let a = min(anchor, t), b = max(anchor, t)
            let minLen = VideoSegment.minDuration(kind)
            // Click nhanh (không kéo) → tạo đoạn mặc định 2 giây.
            let (start, end) = (b - a) < minLen ? (a, min(a + 2, store.duration)) : (a, b)
            if end > start {
                snapshotOnce()
                store.addSegment(kind: kind, start: start, end: end)
            }
        case .trimStart, .trimEnd, .move, .resizeLeft, .resizeRight:
            store.touch()
        case .scrub, .idle:
            break
        }
        ghost = nil
        mode = .idle
        didSnapshot = false
    }

    private func segment(at p: CGPoint, width w: CGFloat) -> VideoSegment? {
        // Duyệt ngược để pill vẽ sau (nằm trên) được ưu tiên.
        for seg in store.segments.reversed() {
            let top = laneTop(seg.kind.lane)
            guard p.y >= top, p.y <= top + laneH else { continue }
            let (px, pw) = pill(seg, w)
            if p.x >= px - 2, p.x <= px + pw + 2 { return seg }
        }
        return nil
    }

    /// Chụp undo đúng MỘT lần cho cả lượt kéo.
    private func snapshotOnce() {
        guard !didSnapshot else { return }
        didSnapshot = true
        store.pushUndo()
    }
}
