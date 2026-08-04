import SwiftUI

// Bộ callback cho từng nút trên 1 dòng lịch sử (logic thật nằm ở ScreenCapturer).
struct HistoryActions {
    var copy:   (HistoryItem) -> Void
    var saveAs: (HistoryItem) -> Void
    var edit:   (HistoryItem) -> Void
    var open:   (HistoryItem) -> Void
    var delete: (HistoryItem) -> Void
    var openSettings: () -> Void
}

// ─────────────────────────────────────────────────────────────────────────
// Cửa sổ "Capture History": list dọc, mỗi dòng 1 capture (thumbnail + nút).
// NSWindowController bọc 1 NSHostingController để nhúng SwiftUI vào AppKit.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class HistoryWindowController {
    private var window: NSWindow?

    func show(history: CaptureHistory, actions: HistoryActions) {
        if let window {                       // đã mở rồi → đưa lên trước
            NSApp.activate(ignoringOtherApps: true)
            window.makeKeyAndOrderFront(nil)
            return
        }
        let view = HistoryView(history: history, actions: actions)
        let host = NSHostingController(rootView: view)

        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 420, height: 520),
                           styleMask: [.titled, .closable, .miniaturizable, .resizable],
                           backing: .buffered, defer: false)
        win.title = "Capture History"
        win.titlebarAppearsTransparent = true
        win.contentViewController = host
        win.isReleasedWhenClosed = false
        win.center()
        win.minSize = NSSize(width: 360, height: 320)

        // Khi đóng cửa sổ thì xoá ref để lần sau mở lại sạch sẽ.
        NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: win, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.window = nil }
        }

        NSApp.activate(ignoringOtherApps: true)
        win.makeKeyAndOrderFront(nil)
        self.window = win
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Nội dung SwiftUI của cửa sổ lịch sử.
// ─────────────────────────────────────────────────────────────────────────
private struct HistoryView: View {
    @ObservedObject var history: CaptureHistory
    let actions: HistoryActions

    var body: some View {
        VStack(spacing: 0) {
            // Header: tiêu đề + Settings + Clear all.
            HStack {
                Text("Recent captures")
                    .font(.headline)
                Spacer()
                Button { actions.openSettings() } label: {
                    Image(systemName: "gearshape")
                }
                .buttonStyle(.borderless)
                .help("Settings")

                Button { history.clear() } label: {
                    Image(systemName: "trash")
                }
                .buttonStyle(.borderless)
                .help("Clear all")
                .disabled(history.items.isEmpty)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)

            Divider()

            if history.items.isEmpty {
                // Trạng thái rỗng.
                VStack(spacing: 8) {
                    Image(systemName: "clock.arrow.circlepath")
                        .font(.system(size: 34))
                        .foregroundStyle(.tertiary)
                    Text("No captures yet")
                        .foregroundStyle(.secondary)
                    Text("Your screenshots, recordings and OCR text will show up here.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 260)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(history.items) { item in
                            HistoryRow(item: item,
                                       thumbnail: history.thumbnail(for: item),
                                       actions: actions)
                            Divider().padding(.leading, 84)
                        }
                    }
                }
            }
        }
        .frame(minWidth: 360, minHeight: 320)
    }
}

// 1 dòng lịch sử.
private struct HistoryRow: View {
    let item: HistoryItem
    let thumbnail: NSImage?
    let actions: HistoryActions
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 12) {
            // Thumbnail (hoặc icon thay thế cho text).
            ZStack {
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color(nsColor: .controlBackgroundColor))
                if let thumbnail {
                    Image(nsImage: thumbnail)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .frame(width: 56, height: 40)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                } else {
                    Image(systemName: item.kind == .text ? "text.alignleft" : "photo")
                        .foregroundStyle(.secondary)
                }
                // Badge ▶ cho video.
                if item.kind == .video {
                    Image(systemName: "play.circle.fill")
                        .foregroundStyle(.white)
                        .shadow(radius: 1)
                }
            }
            .frame(width: 56, height: 40)

            // Tiêu đề + phụ đề.
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).font(.system(size: 13, weight: .semibold))
                Text("\(item.subtitle) · \(Self.relative(item.date))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            // Nút thao tác (hiện rõ khi hover cho gọn).
            HStack(spacing: 6) {
                iconButton("doc.on.doc", "Copy") { actions.copy(item) }
                if item.kind != .text {
                    iconButton("square.and.arrow.down", "Save as…") { actions.saveAs(item) }
                }
                switch item.kind {
                case .image: iconButton("pencil.tip.crop.circle", "Edit") { actions.edit(item) }
                case .video: iconButton("play.fill", "Play") { actions.open(item) }
                case .text:  iconButton("character.bubble", "Open & translate") { actions.edit(item) }
                }
                iconButton("trash", "Delete") { actions.delete(item) }
            }
            .opacity(hovering ? 1 : 0.55)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(hovering ? Color.primary.opacity(0.06) : .clear)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
    }

    private func iconButton(_ symbol: String, _ help: String,
                            _ action: @escaping () -> Void) -> some View {
        Button(action: action) { Image(systemName: symbol) }
            .buttonStyle(.borderless)
            .help(help)
    }

    // "just now" / "2m ago" / "yesterday"…
    private static func relative(_ date: Date) -> String {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .abbreviated
        return f.localizedString(for: date, relativeTo: Date())
    }
}
