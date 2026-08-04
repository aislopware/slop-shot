import SwiftUI

// 1 mục trong lịch sử. Codable để ghi JSON; thumbnail lưu riêng ra file PNG nhỏ.
struct HistoryItem: Identifiable, Codable {
    enum Kind: String, Codable { case image, video, text, color }
    let id: UUID
    let kind: Kind
    let date: Date
    var fileURL: URL?      // ảnh/video (nil nếu là text)
    var text: String?      // nội dung OCR (chỉ với kind == .text)
    var subtitle: String   // "1440×900" / "0:12" / "142 chars"

    // Tên hiển thị theo loại.
    var title: String {
        switch kind {
        case .image: return "Screenshot"
        case .video: return "Recording"
        case .text:  return "Text (OCR)"
        case .color: return "Color"
        }
    }
    var thumbFileName: String { id.uuidString + ".png" }
}

// ─────────────────────────────────────────────────────────────────────────
// Store lịch sử: danh sách item + thumbnail. @Published để History UI tự cập nhật.
// Persist: history.json + thư mục Thumbnails trong Application Support/SlopShot.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class CaptureHistory: ObservableObject {
    static let shared = CaptureHistory()

    @Published private(set) var items: [HistoryItem] = []
    private let maxItems = 40
    private var thumbCache: [UUID: NSImage] = [:]   // cache để khỏi đọc đĩa liên tục

    private let dir: URL
    private let thumbsDir: URL
    private var jsonURL: URL { dir.appendingPathComponent("history.json") }

    private init() {
        let base = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("SlopShot", isDirectory: true)
        dir = base
        thumbsDir = base.appendingPathComponent("Thumbnails", isDirectory: true)
        try? FileManager.default.createDirectory(at: thumbsDir, withIntermediateDirectories: true)
        load()
    }

    // ── Thêm 1 mục mới (gọi sau mỗi lần chụp/quay/OCR) ─────────────────────
    func add(kind: HistoryItem.Kind, fileURL: URL?, text: String?,
             subtitle: String, thumbnail: NSImage?) {
        let item = HistoryItem(id: UUID(), kind: kind, date: Date(),
                               fileURL: fileURL, text: text, subtitle: subtitle)
        if let thumbnail { writeThumb(thumbnail, for: item) }
        items.insert(item, at: 0)        // mới nhất lên đầu
        trim()
        save()
    }

    func remove(_ item: HistoryItem) {
        items.removeAll { $0.id == item.id }
        try? FileManager.default.removeItem(at: thumbsDir.appendingPathComponent(item.thumbFileName))
        thumbCache[item.id] = nil
        save()
    }

    func clear() {
        for item in items {
            try? FileManager.default.removeItem(at: thumbsDir.appendingPathComponent(item.thumbFileName))
        }
        items.removeAll()
        thumbCache.removeAll()
        save()
    }

    // Lấy thumbnail (đọc đĩa 1 lần rồi cache).
    func thumbnail(for item: HistoryItem) -> NSImage? {
        if let img = thumbCache[item.id] { return img }
        let url = thumbsDir.appendingPathComponent(item.thumbFileName)
        guard let img = NSImage(contentsOf: url) else { return nil }
        thumbCache[item.id] = img
        return img
    }

    // ── Riêng tư ────────────────────────────────────────────────────────────
    private func trim() {
        guard items.count > maxItems else { return }
        for item in items[maxItems...] {
            try? FileManager.default.removeItem(at: thumbsDir.appendingPathComponent(item.thumbFileName))
            thumbCache[item.id] = nil
        }
        items = Array(items.prefix(maxItems))
    }

    // Thu nhỏ ảnh xuống tối đa 240px cạnh dài rồi ghi PNG (cho nhẹ đĩa).
    private func writeThumb(_ image: NSImage, for item: HistoryItem) {
        let maxSide: CGFloat = 240
        let s = image.size
        guard s.width > 0, s.height > 0 else { return }
        let scale = min(1, maxSide / max(s.width, s.height))
        let target = NSSize(width: max(s.width * scale, 1), height: max(s.height * scale, 1))

        let resized = NSImage(size: target)
        resized.lockFocus()
        image.draw(in: NSRect(origin: .zero, size: target),
                   from: NSRect(origin: .zero, size: s),
                   operation: .copy, fraction: 1)
        resized.unlockFocus()

        guard let tiff = resized.tiffRepresentation,
              let rep = NSBitmapImageRep(data: tiff),
              let png = rep.representation(using: .png, properties: [:]) else { return }
        try? png.write(to: thumbsDir.appendingPathComponent(item.thumbFileName))
        thumbCache[item.id] = resized
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(items) else { return }
        try? data.write(to: jsonURL)
    }

    private func load() {
        guard let data = try? Data(contentsOf: jsonURL),
              let saved = try? JSONDecoder().decode([HistoryItem].self, from: data) else { return }
        items = saved
    }
}
