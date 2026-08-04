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

        // Menu App (chỉ cần Quit).
        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appItem.submenu = appMenu
        appMenu.addItem(withTitle: "Quit SlopShot",
                        action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

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

    var body: some Scene {
        // MenuBarExtra = icon trên thanh menu + menu xổ xuống.
        // Không còn WindowGroup -> app không có cửa sổ chính, chạy nền hoàn toàn.
        // image: "MenuBarIcon" = asset template (chữ S) → macOS tự tô đen/trắng theo nền.
        MenuBarExtra("SlopShot", image: "MenuBarIcon") {
            MenuContent(capturer: appDelegate.capturer)
        }
    }
}
