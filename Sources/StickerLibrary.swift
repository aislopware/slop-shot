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
//
// Sticker ĐỘNG: file gif/apng/webp động, hoặc sprite sheet ngang (kiểu Zalo).
// Xem FrameSequence trong AnimatedImage.swift.
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
    private var thumbs: [URL: NSImage] = [:]  // khỏi đọc lại đĩa mỗi lần cuộn lưới
    private var animated: [URL: Bool] = [:]   // có nhiều frame không (không giải nén frame nào)

    /// Bản nonisolated để Sticker Store gọi được từ luồng nền lúc giải nén.
    nonisolated static var rootURL: URL {
        FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("SlopShot", isDirectory: true)
            .appendingPathComponent("Stickers", isDirectory: true)
    }

    private init() {
        root = Self.rootURL
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        reload()
    }

    // ── Quét đĩa ───────────────────────────────────────────────────────────
    func reload() {
        // Nút ⟳ cũng dùng để "tôi vừa thay file ngoài Finder" → bỏ cache theo.
        thumbs.removeAll(); animated.removeAll()
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

    nonisolated static func isImage(_ url: URL) -> Bool {
        UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true
    }

    // ── Cài từ Sticker Store ───────────────────────────────────────────────
    // Ảnh ở đây đến từ file zip tải trên mạng về, nên tên file là dữ liệu LẠ:
    // chỉ lấy phần tên cuối và vứt mọi thành phần đường dẫn, để gói có bịa
    // "../../" cũng không ghi được ra ngoài thư mục bộ.
    //
    // nonisolated vì chạy trên luồng nền — copy cả trăm file mà chặn main thì
    // popover đứng hình.
    @discardableResult
    nonisolated static func install(_ files: [URL], asPackNamed rawName: String) throws -> Int {
        let fm = FileManager.default
        let name = safeFolderName(rawName)
        guard !name.isEmpty, !files.isEmpty else { return 0 }
        let dir = rootURL.appendingPathComponent(name, isDirectory: true)
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)

        // Gom sẵn những gì đang có, tra theo tên-không-đuôi: bản cũ của cùng con
        // sticker có thể là .png trong khi bản mới là .webp, không xoá thì bộ có
        // hai bản trùng nhau nằm cạnh nhau.
        var existing: [String: [URL]] = [:]
        for u in (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? [] {
            existing[u.deletingPathExtension().lastPathComponent, default: []].append(u)
        }

        var n = 0
        for f in files where isImage(f) {
            let file = f.lastPathComponent
            guard !file.hasPrefix("."), !file.contains("/"), !file.contains(":") else { continue }
            let stem = (file as NSString).deletingPathExtension
            for old in existing[stem] ?? [] { try? fm.removeItem(at: old) }
            existing[stem] = nil
            let dest = dir.appendingPathComponent(file)
            try? fm.removeItem(at: dest)
            try fm.copyItem(at: f, to: dest)
            n += 1
        }
        return n
    }

    private nonisolated static func safeFolderName(_ raw: String) -> String {
        raw.components(separatedBy: CharacterSet(charactersIn: "/:\\"))
            .joined(separator: "-")
            .replacingOccurrences(of: "..", with: ".")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // ── Đọc ảnh ────────────────────────────────────────────────────────────
    // Lưới sticker chỉ cần frame ĐẦU và thu nhỏ sẵn: bộ 110 sticker gif 360px mà
    // giữ nguyên cỡ trong cache là ngót 60MB RAM, thu về 160px còn ~11MB.
    func thumbnail(_ sticker: Sticker) -> NSImage? {
        if let img = thumbs[sticker.url] { return img }
        guard let img = FrameSequence.thumbnail(sticker.url) else { return nil }
        thumbs[sticker.url] = img
        return img
    }

    func isAnimated(_ sticker: Sticker) -> Bool {
        if let flag = animated[sticker.url] { return flag }
        let flag = FrameSequence.isAnimated(sticker.url)
        animated[sticker.url] = flag
        return flag
    }

    /// Toàn bộ frame ở kích thước gốc — chỉ gọi lúc thật sự chèn sticker vào ảnh.
    func sequence(_ sticker: Sticker) -> FrameSequence? {
        FrameSequence.load(sticker.url)
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
    var onPick: (FrameSequence, String) -> Void

    @ObservedObject private var lib = StickerLibrary.shared
    @ObservedObject private var store = StickerStore.shared
    @AppStorage("lastStickerPack") private var lastPack = ""
    @State private var dropTargeted = false
    @State private var showStore = false

    // Bộ đang xem: bộ đã chọn lần trước, không còn thì lấy bộ đầu tiên.
    private var pack: StickerPack? {
        lib.packs.first { $0.name == lastPack } ?? lib.packs.first
    }

    var body: some View {
        VStack(spacing: 0) {
            if showStore { storePane }
            else if lib.packs.isEmpty { emptyState }
            else { browser }
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
                        if let seq = lib.sequence(s) { onPick(seq, s.name) }
                    } label: {
                        Group {
                            if let img = lib.thumbnail(s) {
                                Image(nsImage: img).resizable().interpolation(.high)
                                    .aspectRatio(contentMode: .fit)
                            } else {
                                Image(systemName: "exclamationmark.triangle").foregroundStyle(.secondary)
                            }
                        }
                        .frame(width: 56, height: 56)
                        .padding(3)
                        .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
                        // Lưới chỉ vẽ frame đầu (cho nhẹ), nên phải có dấu báo con
                        // nào là sticker động — không thì nhìn y hệt ảnh tĩnh.
                        .overlay(alignment: .bottomTrailing) {
                            if lib.isAnimated(s) { animatedBadge }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help(lib.isAnimated(s) ? "\(s.name) · động" : s.name)
                }
            }
            .padding(10)
        }
        .frame(maxWidth: .infinity)
    }

    private var animatedBadge: some View {
        Text("GIF")
            .font(.system(size: 7, weight: .heavy))
            .foregroundStyle(.white)
            .padding(.horizontal, 3).padding(.vertical, 1)
            .background(Color.black.opacity(0.65), in: RoundedRectangle(cornerRadius: 3))
            .padding(4)
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
            if store.isConfigured {
                Button("Browse packs…") { showStore = true }
                    .controlSize(.small)
                    .padding(.top, 2)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // ── Store: tải bộ về thay vì nhét sẵn trong app ────────────────────────
    // App chỉ nặng vài MB; kho sticker cả trăm MB nằm trên R2, ai cần bộ nào thì
    // bấm tải bộ đó.
    private var storePane: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                ForEach(store.packs) { p in
                    storeRow(p)
                    Divider().padding(.leading, 62)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay {
            if store.loading && store.packs.isEmpty {
                ProgressView().controlSize(.small)
            } else if let err = store.loadError {
                VStack(spacing: 8) {
                    Image(systemName: "wifi.exclamationmark")
                        .font(.system(size: 26, weight: .light))
                        .foregroundStyle(.secondary)
                    Text(err).font(.system(size: 11)).foregroundStyle(.secondary)
                    Button("Try again") { Task { await store.refresh() } }.controlSize(.small)
                }
            } else if store.packs.isEmpty && !store.loading {
                Text("No packs published yet")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
            }
        }
        .task { await store.loadIfNeeded() }
    }

    private func storeRow(_ p: RemotePack) -> some View {
        let installed = lib.packs.contains { $0.name == p.name }
        return HStack(spacing: 10) {
            Group {
                if let img = store.cover(p) {
                    Image(nsImage: img).resizable().interpolation(.high).aspectRatio(contentMode: .fit)
                } else {
                    Image(systemName: "shippingbox").foregroundStyle(.tertiary)
                }
            }
            .frame(width: 40, height: 40)
            .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))

            VStack(alignment: .leading, spacing: 2) {
                Text(p.name).font(.system(size: 12, weight: .medium)).lineLimit(1)
                Text(p.subtitle).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 6)
            storeAction(p, installed: installed)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .task { await store.loadCover(p) }
    }

    @ViewBuilder
    private func storeAction(_ p: RemotePack, installed: Bool) -> some View {
        switch store.status[p.id] {
        case .downloading(let frac):
            // Có tổng dung lượng sẵn trong manifest nên hiện thanh thật, không
            // phải vòng xoay vô định.
            ProgressView(value: frac).progressViewStyle(.linear).frame(width: 64)
        case .installing:
            ProgressView().controlSize(.small)
        case .failed(let msg):
            Button("Retry") { Task { await store.download(p) } }
                .controlSize(.small).help(msg)
        default:
            if installed {
                Label("Installed", systemImage: "checkmark")
                    .font(.system(size: 10)).foregroundStyle(.secondary).labelStyle(.titleAndIcon)
            } else {
                Button("Get") { Task { await store.download(p) } }.controlSize(.small)
            }
        }
    }

    private var footer: some View {
        HStack(spacing: 8) {
            if showStore {
                Button { showStore = false } label: { Label("My packs", systemImage: "chevron.left") }
                Spacer()
                Button { Task { await store.refresh() } } label: { Image(systemName: "arrow.clockwise") }
                    .help("Check for new packs")
            } else {
                Button("Import pack…") { importPack() }
                Button("Open folder") { lib.revealInFinder() }
                if store.isConfigured && !lib.packs.isEmpty {
                    Button { showStore = true } label: {
                        Label("Get packs", systemImage: "arrow.down.circle")
                    }
                }
                Spacer()
                Button { lib.reload() } label: { Image(systemName: "arrow.clockwise") }
                    .help("Rescan the stickers folder")
            }
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
