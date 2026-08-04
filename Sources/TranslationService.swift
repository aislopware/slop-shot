import Foundation
import NaturalLanguage   // dò ngôn ngữ nguồn
import Translation       // framework dịch của Apple (macOS 15+), chạy offline

// ─────────────────────────────────────────────────────────────────────────
// Dịch chữ bằng framework Translation của Apple.
//
// Vì sao không gọi API Google như macshot: bản đó xài endpoint không chính
// thức (translate.googleapis.com/translate_a/single) — không key, dễ bị chặn,
// và gửi chữ của user ra ngoài. Framework của Apple thì miễn phí, chạy
// OFFLINE sau lần tải gói ngôn ngữ đầu tiên, và hỗ trợ đủ 38 thứ tiếng
// (có vi-VN) — kiểm bằng LanguageAvailability trên chính máy này.
//
// Phần "chạy dịch" nằm ở view (modifier .translationTask) vì Apple chỉ cấp
// TranslationSession qua SwiftUI; file này lo danh sách ngôn ngữ + dò nguồn.
// ─────────────────────────────────────────────────────────────────────────
enum TranslationService {

    struct Language: Identifiable, Hashable {
        let code: String
        let name: String
        var id: String { code }
    }

    // Danh sách rút từ LanguageAvailability().supportedLanguages (macOS 15).
    static let languages: [Language] = [
        .init(code: "en",      name: "English"),
        .init(code: "vi",      name: "Tiếng Việt"),
        .init(code: "zh-Hans", name: "中文 (简体)"),
        .init(code: "zh-Hant", name: "中文 (繁體)"),
        .init(code: "ja",      name: "日本語"),
        .init(code: "ko",      name: "한국어"),
        .init(code: "th",      name: "ไทย"),
        .init(code: "id",      name: "Bahasa Indonesia"),
        .init(code: "hi",      name: "हिन्दी"),
        .init(code: "ar",      name: "العربية"),
        .init(code: "ru",      name: "Русский"),
        .init(code: "uk",      name: "Українська"),
        .init(code: "fr",      name: "Français"),
        .init(code: "de",      name: "Deutsch"),
        .init(code: "es",      name: "Español"),
        .init(code: "it",      name: "Italiano"),
        .init(code: "pt-BR",   name: "Português (Brasil)"),
        .init(code: "pt-PT",   name: "Português (Portugal)"),
        .init(code: "nl",      name: "Nederlands"),
        .init(code: "pl",      name: "Polski"),
        .init(code: "tr",      name: "Türkçe"),
        .init(code: "sv",      name: "Svenska"),
        .init(code: "da",      name: "Dansk"),
        .init(code: "nb",      name: "Norsk bokmål"),
    ]

    /// Ngôn ngữ đích đang chọn (nhớ lại cho lần sau).
    static var target: String {
        get {
            if let saved = UserDefaults.standard.string(forKey: key),
               languages.contains(where: { $0.code == saved }) { return saved }
            // Lần đầu: lấy theo ngôn ngữ hệ thống, không khớp thì tiếng Anh.
            let sys = Locale.current.language.languageCode?.identifier ?? "en"
            return languages.first { $0.code == sys || $0.code.hasPrefix(sys + "-") }?.code ?? "en"
        }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }
    private static let key = "translate.target"

    static func name(of code: String) -> String {
        languages.first { $0.code == code }?.name ?? code
    }

    /// Dò ngôn ngữ nguồn để KHỎI bị Apple bật hộp thoại "Choose Language".
    /// Không dò ra (chữ quá ngắn / lẫn lộn) thì trả nil — lúc đó để framework tự lo.
    static func detectSource(of text: String) -> Locale.Language? {
        let sample = String(text.prefix(1000))
        guard sample.trimmingCharacters(in: .whitespacesAndNewlines).count >= 4 else { return nil }
        let recognizer = NLLanguageRecognizer()
        recognizer.processString(sample)
        guard let lang = recognizer.dominantLanguage, lang != .undetermined else { return nil }
        return Locale.Language(identifier: lang.rawValue)
    }

    /// Ngôn ngữ đích mặc định thông minh: đang đọc tiếng Việt thì dịch sang Anh
    /// và ngược lại, khỏi phải đổi picker mỗi lần.
    static func suggestedTarget(for text: String) -> String {
        let saved = target
        guard let src = detectSource(of: text)?.languageCode?.identifier else { return saved }
        guard src == saved || saved.hasPrefix(src + "-") else { return saved }
        return src == "en" ? "vi" : "en"
    }
}
