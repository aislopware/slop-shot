import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Chớp trắng nhẹ đúng vùng vừa chụp — cùng lúc với tiếng màn trập, để mắt thấy
// "đã chụp" ngay cả khi tắt tiếng. Panel không nhận chuột, tự gỡ sau khi tắt.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
enum ScreenFlash {
    /// `rect`: points, gốc TRÊN-trái của `screen` (cùng hệ với vùng chọn).
    /// nil = cả màn hình.
    static func flash(_ rect: CGRect? = nil, on screen: NSScreen) {
        let sf = screen.frame
        let r = rect ?? CGRect(origin: .zero, size: sf.size)
        let frame = NSRect(x: sf.minX + r.minX, y: sf.maxY - r.maxY, width: r.width, height: r.height)

        let p = NSPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel],
                        backing: .buffered, defer: false)
        p.level = .screenSaver
        p.backgroundColor = .white
        p.isOpaque = false
        p.hasShadow = false
        p.ignoresMouseEvents = true
        p.animationBehavior = .none
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        p.alphaValue = 0.45
        p.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.35
            ctx.timingFunction = CAMediaTimingFunction(name: .easeOut)
            p.animator().alphaValue = 0
        }, completionHandler: { p.orderOut(nil) })
    }
}
