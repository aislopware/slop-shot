import Foundation

// ─────────────────────────────────────────────────────────────────────────
// Tên file trong thư mục tạm, không bao giờ trùng với file đã có.
//
// Tên chỉ có tới giây ("SlopShot 2026-09-30 at 09.12.44"), nên chụp 2 tấm
// trong cùng 1 giây từng ra đúng 1 tên: tấm sau đè tấm trước, và mục lịch sử,
// clipboard, preview của tấm trước đều trỏ sang ảnh mới.
// ─────────────────────────────────────────────────────────────────────────
enum TempFiles {
    /// `reserve`: tạo sẵn file rỗng để giữ chỗ. Cần khi file được ghi sau (ở
    /// luồng nền) — không giữ chỗ thì lần chụp kế tiếp vẫn thấy tên đó trống.
    /// Đừng bật cho AVAssetWriter: nó từ chối ghi vào file đã tồn tại.
    static func uniqueURL(named base: String, ext: String, reserve: Bool = false) -> URL {
        let fm = FileManager.default
        let dir = fm.temporaryDirectory
        var url = dir.appendingPathComponent("\(base).\(ext)")
        var i = 2
        while fm.fileExists(atPath: url.path) {
            url = dir.appendingPathComponent("\(base) (\(i)).\(ext)")
            i += 1
        }
        if reserve { fm.createFile(atPath: url.path, contents: nil) }
        return url
    }
}
