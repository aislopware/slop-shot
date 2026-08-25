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
    @State private var query = ""
    @FocusState private var searchFocused: Bool

    // Chuỗi tìm đã chuẩn hoá; rỗng = không lọc.
    private var normalized: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }
    private var shown: [HistoryItem] {
        let q = normalized
        return q.isEmpty ? history.items : history.items.filter { $0.matches(q) }
    }

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
            .padding(.top, 12)
            .padding(.bottom, 8)

            // Ô tìm kiếm: gõ chữ NẰM TRONG ảnh cũng ra, vì mỗi ảnh chụp đều được
            // OCR ngầm rồi index (xem CaptureHistory.indexText).
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                TextField("Search text inside captures…", text: $query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 12))
                    .focused($searchFocused)
                if !query.isEmpty {
                    Button { query = "" } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 12))
                            .foregroundStyle(.tertiary)
                    }
                    .buttonStyle(.borderless)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(Color(nsColor: .controlBackgroundColor),
                        in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6)
                        .stroke(Color(nsColor: .separatorColor), lineWidth: 1))
            .padding(.horizontal, 16)
            .padding(.bottom, 10)
            // ⎋ để xoá ô tìm thay vì đóng cửa sổ.
            .onExitCommand { query = "" }

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
            } else if shown.isEmpty {
                // Có lịch sử nhưng không khớp từ khoá.
                VStack(spacing: 8) {
                    Image(systemName: "text.magnifyingglass")
                        .font(.system(size: 30))
                        .foregroundStyle(.tertiary)
                    Text("No captures match “\(query)”")
                        .foregroundStyle(.secondary)
                    Text("Search looks at the text SlopShot read inside each screenshot.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 260)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(shown) { item in
                            HistoryRow(item: item,
                                       thumbnail: history.thumbnail(for: item),
                                       snippet: normalized.isEmpty
                                                ? nil : item.snippet(for: normalized),
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
    let snippet: String?          // đoạn chữ khớp từ khoá (nil khi không tìm kiếm)
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
                    Image(systemName: item.kind == .text ? "text.alignleft"
                                    : item.kind == .color ? "eyedropper" : "photo")
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
                // Khớp ở chữ trong ảnh → cho thấy khớp chỗ nào, không thì kết quả
                // trông như trả về bừa.
                if let snippet {
                    Text(snippet)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }

            Spacer()

            // Nút thao tác (hiện rõ khi hover cho gọn).
            // File ảnh nằm ở thư mục tạm; macOS dọn mất thì chỉ còn thumbnail +
            // chữ đã index — vẫn tìm/xoá được, nhưng Save/Edit thì hết cửa.
            HStack(spacing: 6) {
                iconButton("doc.on.doc", "Copy") { actions.copy(item) }
                if item.kind != .text, item.kind != .color, item.fileExists {
                    iconButton("square.and.arrow.down", "Save as…") { actions.saveAs(item) }
                }
                switch item.kind {
                case .image where item.fileExists:
                    iconButton("pencil.tip.crop.circle", "Edit") { actions.edit(item) }
                case .video where item.fileExists:
                    iconButton("play.fill", "Play") { actions.open(item) }
                case .text:
                    iconButton("character.bubble", "Open & translate") { actions.edit(item) }
                default:
                    EmptyView()   // màu: chép mã là xong; file mất: không mở được nữa
                }
                iconButton("trash", "Delete") { actions.delete(item) }
            }
            // 0.55 lúc không hover trông y hệt nút bị disable — nhạt vừa đủ để
            // dòng đang trỏ nổi lên là được, đừng nhạt tới mức tưởng bấm không ăn.
            .opacity(hovering ? 1 : 0.85)
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
