import Foundation
import Vision
import CoreGraphics

// ─────────────────────────────────────────────────────────────────────────
// Dò dữ liệu nhạy cảm trong ảnh: chạy OCR rồi soi từng dòng chữ tìm email,
// số điện thoại, số thẻ, token… Trả về KHUNG của đúng đoạn chữ đó (không phải
// cả dòng) để editor úp một ô blur lên trên.
//
// Toàn bộ chạy on-device bằng Vision — không có gì rời khỏi máy.
// ─────────────────────────────────────────────────────────────────────────
struct SensitiveMatch {
    enum Kind: String {
        case email, phone, card, token

        var label: String {
            switch self {
            case .email: return "email"
            case .phone: return "phone number"
            case .card:  return "card number"
            case .token: return "key/token"
            }
        }
        // Số nhiều cho dòng trạng thái ("2 emails").
        var plural: String { label + "s" }
    }

    let kind: Kind
    /// Khung đoạn chữ, normalized 0…1, gốc GÓC TRÊN-TRÁI (đã đổi hệ từ Vision).
    let rect: CGRect
}

enum SensitiveScanner {

    // ── API chính ──────────────────────────────────────────────────────────
    static func scan(in cgImage: CGImage) async -> [SensitiveMatch] {
        let lines = await TextRecognizer.lines(in: cgImage)
        var found: [SensitiveMatch] = []

        for line in lines {
            let s = line.string
            for (kind, range) in matches(in: s) {
                // boundingBox(for:) nhận Range<String.Index> TRÊN CHÍNH chuỗi này,
                // nên mọi range phải sinh ra từ `s` chứ không phải bản copy khác.
                guard let box = try? line.boundingBox(for: range) else { continue }
                let bb = box.boundingBox            // normalized, gốc góc DƯỚI-trái
                guard bb.width > 0, bb.height > 0 else { continue }
                // Vision: y tính từ đáy lên. Annotation trong editor: y từ đỉnh xuống.
                found.append(SensitiveMatch(
                    kind: kind,
                    rect: CGRect(x: bb.minX, y: 1 - bb.maxY,
                                 width: bb.width, height: bb.height)))
            }
        }
        return merged(found)
    }

    /// Tóm tắt kiểu "2 emails, 1 card number" cho dòng trạng thái.
    static func summary(_ items: [SensitiveMatch]) -> String {
        var counts: [SensitiveMatch.Kind: Int] = [:]
        for m in items { counts[m.kind, default: 0] += 1 }
        let order: [SensitiveMatch.Kind] = [.email, .phone, .card, .token]
        return order
            .compactMap { kind -> String? in
                guard let n = counts[kind] else { return nil }
                return "\(n) \(n == 1 ? kind.label : kind.plural)"
            }
            .joined(separator: ", ")
    }

    // ── Dò mẫu trong 1 dòng chữ ────────────────────────────────────────────
    private static func matches(in s: String) -> [(SensitiveMatch.Kind, Range<String.Index>)] {
        guard !s.isEmpty else { return [] }
        var out: [(SensitiveMatch.Kind, Range<String.Index>)] = []

        for (kind, regex) in regexes {
            let full = NSRange(s.startIndex..., in: s)
            for m in regex.matches(in: s, range: full) {
                guard let r = Range(m.range, in: s) else { continue }
                // Số thẻ phải qua Luhn mới tính — không thì mọi dãy số dài đều dính.
                if kind == .card, !luhnValid(String(s[r])) { continue }
                out.append((kind, r))
            }
        }

        // Điện thoại: dùng NSDataDetector thay regex. Regex số điện thoại luôn
        // bắt nhầm mã đơn hàng / timestamp; detector của hệ thống hiểu định dạng
        // theo vùng nên chắc tay hơn nhiều.
        if let detector = phoneDetector {
            let full = NSRange(s.startIndex..., in: s)
            detector.enumerateMatches(in: s, range: full) { m, _, _ in
                guard let m, let r = Range(m.range, in: s) else { return }
                // Bỏ số quá ngắn (giờ giấc, số trang…).
                guard s[r].filter(\.isNumber).count >= 8 else { return }
                out.append((.phone, r))
            }
        }
        return out
    }

    // Biên dịch sẵn một lần — NSRegularExpression khởi tạo không rẻ, mà mỗi ảnh
    // có thể có hàng trăm dòng chữ.
    private static let regexes: [(SensitiveMatch.Kind, NSRegularExpression)] = {
        let patterns: [(SensitiveMatch.Kind, String)] = [
            (.email, #"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}"#),
            // JWT: header.payload — cả hai phần đều bắt đầu bằng "eyJ" (base64 của '{"').
            (.token, #"eyJ[A-Za-z0-9_\-]{5,}\.eyJ[A-Za-z0-9_\-]{5,}(?:\.[A-Za-z0-9_\-]+)?"#),
            // Token có tiền tố nhận dạng rõ ràng — gần như không thể dương tính giả.
            (.token, #"(?:sk-[A-Za-z0-9_\-]{16,}"#
                   + #"|ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}"#
                   + #"|AKIA[0-9A-Z]{12,}"#
                   + #"|xox[abps]-[A-Za-z0-9\-]{10,}"#
                   + #"|AIza[0-9A-Za-z_\-]{30,})"#),
            (.token, #"-----BEGIN [A-Z ]*PRIVATE KEY-----"#),
            // 13–19 chữ số, cho phép cách/gạch ở giữa; Luhn lọc tiếp ở trên.
            (.card,  #"(?<![0-9\-])(?:[0-9][ \-]?){12,18}[0-9](?![0-9\-])"#),
        ]
        return patterns.compactMap { kind, p in
            (try? NSRegularExpression(pattern: p)).map { (kind, $0) }
        }
    }()

    private static let phoneDetector = try? NSDataDetector(
        types: NSTextCheckingResult.CheckingType.phoneNumber.rawValue)

    // Thuật toán Luhn — checksum mọi số thẻ tín dụng thật đều thoả.
    private static func luhnValid(_ raw: String) -> Bool {
        let digits = raw.compactMap { $0.wholeNumberValue }
        guard (13...19).contains(digits.count) else { return false }
        var sum = 0
        for (i, d) in digits.reversed().enumerated() {
            var v = d
            if i % 2 == 1 { v *= 2; if v > 9 { v -= 9 } }
            sum += v
        }
        return sum % 10 == 0
    }

    // Hai mẫu chồng lên nhau (vd. token nằm trong chuỗi dài) → gộp thành 1 ô blur,
    // đỡ đè mấy lớp mờ lên cùng chỗ.
    private static func merged(_ items: [SensitiveMatch]) -> [SensitiveMatch] {
        var out: [SensitiveMatch] = []
        for m in items {
            if let i = out.firstIndex(where: { $0.rect.intersects(m.rect) }) {
                out[i] = SensitiveMatch(kind: out[i].kind, rect: out[i].rect.union(m.rect))
            } else {
                out.append(m)
            }
        }
        return out
    }
}
