// Phóng to sticker cho nét hơn khi dán lên ảnh chụp độ phân giải cao.
//
//   swift tools/upscale_stickers.swift                 # cả kho
//   swift tools/upscale_stickers.swift "Ami Bụng Bự"   # một bộ
//   swift tools/upscale_stickers.swift --target 768 "Ami Bụng Bự"
//
// Vì sao cần: sticker nguồn chỉ 192–240px (đó là trần mà API trả về), trong khi
// dán vào ảnh chụp Retina 3000px ở mức ~28% bề ngang là phải kéo lên ~800px.
// Kéo lúc vẽ thì dùng nội suy mặc định → nhòe. Ở đây phóng sẵn bằng Lanczos rồi
// unsharp nhẹ, ghi ra PNG RGBA truecolor.
//
// Nói thẳng: phóng to KHÔNG tạo thêm chi tiết vốn không có. Nó chỉ cho nét viền
// gọn hơn và tránh nội suy thô lúc vẽ. Nguồn 240px vẫn là nguồn 240px.
//
// Ảnh từ Zalo là PNG-8 (bảng màu 256) nên viền chống răng cưa bị lượng tử hoá.
// Unsharp để mức thấp (0.6) cho khỏi lòi viền răng cưa đó ra.

import Foundation
import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins

let stickersRoot = FileManager.default
    .urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
    .appendingPathComponent("SlopShot/Stickers", isDirectory: true)

// ── Tham số dòng lệnh ────────────────────────────────────────────────────
var target: CGFloat = 768        // cạnh dài mong muốn sau khi phóng
var packNames: [String] = []
var args = Array(CommandLine.arguments.dropFirst())
while let a = args.first {
    args.removeFirst()
    if a == "--target", let v = args.first.flatMap({ Double($0) }) {
        target = CGFloat(v); args.removeFirst()
    } else {
        packNames.append(a)
    }
}

let ctx = CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.sRGB)!])

// Phóng 1 ảnh. Trả về nil nếu không cần đụng tới (đã đủ to) hoặc lỗi.
func upscaled(_ url: URL) -> Data? {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          CGImageSourceGetCount(src) > 0,
          var cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }

    // Sprite sheet ngang (nhiều bộ Zalo đóng gói animation kiểu này) → lấy ô đầu,
    // giống hệt cách app hiển thị, để phóng đúng thứ nhìn thấy.
    if cg.height > 0, cg.width >= cg.height * 3, cg.width % cg.height == 0,
       let first = cg.cropping(to: CGRect(x: 0, y: 0, width: cg.height, height: cg.height)) {
        cg = first
    }

    let longEdge = CGFloat(max(cg.width, cg.height))
    guard longEdge > 0, longEdge < target else { return nil }   // đã đủ nét thì bỏ qua
    let scale = target / longEdge

    let input = CIImage(cgImage: cg)
    let lanczos = CIFilter.lanczosScaleTransform()
    lanczos.inputImage = input
    lanczos.scale = Float(scale)
    lanczos.aspectRatio = 1
    guard let big = lanczos.outputImage else { return nil }

    let sharpen = CIFilter.unsharpMask()
    sharpen.inputImage = big
    sharpen.radius = 1.6
    sharpen.intensity = 0.6          // nhẹ tay: ảnh nguồn palettized, đẩy mạnh là lòi răng cưa
    guard let out = sharpen.outputImage?.cropped(to: big.extent) else { return nil }

    guard let result = ctx.createCGImage(out, from: big.extent,
                                         format: .RGBA8,
                                         colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!)
    else { return nil }

    let data = NSMutableData()
    guard let dest = CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil)
    else { return nil }
    CGImageDestinationAddImage(dest, result, nil)
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
    var done = 0, skipped = 0
    for f in files.sorted(by: { $0.lastPathComponent < $1.lastPathComponent })
        where ["png", "webp", "jpg", "jpeg", "gif"].contains(f.pathExtension.lowercased()) {
        guard let data = upscaled(f) else { skipped += 1; continue }
        // Luôn ghi ra .png (nguồn có thể là webp) rồi dọn file cũ nếu đổi đuôi.
        let out = f.deletingPathExtension().appendingPathExtension("png")
        do {
            try data.write(to: out)
            if out != f { try? fm.removeItem(at: f) }
            done += 1
        } catch {
            print("⚠️  ghi hỏng: \(f.lastPathComponent)"); skipped += 1
        }
    }
    print("✅ \(pack.lastPathComponent): phóng \(done), bỏ qua \(skipped) → cạnh dài \(Int(target))px")
}
