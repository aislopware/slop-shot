import SwiftUI
import AppKit
import UniformTypeIdentifiers

// ─────────────────────────────────────────────────────────────────────────
// Thư viện sticker. Quy ước RẤT đơn giản, không cần file cấu hình:
//
//   ~/Library/Application Support/SlopShot/Stickers/
//       Ami bụng bự/      ← mỗi THƯ MỤC CON là 1 bộ sticker
//           01.png        ← mỗi FILE ẢNH là 1 sticker (png/webp/gif/jpg…)
//           02.png
//       Meme/
//           ...
//
// Muốn thêm bộ mới: kéo cả thư mục ảnh vào (nút Import) hoặc copy tay vào đây.
// Tên thư mục = tên bộ hiện trên UI, tên file = tooltip của từng sticker.
// ─────────────────────────────────────────────────────────────────────────
struct Sticker: Identifiable, Hashable {
    let url: URL
    var id: URL { url }
    var name: String { url.deletingPathExtension().lastPathComponent }
}

struct StickerPack: Identifiable, Hashable {
    let dir: URL
    let stickers: [Sticker]
    var id: URL { dir }
    var name: String { dir.lastPathComponent }
}

@MainActor
final class StickerLibrary: ObservableObject {
    static let shared = StickerLibrary()

    @Published private(set) var packs: [StickerPack] = []

    let root: URL
    private var cache: [URL: NSImage] = [:]   // khỏi đọc lại đĩa mỗi lần cuộn lưới

    private init() {
        root = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("SlopShot", isDirectory: true)
            .appendingPathComponent("Stickers", isDirectory: true)
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        reload()
    }

    // ── Quét đĩa ───────────────────────────────────────────────────────────
    func reload() {
        let fm = FileManager.default
        let entries = (try? fm.contentsOfDirectory(at: root,
                                                   includingPropertiesForKeys: [.isDirectoryKey],
                                                   options: [.skipsHiddenFiles])) ?? []
        packs = entries
            .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
            .map { StickerPack(dir: $0, stickers: Self.images(in: $0)) }
            .filter { !$0.stickers.isEmpty }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    private static func images(in dir: URL) -> [Sticker] {
        let urls = (try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles])) ?? []
        return urls
            .filter { isImage($0) }
            // localizedStandard: "10.png" xếp sau "9.png" chứ không phải trước.
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
            .map(Sticker.init)
    }

    static func isImage(_ url: URL) -> Bool {
        UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true
    }

    // ── Đọc ảnh (có cache) ─────────────────────────────────────────────────
    func image(_ sticker: Sticker) -> NSImage? {
        if let img = cache[sticker.url] { return img }
        guard let img = Self.firstFrame(of: sticker.url) else { return nil }
        cache[sticker.url] = img
        return img
    }

    // Lấy 1 hình TĨNH từ file sticker. Hai trường hợp phải gỡ:
    //  1. Ảnh nhiều frame (gif/webp động) → lấy frame 0 qua CGImageSource.
    //  2. SPRITE SHEET ngang: rất nhiều bộ sticker (Zalo chẳng hạn) đóng gói cả
    //     animation vào 1 ảnh, các frame xếp cạnh nhau — ví dụ 1560×130 = 12 frame
    //     130×130. Vẽ nguyên ảnh sẽ ra vệt kẻ sọc, nên cắt lấy ô vuông đầu tiên.
    //     Chỉ cắt khi ngang chia hết cho cao và ≥3 lần, để không đụng ảnh
    //     panorama/banner bình thường.
    static func firstFrame(of url: URL) -> NSImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(src) > 0,
              var cg = CGImageSourceCreateImageAtIndex(src, 0, nil)
        else { return NSImage(contentsOf: url) }   // định dạng lạ → để NSImage tự lo
        let w = cg.width, h = cg.height
        if h > 0, w >= h * 3, w % h == 0,
           let first = cg.cropping(to: CGRect(x: 0, y: 0, width: h, height: h)) {
            cg = first
        }
        return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
    }

    // ── Import ─────────────────────────────────────────────────────────────
    // Copy 1 thư mục ảnh vào kho → thành 1 bộ mới. Trùng tên thì thêm " 2", " 3"…
    @discardableResult
    func importPack(from folder: URL) -> String? {
        let stickers = Self.images(in: folder)
        guard !stickers.isEmpty else { return nil }
        let dest = uniqueDir(named: folder.lastPathComponent)
        try? FileManager.default.createDirectory(at: dest, withIntermediateDirectories: true)
        for s in stickers { copy(s.url, into: dest) }
        reload()
        return dest.lastPathComponent
    }

    // Copy 1 loạt file ảnh rời vào 1 bộ (tạo mới nếu chưa có).
    @discardableResult
    func addImages(_ urls: [URL], toPackNamed name: String) -> String? {
        let files = urls.filter(Self.isImage)
        guard !files.isEmpty else { return nil }
        let dest = root.appendingPathComponent(name, isDirectory: true)
        try? FileManager.default.createDirectory(at: dest, withIntermediateDirectories: true)
        for f in files { copy(f, into: dest) }
        reload()
        return dest.lastPathComponent
    }

    // Kéo-thả hỗn hợp (vừa thư mục vừa file) → thư mục thành bộ riêng, file rời
    // gom vào bộ đang mở.
    func absorb(_ urls: [URL], currentPack: String?) -> String? {
        var last: String?
        var loose: [URL] = []
        for url in urls {
            if (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
                if let name = importPack(from: url) { last = name }
            } else if Self.isImage(url) {
                loose.append(url)
            }
        }
        if !loose.isEmpty {
            let target = currentPack ?? "My stickers"
            if let name = addImages(loose, toPackNamed: target) { last = name }
        }
        return last
    }

    func revealInFinder() {
        NSWorkspace.shared.activateFileViewerSelecting([root])
    }

    // ── Helper ─────────────────────────────────────────────────────────────
    private func uniqueDir(named raw: String) -> URL {
        let base = raw.replacingOccurrences(of: "/", with: "-")
        var name = base, n = 2
        while FileManager.default.fileExists(atPath: root.appendingPathComponent(name).path) {
            name = "\(base) \(n)"; n += 1
        }
        return root.appendingPathComponent(name, isDirectory: true)
    }

    // Copy giữ nguyên tên; trùng tên file thì thêm hậu tố số.
    private func copy(_ src: URL, into dir: URL) {
        let fm = FileManager.default
        let ext = src.pathExtension
        let stem = src.deletingPathExtension().lastPathComponent
        var dest = dir.appendingPathComponent(src.lastPathComponent)
        var n = 2
        while fm.fileExists(atPath: dest.path) {
            dest = dir.appendingPathComponent("\(stem) \(n).\(ext)"); n += 1
        }
        try? fm.copyItem(at: src, to: dest)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Popover chọn sticker: cột trái danh sách bộ, phải là lưới sticker.
// ─────────────────────────────────────────────────────────────────────────
struct StickerPicker: View {
    var onPick: (NSImage, String) -> Void

    @ObservedObject private var lib = StickerLibrary.shared
    @AppStorage("lastStickerPack") private var lastPack = ""
    @State private var dropTargeted = false

    // Bộ đang xem: bộ đã chọn lần trước, không còn thì lấy bộ đầu tiên.
    private var pack: StickerPack? {
        lib.packs.first { $0.name == lastPack } ?? lib.packs.first
    }

    var body: some View {
        VStack(spacing: 0) {
            if lib.packs.isEmpty { emptyState } else { browser }
            Divider()
            footer
        }
        .frame(width: 470, height: 330)
        .dropDestination(for: URL.self) { urls, _ in
            if let name = lib.absorb(urls, currentPack: pack?.name) { lastPack = name }
            return true
        } isTargeted: { dropTargeted = $0 }
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(Color.accentColor, lineWidth: 2).padding(2)
            }
        }
    }

    // ── Cột bộ sticker | lưới sticker ──────────────────────────────────────
    private var browser: some View {
        HStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(lib.packs) { p in packRow(p) }
                }
                .padding(6)
            }
            .frame(width: 132)
            .background(Color.primary.opacity(0.04))

            Divider()

            if let p = pack { grid(p) } else { Spacer() }
        }
    }

    private func packRow(_ p: StickerPack) -> some View {
        let active = p.id == pack?.id
        return Button { lastPack = p.name } label: {
            VStack(alignment: .leading, spacing: 1) {
                Text(p.name).font(.system(size: 12, weight: active ? .semibold : .regular))
                    .lineLimit(1)
                Text("\(p.stickers.count)").font(.system(size: 10)).opacity(0.6)
            }
            .foregroundStyle(active ? Color.white : Color.primary)
            .padding(.horizontal, 8).padding(.vertical, 5)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 6)
                            .fill(active ? Color.accentColor : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func grid(_ p: StickerPack) -> some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 62), spacing: 8)], spacing: 8) {
                ForEach(p.stickers) { s in
                    Button {
                        if let img = lib.image(s) { onPick(img, s.name) }
                    } label: {
                        Group {
                            if let img = lib.image(s) {
                                Image(nsImage: img).resizable().interpolation(.high)
                                    .aspectRatio(contentMode: .fit)
                            } else {
                                Image(systemName: "exclamationmark.triangle").foregroundStyle(.secondary)
                            }
                        }
                        .frame(width: 56, height: 56)
                        .padding(3)
                        .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help(s.name)
                }
            }
            .padding(10)
        }
        .frame(maxWidth: .infinity)
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "face.dashed")
                .font(.system(size: 30, weight: .light))
                .foregroundStyle(.secondary)
            Text("No sticker packs yet")
                .font(.system(size: 13, weight: .semibold))
            Text("Drop a folder of images here, or use Import — each folder becomes one pack.")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 300)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var footer: some View {
        HStack(spacing: 8) {
            Button("Import pack…") { importPack() }
            Button("Open folder") { lib.revealInFinder() }
            Spacer()
            Button { lib.reload() } label: { Image(systemName: "arrow.clockwise") }
                .help("Rescan the stickers folder")
        }
        .controlSize(.small)
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
    }

    private func importPack() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = true
        panel.prompt = "Import"
        panel.message = "Choose folders (each becomes a pack) or image files."
        guard panel.runModal() == .OK else { return }
        if let name = lib.absorb(panel.urls, currentPack: pack?.name) { lastPack = name }
    }
}
