// Phóng to sticker cho nét hơn khi dán lên ảnh chụp độ phân giải cao.
//
//   swift tools/upscale_stickers.swift                 # cả kho
//   swift tools/upscale_stickers.swift "Ami Bụng Bự"   # một bộ
//   swift tools/upscale_stickers.swift --target 768 "Ami Bụng Bự"
//   swift tools/upscale_stickers.swift --anim-target 512 "Ami Bụng Bự"
//
// Vì sao cần: sticker nguồn chỉ 130–240px (đó là trần mà API trả về), trong khi
// dán vào ảnh chụp Retina 3000px ở mức ~28% bề ngang là phải kéo lên ~800px.
// Kéo lúc vẽ thì dùng nội suy mặc định → nhòe. Ở đây phóng sẵn bằng Lanczos rồi
// unsharp nhẹ, ghi ra PNG RGBA truecolor.
//
// Nói thẳng: phóng to KHÔNG tạo thêm chi tiết vốn không có. Nó chỉ cho nét viền
// gọn hơn và tránh nội suy thô lúc vẽ. Nguồn 130px vẫn là nguồn 130px.
//
// Ảnh từ Zalo là PNG-8 (bảng màu 256) nên viền chống răng cưa bị lượng tử hoá.
// Unsharp để mức thấp (0.6) cho khỏi lòi viền răng cưa đó ra.
//
// STICKER ĐỘNG: phóng TỪNG frame rồi ghi lại thành APNG (vẫn đuôi .png), giữ
// nguyên animation. Trước đây script này cắt lấy frame đầu của sprite sheet —
// tức là phóng to xong thì sticker chết cứng. Ngưỡng riêng (--anim-target, mặc
// định 384) vì một bộ 72 sticker × 25 frame phóng lên 768 là hàng trăm MB.
// Chọn APNG chứ không phải GIF: GIF chỉ có alpha 1 bit, dán lên ảnh là lòi viền
// răng cưa quanh sticker. GIF chỉ dùng ở đầu ra cuối cùng (lúc Copy/Save).

import Foundation
import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import ImageIO
import UniformTypeIdentifiers

let stickersRoot = FileManager.default
    .urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
    .appendingPathComponent("SlopShot/Stickers", isDirectory: true)

// ── Tham số dòng lệnh ────────────────────────────────────────────────────
var target: CGFloat = 768        // cạnh dài mong muốn sau khi phóng (ảnh tĩnh)
var animTarget: CGFloat = 384    // như trên, cho sticker động
var packNames: [String] = []
var args = Array(CommandLine.arguments.dropFirst())
while let a = args.first {
    args.removeFirst()
    if a == "--target", let v = args.first.flatMap({ Double($0) }) {
        target = CGFloat(v); args.removeFirst()
    } else if a == "--anim-target", let v = args.first.flatMap({ Double($0) }) {
        animTarget = CGFloat(v); args.removeFirst()
    } else {
        packNames.append(a)
    }
}

let ctx = CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.sRGB)!])
let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!

// ── Đọc frame ────────────────────────────────────────────────────────────
// Bản rút gọn của FrameSequence trong Sources/AnimatedImage.swift. Chép lại vì
// script này chạy bằng `swift <file>`, không link được với source của app.
// Sửa quy ước nhận dạng sprite sheet ở một bên thì nhớ sửa cả bên kia.
struct Frames {
    var images: [CGImage]
    var delays: [Double]
    var isAnimated: Bool { images.count > 1 }
}

let spriteFPS = 12.0

func read(_ url: URL) -> Frames? {
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
    // Sprite sheet ngang: cắt thành từng frame thay vì lấy mỗi ô đầu.
    let w = cg.width, h = cg.height
    if h > 0, w >= h * 3, w % h == 0 {
        let cells = (0..<(w / h)).compactMap {
            cg.cropping(to: CGRect(x: $0 * h, y: 0, width: h, height: h))
        }
        if cells.count > 1 {
            return Frames(images: cells, delays: Array(repeating: 1 / spriteFPS, count: cells.count))
        }
    }
    return Frames(images: [cg], delays: [0])
}

func delay(_ src: CGImageSource, _ i: Int) -> Double {
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

// ── Phóng 1 frame ────────────────────────────────────────────────────────
func enlarge(_ cg: CGImage, scale: CGFloat) -> CGImage? {
    let input = CIImage(cgImage: cg)
    let lanczos = CIFilter.lanczosScaleTransform()
    lanczos.inputImage = input
    lanczos.scale = Float(scale)
    lanczos.aspectRatio = 1
    guard let big = lanczos.outputImage else { return nil }

    let sharpen = CIFilter.unsharpMask()
    sharpen.inputImage = big
    sharpen.radius = 1.6
    sharpen.intensity = 0.6      // nhẹ tay: ảnh nguồn palettized, đẩy mạnh là lòi răng cưa
    guard let out = sharpen.outputImage?.cropped(to: big.extent) else { return nil }
    return ctx.createCGImage(out, from: big.extent, format: .RGBA8, colorSpace: sRGB)
}

// ── Phóng cả file ────────────────────────────────────────────────────────
// Trả về nil nếu không cần đụng tới (đã đủ to) hoặc lỗi.
func upscaled(_ url: URL) -> Data? {
    guard let f = read(url), let first = f.images.first else { return nil }

    let longEdge = CGFloat(max(first.width, first.height))
    let want = f.isAnimated ? animTarget : target
    guard longEdge > 0, longEdge < want else { return nil }   // đã đủ nét thì bỏ qua
    // Ảnh động phóng chưa tới 1.5× thì không bõ: nét thêm không đáng kể mà file
    // phình lên vài lần (mỗi frame là một ảnh PNG riêng).
    guard !f.isAnimated || want / longEdge >= 1.5 else { return nil }
    let scale = want / longEdge

    let data = NSMutableData()
    let type = UTType.png.identifier as CFString
    guard let dest = CGImageDestinationCreateWithData(data, type, f.images.count, nil) else { return nil }
    if f.isAnimated {
        CGImageDestinationSetProperties(dest, [
            kCGImagePropertyPNGDictionary: [kCGImagePropertyAPNGLoopCount: 0],
        ] as CFDictionary)
    }
    for (i, cg) in f.images.enumerated() {
        guard let big = enlarge(cg, scale: scale) else { return nil }
        let props: CFDictionary? = f.isAnimated
            ? [kCGImagePropertyPNGDictionary: [kCGImagePropertyAPNGDelayTime: f.delays[i]]] as CFDictionary
            : nil
        CGImageDestinationAddImage(dest, big, props)
    }
    guard CGImageDestinationFinalize(dest) else { return nil }
    return data as Data
}

// ── Chạy ─────────────────────────────────────────────────────────────────
let fm = FileManager.default
let packs: [URL]
if packNames.isEmpty {
    packs = ((try? fm.contentsOfDirectory(at: stickersRoot, includingPropertiesForKeys: [.isDirectoryKey])) ?? [])
        .filter { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
} else {
    packs = packNames.map { stickersRoot.appendingPathComponent($0, isDirectory: true) }
}

for pack in packs {
    guard let files = try? fm.contentsOfDirectory(at: pack, includingPropertiesForKeys: nil) else {
        print("⚠️  bỏ qua \(pack.lastPathComponent) — không đọc được"); continue
    }
    var done = 0, skipped = 0, animDone = 0, bytes = 0
    for f in files.sorted(by: { $0.lastPathComponent < $1.lastPathComponent })
        where ["png", "webp", "jpg", "jpeg", "gif"].contains(f.pathExtension.lowercased()) {
        let wasAnimated = (read(f)?.isAnimated ?? false)
        guard let data = upscaled(f) else { skipped += 1; continue }
        // Luôn ghi ra .png (nguồn có thể là webp/gif/sprite sheet) rồi dọn file cũ.
        let out = f.deletingPathExtension().appendingPathExtension("png")
        do {
            try data.write(to: out)
            if out != f { try? fm.removeItem(at: f) }
            done += 1; bytes += data.count
            if wasAnimated { animDone += 1 }
        } catch {
            print("⚠️  ghi hỏng: \(f.lastPathComponent)"); skipped += 1
        }
    }
    let mb = String(format: "%.1f", Double(bytes) / 1_048_576)
    print("✅ \(pack.lastPathComponent): phóng \(done) (động: \(animDone)), bỏ qua \(skipped)"
          + " → tĩnh \(Int(target))px / động \(Int(animTarget))px, ghi \(mb) MB")
}
