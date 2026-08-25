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
    // Chữ đọc được TRONG ảnh chụp, OCR ngầm sau khi chụp — chỉ để tìm kiếm,
    // không hiện ra. Optional nên history.json cũ (chưa có khoá này) vẫn đọc được.
    var ocrText: String?

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

    /// File gốc còn nằm đó không. Ảnh nằm ở thư mục tạm nên macOS có thể dọn mất,
    /// trong khi thumbnail + chữ đã index thì vẫn còn → dòng đó vẫn tìm ra được,
    /// chỉ là hết Edit/Save.
    var fileExists: Bool {
        guard let fileURL else { return false }
        return FileManager.default.fileExists(atPath: fileURL.path)
    }

    // ── Tìm kiếm ───────────────────────────────────────────────────────────
    /// `q` đã lowercase sẵn. So khớp bỏ qua cả dấu — gõ "loi" vẫn ra "lỗi",
    /// vì gõ tiếng Việt có dấu chỉ để tìm một dòng log thì mệt.
    static let searchOptions: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]

    func matches(_ q: String) -> Bool {
        func has(_ s: String?) -> Bool {
            s?.range(of: q, options: Self.searchOptions) != nil
        }
        return has(title) || has(subtitle) || has(text) || has(ocrText)
    }

    /// Đoạn chữ quanh chỗ khớp, để hiện ngay dưới dòng kết quả.
    func snippet(for q: String, radius: Int = 34) -> String? {
        let hay = [text, ocrText].compactMap { $0 }.joined(separator: " · ")
        guard let r = hay.range(of: q, options: Self.searchOptions) else { return nil }
        let lo = hay.index(r.lowerBound, offsetBy: -radius, limitedBy: hay.startIndex) ?? hay.startIndex
        let hi = hay.index(r.upperBound, offsetBy: radius, limitedBy: hay.endIndex) ?? hay.endIndex
        let core = hay[lo..<hi].replacingOccurrences(of: "\n", with: " ")
        return (lo > hay.startIndex ? "…" : "") + core + (hi < hay.endIndex ? "…" : "")
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Store lịch sử: danh sách item + thumbnail. @Published để History UI tự cập nhật.
// Persist: history.json + thư mục Thumbnails trong Application Support/SlopShot.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class CaptureHistory: ObservableObject {
    static let shared = CaptureHistory()

    @Published private(set) var items: [HistoryItem] = []
    // Giữ nhiều hơn hẳn kể từ khi có tìm kiếm — lục 40 mục thì cuộn tay còn nhanh
    // hơn gõ. Chi phí đĩa vẫn nhẹ: mỗi mục chỉ là 1 thumbnail 240px + ít chữ.
    private let maxItems = 100
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
    // `image` là ảnh ĐẦY ĐỦ (không phải thumbnail): store tự thu nhỏ để lưu đĩa,
    // và OCR trên bản full-res để index — OCR ảnh 240px thì ra rác.
    func add(kind: HistoryItem.Kind, fileURL: URL?, text: String?,
             subtitle: String, image: NSImage?) {
        let item = HistoryItem(id: UUID(), kind: kind, date: Date(),
                               fileURL: fileURL, text: text, subtitle: subtitle)
        if let image { writeThumb(image, for: item) }
        items.insert(item, at: 0)        // mới nhất lên đầu
        trim()
        save()
        if kind == .image, let image { indexText(of: image, for: item.id) }
    }

    // OCR ngầm rồi gắn chữ vào mục → sau này tìm ảnh cũ bằng nội dung trong ảnh.
    // Chạy nền, xong lúc nào cập nhật lúc đó; người dùng không phải chờ.
    private func indexText(of image: NSImage, for id: UUID) {
        guard AppSettings.shared.indexCaptureText, let cg = ImageOps.cg(image) else { return }
        Task { [weak self] in
            let text = await TextRecognizer.recognize(in: cg)
            guard !text.isEmpty else { return }
            self?.attachOCR(text, to: id)
        }
    }

    private func attachOCR(_ text: String, to id: UUID) {
        guard let i = items.firstIndex(where: { $0.id == id }) else { return }  // đã bị trim/xoá
        items[i].ocrText = text
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
