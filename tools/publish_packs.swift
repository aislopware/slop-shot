// Đóng gói kho sticker thành artifact cho Sticker Store trên R2.
//
//   xcrun swift tools/publish_packs.swift                      # dựng vào .work/store/
//   xcrun swift tools/publish_packs.swift "Ami Bụng Bự"        # chỉ một bộ
//   xcrun swift tools/publish_packs.swift --upload r2:slopshot-stickers
//
// Sinh ra:
//   .work/store/packs.json              ← manifest app tải về đầu tiên
//   .work/store/packs/<id>.zip          ← cả bộ, nén store (ảnh webp nén sẵn rồi)
//   .work/store/covers/<id>.png         ← ảnh đại diện 192px cho danh sách trong app
//
// Vì sao zip chứ không liệt kê từng file: một bộ trăm con là trăm request, mà
// app còn phải tự biết khi nào tải xong. Một file zip + sha256 thì tải một phát,
// kiểm được toàn vẹn, và hỏng giữa chừng thì bỏ nguyên gói chứ không để lại bộ
// sticker thiếu một nửa.
//
// --upload cần rclone đã cấu hình remote kiểu R2:
//   rclone config create r2 s3 provider=Cloudflare \
//       access_key_id=… secret_access_key=… \
//       endpoint=https://<account_id>.r2.cloudflarestorage.com
//
// Chạy tools/webp_stickers.swift TRƯỚC tool này — không thì đẩy lên cả trăm MB
// PNG.

import Foundation
import AppKit
import ImageIO
import CryptoKit
import UniformTypeIdentifiers

let fm = FileManager.default
let stickersRoot = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
    .appendingPathComponent("SlopShot/Stickers", isDirectory: true)
let outRoot = URL(fileURLWithPath: fm.currentDirectoryPath)
    .appendingPathComponent(".work/store", isDirectory: true)

// ── Tham số ──────────────────────────────────────────────────────────────
let (uploadTo, packNames): (String?, [String]) = {
    var dest: String?, names: [String] = []
    var argv = Array(CommandLine.arguments.dropFirst())
    while let a = argv.first {
        argv.removeFirst()
        if a == "--upload" { dest = argv.first; if !argv.isEmpty { argv.removeFirst() } }
        else { names.append(a) }
    }
    return (dest, names)
}()

// ── Helper ───────────────────────────────────────────────────────────────
func which(_ name: String) -> String? {
    for dir in ["/usr/bin", "/opt/homebrew/bin", "/usr/local/bin", "/bin"] {
        let p = "\(dir)/\(name)"
        if fm.isExecutableFile(atPath: p) { return p }
    }
    return nil
}

@discardableResult
func run(_ tool: String, _ argv: [String], cwd: URL? = nil) -> Bool {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: tool)
    p.arguments = argv
    if let cwd { p.currentDirectoryURL = cwd }
    p.standardOutput = FileHandle.nullDevice
    p.standardError = FileHandle.nullDevice
    do { try p.run() } catch { return false }
    p.waitUntilExit()
    return p.terminationStatus == 0
}

// Tên bộ tiếng Việt → khoá object ASCII. Bỏ dấu rồi thay đ/Đ bằng tay: bộ gấp
// Unicode không tách được nét ngang của đ nên .diacriticInsensitive để nguyên.
func slug(_ s: String) -> String {
    let folded = s
        .replacingOccurrences(of: "đ", with: "d")
        .replacingOccurrences(of: "Đ", with: "D")
        .folding(options: [.diacriticInsensitive, .caseInsensitive], locale: Locale(identifier: "vi"))
    var out = ""
    for ch in folded {
        if ch.isLetter || ch.isNumber { out.append(ch) }
        else if !out.hasSuffix("-") { out.append("-") }
    }
    return out.trimmingCharacters(in: CharacterSet(charactersIn: "-"))
}

func bytes(of url: URL) -> Int {
    ((try? fm.attributesOfItem(atPath: url.path)[.size]) as? Int) ?? 0
}

func mb(_ n: Int) -> String { String(format: "%.1f MB", Double(n) / 1_048_576) }

/// Cover: frame đầu của sticker đầu tiên, thu về 192px. Danh sách trong app chỉ
/// vẽ nó ở 44px nên không cần lớn, nhưng để 192 cho màn Retina và cho sau này.
func makeCover(from sticker: URL, to dest: URL) -> Bool {
    guard let src = CGImageSourceCreateWithURL(sticker as CFURL, nil),
          CGImageSourceGetCount(src) > 0,
          var cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return false }
    // Sprite sheet ngang: lấy ô đầu, không thì cover là một dải dài ngoẵng.
    let w = cg.width, h = cg.height
    if h > 0, w >= h * 3, w % h == 0, let cell = cg.cropping(to: CGRect(x: 0, y: 0, width: h, height: h)) {
        cg = cell
    }
    let edge = 192.0
    let scale = min(edge / Double(cg.width), edge / Double(cg.height), 1)
    let nw = Int((Double(cg.width) * scale).rounded()), nh = Int((Double(cg.height) * scale).rounded())
    guard let ctx = CGContext(data: nil, width: nw, height: nh, bitsPerComponent: 8, bytesPerRow: 0,
                              space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
    ctx.interpolationQuality = .high
    ctx.draw(cg, in: CGRect(x: 0, y: 0, width: nw, height: nh))
    guard let out = ctx.makeImage(),
          let d = CGImageDestinationCreateWithURL(dest as CFURL, UTType.png.identifier as CFString, 1, nil)
    else { return false }
    CGImageDestinationAddImage(d, out, nil)
    return CGImageDestinationFinalize(d)
}

func isAnimated(_ url: URL) -> Bool {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return false }
    if CGImageSourceGetCount(src) > 1 { return true }
    guard let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return false }
    let w = cg.width, h = cg.height
    return h > 0 && w >= h * 3 && w % h == 0        // sprite sheet
}

// ── Dựng artifact ────────────────────────────────────────────────────────
guard let zipTool = which("zip"), let shasum = which("shasum") else {
    print("❌ Thiếu zip/shasum trong hệ thống."); exit(1)
}
_ = shasum

let dirs: [URL]
if packNames.isEmpty {
    dirs = ((try? fm.contentsOfDirectory(at: stickersRoot, includingPropertiesForKeys: [.isDirectoryKey])) ?? [])
        .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
        .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
} else {
    dirs = packNames.map { stickersRoot.appendingPathComponent($0, isDirectory: true) }
}
guard !dirs.isEmpty else { print("❌ Không có bộ nào trong \(stickersRoot.path)"); exit(1) }

try? fm.createDirectory(at: outRoot.appendingPathComponent("packs"), withIntermediateDirectories: true)
try? fm.createDirectory(at: outRoot.appendingPathComponent("covers"), withIntermediateDirectories: true)

var manifest: [[String: Any]] = []
var total = 0

for dir in dirs {
    let name = dir.lastPathComponent
    let id = slug(name)
    guard let files = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil,
                                                  options: [.skipsHiddenFiles]) else {
        print("⚠️  bỏ qua \(name) — không đọc được"); continue
    }
    let images = files
        .filter { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .image) == true }
        .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
    guard let first = images.first else { print("⚠️  bỏ qua \(name) — rỗng"); continue }

    let zipURL = outRoot.appendingPathComponent("packs/\(id).zip")
    try? fm.removeItem(at: zipURL)
    // -1: ảnh webp/gif nén sẵn rồi, ép deflate chỉ tốn CPU đổi lấy vài phần nghìn.
    // -X: bỏ metadata riêng của macOS cho gói gọn và dựng lại giống hệt nhau.
    // Chạy với cwd = thư mục bộ → entry phẳng, không lộ đường dẫn máy tôi.
    guard run(zipTool, ["-q", "-1", "-X", "-r", zipURL.path, "."] + ["-x", ".*", "-x", "__MACOSX/*"], cwd: dir) else {
        print("⚠️  zip lỗi: \(name)"); continue
    }

    let coverURL = outRoot.appendingPathComponent("covers/\(id).png")
    if !makeCover(from: first, to: coverURL) { print("⚠️  cover lỗi: \(name)") }

    let data = (try? Data(contentsOf: zipURL, options: .mappedIfSafe)) ?? Data()
    let sha = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    let size = bytes(of: zipURL)
    total += size

    manifest.append([
        "id": id,
        "name": name,
        "stickers": images.count,
        "animated": images.filter(isAnimated).count,
        "bytes": size,
        "sha256": sha,
        "zip": "packs/\(id).zip",
        "cover": "covers/\(id).png",
    ])
    print("📦 \(name) → \(id).zip  \(images.count) sticker, \(mb(size))")
}

let doc: [String: Any] = ["schema": 1, "packs": manifest]
let json = try JSONSerialization.data(withJSONObject: doc, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
try json.write(to: outRoot.appendingPathComponent("packs.json"))

print("── \(manifest.count) bộ, tổng \(mb(total)) → \(outRoot.path)")

// ── Upload ───────────────────────────────────────────────────────────────
if let dest = uploadTo {
    guard let rclone = which("rclone") else { print("❌ Chưa cài rclone: brew install rclone"); exit(1) }
    print("⬆️  rclone sync → \(dest)")
    // sync chứ không copy: xoá bộ đã gỡ khỏi kho khỏi bucket luôn, để manifest và
    // object không lệch nhau.
    let ok = run(rclone, ["sync", outRoot.path, dest, "--progress",
                          "--s3-no-check-bucket", "--checksum"])
    print(ok ? "✅ Đã đẩy lên \(dest)" : "❌ rclone lỗi — kiểm tra `rclone listremotes`")
    exit(ok ? 0 : 1)
} else {
    print("   Đẩy lên:  xcrun swift tools/publish_packs.swift --upload r2:<tên-bucket>")
}
