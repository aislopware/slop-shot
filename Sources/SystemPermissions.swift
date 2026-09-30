import AppKit
import ApplicationServices

// ─────────────────────────────────────────────────────────────────────────
// Các quyền macOS mà SlopShot cần, kèm cách đọc trạng thái và mở đúng trang
// trong System Settings.
//
// Vì sao phải liệt kê trong Settings: hộp thoại xin quyền của macOS chỉ hiện
// MỘT lần cho mỗi bản app. Bấm nhầm Deny là từ đó im lặng — app không chụp
// được / không tự cuộn được mà chẳng báo gì, và cũng không có đường nào trong
// app để bật lại. Cài đè bản mới cũng xoá luôn quyền cũ (bundle bị thay thì
// macOS dọn dòng TCC của nó).
// ─────────────────────────────────────────────────────────────────────────
enum SystemPermission: String, CaseIterable, Identifiable {
    case screenRecording, accessibility
    var id: String { rawValue }

    var title: String {
        switch self {
        case .screenRecording: return "Screen & System Audio Recording"
        case .accessibility:   return "Accessibility"
        }
    }

    var icon: String {
        switch self {
        case .screenRecording: return "rectangle.dashed.badge.record"
        case .accessibility:   return "figure.walk.circle"
        }
    }

    /// Mất quyền này thì hỏng cái gì — nói bằng triệu chứng người dùng thấy,
    /// không phải bằng tên API.
    var purpose: String {
        switch self {
        case .screenRecording:
            return "Every screenshot and screen recording. Without it captures come out empty."
        case .accessibility:
            return "Scrolling capture: auto-scroll, and reading how far the page has moved."
        }
    }

    /// Chỉ ĐỌC trạng thái, tuyệt đối không dùng bản có prompt — hàm này bị gọi
    /// lại mỗi 2 giây, prompt thì cứ 2 giây một hộp thoại.
    var isGranted: Bool {
        switch self {
        case .screenRecording: return CGPreflightScreenCaptureAccess()
        case .accessibility:   return AXIsProcessTrusted()
        }
    }

    /// Bật xong có chạy được ngay không, hay phải mở lại app.
    var needsRelaunch: Bool { self == .screenRecording }

    private var pane: String {
        switch self {
        case .screenRecording: return "Privacy_ScreenCapture"
        case .accessibility:   return "Privacy_Accessibility"
        }
    }

    /// Tên dịch vụ TCC mà `tccutil` hiểu.
    private var tccService: String {
        switch self {
        case .screenRecording: return "ScreenCapture"
        case .accessibility:   return "Accessibility"
        }
    }

    /// Mở đúng trang trong System Settings — và nếu quyền đang tắt thì làm cho
    /// SlopShot CÓ MẶT sẵn trong danh sách ở trang đó.
    ///
    /// macOS chỉ thêm app vào danh sách khi app XIN quyền. Chỉ mở trang thôi thì
    /// thường không thấy SlopShot đâu (cài đè bản mới là dòng cũ bị dọn), người
    /// dùng phải tự bấm + đi tìm file .app. Còn một ca xấu hơn: dòng cũ còn đó,
    /// công tắc bật, nhưng thuộc về chữ ký của bản build trước → không có tác
    /// dụng gì với bản đang chạy. Nên: xoá dòng của CHÍNH app này (không cần quyền
    /// admin), xin lại để macOS ghi một dòng mới đúng chữ ký, rồi mới mở trang —
    /// người dùng chỉ còn việc gạt công tắc.
    func openSystemSettings() {
        if !isGranted {
            resetOwnEntry()
            requestAccess()
        }
        guard let url = URL(string:
            "x-apple.systempreferences:com.apple.preference.security?\(pane)") else { return }
        NSWorkspace.shared.open(url)
    }

    private func requestAccess() {
        switch self {
        case .screenRecording:
            _ = CGRequestScreenCaptureAccess()
        case .accessibility:
            let key = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
            _ = AXIsProcessTrustedWithOptions([key: true] as CFDictionary)
        }
    }

    private func resetOwnEntry() {
        guard let bundleID = Bundle.main.bundleIdentifier else { return }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
        task.arguments = ["reset", tccService, bundleID]
        task.standardOutput = FileHandle.nullDevice
        task.standardError = FileHandle.nullDevice
        do {
            try task.run()
            task.waitUntilExit()
        } catch {
            NSLog("SlopShot: tccutil reset \(tccService) failed: \(error)")
        }
    }
}
