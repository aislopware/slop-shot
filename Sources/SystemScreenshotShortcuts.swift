import AppKit
import Carbon   // hằng số modifier: shiftKey, cmdKey

// ─────────────────────────────────────────────────────────────────────────
// Giành ⌘⇧3 / ⌘⇧4 / ⌘⇧5 từ công cụ chụp màn hình của macOS.
//
// Phím tắt hệ thống luôn thắng: macOS nuốt ⌘⇧4 trước khi tới RegisterEventHotKey
// của mình, nên bộ mặc định ⌘⇧1…6 của SlopShot có ba phím câm cho tới khi tắt
// bên kia. Việc tắt nằm trong System Settings > Keyboard > Keyboard Shortcuts >
// Screenshots — năm công tắc, ở tận trong ba lớp menu. Ở đây một nút là xong.
//
// Chúng sống trong com.apple.symbolichotkeys, key "AppleSymbolicHotKeys": mỗi
// phím là một số ID. Ghi xong phải gọi activateSettings -u thì WindowServer
// mới đọc lại — không thì phải đăng xuất.
// ─────────────────────────────────────────────────────────────────────────
enum SystemScreenshotShortcuts {
    /// ID phím hệ thống → (ký tự ASCII, keyCode, mask modifier kiểu NSEvent) —
    /// đúng bộ giá trị macOS ghi khi tự tắt/bật trong System Settings.
    private static let entries: [(id: String, params: [Int])] = [
        ("28",  [51, 20, 1_179_648]),   // ⌘⇧3   Save picture of screen as a file
        ("29",  [51, 20, 1_441_792]),   // ⌃⌘⇧3  Copy picture of screen to the clipboard
        ("30",  [52, 21, 1_179_648]),   // ⌘⇧4   Save picture of selected area as a file
        ("31",  [52, 21, 1_441_792]),   // ⌃⌘⇧4  Copy picture of selected area to the clipboard
        ("184", [53, 23, 1_179_648]),   // ⌘⇧5   Screenshot and recording options
    ]

    private static let domain = "com.apple.symbolichotkeys" as CFString
    private static let key = "AppleSymbolicHotKeys" as CFString

    /// Phím chụp của macOS có còn cái nào đang bật không. Thiếu hẳn một ID trong
    /// plist nghĩa là chưa ai đụng vào → macOS dùng mặc định, tức là đang BẬT.
    static var systemShortcutsEnabled: Bool {
        let all = CFPreferencesCopyAppValue(key, domain) as? [String: Any] ?? [:]
        return entries.contains { e in
            guard let item = all[e.id] as? [String: Any] else { return true }
            return (item["enabled"] as? Bool) ?? true
        }
    }

    /// Các tổ hợp macOS đang giữ (khi còn bật) — để cảnh báo phím SlopShot nào
    /// đang bị nuốt mất.
    static let reserved: [Hotkey] = {
        let mods = UInt32(shiftKey | cmdKey)
        let ctrl = mods | UInt32(controlKey)
        return [Hotkey(keyCode: 20, modifiers: mods), Hotkey(keyCode: 20, modifiers: ctrl),
                Hotkey(keyCode: 21, modifiers: mods), Hotkey(keyCode: 21, modifiers: ctrl),
                Hotkey(keyCode: 23, modifiers: mods)]
    }()

    /// Tắt phím chụp của macOS để ⌘⇧3/4/5 về tay SlopShot.
    static func turnOffSystem() { setSystemShortcuts(enabled: false) }

    /// Trả phím chụp lại cho macOS.
    static func restoreSystem() { setSystemShortcuts(enabled: true) }

    private static func setSystemShortcuts(enabled: Bool) {
        var all = CFPreferencesCopyAppValue(key, domain) as? [String: Any] ?? [:]
        for e in entries {
            all[e.id] = [
                "enabled": enabled,
                "value": ["parameters": e.params, "type": "standard"],
            ] as [String: Any]
        }
        CFPreferencesSetAppValue(key, all as CFDictionary, domain)
        CFPreferencesAppSynchronize(domain)
        applyNow()
    }

    /// Bảo hệ thống nạp lại phím tắt ngay, khỏi phải đăng xuất.
    private static func applyNow() {
        let task = Process()
        task.executableURL = URL(fileURLWithPath:
            "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings")
        task.arguments = ["-u"]
        task.standardOutput = FileHandle.nullDevice
        task.standardError = FileHandle.nullDevice
        do {
            try task.run()
            task.waitUntilExit()
        } catch {
            NSLog("SlopShot: activateSettings failed: \(error)")
        }
    }
}
