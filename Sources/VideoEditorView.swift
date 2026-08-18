import SwiftUI
import AVKit
import AVFoundation

// ─────────────────────────────────────────────────────────────────────────
// VIDEO EDITOR — giao diện.
//
//   ┌ topBar: công cụ · Auto Zoom · Undo ······· Export As… · Export · Done ┐
//   ├─────────────────────────────────────────┬───────────────────────────┤
//   │ player + lớp phủ vẽ vùng                │ inspector: thuộc tính của │
//   ├─────────────────────────────────────────┤ đoạn đang chọn + âm thanh │
//   │ transport: ▶︎ 0:03 / 0:12                │                           │
//   ├─────────────────────────────────────────┴───────────────────────────┤
//   │ timeline: filmstrip + trim + 5 lane hiệu ứng            (NSView)     │
//   ├─────────────────────────────────────────────────────────────────────┤
//   │ bottomBar: Format · Quality · Resolution · Frame rate                │
//   └─────────────────────────────────────────────────────────────────────┘
//
// Cùng tông với EditorView.swift (nền 0.11, thanh 0.13, pill trắng 6%): cả hai
// đều là "cửa sổ" nên phải giống nhau. State nằm hết ở VideoEditStore.
//
// Ba mảnh vẫn là NSView bọc qua NSViewRepresentable, vì chúng cần vẽ tay /
// nhận chuột theo kiểu SwiftUI làm rất cực: player, lớp phủ vẽ vùng, timeline.
// ─────────────────────────────────────────────────────────────────────────

struct VideoEditorView: View {
    @ObservedObject var store: VideoEditStore
    var onClose: () -> Void

    private let barHeight: CGFloat = 52
    private let inspectorWidth: CGFloat = 250

    @State private var keyMonitor: Any?

    var body: some View {
        ZStack(alignment: .top) {
            VStack(spacing: 0) {
                Color.clear.frame(height: barHeight)
                HStack(spacing: 0) {
                    VStack(spacing: 0) {
                        stage
                        transportBar
                    }
                    inspector
                }
                timeline
                bottomBar
            }
            topBar
        }
        .frame(minWidth: 940, minHeight: 640)
        .background(Color(white: 0.11))
        .ignoresSafeArea()
        .task { await store.load() }
        .onAppear { installKeyMonitor() }
        .onDisappear { removeKeyMonitor(); store.tearDown() }
        .alert("Export failed", isPresented: Binding(
            get: { store.exportError != nil },
            set: { if !$0 { store.exportError = nil } })
        ) {
            Button("OK", role: .cancel) { store.exportError = nil }
        } message: {
            Text(store.exportError ?? "")
        }
    }

    // MARK: - Top bar
    // Vùng trống (Spacer) chính là vùng titlebar → vẫn kéo cửa sổ được ở đó.

    private var topBar: some View {
        HStack(spacing: 10) {
            toolPill
            autoZoomButton
            iconButton("arrow.uturn.backward", "Undo (⌘Z)") { store.undo() }
                .disabled(!store.canUndo)
            Spacer(minLength: 8)
            Button("Export As…") { store.export(askWhere: true) }
                .controlSize(.large)
                .buttonBorderShape(.roundedRectangle(radius: 4))
            Button("Export") { store.export(askWhere: false) }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .buttonBorderShape(.roundedRectangle(radius: 4))
            Button("Done") { onClose() }
                .controlSize(.large)
                .buttonBorderShape(.roundedRectangle(radius: 4))
                .keyboardShortcut(.defaultAction)
        }
        .padding(.horizontal, 14)
        .frame(height: barHeight)
        .frame(maxWidth: .infinity)
        .background(Color(white: 0.13))
        .overlay(alignment: .bottom) {
            Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
        }
        .disabled(store.exportProgress != nil)
    }

    /// 6 công cụ gom vào một pill, chia khối cho dễ nhìn: thời gian | pixel.
    private var toolPill: some View {
        HStack(spacing: 3) {
            toolButton(.cut); toolButton(.speed); toolButton(.freeze)
            Rectangle().fill(.white.opacity(0.12))
                .frame(width: 1, height: 22).padding(.horizontal, 2)
            toolButton(.zoom); toolButton(.censor); toolButton(.text)
        }
        .padding(5)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).stroke(.white.opacity(0.08), lineWidth: 1))
    }

    private func toolButton(_ kind: VideoEffectKind) -> some View {
        // Bấm là có ngay một đoạn tại playhead, chọn sẵn cho inspector mở luôn.
        // Nút không có trạng thái bật/tắt: một cú bấm ra đúng một đoạn.
        Button {
            store.drop(kind)
        } label: {
            HStack(spacing: 5) {
                Image(systemName: kind.icon).font(.system(size: 12, weight: .medium))
                Text(kind.label).font(.system(size: 11, weight: .medium))
            }
            .padding(.horizontal, 9)
            .frame(height: 28)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(Color(white: 0.86))
        .help(hint(for: kind))
    }

    private var autoZoomButton: some View {
        Button {
            if store.applyAutoZoom() == 0 { NSSound.beep() }
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "wand.and.stars").font(.system(size: 12, weight: .medium))
                Text("Auto Zoom").font(.system(size: 11, weight: .medium))
            }
            .foregroundStyle(Color(white: 0.85))
            .padding(.horizontal, 10)
            .frame(height: 30)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .disabled(store.clicks.isEmpty)
        .help(store.clicks.isEmpty
              ? "No clicks were recorded for this take"
              : "Turn the \(store.clicks.count) clicks recorded while shooting into zoom-ins")
    }

    private func iconButton(_ symbol: String, _ tip: String,
                            _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Color(white: 0.85))
                .frame(width: 30, height: 30)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 9))
        .help(tip)
    }

    private func hint(for kind: VideoEffectKind) -> String {
        switch kind {
        case .cut:    return "Drop a Cut at the playhead — the clip skips that stretch"
        case .speed:  return "Drop a Speed ramp at the playhead — slow it down or speed it up"
        case .freeze: return "Drop a Freeze at the playhead — hold that frame still"
        case .zoom:   return "Drop a Zoom at the playhead, then draw the region on the video"
        case .censor: return "Drop a Censor box at the playhead, then draw what to blur out"
        case .text:   return "Drop a caption at the playhead, then place it on the video"
        }
    }

    // MARK: - Sân khấu: player + lớp phủ vẽ vùng

    private var stage: some View {
        ZStack {
            Color.black
            PlayerLayer(player: store.player)
            VideoRegionOverlay(store: store)
            if let p = store.exportProgress {
                VStack(spacing: 12) {
                    Text("Exporting \(store.options.format.label)… \(Int(p * 100))%")
                        .font(.system(size: 13, weight: .medium))
                    ProgressView(value: p).frame(width: 260)
                }
                .padding(24)
                .background(.black.opacity(0.72), in: RoundedRectangle(cornerRadius: 12))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Transport

    private var transportBar: some View {
        HStack(spacing: 10) {
            Button { store.togglePlay() } label: {
                Image(systemName: store.isPlaying ? "pause.fill" : "play.fill")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(Color(white: 0.85))
                    .frame(width: 26, height: 26)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Play / Pause (Space)")

            Button { store.goToStart() } label: {
                Image(systemName: "backward.end.fill")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(Color(white: 0.85))
                    .frame(width: 26, height: 26)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Back to the start of the trim")

            Text("\(VideoEditStore.timecode(store.compTime)) / \(VideoEditStore.timecode(store.compDuration))")
                .font(.system(size: 11).monospacedDigit())
                .foregroundStyle(Color(white: 0.75))

            Spacer()

            Text("A tool drops in at the playhead — drag it on its lane to place it")
                .font(.caption)
                .foregroundStyle(Color(white: 0.5))
        }
        .padding(.horizontal, 14)
        .frame(height: 36)
        .background(Color(white: 0.13))
        .overlay(alignment: .top) {
            Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
        }
    }

    // MARK: - Timeline

    private var timeline: some View {
        VideoTimeline(store: store)
            .overlay(alignment: .top) {
                Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
            }
    }

    // MARK: - Inspector

    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let seg = store.selected {
                    segmentSection(seg)
                } else {
                    sectionTitle("Clip")
                    caption("Trim: \(VideoEditStore.timecode(store.trimStart, decimals: true)) → "
                            + VideoEditStore.timecode(store.trimEnd, decimals: true))
                    caption("Select a clip on the timeline to edit it, or pick a tool above and drag on its lane.")
                }

                if store.audioTrackCount > 0 {
                    Divider().overlay(Color.white.opacity(0.08))
                    audioSection
                }
                Spacer(minLength: 0)
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(width: inspectorWidth)
        .background(Color(white: 0.13))
        .overlay(alignment: .leading) {
            Rectangle().fill(.white.opacity(0.08)).frame(width: 1)
        }
    }

    @ViewBuilder
    private func segmentSection(_ seg: VideoSegment) -> some View {
        HStack(spacing: 6) {
            Image(systemName: seg.kind.icon).font(.system(size: 11, weight: .bold))
                .foregroundStyle(Color(nsColor: seg.kind.tint))
            sectionTitle(seg.kind.label)
        }
        caption("\(VideoEditStore.timecode(seg.start, decimals: true)) → "
                + "\(VideoEditStore.timecode(seg.end, decimals: true))  ("
                + String(format: "%.2fs", seg.duration) + ")")

        switch seg.kind {
        case .zoom:
            sliderRow("Zoom", value: bind(seg, \.zoom), in: 1.2...5) {
                String(format: "%.1f×", $0)
            }
            sliderRow("Ease in/out", value: bind(seg, \.fade), in: 0...1.2) {
                String(format: "%.2fs", $0)
            }
            caption("Drag the yellow box on the video to choose what to zoom into.")

        case .speed:
            Picker("", selection: bind(seg, \.speed, commit: true)) {
                ForEach([0.25, 0.5, 0.75, 2.0, 3.0, 5.0, 10.0], id: \.self) { f in
                    Text(f == f.rounded() ? "\(Int(f))×" : "\(f)×").tag(f)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            caption(String(format: "%.2fs of the clip plays in %.2fs.",
                           seg.duration, seg.duration / max(seg.speed, 0.01)))

        case .freeze:
            sliderRow("Hold", value: Binding(
                get: { seg.duration },
                set: { seg.end = seg.start + $0; store.touch() }), in: 0.2...6) {
                    String(format: "%.1fs", $0)
                }

        case .censor:
            Picker("", selection: bind(seg, \.censorStyle, commit: true)) {
                ForEach(CensorStyle.allCases, id: \.self) { Text($0.label).tag($0) }
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            sliderRow("Strength", value: bind(seg, \.strength), in: 0...1) {
                "\(Int($0 * 100))%"
            }
            caption("Drag the purple box on the video to cover what should stay private.")

        case .text:
            TextField("Caption", text: bind(seg, \.text))
                .textFieldStyle(.roundedBorder)
            sliderRow("Size", value: bind(seg, \.fontScale), in: 0.03...0.25) {
                "\(Int($0 * 100))%"
            }
            HStack(spacing: 12) {
                ColorPicker("", selection: Binding(
                    get: { Color(nsColor: seg.textColor) },
                    set: { seg.textColor = NSColor($0); store.touch() }))
                    .labelsHidden()
                Toggle("Shadow", isOn: bind(seg, \.hasShadow, commit: true))
                    .toggleStyle(.checkbox)
                    .font(.system(size: 11))
            }
            caption("Drag the green box on the video to place the caption.")

        case .cut:
            caption("These frames are dropped from the exported clip.")
        }

        Button(role: .destructive) {
            store.deleteSelected()
        } label: {
            Label("Delete", systemImage: "trash").font(.system(size: 11))
        }
        .controlSize(.small)
    }

    private var audioSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle("Audio")
            Toggle("Mute all", isOn: Binding(
                get: { store.audio.muted },
                set: { store.audio.muted = $0; store.touch() }))
                .toggleStyle(.checkbox)
                .font(.system(size: 11))
            if !store.audio.muted {
                ForEach(0..<store.audioTrackCount, id: \.self) { i in
                    sliderRow(store.audioTrackCount == 1 ? "Volume" : (i == 0 ? "System" : "Microphone"),
                              value: Binding(
                                get: { Double(store.audio.volumes.indices.contains(i) ? store.audio.volumes[i] : 1) },
                                set: {
                                    guard store.audio.volumes.indices.contains(i) else { return }
                                    store.audio.volumes[i] = Float($0)
                                    store.touch()
                                }), in: 0...1) { "\(Int($0 * 100))%" }
                }
            }
        }
    }

    // MARK: - Bottom bar (tuỳ chọn xuất file)

    private var bottomBar: some View {
        HStack(spacing: 16) {
            labeledPicker("Format", selection: Binding(
                get: { store.options.format },
                set: { f in
                    store.options.format = f
                    // GIF chỉ nhận fps thấp → kéo lựa chọn về khoảng hợp lệ.
                    let allowed = VideoExportOptions.FrameRate.choices(for: f)
                    if !allowed.contains(store.options.frameRate) {
                        store.options.frameRate = allowed[0]
                    }
                })) {
                ForEach(VideoExportOptions.Format.allCases, id: \.self) { Text($0.label).tag($0) }
            }
            if store.options.format != .gif {
                labeledPicker("Quality", selection: $store.options.quality) {
                    ForEach(VideoExportOptions.Quality.allCases, id: \.self) { Text($0.label).tag($0) }
                }
            }
            labeledPicker("Resolution", selection: $store.options.resolution) {
                ForEach(VideoExportOptions.Resolution.allCases, id: \.self) { Text($0.label).tag($0) }
            }
            labeledPicker("Frame rate", selection: $store.options.frameRate) {
                ForEach(VideoExportOptions.FrameRate.choices(for: store.options.format), id: \.self) {
                    Text($0.label).tag($0)
                }
            }
            Spacer()
            Text(summary)
                .font(.caption)
                .foregroundStyle(Color(white: 0.55))
        }
        .padding(.horizontal, 16)
        .frame(height: 52)
        .background(Color(white: 0.13))
        .overlay(alignment: .top) {
            Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
        }
        .disabled(store.exportProgress != nil)
    }

    private var summary: String {
        let n = store.segments.count
        let len = VideoEditStore.timecode(store.compDuration, decimals: true)
        return n == 0 ? "\(len) · no edits"
                      : "\(len) · \(n) edit\(n == 1 ? "" : "s")"
    }

    private func labeledPicker<S: Hashable, C: View>(_ title: String, selection: Binding<S>,
                                                     @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.system(size: 9)).foregroundStyle(Color(white: 0.5))
            Picker("", selection: selection, content: content)
                .labelsHidden()
                .pickerStyle(.menu)
                .frame(width: 104)
        }
    }

    // MARK: - Mấy view con dùng lại

    private func sectionTitle(_ s: String) -> some View {
        Text(s.uppercased())
            .font(.system(size: 10, weight: .bold))
            .foregroundStyle(Color(white: 0.55))
    }

    private func caption(_ s: String) -> some View {
        Text(s)
            .font(.system(size: 11))
            .foregroundStyle(Color(white: 0.68))
            .fixedSize(horizontal: false, vertical: true)
    }

    private func sliderRow(_ title: String, value: Binding<Double>,
                           in range: ClosedRange<Double>,
                           format: @escaping (Double) -> String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title).font(.system(size: 11)).foregroundStyle(Color(white: 0.75))
                Spacer()
                Text(format(value.wrappedValue))
                    .font(.system(size: 10).monospacedDigit())
                    .foregroundStyle(Color(white: 0.55))
            }
            // Bắt đầu kéo mới chụp undo — kéo cả quãng chỉ tính là MỘT bước undo.
            Slider(value: value, in: range) { started in
                if started { store.pushUndo() }
            }
            .controlSize(.small)
        }
    }

    /// Binding vào field của một `VideoSegment` (class) — set xong phải báo cho
    /// store biết, vì sửa field bên trong class không tự bắn objectWillChange.
    private func bind<T>(_ seg: VideoSegment,
                         _ keyPath: ReferenceWritableKeyPath<VideoSegment, T>,
                         commit: Bool = false) -> Binding<T> {
        Binding(
            get: { seg[keyPath: keyPath] },
            set: { new in
                if commit { store.pushUndo() }
                seg[keyPath: keyPath] = new
                store.touch()
            })
    }

    private func bind(_ seg: VideoSegment,
                      _ keyPath: ReferenceWritableKeyPath<VideoSegment, CGFloat>) -> Binding<Double> {
        Binding(
            get: { Double(seg[keyPath: keyPath]) },
            set: { seg[keyPath: keyPath] = CGFloat($0); store.touch() })
    }

    // MARK: - Phím tắt

    private func installKeyMonitor() {
        guard keyMonitor == nil else { return }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            // Chỉ móc mấy giá trị kiểu trị ra khỏi NSEvent — bản thân NSEvent
            // không Sendable nên không mang nó qua ranh giới actor được.
            let code = event.keyCode
            let cmd = event.modifierFlags.contains(.command)
            let chars = event.charactersIgnoringModifiers?.lowercased() ?? ""
            let handled = MainActor.assumeIsolated { handleKey(code: code, cmd: cmd, chars: chars) }
            return handled ? nil : event      // nuốt phím, hoặc trả lại cho hệ thống
        }
    }

    /// Trả về true nếu đã xử lý (nuốt phím).
    private func handleKey(code: UInt16, cmd: Bool, chars: String) -> Bool {
        // Đang gõ trong ô text thì trả phím lại cho ô đó.
        if NSApp.keyWindow?.firstResponder is NSTextView { return false }

        if cmd {
            guard chars == "z" else { return false }
            store.undo()
            return true
        }
        switch code {
        case 49: store.togglePlay()                     // Space
        case 123: store.step(by: -1.0 / 30)             // ←
        case 124: store.step(by: 1.0 / 30)              // →
        case 51, 117:                                   // ⌫ / ⌦
            guard store.selectedID != nil else { return false }
            store.deleteSelected()
        case 53: store.selectedID = nil                 // Esc
        default: return false
        }
        return true
    }

    private func removeKeyMonitor() {
        if let keyMonitor { NSEvent.removeMonitor(keyMonitor) }
        keyMonitor = nil
    }
}

// MARK: - Mảnh AppKit duy nhất còn lại

/// Trình phát. SwiftUI có `VideoPlayer` nhưng luôn kèm bộ điều khiển của hệ
/// thống và không tắt được — ở đây transport tự vẽ ở dưới nên cần một mặt phát
/// trần, chỉ AVPlayerView (`controlsStyle = .none`) mới cho.
private struct PlayerLayer: NSViewRepresentable {
    let player: AVPlayer

    func makeNSView(context: Context) -> AVPlayerView {
        let v = AVPlayerView()
        v.controlsStyle = .none
        v.videoGravity = .resizeAspect
        v.player = player
        return v
    }
    func updateNSView(_ v: AVPlayerView, context: Context) {
        if v.player !== player { v.player = player }
    }
}
