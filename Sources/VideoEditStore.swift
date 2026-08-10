import SwiftUI
import AVKit
import AVFoundation

// ─────────────────────────────────────────────────────────────────────────
// "State + logic" của Video Editor, tách hẳn khỏi giao diện.
//
// React-analogy: đây là cái store (useReducer/zustand); `VideoEditorView` chỉ
// đọc state ra và gọi mấy hàm ở đây — không tự tính toán gì.
//
// Nguyên tắc xuyên suốt: MỌI thay đổi đều đi qua `scheduleRebuild()` → dựng lại
// composition từ state rồi nạp vào player. Preview và file xuất ra gọi CHUNG
// một hàm dựng (VideoCompositionBuilder) nên không bao giờ lệch nhau.
//
// Lưu ý: `VideoSegment` là CLASS (để timeline kéo thả sửa tại chỗ), nên sửa
// field bên trong nó KHÔNG tự bắn objectWillChange. Chỗ nào sửa xong phải gọi
// `touch(live:)` — mấy Binding trong inspector đã lo sẵn việc này.
// ─────────────────────────────────────────────────────────────────────────

@MainActor
final class VideoEditStore: ObservableObject {

    // ── nguồn ─────────────────────────────────────────────────────────────
    let sourceURL: URL
    let asset: AVURLAsset
    let saveFolder: URL
    /// Click ghi lại lúc quay → nút Auto Zoom.
    let clicks: [RecordedClick]

    let player = AVPlayer()

    // ── state hiển thị ────────────────────────────────────────────────────
    @Published private(set) var duration: Double = 0
    @Published private(set) var sourceSize = CGSize(width: 16, height: 9)
    @Published var trimStart: Double = 0
    @Published var trimEnd: Double = 0
    @Published private(set) var segments: [VideoSegment] = []
    @Published var selectedID: UUID?
    @Published var activeTool: VideoEffectKind?
    @Published private(set) var playheadSource: Double = 0
    @Published private(set) var compTime: Double = 0
    @Published private(set) var compDuration: Double = 0
    @Published private(set) var isPlaying = false
    @Published private(set) var thumbnails: [NSImage] = []
    @Published private(set) var audioTrackCount = 0
    @Published var audio = VideoAudioSettings()
    @Published var options = VideoExportOptions()
    /// nil = không xuất; 0…1 = đang xuất.
    @Published private(set) var exportProgress: Double?
    @Published var exportError: String?
    @Published private(set) var canUndo = false
    /// Tăng mỗi lần sửa field bên trong một segment (ép SwiftUI vẽ lại).
    @Published private(set) var revision = 0

    var onDone: ((URL?) -> Void)?

    private(set) var timeMap = VideoTimeMap(pieces: [])
    var selected: VideoSegment? { segments.first { $0.id == selectedID } }

    /// Đoạn đang chọn có vùng vẽ trên hình không, và playhead có nằm trong nó không.
    var selectedRegion: VideoSegment? {
        guard let s = selected, s.kind.hasRegion else { return nil }
        return s
    }
    var selectedRegionIsLive: Bool {
        guard let s = selectedRegion else { return true }
        return playheadSource >= s.start - 0.001 && playheadSource <= s.end + 0.001
    }

    // ── undo ──────────────────────────────────────────────────────────────
    private struct Snapshot { let trimStart: Double; let trimEnd: Double; let segments: [VideoSegment] }
    private var undoStack: [Snapshot] = []

    private var timeObserver: Any?
    private var rebuildTask: Task<Void, Never>?

    init(url: URL, saveFolder: URL, clicks: [RecordedClick]) {
        self.sourceURL = url
        self.saveFolder = saveFolder
        self.clicks = clicks
        self.asset = AVURLAsset(url: url)
        player.actionAtItemEnd = .pause
    }

    func tearDown() {
        rebuildTask?.cancel()
        if let timeObserver { player.removeTimeObserver(timeObserver) }
        timeObserver = nil
        player.pause()
        player.replaceCurrentItem(with: nil)
    }

    // MARK: - Nạp clip

    func load() async {
        duration = max((try? await asset.load(.duration).seconds) ?? 0, 0.05)
        trimStart = 0
        trimEnd = duration

        if let track = try? await asset.loadTracks(withMediaType: .video).first,
           let natural = try? await track.load(.naturalSize),
           let transform = try? await track.load(.preferredTransform) {
            sourceSize = CGSize(width: abs(natural.applying(transform).width),
                                height: abs(natural.applying(transform).height))
        }
        audioTrackCount = (try? await asset.loadTracks(withMediaType: .audio).count) ?? 0
        audio.volumes = Array(repeating: 1, count: audioTrackCount)

        addTimeObserver()
        await rebuildNow(seekTo: 0)
        await loadThumbnails()
    }

    private func loadThumbnails() async {
        let count = 26
        let gen = AVAssetImageGenerator(asset: asset)
        gen.appliesPreferredTrackTransform = true
        gen.maximumSize = CGSize(width: 160, height: 160)
        gen.requestedTimeToleranceBefore = CMTime(seconds: 0.3, preferredTimescale: 600)
        gen.requestedTimeToleranceAfter = CMTime(seconds: 0.3, preferredTimescale: 600)

        var out: [NSImage] = []
        for i in 0..<count {
            // Trừ hụt một chút ở cuối — hỏi đúng khung cuối cùng thì generator
            // hay trả lỗi, filmstrip bị cụt mất một đoạn bên phải.
            let t = min(duration * Double(i) / Double(count - 1), duration - 0.05)
            guard let cg = try? await gen.image(at: CMTime(seconds: max(t, 0),
                                                           preferredTimescale: 600)).image
            else { continue }
            out.append(NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height)))
        }
        thumbnails = out
    }

    // MARK: - Dựng lại composition

    /// Gộp nhiều thay đổi liên tiếp (kéo slider) thành một lần dựng cho khỏi giật.
    func scheduleRebuild() {
        rebuildTask?.cancel()
        rebuildTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 120_000_000)
            guard !Task.isCancelled, let self else { return }
            await self.rebuildNow(seekTo: self.playheadSource)
        }
    }

    private func rebuildNow(seekTo sourceTime: Double) async {
        let wasPlaying = player.rate > 0
        do {
            let built = try await VideoCompositionBuilder.build(
                asset: asset, trimStart: trimStart, trimEnd: trimEnd,
                segments: segments, audio: audio)
            timeMap = built.timeMap
            compDuration = built.timeMap.duration

            let item = AVPlayerItem(asset: built.composition)
            item.videoComposition = built.videoComposition
            item.audioMix = built.audioMix
            player.replaceCurrentItem(with: item)

            let comp = timeMap.compTime(source: min(max(sourceTime, trimStart), trimEnd))
            await player.seek(to: CMTime(seconds: comp, preferredTimescale: 600),
                              toleranceBefore: .zero, toleranceAfter: .zero)
            if wasPlaying { player.play() }
        } catch {
            exportError = "Couldn't build the preview — \(error.localizedDescription)"
        }
    }

    private func addTimeObserver() {
        guard timeObserver == nil else { return }
        timeObserver = player.addPeriodicTimeObserver(
            forInterval: CMTime(seconds: 0.03, preferredTimescale: 600), queue: .main
        ) { [weak self] t in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.compTime = t.seconds
                self.playheadSource = self.timeMap.sourceTime(comp: t.seconds)
                self.isPlaying = self.player.rate > 0
                // Chạm cuối thì dừng lại cho dễ xem lại.
                if t.seconds >= self.timeMap.duration - 0.03, self.player.rate > 0 {
                    self.player.pause()
                    self.isPlaying = false
                }
            }
        }
    }

    // MARK: - Sửa state

    /// Gọi sau khi sửa field bên trong một segment.
    /// `live` = đang kéo (slider/khung trên video) → chỉ vẽ lại, chưa chốt undo.
    func touch(live: Bool = false) {
        revision &+= 1
        scheduleRebuild()
    }

    func pushUndo() {
        undoStack.append(Snapshot(trimStart: trimStart, trimEnd: trimEnd,
                                  segments: segments.map { $0.copy() }))
        if undoStack.count > 40 { undoStack.removeFirst() }
        canUndo = true
    }

    func undo() {
        guard let snap = undoStack.popLast() else { NSSound.beep(); return }
        canUndo = !undoStack.isEmpty
        trimStart = snap.trimStart
        trimEnd = snap.trimEnd
        segments = snap.segments
        selectedID = nil
        activeTool = nil
        touch()
    }

    func addSegment(kind: VideoEffectKind, start: Double, end: Double) {
        let seg = VideoSegment(kind: kind, start: start, end: end)
        if kind == .text { seg.rect = CGRect(x: 0.15, y: 0.72, width: 0.7, height: 0.14) }
        segments.append(seg)
        selectedID = seg.id
        activeTool = nil               // tạo xong nhả công cụ ra
        seek(source: (start + end) / 2)
        touch()
    }

    func delete(_ id: UUID) {
        pushUndo()
        segments.removeAll { $0.id == id }
        if selectedID == id { selectedID = nil }
        touch()
    }

    func deleteSelected() {
        guard let id = selectedID else { return }
        delete(id)
    }

    // MARK: - Auto Zoom

    /// Gom các cú click lúc quay thành từng cụm rồi đẻ ra một đoạn zoom cho mỗi
    /// cụm: phóng vào đúng chỗ vừa bấm, vào/ra có ease nên không giật.
    @discardableResult
    func applyAutoZoom() -> Int {
        guard !clicks.isEmpty else { return 0 }
        pushUndo()
        // Bỏ zoom cũ để bấm nhiều lần không chồng chất lên nhau.
        segments.removeAll { $0.kind == .zoom }

        let lead = 0.45, tail = 1.6, gap = 1.2

        var clusters: [[RecordedClick]] = []
        for c in clicks.sorted(by: { $0.time < $1.time }) {
            if let prev = clusters.last?.last, c.time - prev.time <= gap {
                clusters[clusters.count - 1].append(c)
            } else {
                clusters.append([c])
            }
        }

        var added = 0
        for cluster in clusters {
            guard let first = cluster.first, let last = cluster.last else { continue }
            let start = max(0, first.time - lead)
            let end = min(duration, last.time + tail)
            guard end - start > 0.5 else { continue }

            let seg = VideoSegment(kind: .zoom, start: start, end: end)
            seg.center = CGPoint(x: cluster.map(\.point.x).reduce(0, +) / Double(cluster.count),
                                 y: cluster.map(\.point.y).reduce(0, +) / Double(cluster.count))
            seg.zoom = 2.0
            seg.fade = min(0.4, (end - start) / 4)
            // Không cho hai đoạn zoom đè nhau — đoạn trước bị cắt ngắn lại.
            if let prev = segments.last(where: { $0.kind == .zoom }), prev.end > seg.start {
                prev.end = max(prev.start + 0.4, seg.start - 0.05)
            }
            segments.append(seg)
            added += 1
        }
        selectedID = segments.last(where: { $0.kind == .zoom })?.id
        touch()
        return added
    }

    // MARK: - Điều khiển phát

    func togglePlay() {
        if player.rate > 0 {
            player.pause()
        } else {
            if player.currentTime().seconds >= timeMap.duration - 0.05 {
                player.seek(to: .zero)
            }
            player.play()
        }
        isPlaying = player.rate > 0
    }

    func step(by seconds: Double) {
        player.pause()
        isPlaying = false
        let t = player.currentTime().seconds + seconds
        player.seek(to: CMTime(seconds: min(max(t, 0), timeMap.duration), preferredTimescale: 600),
                    toleranceBefore: .zero, toleranceAfter: .zero)
    }

    func seek(source: Double) {
        playheadSource = source
        player.seek(to: CMTime(seconds: timeMap.compTime(source: source), preferredTimescale: 600),
                    toleranceBefore: .zero, toleranceAfter: .zero)
    }

    func goToStart() { seek(source: trimStart) }

    // MARK: - Xuất file

    func export(askWhere: Bool) {
        player.pause()
        isPlaying = false

        let name = VideoExportService.defaultFilename(ext: options.format.ext)
        var target = saveFolder.appendingPathComponent(name)
        if askWhere {
            let panel = NSSavePanel()
            panel.nameFieldStringValue = name
            panel.directoryURL = saveFolder
            panel.canCreateDirectories = true
            guard panel.runModal() == .OK, let url = panel.url else { return }
            target = url
        }

        exportProgress = 0
        Task { [weak self] in
            guard let self else { return }
            do {
                try await VideoExportService.export(
                    asset: self.asset, trimStart: self.trimStart, trimEnd: self.trimEnd,
                    segments: self.segments, audio: self.audio, options: self.options,
                    to: target,
                    onProgress: { [weak self] p in self?.exportProgress = p })
                self.exportProgress = nil
                self.onDone?(target)
                NSWorkspace.shared.activateFileViewerSelecting([target])
            } catch {
                self.exportProgress = nil
                self.exportError = error.localizedDescription
            }
        }
    }

    // MARK: - Định dạng thời gian

    /// "1:04.2" — có phần thập phân vì đoạn hiệu ứng thường ngắn hơn 1 giây.
    static func timecode(_ t: Double, decimals: Bool = false) -> String {
        guard t.isFinite, t >= 0 else { return decimals ? "0:00.0" : "0:00" }
        let s = Int(t)
        return decimals
            ? String(format: "%d:%02d.%d", s / 60, s % 60, Int((t - Double(s)) * 10))
            : String(format: "%d:%02d", s / 60, s % 60)
    }
}
