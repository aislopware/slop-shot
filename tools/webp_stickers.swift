// Nén kho sticker sang WebP — nhẹ hơn ~6 lần mà mắt không thấy khác.
//
//   xcrun swift tools/webp_stickers.swift                    # cả kho
//   xcrun swift tools/webp_stickers.swift "Ami Bụng Bự"      # một bộ
//   xcrun swift tools/webp_stickers.swift --quality 90 "Ami Bụng Bự"
//   xcrun swift tools/webp_stickers.swift --lossless         # không mất pixel nào
//   xcrun swift tools/webp_stickers.swift --dry-run          # chỉ xem sẽ tiết kiệm bao nhiêu
//
// Vì sao WebP: sticker động lưu dạng APNG hay GIF đều phí. APNG là PNG từng
// frame nên một con 384px 25 frame đã 2.4MB; GIF thì nhẹ hơn nhưng trần 256 màu
// và alpha 1 bit, dán lên ảnh là lòi viền răng cưa. WebP động vừa RGBA đầy đủ
// vừa nén liên-frame → đổi từ GIF sang là vừa NHỎ hơn vừa NÉT hơn.
//
// Đo thực tế trên một con Ami (384px, 25 frame, APNG 2416 KB):
//     lossless  1300 KB  — trùng khớp từng pixel
//     q80        328 KB  — lệch tối đa 22/255, trung bình 0.73
// Cả kho 282 file ở q80: 229.8 MB → 33.1 MB (6.9×), 22 giây trên M-series.
// Ghép thử lên nền tối và nền trắng cho sai lệch y hệt nhau, tức alpha nguyên
// vẹn, lỗi chỉ ở màu. Sticker rồi cũng bị ghép xuống GIF lúc Copy/Export (GIF
// còn quăng đi nhiều hơn thế), nên mất mát này không đến được đầu ra.
//
// Cần: brew install webp  (cwebp cho ảnh tĩnh, img2webp cho ảnh động,
// gif2webp cho GIF — bản GIF dùng tool riêng vì nó tối ưu được cả frame disposal).
//
// LƯU Ý: chạy tools/upscale_stickers.swift SAU tool này sẽ ghi ngược ra .png và
// phình lại. Thứ tự đúng là phóng trước, nén sau.

import Foundation
import AppKit
import ImageIO
import UniformTypeIdentifiers

let stickersRoot = FileManager.default
    .urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
    .appendingPathComponent("SlopShot/Stickers", isDirectory: true)

// ── Tham số dòng lệnh ────────────────────────────────────────────────────
// Gói thành hằng số: phần nén chạy đa luồng nên không được đọc biến toàn cục
// còn thay đổi được.
struct Options { var quality = 80; var lossless = false; var dryRun = false }
let (opts, packNames): (Options, [String]) = {
    var o = Options(), names: [String] = []
    var args = Array(CommandLine.arguments.dropFirst())
    while let a = args.first {
        args.removeFirst()
        switch a {
        case "--quality": if let v = args.first.flatMap({ Int($0) }) { o.quality = v; args.removeFirst() }
        case "--lossless": o.lossless = true
        case "--dry-run": o.dryRun = true
        default: names.append(a)
        }
    }
    return (o, names)
}()

// ── Công cụ ngoài ────────────────────────────────────────────────────────
@Sendable func which(_ name: String) -> String? {
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let p = "\(dir)/\(name)"
        if FileManager.default.isExecutableFile(atPath: p) { return p }
    }
    return nil
}

guard let cwebp = which("cwebp"), let img2webp = which("img2webp"),
      let gif2webp = which("gif2webp") else {
    print("❌ Thiếu công cụ WebP. Cài bằng:  brew install webp"); exit(1)
}

@discardableResult
@Sendable func run(_ tool: String, _ argv: [String]) -> Bool {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: tool)
    p.arguments = argv
    p.standardOutput = FileHandle.nullDevice
    p.standardError = FileHandle.nullDevice
    do { try p.run() } catch { return false }
    p.waitUntilExit()
    return p.terminationStatus == 0
}

// ── Đọc frame ────────────────────────────────────────────────────────────
// Bản rút gọn của FrameSequence trong Sources/AnimatedImage.swift, chép lại vì
// script chạy bằng `swift <file>` nên không link được với source của app.
struct Frames { var images: [CGImage]; var delays: [Double] }
let spriteFPS = 12.0

@Sendable func read(_ url: URL) -> Frames? {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          CGImageSourceGetCount(src) > 0 else { return nil }
    let n = CGImageSourceGetCount(src)
    if n > 1 {
        var imgs: [CGImage] = [], delays: [Double] = []
        for i in 0..<n {
            guard let cg = CGImageSourceCreateImageAtIndex(src, i, nil) else { continue }
            imgs.append(cg); delays.append(delay(src, i))
        }
        return imgs.isEmpty ? nil : Frames(images: imgs, delays: delays)
    }
    guard let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
    let w = cg.width, h = cg.height
    if h > 0, w >= h * 3, w % h == 0 {          // sprite sheet ngang
        let cells = (0..<(w / h)).compactMap {
            cg.cropping(to: CGRect(x: $0 * h, y: 0, width: h, height: h))
        }
        if cells.count > 1 {
            return Frames(images: cells, delays: Array(repeating: 1 / spriteFPS, count: cells.count))
        }
    }
    return Frames(images: [cg], delays: [0])
}

@Sendable func delay(_ src: CGImageSource, _ i: Int) -> Double {
    guard let p = CGImageSourceCopyPropertiesAtIndex(src, i, nil) as? [CFString: Any] else { return 0.1 }
    let slots: [(CFString, CFString, CFString)] = [
        (kCGImagePropertyPNGDictionary, kCGImagePropertyAPNGUnclampedDelayTime, kCGImagePropertyAPNGDelayTime),
        (kCGImagePropertyGIFDictionary, kCGImagePropertyGIFUnclampedDelayTime, kCGImagePropertyGIFDelayTime),
        (kCGImagePropertyWebPDictionary, kCGImagePropertyWebPUnclampedDelayTime, kCGImagePropertyWebPDelayTime),
    ]
    for (dict, unclamped, clamped) in slots {
        guard let d = p[dict] as? [CFString: Any] else { continue }
        if let v = d[unclamped] as? Double, v > 0 { return v }
        if let v = d[clamped] as? Double, v > 0 { return v }
    }
    return 0.1
}

@Sendable func writePNG(_ cg: CGImage, to url: URL) -> Bool {
    guard let dest = CGImageDestinationCreateWithURL(
        url as CFURL, UTType.png.identifier as CFString, 1, nil) else { return false }
    CGImageDestinationAddImage(dest, cg, nil)
    return CGImageDestinationFinalize(dest)
}

// ── Nén 1 file ───────────────────────────────────────────────────────────
// Trả về đường dẫn .webp mới, hoặc nil nếu bỏ qua/lỗi.
let fm = FileManager.default

@Sendable func compress(_ url: URL) -> URL? {
    let out = url.deletingPathExtension().appendingPathExtension("webp")
    let ext = url.pathExtension.lowercased()
    if ext == "webp" { return nil }                       // đã nén rồi

    // GIF: gif2webp đọc thẳng, tối ưu được cả frame disposal nên nhỏ hơn đường vòng.
    if ext == "gif" {
        var argv = [url.path, "-o", out.path, "-m", "4"]
        argv += opts.lossless ? ["-lossless"] : ["-q", "\(opts.quality)", "-mixed"]
        return run(gif2webp, argv) ? out : nil
    }

    guard let f = read(url), let first = f.images.first else { return nil }

    // Ảnh tĩnh: cwebp một phát.
    if f.images.count == 1 {
        var argv = [url.path, "-o", out.path, "-m", "4"]
        argv += opts.lossless ? ["-lossless"] : ["-q", "\(opts.quality)"]
        // Sprite sheet 1 ô hay ảnh thường đều rơi vào đây; giữ nguyên alpha.
        _ = first
        return run(cwebp, argv) ? out : nil
    }

    // Ảnh động (APNG / sprite sheet đã cắt): xả frame ra PNG rồi ghép bằng img2webp.
    // Truyền -d riêng cho TỪNG frame để giữ đúng nhịp gốc, kể cả nhịp không đều.
    let tmp = fm.temporaryDirectory.appendingPathComponent("webpstk-\(UUID().uuidString)")
    try? fm.createDirectory(at: tmp, withIntermediateDirectories: true)
    defer { try? fm.removeItem(at: tmp) }

    // Không bật -min_size: nó thử nhiều cấu hình nên chậm gấp bội mà chỉ nhỏ hơn
    // chừng 10%.
    var argv = ["-loop", "0"]
    argv += opts.lossless ? ["-lossless"] : ["-lossy", "-q", "\(opts.quality)", "-m", "4"]
    for (i, cg) in f.images.enumerated() {
        let frame = tmp.appendingPathComponent(String(format: "%04d.png", i))
        guard writePNG(cg, to: frame) else { return nil }
        argv += ["-d", "\(max(Int((f.delays[i] * 1000).rounded()), 20))", frame.path]
    }
    argv += ["-o", out.path]
    return run(img2webp, argv) ? out : nil
}

// ── Chạy ─────────────────────────────────────────────────────────────────
let packs: [URL]
if packNames.isEmpty {
    packs = ((try? fm.contentsOfDirectory(at: stickersRoot, includingPropertiesForKeys: [.isDirectoryKey])) ?? [])
        .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
        .sorted { $0.lastPathComponent < $1.lastPathComponent }
} else {
    packs = packNames.map { stickersRoot.appendingPathComponent($0, isDirectory: true) }
}

func mb(_ bytes: Int) -> String { String(format: "%.1f MB", Double(bytes) / 1_048_576) }

var grandBefore = 0, grandAfter = 0
for pack in packs {
    guard let files = try? fm.contentsOfDirectory(at: pack, includingPropertiesForKeys: nil) else {
        print("⚠️  bỏ qua \(pack.lastPathComponent) — không đọc được"); continue
    }
    let todo = files.sorted { $0.lastPathComponent < $1.lastPathComponent }
        .filter { ["png", "gif", "jpg", "jpeg"].contains($0.pathExtension.lowercased()) }

    // Chạy song song: mỗi file là một tiến trình cwebp/img2webp độc lập, mà nén
    // một con 72 frame mất vài giây — làm tuần tự cả kho là chờ mười mấy phút.
    var before = 0, after = 0, done = 0, failed = 0
    let lock = NSLock()
    DispatchQueue.concurrentPerform(iterations: todo.count) { i in
        let f = todo[i]
        let sizeBefore = ((try? fm.attributesOfItem(atPath: f.path)[.size]) as? Int) ?? 0
        guard let out = compress(f),
              let sizeAfter = (try? fm.attributesOfItem(atPath: out.path)[.size]) as? Int else {
            lock.lock(); failed += 1; lock.unlock(); return
        }
        if opts.dryRun { try? fm.removeItem(at: out) } else { try? fm.removeItem(at: f) }
        lock.lock(); before += sizeBefore; after += sizeAfter; done += 1; lock.unlock()
    }
    grandBefore += before; grandAfter += after
    let ratio = after > 0 ? Double(before) / Double(after) : 0
    print("✅ \(pack.lastPathComponent): \(done) file, \(mb(before)) → \(mb(after))"
          + String(format: " (%.1f× nhỏ hơn)", ratio)
          + (failed > 0 ? "  ⚠️ lỗi \(failed)" : ""))
}

if packs.count > 1 {
    let ratio = grandAfter > 0 ? Double(grandBefore) / Double(grandAfter) : 0
    print(String(format: "── TỔNG: %@ → %@ (%.1f× nhỏ hơn)", mb(grandBefore), mb(grandAfter), ratio))
}
if opts.dryRun { print("   (--dry-run: không đụng file gốc)") }
