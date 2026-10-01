import ScreenCaptureKit   // API của Apple để chụp/record màn hình (CleanShot cũng xài cái này)
import AppKit             // NSBitmapImageRep, NSScreen, NSWorkspace...
import AVFoundation        // tạo ảnh poster từ video (1 frame đầu)
import UniformTypeIdentifiers   // UTType.quickTimeMovie cho save panel

// "Store" nhỏ: chứa logic chụp + state để UI bám vào.
// ObservableObject ~ 1 store Zustand. @Published ~ các slice state.
// @MainActor: đảm bảo cập nhật state luôn chạy trên main thread (UI thread).
@MainActor
final class ScreenCapturer: ObservableObject {

    @Published var lastStatus: String = ""   // rỗng = chưa làm gì → menu không hiện dòng status
    @Published var lastSavedURL: URL?
    @Published var isRecording = false   // đang quay vùng hay không

    private let selection = RegionSelectionController()
    private let thumbnail = ThumbnailController()
    private let editor = EditorWindowController()
    private let recorder = ScreenRecorder()
    private let videoViewer = VideoPlayerWindowController()       // 👁 Quick Look clip trong app
    private let videoEditor = VideoEditorWindowController()       // ✂️ cắt/zoom/che/chữ + xuất file
    private let recordingBar = RecordingBarController()
    private let recordingOverlay = RecordingOverlayController()   // dim + viền focus
    private let clickEffect = ClickEffectController()             // vòng tròn click
    private let scrollCapture = ScrollCaptureController()         // chụp cuộn (ghép ảnh dài)
    private let settings = AppSettings.shared                     // cấu hình (folder/format…)
    private let history = CaptureHistory.shared                   // lịch sử capture
    private let historyWindow = HistoryWindowController()
    private let ocrWindow = OCRWindowController()                 // kết quả OCR + dịch
    private let colorPicker = ColorPickerController()             // hút màu trên màn hình
    private let settingsWindow = SettingsWindowController()
    private var recRect: CGRect = .zero      // vùng đang quay (để Restart)
    private var recScreen: NSScreen?         // màn hình đang quay
    private var lastImage: NSImage?   // giữ ảnh gần nhất để mở editor

    /// Đang có một lượt chụp dở (overlay chọn vùng, chụp cuộn…). Bấm phím tắt
    /// thêm lần nữa lúc này từng mở chồng overlay thứ hai lên, overlay cũ bị bỏ
    /// rơi nằm đè màn hình mãi. Giờ phím tắt thứ hai bị bỏ qua.
    private var busy = false

    /// Danh sách màn hình/app của ScreenCaptureKit, hỏi sẵn một lần rồi dùng lại.
    /// Hỏi lại mỗi lần bấm phím tắt là một nhịp chờ tccd + WindowServer trước khi
    /// overlay kịp hiện. Đổi màn hình (cắm/rút, đổi độ phân giải) thì bỏ cache.
    private var shareable: SCShareableContent?
    private var screenObserver: NSObjectProtocol?

    init() {
        screenObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.shareable = nil }
        }
        // Làm nóng sẵn để phím tắt đầu tiên cũng nhanh như các lần sau.
        if CGPreflightScreenCaptureAccess() {
            Task { _ = try? await self.shareableContent() }
        }
    }

    // Chỉ ẢNH mới mở được editor; clip video thì không (lastImage = nil).
    var canEditLast: Bool { lastImage != nil }

    // Bắt đầu chụp/quay mới → tự đóng MỌI cửa sổ đang mở (editor ảnh, Quick Look,
    // Video Editor) mà KHÔNG lưu — giống CleanShot. Tránh việc thao tác mới nhảy
    // về cái cũ.
    /// Trả về false nếu người dùng muốn giữ editor đang có nét vẽ chưa lưu —
    /// khi đó lượt chụp mới thôi, không đóng gì cả.
    private func dismissOpenEditors() -> Bool {
        if editor.hasUnsavedAnnotations {
            NSApp.activate()
            let alert = NSAlert()
            alert.messageText = "Discard your annotations?"
            alert.informativeText = "The editor is still open with drawings you haven't copied or saved. Starting a new capture closes it."
            alert.addButton(withTitle: "Discard & Capture")
            alert.addButton(withTitle: "Keep Editing")
            guard alert.runModal() == .alertFirstButtonReturn else { return false }
        }
        editor.dismiss()
        videoViewer.dismiss()
        videoEditor.dismiss()
        ocrWindow.dismiss()
        return true
    }

    /// Cổng vào chung của mọi lượt chụp: không chồng lượt, có quyền, dọn editor.
    /// Qua được thì `busy` bật — người gọi phải `defer { busy = false }`.
    private func beginSession() -> Bool {
        guard !busy, ensureScreenAccess(), dismissOpenEditors() else { return false }
        busy = true
        thumbnail.hide()       // cửa sổ của chính app đã bị loại khỏi ảnh, khỏi cần chờ
        return true
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Quyền Screen Recording.
    //
    // Thiếu quyền thì MỌI đường chụp đều ném lỗi, mà lỗi chỉ ghi vào lastStatus
    // (nằm trong menu, không ai mở ra xem) → nhìn như "bấm phím tắt xong chả ra
    // gì". Kiểm tra trước và nói thẳng ra bằng hộp thoại.
    //
    // Hay dính nhất là sau khi cài đè bản mới vào /Applications: macOS coi đó là
    // một app khác nếu chữ ký đổi, quyền cũ không còn hiệu lực.
    // ═══════════════════════════════════════════════════════════════════════
    private func ensureScreenAccess() -> Bool {
        if CGPreflightScreenCaptureAccess() { return true }

        // Đưa SlopShot vào danh sách trong System Settings để người dùng chỉ còn
        // việc gạt công tắc (prompt của macOS chỉ hiện 1 lần cho mỗi bản app;
        // lần sau nó im nên vẫn phải tự chỉ đường bên dưới).
        SystemPermission.registerScreenCapture()
        lastStatus = "❌ Screen Recording permission is off — SlopShot can't capture anything."

        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "SlopShot needs Screen Recording permission"
        alert.informativeText = """
            macOS is blocking screen capture, so every screenshot comes out empty.

            Open System Settings › Privacy & Security › Screen & System Audio \
            Recording, turn SlopShot on, then quit and reopen SlopShot.

            If SlopShot is already listed and enabled, remove it with the “–” \
            button and add it again — reinstalling the app invalidates the old entry.
            """
        alert.addButton(withTitle: "Open System Settings")
        alert.addButton(withTitle: "Later")
        if alert.runModal() == .alertFirstButtonReturn,
           let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
            NSWorkspace.shared.open(url)
        }
        return false
    }

    /// Báo lỗi ra tận mặt thay vì chỉ nhét vào menu.
    private func report(_ error: Error) {
        lastStatus = "❌ \(error.localizedDescription)"
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = "Capture failed"
        alert.informativeText = error.localizedDescription
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }

    // Mở editor cho ảnh gần nhất (gọi từ menu).
    func openLastInEditor() {
        guard let img = lastImage else { return }
        editor.open(image: img, sourceURL: lastSavedURL)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // BƯỚC 1: chụp toàn màn hình chính.
    // ═══════════════════════════════════════════════════════════════════════
    func captureFullScreen() async {
        guard beginSession() else { return }   // chụp mới → tự đóng editor cũ, như CleanShot
        defer { busy = false }

        do {
            // Màn hình đang có chuột, như mọi chế độ khác — không phải màn "chính".
            let screen = screenUnderCursor()
            let cgImage = try await captureDisplay(on: screen)
            ScreenFlash.flash(on: screen)
            finishImage(cgImage, subtitle: "\(cgImage.width)×\(cgImage.height)px", on: screen)
        } catch {
            report(error)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // BƯỚC 2: kéo chuột chọn vùng rồi chụp đúng vùng đó.
    // ═══════════════════════════════════════════════════════════════════════
    func captureRegion() async {
        guard beginSession() else { return }
        defer { busy = false }
        guard let picked = await selectArea(editInline: settings.editAfterSelect) else {
            lastStatus = "Selection cancelled."
            return
        }
        let (rect, screen, frozen) = (picked.rect, picked.screen, picked.frozen)

        // Vẽ ngay trên vùng chọn (kiểu macshot) cần ảnh đóng băng để làm nền.
        if settings.editAfterSelect, let frozen {
            await editInPlace(frozen, rect: rect, on: screen)
            return
        }

        do {
            // Có ảnh đóng băng thì chỉ việc cắt — không cần chụp lại, không cần chờ.
            let cropped: CGImage
            if let frozen {
                cropped = try crop(frozen, to: rect, on: screen)
            } else {
                try? await Task.sleep(nanoseconds: 150_000_000)   // đợi overlay biến mất
                cropped = try await captureCropped(rect: rect, on: screen)
            }
            ScreenFlash.flash(rect, on: screen)
            finishImage(cropped, subtitle: "\(cropped.width)×\(cropped.height)px", on: screen)
        } catch {
            report(error)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // OCR: kéo chọn vùng → đọc chữ trong vùng → copy thẳng text vào clipboard.
    // ═══════════════════════════════════════════════════════════════════════
    func captureText() async {
        guard beginSession() else { return }
        defer { busy = false }
        guard let picked = await selectArea(tool: "Capture Text", toolIcon: "text.viewfinder",
                                            confirmTitle: "Read Text", confirmIcon: "text.viewfinder")
        else { lastStatus = "Text capture cancelled."; return }
        let (rect, screen, frozen) = (picked.rect, picked.screen, picked.frozen)

        do {
            let cropped: CGImage
            if let frozen {
                cropped = try crop(frozen, to: rect, on: screen)
            } else {
                try? await Task.sleep(nanoseconds: 150_000_000)
                cropped = try await captureCropped(rect: rect, on: screen)
            }
            let ocr = await TextRecognizer.scan(in: cropped)
            guard !ocr.isEmpty else { lastStatus = "No text found in the selected area."; return }

            let copyText = ocr.copyText
            let pb = NSPasteboard.general
            pb.clearContents()
            pb.setString(copyText, forType: .string)
            let count = copyText.count
            history.add(kind: .text, fileURL: nil, text: copyText,
                        subtitle: "\(count) chars", image: nil)

            // Hiện cửa sổ kết quả: đối chiếu ảnh ↔ chữ, sửa tay, dịch.
            let preview = NSImage(cgImage: cropped,
                                  size: NSSize(width: cropped.width, height: cropped.height))
            ocrWindow.show(result: ocr, image: preview)
            lastStatus = "✅ Copied \(count) character\(count == 1 ? "" : "s") of text to clipboard."
        } catch {
            report(error)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Hút màu: đóng băng màn hình → soi từng pixel → chép mã màu ra clipboard.
    // ═══════════════════════════════════════════════════════════════════════
    func pickColor() async {
        guard beginSession() else { return }
        defer { busy = false }
        let screen = screenUnderCursor()

        // Không có ảnh đóng băng thì không đọc được pixel nào — khác các luồng
        // chụp khác, ở đây không có đường lui nào cả.
        guard let frozen = try? await captureDisplay(on: screen) else {
            report(NSError(domain: "SlopShot", code: 4, userInfo: [
                NSLocalizedDescriptionKey: "Couldn't read the screen to pick a color from."]))
            return
        }

        let picked: NSColor? = await withCheckedContinuation { cont in
            colorPicker.begin(on: screen, frozen: frozen) { cont.resume(returning: $0) }
        }
        guard let color = picked else { lastStatus = "Color pick cancelled."; return }

        // Đọc định dạng SAU khi chọn xong: user đổi bằng ← → lúc đang soi thì
        // chép ra đúng cái họ đang nhìn thấy.
        let text = settings.colorFormat.string(from: color)
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)

        history.add(kind: .color, fileURL: nil, text: text,
                    subtitle: OverlayChrome.hex(of: color), image: swatch(color))
        lastStatus = "\u{2705} Copied \(text) to clipboard."
    }

    // Ô màu đặc làm thumbnail cho dòng lịch sử.
    private func swatch(_ color: NSColor) -> NSImage {
        let img = NSImage(size: NSSize(width: 56, height: 40))
        img.lockFocus()
        color.setFill()
        NSRect(x: 0, y: 0, width: 56, height: 40).fill()
        img.unlockFocus()
        return img
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CHỤP CUỘN: chọn vùng → vừa cuộn vừa chụp → ghép thành 1 ảnh dài.
    // ═══════════════════════════════════════════════════════════════════════
    func captureScrollingArea() async {
        guard beginSession() else { return }
        defer { busy = false }
        guard let picked = await selectArea(tool: "Scrolling Capture",
                                            toolIcon: "arrow.up.and.down.text.horizontal",
                                            confirmTitle: "Start Scrolling Capture",
                                            confirmIcon: "arrow.up.and.down.text.horizontal")
        else { lastStatus = "Scrolling capture cancelled."; return }
        let (rect, screen) = (picked.rect, picked.screen)
        try? await Task.sleep(nanoseconds: 150_000_000)

        recordingOverlay.show(rect: rect, on: screen)   // tối xung quanh + viền focus
        lastStatus = "Scroll through the area, then click Done."

        do {
            // start() bật phiên rồi trả về ngay; continuation chờ tới khi bấm Done/Cancel.
            // onStop chạy NGAY lúc bấm Done → ẩn viền đỏ liền, không đợi dựng ảnh dài
            // (ảnh có thể rất cao nên dựng mất chút thời gian).
            let result: CGImage? = try await withCheckedThrowingContinuation { cont in
                Task { @MainActor in
                    do {
                        try await scrollCapture.start(
                            rect: rect, screen: screen,
                            onStop: { [weak self] in self?.recordingOverlay.hide() },
                            onFinish: { image in cont.resume(returning: image) })
                    } catch {
                        cont.resume(throwing: error)
                    }
                }
            }
            recordingOverlay.hide()

            guard let cg = result else {
                lastStatus = "Scrolling capture cancelled (nothing captured)."
                return
            }
            finishImage(cg, subtitle: "\(cg.width)×\(cg.height)px (scrolling)", on: screen)
        } catch {
            recordingOverlay.hide()
            report(error)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // BƯỚC 3: QUAY VIDEO 1 vùng màn hình. Bấm lần nữa (hoặc nút ⏹) để dừng.
    // ═══════════════════════════════════════════════════════════════════════
    func recordRegion() async {
        // Đang quay → coi như lệnh dừng (để phím tắt bật/tắt cùng 1 tổ hợp).
        if isRecording { await stopRecording(); return }
        guard beginSession() else { return }   // bắt đầu quay mới → đóng editor cũ (không lưu)
        defer { busy = false }

        // Kéo chọn vùng trên màn nào cũng được (tái dùng overlay của chụp ảnh).
        // Kéo xong chưa quay ngay: khung ở lại để chỉnh, bấm "Start Recording"
        // (hoặc ↩) mới bắt đầu.
        guard let picked = await selectArea(tool: "Record Area", toolIcon: "record.circle",
                                            confirmTitle: "Start Recording", confirmIcon: "record.circle")
        else { lastStatus = "Recording cancelled."; return }
        let (rect, screen) = (picked.rect, picked.screen)

        // Đợi overlay biến mất rồi mới bật stream (~150ms).
        try? await Task.sleep(nanoseconds: 150_000_000)

        do {
            try await startRecorder(rect: rect, screen: screen)
            isRecording = true
            recRect = rect; recScreen = screen
            recordingBar.onStop      = { [weak self] in Task { await self?.stopRecording() } }
            recordingBar.onPauseToggle = { [weak self] paused in
                if paused { self?.recorder.pause() } else { self?.recorder.resume() }
                self?.clickEffect.setPaused(paused)   // nhật ký click dừng theo
            }
            recordingBar.onRestart   = { [weak self] in Task { await self?.restartRecording() } }
            recordingBar.onDiscard   = { [weak self] in Task { await self?.discardRecording() } }
            recordingOverlay.show(rect: rect, on: screen)   // tối xung quanh + viền focus
            recordingBar.show(below: rect, on: screen)
            // Vùng quay theo toạ độ AppKit toàn cục (gốc dưới-trái) cho hiệu ứng click.
            let globalRegion = CGRect(
                x: screen.frame.minX + rect.minX,
                y: screen.frame.minY + (screen.frame.height - rect.maxY),
                width: rect.width, height: rect.height)
            clickEffect.start(in: globalRegion)
            lastStatus = "🔴 Recording… click ⏹ (or \(settings.hotkey(for: .recordArea).display)) to stop."
        } catch {
            recordingOverlay.hide()
            report(error)
        }
    }

    /// Bật recorder với tuỳ chọn âm thanh hiện tại. Mic thì hỏi quyền trước —
    /// bị từ chối thì vẫn quay, chỉ là không có tiếng mic.
    private func startRecorder(rect: CGRect, screen: NSScreen) async throws {
        var mic = settings.recordMicrophone
        if mic, AVCaptureDevice.authorizationStatus(for: .audio) != .authorized {
            mic = await AVCaptureDevice.requestAccess(for: .audio)
        }
        recorder.onStreamError = { [weak self] error in
            guard let self, self.isRecording else { return }
            Task {
                await self.stopRecording()
                self.report(error)
            }
        }
        try await recorder.start(rect: rect, screen: screen,
                                 systemAudio: settings.recordSystemAudio, microphone: mic)
    }

    // Quay lại từ đầu: bỏ clip hiện tại rồi bật quay lại đúng vùng đó.
    func restartRecording() async {
        guard isRecording, let screen = recScreen else { return }
        await recorder.discard()
        do {
            try await startRecorder(rect: recRect, screen: screen)
            lastStatus = "🔴 Restarting…"
        } catch {
            isRecording = false
            recordingBar.hide()
            recordingOverlay.hide()
            clickEffect.stop()
            report(error)
        }
    }

    // Huỷ: dừng + xoá file, không hiện preview.
    func discardRecording() async {
        guard isRecording else { return }
        isRecording = false
        recordingBar.hide()
        recordingOverlay.hide()
        clickEffect.stop()
        await recorder.discard()
        lastStatus = "Recording discarded (not saved)."
    }

    func stopRecording() async {
        guard isRecording else { return }
        isRecording = false
        recordingBar.hide()
        recordingOverlay.hide()
        clickEffect.stop()
        // Chốt nhật ký click NGAY (lần quay sau sẽ xoá sạch nó).
        let clickLog = clickEffect.clicks

        guard let tmpURL = await recorder.stop() else {
            report(NSError(domain: "SlopShot", code: 11, userInfo: [
                NSLocalizedDescriptionKey: "The recording couldn't be saved — no frames were captured."]))
            return
        }
        playCaptureSound()

        // Giữ nguyên file .mov TẠM; bấm Save trên preview mới copy ra folder.
        let url = tmpURL

        // Chép FILE video lên clipboard (tùy cài đặt).
        if settings.copyToClipboard {
            let pb = NSPasteboard.general
            pb.clearContents()
            pb.writeObjects([url as NSURL])
        }

        lastSavedURL = url
        lastImage = nil
        lastStatus = "✅ Recording finished. Click Save to keep it."

        // Poster = 1 frame của video để làm ảnh thu nhỏ cho preview card.
        let poster = await posterImage(for: url) ?? NSImage(size: NSSize(width: 16, height: 9))

        // Ghi vào lịch sử (kèm thời lượng).
        history.add(kind: .video, fileURL: url, text: nil,
                    subtitle: await videoDuration(url), image: poster)

        guard settings.showThumbnail else { return }
        thumbnail.show(
            image: poster,
            fileURL: url,
            isVideo: true,
            on: recScreen,
            onEdit: { [weak self] in self?.videoViewer.open(url: url) },   // 👁 Quick Look trong app
            onCopy: {
                let pb = NSPasteboard.general
                pb.clearContents()
                pb.writeObjects([url as NSURL])
            },
            onSave: { [weak self] in self?.quickSaveVideo(url) },
            onTrim: { [weak self] in   // ✂️ mở Video Editor với clip vừa quay
                guard let self else { return }
                self.videoEditor.onDone = { [weak self] out in
                    guard let self, let out else { return }
                    self.lastSavedURL = out
                    self.lastStatus = "✅ Exported to \(out.deletingLastPathComponent().lastPathComponent)"
                }
                // Nhật ký click của lần quay này → nút Auto Zoom trong editor.
                self.videoEditor.open(url: url,
                                      saveFolder: self.settings.saveFolderURL,
                                      clicks: clickLog)
            }
        )
    }

    // Thời lượng video → "m:ss" cho phụ đề lịch sử. (load(.duration) async, không deprecated)
    private func videoDuration(_ url: URL) async -> String {
        let dur = (try? await AVURLAsset(url: url).load(.duration)) ?? .zero
        let secs = CMTimeGetSeconds(dur)
        guard secs.isFinite, secs >= 0 else { return "video" }
        let s = Int(secs.rounded())
        return String(format: "%d:%02d", s / 60, s % 60)
    }

    // Lấy 1 khung hình ~0.1s đầu làm ảnh đại diện (tránh frame đen đầu clip).
    private func posterImage(for url: URL) async -> NSImage? {
        let gen = AVAssetImageGenerator(asset: AVURLAsset(url: url))
        gen.appliesPreferredTrackTransform = true
        let time = CMTime(seconds: 0.1, preferredTimescale: 600)
        // image(at:) async thay cho copyCGImage (deprecated từ macOS 15).
        guard let r = try? await gen.image(at: time) else { return nil }
        return NSImage(cgImage: r.image, size: NSSize(width: r.image.width, height: r.image.height))
    }

    // Bấm "Save" trên preview ẢNH: ghi THẲNG vào folder đích (đúng định dạng đã chọn).
    private func quickSaveImage(_ cgImage: CGImage, baseName: String) {
        do {
            let dest = try writeImage(cgImage, baseName: baseName, to: settings.saveFolderURL)
            lastSavedURL = dest
            lastStatus = "✅ Saved to \(settings.saveFolderDisplay)"
            // KHÔNG hide() ở đây — preview tự hiện tick rồi trượt đi (như Copy).
        } catch {
            report(error)
        }
    }

    // Bấm "Save" trên preview VIDEO: copy .mov tạm THẲNG vào folder đích.
    private func quickSaveVideo(_ url: URL) {
        do {
            try FileManager.default.createDirectory(
                at: settings.saveFolderURL, withIntermediateDirectories: true)
            let dest = uniqueURL(settings.saveFolderURL.appendingPathComponent(url.lastPathComponent))
            try FileManager.default.copyItem(at: url, to: dest)
            lastSavedURL = dest
            lastStatus = "✅ Saved to \(settings.saveFolderDisplay)"
            // KHÔNG hide() ở đây — preview tự hiện tick rồi trượt đi (như Copy).
        } catch {
            report(error)
        }
    }

    // Lưu 1 mục lịch sử vào folder đích (copy file gốc sang, không hỏi).
    private func saveHistoryItemToFolder(_ src: URL) {
        do {
            try FileManager.default.createDirectory(
                at: settings.saveFolderURL, withIntermediateDirectories: true)
            let dest = uniqueURL(settings.saveFolderURL.appendingPathComponent(src.lastPathComponent))
            try FileManager.default.copyItem(at: src, to: dest)
            lastStatus = "✅ Saved to \(settings.saveFolderDisplay)"
        } catch {
            report(error)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Sau khi chụp xong: chép clipboard + hiện thumbnail nổi + cập nhật state.
    // ═══════════════════════════════════════════════════════════════════════
    //
    // Thứ tự quan trọng cho cảm giác "nhanh": tiếng chụp + preview hiện NGAY,
    // còn mã hoá PNG (100-300ms với màn 5K) và ghi đĩa chạy ở luồng nền. Xong
    // file mới chép clipboard (clipboard có kèm URL file) và ghi lịch sử.
    /// Kéo chọn xong → thanh vẽ hiện ngay trên vùng đó. ⌘C chép (luôn chép,
    /// bất kể cài đặt clipboard — đó là ý của phím), ⌘S lưu thẳng vào folder.
    private func editInPlace(_ frozen: CGImage, rect: CGRect, on screen: NSScreen) async {
        let cropped: CGImage
        do {
            cropped = try crop(frozen, to: rect, on: screen)
        } catch {
            selection.cancel()
            report(error)
            return
        }
        let base = NSImage(cgImage: cropped, size: NSSize(width: cropped.width, height: cropped.height))
        let result = await withCheckedContinuation { cont in
            selection.showInlineEditor(image: base, rect: rect) { cont.resume(returning: $0) }
        }
        guard let (edited, action) = result, let rendered = ImageOps.cg(edited) else {
            lastStatus = "Selection cancelled."
            return
        }
        // SwiftUI xuất ảnh 16-bit float (dải màu mở rộng): file PNG nặng gấp đôi,
        // vài app dán vào còn hiện sai màu. Vẽ lại về 8-bit, giữ hệ màu của ảnh
        // chụp gốc (Display P3 trên màn Mac) để không mất màu.
        let cg = Self.eightBit(rendered, space: cropped.colorSpace)
        let subtitle = "\(cg.width)×\(cg.height)px"
        finishImage(cg, subtitle: subtitle, on: screen, forceCopy: action == .copy)
        if action == .save { quickSaveImage(cg, baseName: "SlopShot \(timestampNow())") }
    }

    private static func eightBit(_ cg: CGImage, space: CGColorSpace?) -> CGImage {
        guard cg.bitsPerComponent > 8 else { return cg }
        let cs = space.flatMap { $0.model == .rgb ? $0 : nil } ?? CGColorSpace(name: CGColorSpace.sRGB)!
        guard let ctx = CGContext(data: nil, width: cg.width, height: cg.height, bitsPerComponent: 8,
                                  bytesPerRow: 0, space: cs,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return cg }
        ctx.draw(cg, in: CGRect(x: 0, y: 0, width: cg.width, height: cg.height))
        return ctx.makeImage() ?? cg
    }

    private func finishImage(_ cgImage: CGImage, subtitle: String, on screen: NSScreen,
                             forceCopy: Bool = false) {
        let nsImage = NSImage(cgImage: cgImage,
                              size: NSSize(width: cgImage.width, height: cgImage.height))
        let baseName = "SlopShot \(timestampNow())"
        // Giữ chỗ tên file ngay, file thật ghi sau (LUÔN ghi tạm; bấm Save trên
        // preview mới ra file chính thức).
        let url = TempFiles.uniqueURL(named: baseName, ext: "png", reserve: true)

        playCaptureSound()
        lastImage = nsImage
        lastSavedURL = url
        let copy = settings.copyToClipboard || forceCopy
        lastStatus = copy
            ? "✅ \(subtitle) · copied to clipboard."
            : "✅ Captured \(subtitle)."

        // Preview. Save = ghi THẲNG vào folder đích (không hỏi).
        if settings.showThumbnail {
            thumbnail.show(
                image: nsImage,
                fileURL: url,
                on: screen,
                onEdit: { [weak self] in self?.editor.open(image: nsImage, sourceURL: url) },
                onCopy: {
                    let pb = NSPasteboard.general
                    pb.clearContents()
                    pb.writeObjects([nsImage, url as NSURL])
                },
                onSave: { [weak self] in self?.quickSaveImage(cgImage, baseName: baseName) }
            )
        }

        Task {
            let written = await Task.detached(priority: .userInitiated) {
                guard let data = AppSettings.ImageFormat.png.encode(cgImage) else { return false }
                return (try? data.write(to: url)) != nil
            }.value
            if !written { try? FileManager.default.removeItem(at: url) }
            let fileURL = written ? url : nil

            // Clipboard (tùy cài đặt): chép cả ảnh lẫn file.
            if copy {
                let pb = NSPasteboard.general
                pb.clearContents()
                pb.writeObjects(fileURL.map { [nsImage, $0 as NSURL] } ?? [nsImage])
            }
            history.add(kind: .image, fileURL: fileURL, text: nil, subtitle: subtitle, image: nsImage)
        }
    }

    // Tiếng "tách" của macOS khi chụp (cùng file công cụ chụp của hệ thống dùng).
    private func playCaptureSound() {
        guard settings.playCaptureSound else { return }
        Self.shutter?.stop()
        Self.shutter?.play()
    }
    private static let shutter: NSSound? = {
        let system = "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif"
        return NSSound(contentsOfFile: system, byReference: true) ?? NSSound(named: "Tink")
    }()

    // Ghi CGImage thành file ảnh (theo format đã chọn) vào `folder`, tránh trùng tên.
    @discardableResult
    private func writeImage(_ cgImage: CGImage, baseName: String, to folder: URL) throws -> URL {
        let fmt = settings.imageFormat
        guard let data = fmt.encode(cgImage) else {
            throw NSError(domain: "SlopShot", code: 4,
                          userInfo: [NSLocalizedDescriptionKey: "Couldn't encode the image."])
        }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let dest = uniqueURL(folder.appendingPathComponent("\(baseName).\(fmt.ext)"))
        try data.write(to: dest)
        return dest
    }

    // Nếu file đã tồn tại thì thêm " (2)", " (3)"… để khỏi ghi đè.
    private func uniqueURL(_ url: URL) -> URL {
        guard FileManager.default.fileExists(atPath: url.path) else { return url }
        let dir = url.deletingLastPathComponent()
        let name = url.deletingPathExtension().lastPathComponent
        let ext = url.pathExtension
        var i = 2
        while true {
            let candidate = dir.appendingPathComponent("\(name) (\(i)).\(ext)")
            if !FileManager.default.fileExists(atPath: candidate.path) { return candidate }
            i += 1
        }
    }

    private func timestampNow() -> String {
        let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd 'at' HH.mm.ss"
        return f.string(from: Date())
    }

    // ── Mở cửa sổ History / Settings (gọi từ menu) ─────────────────────────
    func showHistory() {
        let actions = HistoryActions(
            copy:   { [weak self] in self?.historyCopy($0) },
            saveAs: { [weak self] in self?.historySaveAs($0) },
            edit:   { [weak self] in self?.historyEdit($0) },
            open:   { [weak self] in if let u = $0.fileURL { NSWorkspace.shared.open(u) } },
            delete: { [weak self] in self?.history.remove($0) },
            openSettings: { [weak self] in self?.showSettings() }
        )
        historyWindow.show(history: history, actions: actions)
    }

    func showSettings() { settingsWindow.show(settings: settings) }

    // Copy 1 mục lịch sử lên clipboard.
    private func historyCopy(_ item: HistoryItem) {
        let pb = NSPasteboard.general
        pb.clearContents()
        // .color cũng cất mã màu trong `text` — thiếu nó thì bấm Copy ở dòng màu
        // chẳng chép gì mà vẫn báo "đã chép".
        if item.kind == .text || item.kind == .color, let text = item.text {
            pb.setString(text, forType: .string)
        } else if !item.fileExists, let ocr = item.ocrText {
            // File tạm đã bị dọn nhưng chữ trong ảnh vẫn còn index → chép chữ,
            // vẫn hơn là chép ra không khí rồi báo "Copied".
            pb.setString(ocr, forType: .string)
            lastStatus = "✅ File is gone — copied the text read from it."
            return
        } else if let url = item.fileURL {
            if item.kind == .image, let img = NSImage(contentsOf: url) {
                pb.writeObjects([img, url as NSURL])
            } else {
                pb.writeObjects([url as NSURL])
            }
        }
        lastStatus = "✅ Copied from history."
    }

    // "Save" cho 1 mục lịch sử: copy file vào folder đích (không hỏi).
    private func historySaveAs(_ item: HistoryItem) {
        guard let url = item.fileURL else { return }
        saveHistoryItemToFolder(url)
    }

    // Mở lại 1 mục lịch sử: ảnh → editor, chữ → cửa sổ OCR (để dịch lại).
    private func historyEdit(_ item: HistoryItem) {
        if item.kind == .text, let text = item.text {
            ocrWindow.show(result: OCRResult(text: text, qrCodes: []), image: nil)
            return
        }
        guard item.kind == .image, let url = item.fileURL,
              let img = NSImage(contentsOf: url) else { return }
        editor.open(image: img, sourceURL: url)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Hàm dùng chung
    // ═══════════════════════════════════════════════════════════════════════

    // Màn hình đang có con trỏ chuột (xài đa màn hình vẫn đúng).
    /// ĐÓNG BĂNG mọi màn hình NGAY lúc bấm phím tắt, TRƯỚC khi mở overlay —
    /// ảnh giữ đúng những gì đang thấy (app dưới có tự đóng lightbox, ẩn
    /// tooltip, đổi frame video… cũng kệ) — rồi cho chọn vùng trên màn nào cũng được.
    private func selectArea(tool: String = "Capture Area", toolIcon: String = "camera.viewfinder",
                            confirmTitle: String = "Capture", confirmIcon: String = "camera.fill",
                            editInline: Bool = false) async -> RegionSelection? {
        let screens = NSScreen.screens
        var frozen = [CGImage?](repeating: nil, count: screens.count)
        await withTaskGroup(of: (Int, CGImage?).self) { group in
            for (i, screen) in screens.enumerated() {
                group.addTask { @MainActor in (i, try? await self.captureDisplay(on: screen)) }
            }
            for await (i, image) in group { frozen[i] = image }
        }
        return await withCheckedContinuation { cont in
            selection.begin(on: Array(zip(screens, frozen)).map { (screen: $0, frozen: $1) },
                            tool: tool, toolIcon: toolIcon,
                            confirmTitle: confirmTitle, confirmIcon: confirmIcon,
                            editInline: editInline) { cont.resume(returning: $0) }
        }
    }

    private func screenUnderCursor() -> NSScreen {
        let mouse = NSEvent.mouseLocation
        return NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) }
            ?? NSScreen.main ?? NSScreen.screens.first!
    }

    // Chụp nguyên 1 màn hình → CGImage (đúng độ phân giải pixel theo scale).
    // Dòng SCShareableContent cũng là chỗ macOS xin quyền Screen Recording lần đầu.
    //
    // Cửa sổ của CHÍNH SlopShot (thumbnail, overlay, thanh recording…) bị loại
    // khỏi ảnh ngay từ bộ lọc → khỏi phải "ẩn rồi ngủ 120ms" cầu may như trước.
    private func captureDisplay(on screen: NSScreen) async throws -> CGImage {
        do {
            return try await captureDisplay(on: screen, content: try await shareableContent())
        } catch {
            // Cache có thể đã cũ (màn hình vừa đổi mà chưa kịp báo) → hỏi lại 1 lần.
            return try await captureDisplay(on: screen, content: try await shareableContent(refresh: true))
        }
    }

    private func shareableContent(refresh: Bool = false) async throws -> SCShareableContent {
        if !refresh, let shareable { return shareable }
        let content = try await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: false)
        shareable = content
        return content
    }

    private func captureDisplay(on screen: NSScreen,
                                content: SCShareableContent) async throws -> CGImage {
        guard let display = content.displays.first(where: { $0.displayID == screen.displayID }) else {
            throw NSError(domain: "SlopShot", code: 3,
                          userInfo: [NSLocalizedDescriptionKey: "No display found."])
        }
        let me = content.applications.filter {
            $0.bundleIdentifier == Bundle.main.bundleIdentifier
        }
        let filter = SCContentFilter(display: display,
                                     excludingApplications: me, exceptingWindows: [])
        let scale = screen.backingScaleFactor
        let config = SCStreamConfiguration()
        config.width  = Int(CGFloat(display.width)  * scale)
        config.height = Int(CGFloat(display.height) * scale)
        config.showsCursor = false
        return try await SCScreenshotManager.captureImage(
            contentFilter: filter, configuration: config
        )
    }

    // Cắt 1 vùng (points, gốc trên-trái màn hình) ra khỏi ảnh full màn hình.
    // Tỉ lệ point→pixel lấy TỪ CHÍNH ảnh (không dùng backingScaleFactor) để màn
    // hình ngoài / độ phân giải scale lạ vẫn cắt đúng chỗ.
    private func crop(_ full: CGImage, to rect: CGRect, on screen: NSScreen) throws -> CGImage {
        let scale = CGFloat(full.width) / max(screen.frame.width, 1)
        let pixelRect = CGRect(x: rect.minX * scale, y: rect.minY * scale,
                               width: rect.width * scale, height: rect.height * scale)
        let imageBounds = CGRect(x: 0, y: 0, width: full.width, height: full.height)
        let safeRect = pixelRect.intersection(imageBounds).integral
        guard safeRect.width >= 1, safeRect.height >= 1,
              let cropped = full.cropping(to: safeRect) else {
            throw NSError(domain: "SlopShot", code: 2,
                          userInfo: [NSLocalizedDescriptionKey: "Couldn't crop the selected area."])
        }
        return cropped
    }

    // Chụp lại màn hình rồi CẮT đúng vùng `rect` — đường lui khi không có ảnh
    // đóng băng (vd. lần đầu chưa được cấp quyền nên chụp trước đó lỗi).
    private func captureCropped(rect: CGRect, on screen: NSScreen) async throws -> CGImage {
        let cgFull = try await captureDisplay(on: screen)
        return try crop(cgFull, to: rect, on: screen)
    }

}
