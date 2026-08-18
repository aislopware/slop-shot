import SwiftUI
import AppKit
import CryptoKit

// ─────────────────────────────────────────────────────────────────────────
// Sticker Store: tải bộ sticker về theo yêu cầu thay vì nhét sẵn vào app.
//
// Kho nằm trên Cloudflare R2, chỉ là file tĩnh, không có server nào:
//
//   <base>/packs.json          ← manifest, app đọc đầu tiên
//   <base>/packs/<id>.zip      ← nguyên một bộ
//   <base>/covers/<id>.png     ← ảnh đại diện cho danh sách
//
// Dựng và đẩy lên bằng tools/publish_packs.swift.
//
// Đổi kho khác không cần build lại:
//   defaults write com.thanglb.slopshot stickerStoreURL "https://…"
// ─────────────────────────────────────────────────────────────────────────

struct RemotePack: Codable, Identifiable, Hashable {
    let id: String
    let name: String
    let stickers: Int
    let animated: Int
    let bytes: Int
    let sha256: String
    let zip: String
    let cover: String

    var sizeText: String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }

    var subtitle: String {
        let n = "\(stickers) sticker\(stickers == 1 ? "" : "s")"
        return animated > 0 ? "\(n) · \(animated) animated · \(sizeText)" : "\(n) · \(sizeText)"
    }
}

private struct Manifest: Codable {
    let schema: Int
    let packs: [RemotePack]
}

@MainActor
final class StickerStore: ObservableObject {
    static let shared = StickerStore()

    /// Đổi được lúc chạy qua `defaults write`, khỏi phải build lại chỉ để trỏ
    /// sang bucket khác (bucket thử, bản fork của người khác…).
    static let defaultBase = "https://pub-def5854c65c3430f9a38ca0585e79c1e.r2.dev"

    enum Status: Equatable {
        case idle
        case downloading(Double)      // 0…1
        case installing
        case failed(String)
    }

    @Published private(set) var packs: [RemotePack] = []
    @Published private(set) var loading = false
    @Published private(set) var loadError: String?
    @Published private(set) var status: [String: Status] = [:]

    private var covers: [String: NSImage] = [:]
    private var loadedOnce = false

    var base: URL? {
        let s = UserDefaults.standard.string(forKey: "stickerStoreURL") ?? Self.defaultBase
        return URL(string: s)
    }

    /// Kho đã được trỏ vào đâu chưa — bản build chưa điền URL thì ẩn hẳn Store
    /// đi, hơn là để người dùng bấm vào rồi ăn lỗi mạng khó hiểu.
    var isConfigured: Bool {
        guard let base else { return false }
        return !base.absoluteString.contains("REPLACE-ME") && base.scheme?.hasPrefix("http") == true
    }

    // ── Manifest ───────────────────────────────────────────────────────────
    func loadIfNeeded() async {
        guard !loadedOnce else { return }
        await refresh()
    }

    func refresh() async {
        guard let base, isConfigured else { return }
        loading = true; loadError = nil
        defer { loading = false; loadedOnce = true }
        do {
            // Manifest bé và hay đổi; bỏ qua cache để bấm ⟳ là thấy bộ mới ngay.
            var req = URLRequest(url: base.appendingPathComponent("packs.json"))
            req.cachePolicy = .reloadIgnoringLocalCacheData
            req.timeoutInterval = 20
            let (data, resp) = try await URLSession.shared.data(for: req)
            try check(resp)
            packs = try JSONDecoder().decode(Manifest.self, from: data).packs
        } catch {
            packs = []
            loadError = message(for: error)
        }
    }

    // ── Cover ──────────────────────────────────────────────────────────────
    func cover(_ pack: RemotePack) -> NSImage? { covers[pack.id] }

    func loadCover(_ pack: RemotePack) async {
        guard covers[pack.id] == nil, let base else { return }
        guard let (data, resp) = try? await URLSession.shared.data(
                from: base.appendingPathComponent(pack.cover)),
              (resp as? HTTPURLResponse)?.statusCode == 200,
              let img = NSImage(data: data) else { return }
        covers[pack.id] = img
        objectWillChange.send()
    }

    // ── Tải & cài ──────────────────────────────────────────────────────────
    func download(_ pack: RemotePack) async {
        guard let base, status[pack.id] == nil || status[pack.id] == .idle
                || isFailed(pack.id) else { return }
        status[pack.id] = .downloading(0)
        do {
            let url = base.appendingPathComponent(pack.zip)
            let progress = DownloadProgress { [weak self] p in
                Task { @MainActor in
                    // Chỉ nhích thanh khi đang tải; huỷ giữa chừng thì kệ.
                    if case .downloading = self?.status[pack.id] { self?.status[pack.id] = .downloading(p) }
                }
            }
            let (tmp, resp) = try await URLSession.shared.download(from: url, delegate: progress)
            try check(resp)

            status[pack.id] = .installing
            // Kiểm sha256 trước khi giải nén: gói hỏng giữa đường thì dừng ở đây
            // chứ đừng rải file rác vào kho sticker.
            let data = try Data(contentsOf: tmp, options: .mappedIfSafe)
            let sum = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            guard sum == pack.sha256 else { throw StoreError.corrupt }

            let n = try await Task.detached { try Self.unpack(tmp, as: pack.name) }.value
            guard n > 0 else { throw StoreError.empty }
            StickerLibrary.shared.reload()
            status[pack.id] = nil
        } catch {
            status[pack.id] = .failed(message(for: error))
        }
    }

    /// Giải nén ra thư mục tạm rồi mới đưa vào kho — chạy ngoài main actor vì
    /// ditto và copy file là I/O chặn.
    private nonisolated static func unpack(_ zip: URL, as name: String) throws -> Int {
        let fm = FileManager.default
        let tmp = fm.temporaryDirectory.appendingPathComponent("slopshot-pack-\(UUID().uuidString)")
        try fm.createDirectory(at: tmp, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: tmp) }

        // ditto có sẵn trên mọi máy macOS, khỏi kéo thêm thư viện zip.
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        p.arguments = ["-x", "-k", zip.path, tmp.path]
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        try p.run()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { throw StoreError.unzip }

        let files = (fm.enumerator(at: tmp, includingPropertiesForKeys: nil,
                                   options: [.skipsHiddenFiles])?
            .compactMap { $0 as? URL } ?? [])
            .filter { StickerLibrary.isImage($0) }
        return try StickerLibrary.install(files, asPackNamed: name)
    }

    func clearError(_ id: String) { status[id] = nil }

    func isFailed(_ id: String) -> Bool {
        if case .failed = status[id] { return true }
        return false
    }

    // ── Lỗi ────────────────────────────────────────────────────────────────
    enum StoreError: LocalizedError {
        case http(Int), corrupt, unzip, empty
        var errorDescription: String? {
            switch self {
            case .http(404): "Not found on the server"
            case .http(let c): "Server error \(c)"
            case .corrupt: "Download was corrupted"
            case .unzip: "Could not unpack the archive"
            case .empty: "The pack had no images in it"
            }
        }
    }

    private func check(_ resp: URLResponse) throws {
        guard let http = resp as? HTTPURLResponse else { return }
        guard (200..<300).contains(http.statusCode) else { throw StoreError.http(http.statusCode) }
    }

    private func message(for error: Error) -> String {
        if let e = error as? StoreError { return e.localizedDescription }
        if let e = error as? URLError, e.code == .notConnectedToInternet { return "No internet connection" }
        return (error as NSError).localizedDescription
    }
}

/// Chỉ để lấy phần trăm: bản `download(from:delegate:)` async không có cách nào
/// khác để biết đã tải được bao nhiêu.
private final class DownloadProgress: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    private let onProgress: (Double) -> Void
    init(_ onProgress: @escaping (Double) -> Void) { self.onProgress = onProgress }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                    didWriteData _: Int64, totalBytesWritten written: Int64,
                    totalBytesExpectedToWrite expected: Int64) {
        guard expected > 0 else { return }
        onProgress(min(Double(written) / Double(expected), 1))
    }

    // Bản async tự lo phần file tạm; delegate vẫn phải khai cho đủ giao thức.
    func urlSession(_: URLSession, downloadTask _: URLSessionDownloadTask,
                    didFinishDownloadingTo _: URL) {}
}
