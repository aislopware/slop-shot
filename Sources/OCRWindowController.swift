import SwiftUI
import Translation

// ─────────────────────────────────────────────────────────────────────────
// Cửa sổ kết quả OCR: ảnh vùng vừa chụp + chữ đọc được + nút dịch.
// Ý tưởng lấy từ macshot (OCRResultController) nhưng dựng bằng SwiftUI cho
// gọn, và dịch bằng framework Translation của Apple thay vì gọi API Google.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
final class OCRWindowController {
    private var window: NSWindow?

    func show(result: OCRResult, image: NSImage?) {
        dismiss()   // mỗi lần OCR mở 1 cửa sổ mới, khỏi chồng đống

        let view = OCRResultView(result: result, image: image)
        let host = NSHostingController(rootView: view)
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 620),
                           styleMask: [.titled, .closable, .resizable],
                           backing: .buffered, defer: false)
        win.title = result.qrCodes.isEmpty ? "Text Recognition" : "Text & QR Recognition"
        win.titlebarAppearsTransparent = true      // nối liền với thanh công cụ bên dưới
        win.contentViewController = host
        win.isReleasedWhenClosed = false
        win.minSize = NSSize(width: 760, height: 440)
        win.center()

        NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: win, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.window = nil }
        }
        NSApp.activate(ignoringOtherApps: true)
        win.makeKeyAndOrderFront(nil)
        self.window = win
    }

    func dismiss() {
        window?.close()
        window = nil
    }
}

// ─────────────────────────────────────────────────────────────────────────
private struct OCRResultView: View {
    let result: OCRResult
    let image: NSImage?

    @State private var text: String            // chữ gốc (sửa tay được)
    @State private var translatedText = ""     // bản dịch (cũng sửa tay được)
    @State private var hasTranslation = false
    @State private var showTranslated = false
    @State private var target: String
    @State private var config: TranslationSession.Configuration?
    @State private var busy = false
    @State private var error: String?
    @State private var copied = false

    init(result: OCRResult, image: NSImage?) {
        self.result = result
        self.image = image
        _text = State(initialValue: result.text)
        _target = State(initialValue: TranslationService.suggestedTarget(for: result.text))
    }

    private var shown: String { showTranslated ? translatedText : text }
    private var isBlank: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            HStack(spacing: 0) {
                if let image {
                    preview(image)
                    Divider()
                }
                editor
            }
            Divider()
            footer
        }
        .frame(minWidth: 760, minHeight: 440)
        // Apple chỉ cấp TranslationSession qua modifier này; đổi config = chạy dịch.
        .translationTask(config) { session in
            do {
                let response = try await session.translate(text)
                translatedText = response.targetText
                hasTranslation = true
                showTranslated = true
                error = nil
            } catch {
                self.error = "Translation failed: \(error.localizedDescription)"
            }
            busy = false
        }
    }

    // ── Thanh công cụ: xem bản nào + dịch sang ngôn ngữ nào ───────────────
    private var toolbar: some View {
        HStack(spacing: 12) {
            if hasTranslation {
                Picker("", selection: $showTranslated) {
                    Text("Original").tag(false)
                    Text(TranslationService.name(of: target)).tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(minWidth: 150, idealWidth: 260, maxWidth: 260)
            } else {
                Label("Recognized text", systemImage: "text.viewfinder")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 12)

            // Nhóm bên phải để .fixedSize() → khung cửa sổ hẹp lại thì phần bên
            // trái co, còn picker + nút Translate không bao giờ bị cắt chữ.
            HStack(spacing: 8) {
                Picker("", selection: $target) {
                    ForEach(TranslationService.languages) { lang in
                        Text(lang.name).tag(lang.code)
                    }
                }
                .labelsHidden()
                .frame(width: 165)
                .help("Language to translate into")
                .onChange(of: target) { _, new in
                    TranslationService.target = new
                    hasTranslation = false      // đổi ngôn ngữ → bản dịch cũ hết giá trị
                    translatedText = ""
                    showTranslated = false
                }

                Button(action: translate) {
                    HStack(spacing: 6) {
                        if busy {
                            ProgressView().controlSize(.small).scaleEffect(0.65)
                                .frame(width: 13, height: 13)
                        } else {
                            Image(systemName: "globe")
                        }
                        Text("Translate")
                    }
                    .frame(width: 92)           // chốt bề rộng → chữ không bị "…"
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(busy || isBlank)
            }
            .fixedSize()
        }
        .controlSize(.large)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.bar)
    }

    // ── Cột trái: ảnh vùng vừa chụp, để đối chiếu OCR đọc đúng chưa ───────
    private func preview(_ image: NSImage) -> some View {
        VStack(spacing: 10) {
            Image(nsImage: image)
                .resizable()
                .aspectRatio(contentMode: .fit)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(Color.primary.opacity(0.14)))
                .shadow(color: .black.opacity(0.22), radius: 7, y: 3)

            Text("\(Int(image.size.width.rounded())) × \(Int(image.size.height.rounded())) pt")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(16)
        .frame(width: 268)
        .background(Color(nsColor: .underPageBackgroundColor))
    }

    // ── Vùng chữ ─────────────────────────────────────────────────────────
    private var editor: some View {
        // Cả bản gốc lẫn bản dịch đều sửa tay được — OCR sai 1 chữ thì gõ lại
        // rồi Copy, khỏi phải mang sang app khác mới sửa được.
        ZStack {
            TextEditor(text: showTranslated ? $translatedText : $text)
                .font(.system(size: 14))
                .lineSpacing(2)
                .scrollContentBackground(.hidden)
                .padding(.horizontal, 14)
                .padding(.vertical, 12)

            if isBlank && !hasTranslation {
                VStack(spacing: 6) {
                    Image(systemName: "text.badge.xmark")
                        .font(.system(size: 30))
                        .foregroundStyle(.tertiary)
                    Text("No text found in this capture")
                        .foregroundStyle(.secondary)
                }
                .allowsHitTesting(false)
            }

            if busy {
                // Lần dịch đầu tiên Apple phải tải gói ngôn ngữ về nên khá lâu —
                // che mờ + báo rõ đang chạy để khỏi tưởng app treo.
                Color(nsColor: .textBackgroundColor).opacity(0.6)
                ProgressView("Translating…")
                    .controlSize(.small)
                    .padding(16)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .textBackgroundColor))
    }

    // ── Thanh dưới: QR + số ký tự + Copy ─────────────────────────────────
    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !result.qrCodes.isEmpty {
                ForEach(result.qrCodes, id: \.self) { code in
                    HStack(spacing: 8) {
                        Image(systemName: "qrcode").foregroundStyle(.secondary)
                        if let url = URL(string: code.trimmingCharacters(in: .whitespacesAndNewlines)),
                           let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https" {
                            Link(code, destination: url).lineLimit(1).truncationMode(.middle)
                        } else {
                            Text(code).lineLimit(1).truncationMode(.middle)
                        }
                        Spacer(minLength: 8)
                        Button("Copy") { copy(code) }
                            .buttonStyle(.borderless)
                    }
                    .font(.callout)
                }
                Divider()
            }

            HStack(spacing: 12) {
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.orange)
                        .lineLimit(2)
                } else {
                    Text("\(shown.count) characters · \(wordCount(shown)) words")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 12)
                Button { copy(shown) } label: {
                    Label(copied ? "Copied" : "Copy text",
                          systemImage: copied ? "checkmark" : "doc.on.doc")
                        .frame(width: 92)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .keyboardShortcut("c", modifiers: [.command, .shift])
                .disabled(shown.isEmpty)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.bar)
    }

    // ── Việc ─────────────────────────────────────────────────────────────
    private func translate() {
        guard !busy else { return }
        error = nil
        busy = true
        let source = TranslationService.detectSource(of: text)
        let next = TranslationSession.Configuration(
            source: source, target: Locale.Language(identifier: target))
        // Cùng cặp ngôn ngữ với lần trước → modifier không thấy "đổi" nên phải
        // invalidate() để nó chạy lại; khác cặp thì gán config mới.
        if var current = config, current.source == next.source, current.target == next.target {
            current.invalidate()
            config = current
        } else {
            config = next
        }
    }

    private func copy(_ s: String) {
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(s, forType: .string)
        copied = true
        Task {
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            copied = false
        }
    }

    private func wordCount(_ s: String) -> Int {
        s.split(whereSeparator: { $0.isWhitespace || $0.isNewline }).count
    }
}
