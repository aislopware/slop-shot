import SwiftUI
import UniformTypeIdentifiers
import ImageIO    // CGImageDestination — mã hoá được nhiều định dạng kể cả HEIC
import ServiceManagement // SMAppService — đăng ký khởi động cùng máy

// ─────────────────────────────────────────────────────────────────────────
// Cấu hình app, lưu xuống UserDefaults (giống 1 store Zustand có persist).
// @Published + didSet: hễ đổi là ghi đĩa ngay, UI bám vào tự cập nhật.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class AppSettings: ObservableObject {
    static let shared = AppSettings()

    // Định dạng ảnh khi bấm Save.
    enum ImageFormat: String, CaseIterable, Identifiable {
        case png, jpeg, heic, tiff, gif, bmp
        var id: String { rawValue }
        var label: String {
            switch self {
            case .png:  return "PNG"
            case .jpeg: return "JPEG"
            case .heic: return "HEIC"
            case .tiff: return "TIFF"
            case .gif:  return "GIF"
            case .bmp:  return "BMP"
            }
        }
        var ext: String {
            switch self {
            case .jpeg: return "jpg"
            case .heic: return "heic"
            default:    return rawValue
            }
        }
        // UTType để ImageIO biết ghi ra định dạng nào.
        var utType: UTType {
            switch self {
            case .png:  return .png
            case .jpeg: return .jpeg
            case .heic: return .heic
            case .tiff: return .tiff
            case .gif:  return .gif
            case .bmp:  return .bmp
            }
        }
        // Định dạng "mất dữ liệu" thì có nén chất lượng.
        var isLossy: Bool { self == .jpeg || self == .heic }

        // Mã hoá CGImage → Data theo định dạng này (một đường dùng chung cho tất cả).
        func encode(_ cg: CGImage) -> Data? {
            let data = NSMutableData()
            guard let dest = CGImageDestinationCreateWithData(
                data, utType.identifier as CFString, 1, nil) else { return nil }
            let options = isLossy ? [kCGImageDestinationLossyCompressionQuality: 0.9] : [:]
            CGImageDestinationAddImage(dest, cg, options as CFDictionary)
            guard CGImageDestinationFinalize(dest) else { return nil }
            return data as Data
        }
    }

    // Kiểu chuỗi màu mà Color Picker chép vào clipboard.
    enum ColorFormat: String, CaseIterable, Identifiable {
        case hex, hexLower, rgb, hsl, swiftUI, nsColor
        var id: String { rawValue }

        var label: String {
            switch self {
            case .hex:      return "HEX  #A4773F"
            case .hexLower: return "hex  #a4773f"
            case .rgb:      return "CSS  rgb(…)"
            case .hsl:      return "CSS  hsl(…)"
            case .swiftUI:  return "SwiftUI Color"
            case .nsColor:  return "AppKit NSColor"
            }
        }

        /// Tên ngắn hiện cạnh giá trị lúc đang chọn màu.
        var shortLabel: String {
            switch self {
            case .hex:      return "HEX"
            case .hexLower: return "hex"
            case .rgb:      return "RGB"
            case .hsl:      return "HSL"
            case .swiftUI:  return "SwiftUI"
            case .nsColor:  return "NSColor"
            }
        }

        func string(from color: NSColor) -> String {
            let c = color.usingColorSpace(.sRGB) ?? color
            let r = c.redComponent, g = c.greenComponent, b = c.blueComponent
            let r8 = Int((r * 255).rounded()), g8 = Int((g * 255).rounded()), b8 = Int((b * 255).rounded())
            switch self {
            case .hex:      return String(format: "#%02X%02X%02X", r8, g8, b8)
            case .hexLower: return String(format: "#%02x%02x%02x", r8, g8, b8)
            case .rgb:      return "rgb(\(r8), \(g8), \(b8))"
            case .hsl:
                let (h, s, l) = Self.hsl(r, g, b)
                return "hsl(\(Int(h.rounded())), \(Int((s * 100).rounded()))%, \(Int((l * 100).rounded()))%)"
            case .swiftUI:
                return String(format: "Color(red: %.3f, green: %.3f, blue: %.3f)", r, g, b)
            case .nsColor:
                return String(format: "NSColor(srgbRed: %.3f, green: %.3f, blue: %.3f, alpha: 1)", r, g, b)
            }
        }

        // RGB (0…1) → HSL: hue độ, saturation/lightness tỉ lệ 0…1.
        private static func hsl(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat) -> (CGFloat, CGFloat, CGFloat) {
            let mx = max(r, g, b), mn = min(r, g, b)
            let l = (mx + mn) / 2
            guard mx > mn else { return (0, 0, l) }           // xám: hue vô nghĩa
            let d = mx - mn
            let s = l > 0.5 ? d / (2 - mx - mn) : d / (mx + mn)
            var h: CGFloat
            switch mx {
            case r:  h = (g - b) / d + (g < b ? 6 : 0)
            case g:  h = (b - r) / d + 2
            default: h = (r - g) / d + 4
            }
            return (h * 60, s, l)
        }
    }

    // Thư mục lưu (lưu dạng path, expand ~ khi dùng). App không sandbox nên path trần là đủ.
    @Published var saveFolderPath: String { didSet { d.set(saveFolderPath, forKey: K.folder) } }
    @Published var copyToClipboard: Bool   { didSet { d.set(copyToClipboard, forKey: K.copy) } }
    @Published var showThumbnail: Bool     { didSet { d.set(showThumbnail, forKey: K.thumb) } }
    @Published var imageFormat: ImageFormat { didSet { d.set(imageFormat.rawValue, forKey: K.format) } }
    // Bắt dính khung chọn vào cạnh cửa sổ / biên item dò được trong ảnh.
    @Published var snapToEdges: Bool       { didSet { d.set(snapToEdges, forKey: K.snap) } }
    // Kiểu chuỗi màu Color Picker chép ra (đổi được ngay lúc đang chọn bằng ← →).
    @Published var colorFormat: ColorFormat { didSet { d.set(colorFormat.rawValue, forKey: K.color) } }

    // Không lưu UserDefaults — SMAppService.mainApp.status mới là nguồn sự thật
    // (user có thể tắt thủ công trong System Settings > Login Items).
    @Published private(set) var launchAtLogin: Bool = SMAppService.mainApp.status == .enabled

    func setLaunchAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            launchAtLogin = enabled
        } catch {
            // Đăng ký thất bại (vd. đang chạy từ Xcode debug build) → giữ nguyên trạng thái cũ.
            refreshLaunchAtLogin()
        }
    }

    // Đồng bộ lại khi có khả năng trạng thái đã đổi ở nơi khác (mở lại Settings).
    func refreshLaunchAtLogin() {
        launchAtLogin = SMAppService.mainApp.status == .enabled
    }

    // Phím tắt do user gán (theo action.rawValue). Thiếu key nào → dùng mặc định.
    @Published private var hotkeyStore: [String: Hotkey] {
        didSet { d.set(try? JSONEncoder().encode(hotkeyStore), forKey: K.hotkeys) }
    }

    func hotkey(for action: ShortcutAction) -> Hotkey {
        hotkeyStore[action.rawValue] ?? action.defaultHotkey
    }
    func setHotkey(_ hk: Hotkey, for action: ShortcutAction) {
        hotkeyStore[action.rawValue] = hk
        NotificationCenter.default.post(name: .slopShotHotkeysChanged, object: nil)
    }
    func resetHotkey(for action: ShortcutAction) {
        hotkeyStore[action.rawValue] = nil
        NotificationCenter.default.post(name: .slopShotHotkeysChanged, object: nil)
    }

    var saveFolderURL: URL {
        URL(fileURLWithPath: (saveFolderPath as NSString).expandingTildeInPath, isDirectory: true)
    }
    // Hiện gọn "~/Desktop" cho đẹp khi path nằm trong home.
    var saveFolderDisplay: String {
        let home = NSHomeDirectory()
        let full = saveFolderURL.path
        return full.hasPrefix(home) ? "~" + full.dropFirst(home.count) : full
    }

    private let d = UserDefaults.standard
    private enum K {
        static let folder = "save.folder", copy = "save.copy"
        static let thumb = "save.thumb", format = "save.format", hotkeys = "hotkeys"
        static let snap = "select.snap", color = "pick.color.format"
    }

    private init() {
        let desktop = FileManager.default
            .urls(for: .desktopDirectory, in: .userDomainMask).first?.path ?? "~/Desktop"
        saveFolderPath  = d.string(forKey: K.folder) ?? desktop
        // object(forKey:) == nil nghĩa là chưa từng set → mặc định bật.
        copyToClipboard = (d.object(forKey: K.copy) as? Bool) ?? true
        showThumbnail   = (d.object(forKey: K.thumb) as? Bool) ?? true
        imageFormat     = ImageFormat(rawValue: d.string(forKey: K.format) ?? "png") ?? .png
        snapToEdges     = (d.object(forKey: K.snap) as? Bool) ?? true
        colorFormat     = ColorFormat(rawValue: d.string(forKey: K.color) ?? "hex") ?? .hex
        if let raw = d.data(forKey: K.hotkeys),
           let saved = try? JSONDecoder().decode([String: Hotkey].self, from: raw) {
            hotkeyStore = saved
        } else {
            hotkeyStore = [:]
        }
    }
}
