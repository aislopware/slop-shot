// Tách sticker khỏi nền phẳng — cho ảnh sticker người ta đăng lên Facebook,
// group, Pinterest… dạng "con vật có viền trắng đặt trên nền xám".
//
//   xcrun swift tools/cutout_stickers.swift --pack "Mèo Hết Tiền" in/*.jpg
//   xcrun swift tools/cutout_stickers.swift --pack "Mèo Hết Tiền" --target 768 in/*.jpg
//   xcrun swift tools/cutout_stickers.swift --out /tmp/thu in/*.jpg     # xem thử, chưa vào kho
//
// Tên file ra = tên file vào, đuôi .png. Đặt tên file vào kiểu "01 Câu chữ.jpg"
// trước cho khớp các bộ khác (số thứ tự quyết định thứ tự trong popover).
//
// Cách làm: màu nền = trung vị các pixel trên mép ảnh, rồi loang từ mép vào,
// ăn mọi pixel gần màu nền. Viền trắng bao quanh sticker chặn vết loang lại nên
// không cần mô hình tách nền nào — và không có chuyện mô hình "tiện tay" xoá
// luôn dòng chữ hay viền trắng, vốn là một phần của sticker. Dải 2px sát vết
// loang là chỗ JPEG trộn viền với nền: tách ngược ra alpha theo giả định
// pixel = a·viền + (1−a)·nền, cho mép mềm thay vì răng cưa.
//
// Sau tool này chạy tools/webp_stickers.swift rồi tools/publish_packs.swift.

import Foundation
import AppKit
import ImageIO
import UniformTypeIdentifiers

// ── Tham số dòng lệnh ────────────────────────────────────────────────────
var packName: String?
var outDir: URL?
var target = 1024            // cạnh dài sau khi cắt, như các bộ ảnh tĩnh sẵn có
var tolerance = 18           // lệch tối đa mỗi kênh so với màu nền vẫn tính là nền
var inputs: [URL] = []

var argv = Array(CommandLine.arguments.dropFirst())
while let a = argv.first {
    argv.removeFirst()
    switch a {
    case "--pack": packName = argv.first; if !argv.isEmpty { argv.removeFirst() }
    case "--out": outDir = argv.first.map { URL(fileURLWithPath: $0, isDirectory: true) }; if !argv.isEmpty { argv.removeFirst() }
    case "--target": if let v = argv.first.flatMap({ Int($0) }) { target = v; argv.removeFirst() }
    case "--tolerance": if let v = argv.first.flatMap({ Int($0) }) { tolerance = v; argv.removeFirst() }
    default: inputs.append(URL(fileURLWithPath: a))
    }
}

if let packName {
    outDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        .appendingPathComponent("SlopShot/Stickers/\(packName)", isDirectory: true)
}
guard let outDir, !inputs.isEmpty else {
    print("Dùng: xcrun swift tools/cutout_stickers.swift (--pack <tên bộ> | --out <thư mục>) ảnh…")
    exit(1)
}
try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

// ── Xử lý một ảnh ────────────────────────────────────────────────────────
struct RGB { var r: Int, g: Int, b: Int }

func cutout(_ url: URL) -> CGImage? {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
    let w = cg.width, h = cg.height
    var px = [UInt8](repeating: 0, count: w * h * 4)
    guard let ctx = CGContext(data: &px, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                              space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))

    func color(_ i: Int) -> RGB { RGB(r: Int(px[i * 4]), g: Int(px[i * 4 + 1]), b: Int(px[i * 4 + 2])) }

    var border: [Int] = []
    for x in 0..<w { border.append(x); border.append((h - 1) * w + x) }
    for y in 1..<(h - 1) { border.append(y * w); border.append(y * w + w - 1) }
    func median(_ k: KeyPath<RGB, Int>) -> Int { border.map { color($0)[keyPath: k] }.sorted()[border.count / 2] }
    let bg = RGB(r: median(\.r), g: median(\.g), b: median(\.b))

    func nearBG(_ i: Int) -> Bool {
        let c = color(i)
        return max(abs(c.r - bg.r), abs(c.g - bg.g), abs(c.b - bg.b)) <= tolerance
    }

    // Loang 4 hướng từ mép. Mảng làm stack cho khỏi đệ quy tràn ngăn xếp ở ảnh 2048².
    var isBG = [Bool](repeating: false, count: w * h)
    var stack = border.filter(nearBG)
    for i in stack { isBG[i] = true }
    while let i = stack.popLast() {
        let x = i % w, y = i / w
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
        where nx >= 0 && ny >= 0 && nx < w && ny < h {
            let n = ny * w + nx
            if !isBG[n] && nearBG(n) { isBG[n] = true; stack.append(n) }
        }
    }

    // Khoảng cách (Chebyshev, tối đa 2) tới pixel nền gần nhất, để biết dải mép.
    var ring = [UInt8](repeating: 0, count: w * h)
    for pass in 1...2 {
        for y in 0..<h {
            for x in 0..<w {
                let i = y * w + x
                guard !isBG[i], ring[i] == 0 else { continue }
                var hit = false
                for dy in -1...1 where !hit {
                    for dx in -1...1 {
                        let nx = x + dx, ny = y + dy
                        guard nx >= 0, ny >= 0, nx < w, ny < h else { continue }
                        let n = ny * w + nx
                        if (pass == 1 && isBG[n]) || (pass == 2 && ring[n] == 1) { hit = true; break }
                    }
                }
                if hit { ring[i] = UInt8(pass) }
            }
        }
    }

    // Màu ở mép tham chiếu theo viền trắng: sticker kiểu này luôn viền trắng.
    let edge = RGB(r: 255, g: 255, b: 255)
    var minX = w, minY = h, maxX = -1, maxY = -1
    for i in 0..<(w * h) {
        let o = i * 4
        if isBG[i] {
            px[o] = 0; px[o + 1] = 0; px[o + 2] = 0; px[o + 3] = 0
            continue
        }
        if ring[i] > 0 {
            let c = color(i)
            func frac(_ v: Int, _ b: Int, _ e: Int) -> Double { e == b ? 1 : Double(v - b) / Double(e - b) }
            let a = min(1, max(0, max(frac(c.r, bg.r, edge.r), frac(c.g, bg.g, edge.g), frac(c.b, bg.b, edge.b))))
            // Premultiplied: phần màu còn lại sau khi trừ đi phần nền đã trộn vào.
            func un(_ v: Int, _ b: Int) -> UInt8 { UInt8(min(255 * a, max(0, Double(v) - (1 - a) * Double(b))).rounded()) }
            px[o] = un(c.r, bg.r); px[o + 1] = un(c.g, bg.g); px[o + 2] = un(c.b, bg.b)
            px[o + 3] = UInt8((255 * a).rounded())
            if a == 0 { continue }
        }
        let x = i % w, y = i / w
        minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
    }
    guard maxX >= minX, let full = ctx.makeImage() else { return nil }

    let crop = CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1)
    guard let trimmed = full.cropping(to: crop) else { return nil }

    let scale = min(1, Double(target) / Double(max(trimmed.width, trimmed.height)))
    let nw = Int((Double(trimmed.width) * scale).rounded()), nh = Int((Double(trimmed.height) * scale).rounded())
    guard let out = CGContext(data: nil, width: nw, height: nh, bitsPerComponent: 8, bytesPerRow: 0,
                              space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    out.interpolationQuality = .high
    out.draw(trimmed, in: CGRect(x: 0, y: 0, width: nw, height: nh))
    return out.makeImage()
}

// ── Chạy ─────────────────────────────────────────────────────────────────
var failed = 0
for url in inputs {
    let dest = outDir.appendingPathComponent(url.deletingPathExtension().lastPathComponent + ".png")
    guard let img = cutout(url),
          let d = CGImageDestinationCreateWithURL(dest as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        print("❌ \(url.lastPathComponent)"); failed += 1; continue
    }
    CGImageDestinationAddImage(d, img, nil)
    if CGImageDestinationFinalize(d) {
        print("✂️  \(url.lastPathComponent) → \(dest.lastPathComponent)  \(img.width)×\(img.height)")
    } else {
        print("❌ ghi lỗi: \(dest.lastPathComponent)"); failed += 1
    }
}
print("── \(inputs.count - failed)/\(inputs.count) → \(outDir.path)")
exit(failed == 0 ? 0 : 1)
