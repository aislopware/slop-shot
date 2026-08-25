import Foundation
import Vision
import NaturalLanguage
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
        case email, phone, card, cvv, expiry, token, secret, idNumber, name

        var label: String {
            switch self {
            case .email:    return "email"
            case .phone:    return "phone number"
            case .card:     return "card number"
            case .cvv:      return "CVV"
            case .expiry:   return "expiry date"
            case .token:    return "key/token"
            case .secret:   return "password"
            case .idNumber: return "ID number"
            case .name:     return "name on the card"
            }
        }
        // Số nhiều cho dòng trạng thái ("2 emails").
        var plural: String {
            switch self {
            case .name: return "names on the card"
            default:    return label + "s"
            }
        }
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
                guard let m = match(kind, range, on: line) else { continue }
                found.append(m)
            }
        }

        // Tên người thì chỉ bôi KHI ảnh có bối cảnh thẻ (đã thấy số thẻ / CVV /
        // hạn thẻ). Bôi mọi cái tên trong mọi ảnh chụp thì hỏng: screenshot chat,
        // commit log, danh bạ… cái nào cũng đầy tên người.
        //
        // Và chỉ trong PHẠM VI cái thẻ. Có bối cảnh thẻ mà quét tên cả tấm ảnh
        // thì mọi nhãn Title Case ngoài rìa — "Other Tools", "Line Count Tool" —
        // đều trông y như tên người: 2-4 từ, viết hoa đầu, không có số.
        if let zone = cardZone(found) {
            for line in lines {
                let s = line.string
                for range in cardholderNames(in: s) {
                    guard let m = match(.name, range, on: line),
                          zone.contains(CGPoint(x: m.rect.midX, y: m.rect.midY))
                    else { continue }
                    found.append(m)
                }
            }
        }
        return merged(found)
    }

    /// Vùng "quanh cái thẻ": bao cả số thẻ / CVV / hạn thẻ rồi nới ra để với tới
    /// dòng tên (nằm dưới đáy thẻ, cách số thẻ một quãng). `nil` nếu ảnh không có
    /// dữ liệu thẻ nào — lúc đó không dò tên.
    ///
    /// Nới theo CHÍNH kích thước cụm chữ tìm được, cộng một sàn tuyệt đối cho
    /// trường hợp chỉ bắt được mỗi dòng số thẻ (cụm mỏng dính, nhân lên vẫn bé).
    private static func cardZone(_ found: [SensitiveMatch]) -> CGRect? {
        let anchors = found.filter { $0.kind == .card || $0.kind == .cvv || $0.kind == .expiry }
        guard var zone = anchors.first?.rect else { return nil }
        for a in anchors.dropFirst() { zone = zone.union(a.rect) }
        return zone.insetBy(dx: -max(zone.width * 0.4, 0.06),
                            dy: -max(zone.height * 1.4, 0.08))
    }

    /// Tóm tắt kiểu "2 emails, 1 card number" cho dòng trạng thái.
    static func summary(_ items: [SensitiveMatch]) -> String {
        var counts: [SensitiveMatch.Kind: Int] = [:]
        for m in items { counts[m.kind, default: 0] += 1 }
        let order: [SensitiveMatch.Kind] = [
            .email, .phone, .card, .cvv, .expiry, .name, .token, .secret, .idNumber,
        ]
        return order
            .compactMap { kind -> String? in
                guard let n = counts[kind] else { return nil }
                return "\(n) \(n == 1 ? kind.label : kind.plural)"
            }
            .joined(separator: ", ")
    }

    // ── Đổi Range chữ → khung trong ảnh ────────────────────────────────────
    private static func match(_ kind: SensitiveMatch.Kind,
                              _ range: Range<String.Index>,
                              on line: VNRecognizedText) -> SensitiveMatch? {
        // boundingBox(for:) nhận Range<String.Index> TRÊN CHÍNH chuỗi này,
        // nên mọi range phải sinh ra từ `line.string` chứ không phải bản copy khác.
        guard let box = try? line.boundingBox(for: range) else { return nil }
        let bb = box.boundingBox            // normalized, gốc góc DƯỚI-trái
        guard bb.width > 0, bb.height > 0 else { return nil }
        // Vision: y tính từ đáy lên. Annotation trong editor: y từ đỉnh xuống.
        return SensitiveMatch(kind: kind,
                              rect: CGRect(x: bb.minX, y: 1 - bb.maxY,
                                           width: bb.width, height: bb.height))
    }

    // ── Dò mẫu trong 1 dòng chữ ────────────────────────────────────────────
    private static func matches(in s: String) -> [(SensitiveMatch.Kind, Range<String.Index>)] {
        guard !s.isEmpty else { return [] }
        var out: [(SensitiveMatch.Kind, Range<String.Index>)] = []

        for (kind, regex, group) in regexes {
            let full = NSRange(s.startIndex..., in: s)
            for m in regex.matches(in: s, range: full) {
                // group > 0: chỉ bôi phần GIÁ TRỊ, để nguyên cái nhãn đứng trước
                // ("CVV:" còn đọc được, "231" thì không).
                let ns = group < m.numberOfRanges ? m.range(at: group) : m.range
                guard ns.location != NSNotFound, let r = Range(ns, in: s) else { continue }
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
    // có thể có hàng trăm dòng chữ. Số cuối là capture group cần bôi (0 = cả match).
    private static let regexes: [(SensitiveMatch.Kind, NSRegularExpression, Int)] = {
        let patterns: [(SensitiveMatch.Kind, String, Int)] = [
            (.email, #"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}"#, 0),
            // JWT: header.payload — cả hai phần đều bắt đầu bằng "eyJ" (base64 của '{"').
            (.token, #"eyJ[A-Za-z0-9_\-]{5,}\.eyJ[A-Za-z0-9_\-]{5,}(?:\.[A-Za-z0-9_\-]+)?"#, 0),
            // Token có tiền tố nhận dạng rõ ràng — gần như không thể dương tính giả.
            (.token, #"(?:sk-[A-Za-z0-9_\-]{16,}"#
                   + #"|ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}"#
                   + #"|AKIA[0-9A-Z]{12,}"#
                   + #"|xox[abps]-[A-Za-z0-9\-]{10,}"#
                   + #"|AIza[0-9A-Za-z_\-]{30,})"#, 0),
            (.token, #"-----BEGIN [A-Z ]*PRIVATE KEY-----"#, 0),
            // 13–19 chữ số, cho phép cách/gạch ở giữa; Luhn lọc tiếp ở trên.
            (.card,  #"(?<![0-9\-])(?:[0-9][ \-]?){12,18}[0-9](?![0-9\-])"#, 0),
            // Từ đây xuống đều CẦN CÓ NHÃN đứng trước. 3 chữ số trần thì ở đâu
            // cũng có, chỉ khi cạnh chữ "CVV" nó mới là CVV.
            (.cvv, #"(?:CVV|CVC|CVV2|CVC2|CSC|Card\s*Verification(?:\s*(?:Value|Code))?"#
                 + #"|Security\s*Code|Mã\s*bảo\s*mật)\s*[:#.]?\s*([0-9]{3,4})(?![0-9])"#, 1),
            (.expiry, #"(?:Valid\s*Thru|Valid\s*Through|Valid\s*Until|Expiration(?:\s*Date)?"#
                    + #"|Expires?|Exp(?:\.|\b)|Hết\s*hạn|Hiệu\s*lực(?:\s*đến)?)"#
                    + #"\s*[:.]?\s*([0-9]{1,2}\s*[/\-]\s*(?:[0-9]{4}|[0-9]{2}))(?![0-9])"#, 1),
            (.secret, #"(?:password|passwd|pass\s*phrase|pwd|secret|api[ _\-]?key"#
                    + #"|access[ _\-]?token|client[ _\-]?secret|private[ _\-]?key"#
                    + #"|mật\s*khẩu|mã\s*OTP|OTP)\s*[:=]\s*(\S{4,})"#, 1),
            // CCCD/CMND và SSN — cũng phải có nhãn, dãy 9–12 số trần thì đầy rẫy.
            (.idNumber, #"(?:CCCD|CMND|CMTND|Căn\s*cước(?:\s*công\s*dân)?|Số\s*CCCD"#
                      + #"|ID\s*(?:No\.?|Number)|SSN|Social\s*Security(?:\s*Number)?"#
                      + #"|Passport(?:\s*(?:No\.?|Number))?|Hộ\s*chiếu)"#
                      + #"\s*[:#.]?\s*([A-Z0-9][A-Z0-9\- ]{6,17}[A-Z0-9])"#, 1),
        ]
        return patterns.compactMap { kind, p, group in
            (try? NSRegularExpression(pattern: p, options: [.caseInsensitive]))
                .map { (kind, $0, group) }
        }
    }()

    private static let phoneDetector = try? NSDataDetector(
        types: NSTextCheckingResult.CheckingType.phoneNumber.rawValue)

    // ── Tên chủ thẻ ────────────────────────────────────────────────────────
    // Chỉ gọi khi ảnh đã có bối cảnh thẻ (xem `scan`).
    private static func cardholderNames(in s: String) -> [Range<String.Index>] {
        let trimmed = s.trimmingCharacters(in: .whitespaces)
        guard trimmed.count >= 4, trimmed.count <= 40,
              !trimmed.contains(where: \.isNumber),
              !isCardChrome(trimmed) else { return [] }

        // NLTagger biết "Eulah Harris" là tên người; ưu tiên nó.
        let tagger = NLTagger(tagSchemes: [.nameType])
        tagger.string = s
        var out: [Range<String.Index>] = []
        tagger.enumerateTags(in: s.startIndex..<s.endIndex, unit: .word, scheme: .nameType,
                             options: [.omitPunctuation, .omitWhitespace, .joinNames]) { tag, range in
            if tag == .personalName { out.append(range) }
            return true
        }
        if !out.isEmpty { return out }

        // Dự phòng: tên trên thẻ hay là "JOHN A SMITH" viết hoa hết, mà NLTagger
        // thường không nhận ra dạng đó. 2–4 từ, chữ cái thuần, không phải nhãn thẻ.
        let words = trimmed.split(separator: " ")
        guard (2...4).contains(words.count),
              words.allSatisfy({ w in w.count >= 2 && w.allSatisfy(\.isLetter)
                                && (w.first?.isUppercase ?? false) })
        else { return [] }
        // Range phải nằm trên `s` (boundingBox(for:) đòi vậy), không phải `trimmed`.
        guard let r = s.range(of: trimmed) else { return [] }
        return [r]
    }

    // Chữ in sẵn trên thẻ / nhãn form — không phải tên người, đừng bôi.
    private static let cardChrome: Set<String> = [
        "visa", "mastercard", "master card", "american express", "amex", "discover",
        "jcb", "unionpay", "union pay", "diners club", "maestro", "napas",
        "credit card", "debit card", "card type", "card number", "cardholder",
        "cardholder name", "card holder", "name on card", "valid thru", "valid through",
        "good thru", "expires", "expiry date", "member since", "number of cards",
        "generate credit card", "generate", "security code", "authorized signature",
        "chủ thẻ", "thẻ tín dụng", "thẻ ghi nợ", "ngân hàng",
    ]
    private static func isCardChrome(_ s: String) -> Bool {
        let k = s.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ":·-—"))
                 .trimmingCharacters(in: .whitespaces)
        return cardChrome.contains(k)
    }

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
