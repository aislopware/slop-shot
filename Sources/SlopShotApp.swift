import SwiftUI
import Carbon   // để dùng các hằng số modifier: controlKey, optionKey, cmdKey

// AppDelegate: nơi chạy code lúc app vừa khởi động (giống "main()" của app).
// Đánh dấu @MainActor vì mọi thứ UI/AppKit đều ở main thread.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    // 1 instance ScreenCapturer dùng chung: cửa sổ và phím tắt cùng chung trạng thái.
    let capturer = ScreenCapturer()

    func applicationDidFinishLaunching(_ notification: Notification) {
        setupMainMenu()
        registerHotkeys()
        // User đổi phím tắt trong Settings → đăng ký lại bộ mới.
        NotificationCenter.default.addObserver(
            forName: .slopShotHotkeysChanged, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.registerHotkeys() }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // App nền (LSUIElement) mặc định KHÔNG có main menu. Mà trong macOS,
    // các phím soạn thảo chuẩn (⌘C/⌘V/⌘X/⌘A/⌘Z) chỉ chạy khi có Edit menu
    // chứa đúng selector + keyEquivalent trong responder chain.
    // → Không có menu này thì ô nhập chữ (Text annotation) KHÔNG paste được.
    // Giống: web app phải tự khai báo handler cho Cmd+V chứ không "miễn phí".
    // ─────────────────────────────────────────────────────────────────────
    private func setupMainMenu() {
        let mainMenu = NSMenu()

        // Menu App. ⌘Q chỉ đóng cửa sổ đang mở (editor, OCR, Settings…), KHÔNG
        // thoát app: SlopShot là app nền sống nhờ phím tắt, ⌘Q theo thói quen ở
        // một cửa sổ phụ mà tắt luôn cả app thì phím tắt chết theo. Muốn thoát
        // thật thì dùng "Quit SlopShot" trong menu trên thanh menu.
        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appItem.submenu = appMenu
        let close = appMenu.addItem(withTitle: "Close Window",
                                    action: #selector(closeKeyWindow(_:)), keyEquivalent: "q")
        close.target = self

        // Menu Edit — bộ soạn thảo chuẩn. target = nil nghĩa là gửi theo
        // responder chain: ai đang focus (ô text) sẽ tự nhận đúng action.
        let editItem = NSMenuItem()
        mainMenu.addItem(editItem)
        let editMenu = NSMenu(title: "Edit")
        editItem.submenu = editMenu
        editMenu.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        let redo = editMenu.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(withTitle: "Select All",
                         action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")

        NSApp.mainMenu = mainMenu
    }

    // Icon trên thanh menu đã ẩn thì đây là cửa duy nhất vào lại Settings: mở
    // SlopShot lần nữa (Spotlight, Finder, `open -a`) khi nó đang chạy → macOS
    // gửi "reopen" → bật cửa sổ Settings.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        capturer.showSettings()
        return false
    }

    @objc private func closeKeyWindow(_ sender: Any?) {
        guard let win = NSApp.keyWindow ?? NSApp.mainWindow else { return }
        // Editor ẩn nút đóng; performClose vẫn đi qua delegate (hỏi lưu…) nếu
        // cửa sổ có .closable, không thì đóng thẳng.
        if win.styleMask.contains(.closable) { win.performClose(nil) } else { win.close() }
    }

    // Đăng ký TẤT CẢ phím tắt theo cấu hình hiện tại (gỡ hết rồi gắn lại).
    private func registerHotkeys() {
        let mgr = HotKeyManager.shared
        mgr.unregisterAll()
        let settings = AppSettings.shared
        for action in ShortcutAction.allCases {
            let hk = settings.hotkey(for: action)
            mgr.register(keyCode: hk.keyCode, modifiers: hk.modifiers) { [capturer] in
                Task { @MainActor in await AppDelegate.run(action, on: capturer) }
            }
        }
    }

    // Ánh xạ action → method tương ứng trên capturer.
    private static func run(_ action: ShortcutAction, on capturer: ScreenCapturer) async {
        switch action {
        case .captureArea:      await capturer.captureRegion()
        case .captureFullscreen:await capturer.captureFullScreen()
        case .recordArea:       await capturer.recordRegion()
        case .captureText:      await capturer.captureText()
        case .captureScrolling: await capturer.captureScrollingArea()
        case .pickColor:        await capturer.pickColor()
        }
    }
}

@main
struct SlopShotApp: App {
    // Gắn AppDelegate vào app SwiftUI. SwiftUI tạo & giữ nó giúp mình.
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    // Công tắc ẩn/hiện icon trên thanh menu (Settings > General).
    @ObservedObject private var settings = AppSettings.shared

    var body: some Scene {
        // MenuBarExtra = icon trên thanh menu + menu xổ xuống.
        // Không còn WindowGroup -> app không có cửa sổ chính, chạy nền hoàn toàn.
        // image: "MenuBarIcon" = asset template (chữ S) → macOS tự tô đen/trắng theo nền.
        // Binding chỉ GHI khi giá trị thật sự đổi: MenuBarExtra ghi ngược vào
        // isInserted mỗi lần cập nhật, mà @Published ghi trùng giá trị vẫn phát
        // thay đổi → body chạy lại → ghi tiếp… treo cứng (lộ rõ nhất khi mở
        // Settings, vì cửa sổ đó cũng bám AppSettings).
        MenuBarExtra("SlopShot", image: "MenuBarIcon", isInserted: Binding(
            get: { settings.showMenuBarIcon },
            set: { if settings.showMenuBarIcon != $0 { settings.showMenuBarIcon = $0 } }
        )) {
            MenuContent(capturer: appDelegate.capturer)
        }
    }
}
