import Carbon   // hằng số modifier + bảng mã phím
import AppKit
import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// 1 tổ hợp phím tắt: mã phím + mask modifier (theo chuẩn Carbon để đăng ký).
// ─────────────────────────────────────────────────────────────────────────
struct Hotkey: Codable, Equatable {
    var keyCode: UInt32
    var modifiers: UInt32        // mask Carbon: controlKey | optionKey | shiftKey | cmdKey

    var hasModifier: Bool { modifiers != 0 }

    // Đổi cờ modifier của NSEvent (lúc bắt phím) → mask Carbon (lúc đăng ký).
    static func carbonModifiers(from flags: NSEvent.ModifierFlags) -> UInt32 {
        var m: UInt32 = 0
        if flags.contains(.control) { m |= UInt32(controlKey) }
        if flags.contains(.option)  { m |= UInt32(optionKey) }
        if flags.contains(.shift)   { m |= UInt32(shiftKey) }
        if flags.contains(.command) { m |= UInt32(cmdKey) }
        return m
    }

    // Chuỗi hiển thị kiểu "⌃⌥⌘4".
    var display: String {
        var s = ""
        if modifiers & UInt32(controlKey) != 0 { s += "⌃" }
        if modifiers & UInt32(optionKey)  != 0 { s += "⌥" }
        if modifiers & UInt32(shiftKey)   != 0 { s += "⇧" }
        if modifiers & UInt32(cmdKey)     != 0 { s += "⌘" }
        return s + Hotkey.keyLabel(keyCode)
    }

    // Bảng mã phím → ký tự (đủ cho phím thường; phím lạ thì hiện "key<n>").
    static func keyLabel(_ code: UInt32) -> String { keyMap[code] ?? "key\(code)" }

    private static let keyMap: [UInt32: String] = [
        0:"A",1:"S",2:"D",3:"F",4:"H",5:"G",6:"Z",7:"X",8:"C",9:"V",11:"B",
        12:"Q",13:"W",14:"E",15:"R",16:"Y",17:"T",
        18:"1",19:"2",20:"3",21:"4",22:"6",23:"5",24:"=",25:"9",26:"7",27:"-",28:"8",29:"0",
        30:"]",31:"O",32:"U",33:"[",34:"I",35:"P",36:"↩",37:"L",38:"J",39:"'",40:"K",
        41:";",42:"\\",43:",",44:"/",45:"N",46:"M",47:".",48:"⇥",49:"Space",50:"`",51:"⌫",53:"⎋",
        123:"←",124:"→",125:"↓",126:"↑",
        122:"F1",120:"F2",99:"F3",118:"F4",96:"F5",97:"F6",98:"F7",100:"F8",101:"F9",109:"F10",103:"F11",111:"F12",
    ]
}

// ─────────────────────────────────────────────────────────────────────────
// Các hành động có thể gán phím tắt. Mỗi cái có tiêu đề + phím mặc định.
// ─────────────────────────────────────────────────────────────────────────
enum ShortcutAction: String, CaseIterable, Identifiable {
    case captureArea, captureFullscreen, recordArea, captureText, captureScrolling, pickColor
    var id: String { rawValue }

    var title: String {
        switch self {
        case .captureArea:      return "Capture Area"
        case .captureFullscreen:return "Capture Fullscreen"
        case .recordArea:       return "Record Area"
        case .captureText:      return "Capture Text (OCR)"
        case .captureScrolling: return "Capture Scrolling Area"
        case .pickColor:        return "Pick Color"
        }
    }

    var defaultHotkey: Hotkey {
        let mods = UInt32(controlKey | optionKey | cmdKey)   // ⌃⌥⌘
        switch self {
        case .captureArea:      return Hotkey(keyCode: 21, modifiers: mods)   // 4
        case .captureFullscreen:return Hotkey(keyCode: 20, modifiers: mods)   // 3
        case .recordArea:       return Hotkey(keyCode: 23, modifiers: mods)   // 5
        case .captureText:      return Hotkey(keyCode: 22, modifiers: mods)   // 6
        case .captureScrolling: return Hotkey(keyCode: 26, modifiers: mods)   // 7
        case .pickColor:        return Hotkey(keyCode: 28, modifiers: mods)   // 8
        }
    }
}

extension Notification.Name {
    // Bắn ra khi user đổi phím tắt → AppDelegate đăng ký lại.
    static let slopShotHotkeysChanged = Notification.Name("slopShotHotkeysChanged")
}

// ─────────────────────────────────────────────────────────────────────────
// Cầu nối Hotkey (mã Carbon) → kiểu của SwiftUI, để menu hiện đúng hint phím
// tắt theo cấu hình hiện tại (kể cả sau khi rebind), thay vì hardcode.
// ─────────────────────────────────────────────────────────────────────────
extension Hotkey {
    var swiftUIModifiers: SwiftUI.EventModifiers {
        var m: SwiftUI.EventModifiers = []
        if modifiers & UInt32(controlKey) != 0 { m.insert(.control) }
        if modifiers & UInt32(optionKey)  != 0 { m.insert(.option) }
        if modifiers & UInt32(shiftKey)   != 0 { m.insert(.shift) }
        if modifiers & UInt32(cmdKey)     != 0 { m.insert(.command) }
        return m
    }

    var keyEquivalent: KeyEquivalent? {
        switch keyCode {
        case 123: return .leftArrow
        case 124: return .rightArrow
        case 125: return .downArrow
        case 126: return .upArrow
        case 36:  return .return
        case 48:  return .tab
        case 49:  return .space
        case 51:  return .delete
        case 53:  return .escape
        default:
            let label = Hotkey.keyLabel(keyCode)
            guard label.count == 1, let ch = label.lowercased().first else { return nil }
            return KeyEquivalent(ch)
        }
    }
}
