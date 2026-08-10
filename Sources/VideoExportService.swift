import AVFoundation
import ImageIO
import UniformTypeIdentifiers
import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Xuất file từ composition mà editor đang xem.
//
// Ba đường đi, chọn tự động:
//   1) PASSTHROUGH — chỉ trim/cắt, giữ nguyên .mov: chép thẳng sample, không
//      giải nén lại → nhanh nhất và không mất chất lượng.
//   2) TRANSCODE   — có hiệu ứng / đổi MP4 / đổi resolution·fps: đọc bằng
//      AVAssetReader (đã áp videoComposition) rồi ghi bằng AVAssetWriter.
//      Tự cầm reader+writer thay vì AVAssetExportSession vì preset của
//      export session gộp cứng cả bitrate lẫn kích thước — không tách riêng
//      3 núm Quality / Resolution / FPS được.
//   3) GIF         — lấy từng khung qua AVAssetImageGenerator (cũng áp
//      videoComposition, nên GIF có đủ zoom/che/chữ) rồi gói bằng ImageIO.
// ─────────────────────────────────────────────────────────────────────────

struct VideoExportOptions {
    enum Format: String, CaseIterable {
        case mov, mp4, gif
        var label: String { rawValue.uppercased() }
        var ext: String { rawValue }
        var fileType: AVFileType { self == .mp4 ? .mp4 : .mov }
    }

    enum Quality: String, CaseIterable {
        case high, medium, low
        var label: String { rawValue.capitalized }
        /// Bit trên mỗi pixel-frame — nhân với w×h×fps ra bitrate.
        var bitsPerPixel: Double {
            switch self {
            case .high:   return 0.20
            case .medium: return 0.10
            case .low:    return 0.05
            }
        }
    }

    enum Resolution: String, CaseIterable {
        case original, p1080, p720, p480
        var label: String {
            switch self {
            case .original: return "Original"
            case .p1080:    return "1080p"
            case .p720:     return "720p"
            case .p480:     return "480p"
            }
        }
        var targetHeight: CGFloat? {
            switch self {
            case .original: return nil
            case .p1080:    return 1080
            case .p720:     return 720
            case .p480:     return 480
            }
        }
        /// Tỉ lệ thu nhỏ so với clip gốc (không bao giờ phóng to lên).
        func scale(for sourceSize: CGSize) -> CGFloat {
            guard let h = targetHeight, sourceSize.height > 0 else { return 1 }
            return min(1, h / sourceSize.height)
        }
    }

    enum FrameRate: String, CaseIterable {
        case source, fps60, fps30, fps24, fps15, fps10, fps5
        var label: String {
            switch self {
            case .source: return "Source"
            case .fps60:  return "60 fps"
            case .fps30:  return "30 fps"
            case .fps24:  return "24 fps"
            case .fps15:  return "15 fps"
            case .fps10:  return "10 fps"
            case .fps5:   return "5 fps"
            }
        }
        var value: Double? {
            switch self {
            case .source: return nil
            case .fps60:  return 60
            case .fps30:  return 30
            case .fps24:  return 24
            case .fps15:  return 15
            case .fps10:  return 10
            case .fps5:   return 5
            }
        }
        /// Lựa chọn hợp lý cho từng định dạng (GIF nặng nên chặn ở 15fps).
        static func choices(for format: Format) -> [FrameRate] {
            format == .gif ? [.fps15, .fps10, .fps5] : [.source, .fps60, .fps30, .fps24, .fps15]
        }
    }

    var format: Format = .mov
    var quality: Quality = .high
    var resolution: Resolution = .original
    var frameRate: FrameRate = .source
}

enum VideoExportService {

    /// Xuất ra `outURL`. `onProgress` gọi trên main thread, giá trị 0…1.
    @MainActor
    static func export(asset: AVAsset,
                       trimStart: Double,
                       trimEnd: Double,
                       segments: [VideoSegment],
                       audio: VideoAudioSettings,
                       options: VideoExportOptions,
                       to outURL: URL,
                       onProgress: @escaping (Double) -> Void) async throws {

        // Cỡ gốc để quy ra tỉ lệ thu nhỏ.
        guard let track = try await asset.loadTracks(withMediaType: .video).first else {
            throw err("This file has no video track.")
        }
        let natural = try await track.load(.naturalSize)
        let transform = try await track.load(.preferredTransform)
        let sourceSize = CGSize(width: abs(natural.applying(transform).width),
                                height: abs(natural.applying(transform).height))
        let nominalFPS = Double(try await track.load(.nominalFrameRate))

        let scale = options.resolution.scale(for: sourceSize)
        let fps = options.frameRate.value ?? (nominalFPS > 0 ? nominalFPS : 60)

        let built = try await VideoCompositionBuilder.build(
            asset: asset, trimStart: trimStart, trimEnd: trimEnd,
            segments: segments, audio: audio,
            renderScale: scale,
            fps: options.format == .gif ? fps : options.frameRate.value)

        guard built.timeMap.duration > 0.05 else {
            throw err("Nothing left to export — the trim range is empty.")
        }

        try? FileManager.default.removeItem(at: outURL)

        switch options.format {
        case .gif:
            try await exportGIF(built: built, fps: fps, to: outURL, onProgress: onProgress)
        case .mov where built.isPlainTrim && options.frameRate.value == nil:
            try await exportPassthrough(built: built, to: outURL, onProgress: onProgress)
        case .mov, .mp4:
            try await exportTranscoded(built: built, options: options, fps: fps,
                                       to: outURL, onProgress: onProgress)
        }
    }

    // MARK: - 1) Passthrough (chỉ trim/cắt, giữ .mov)

    private static func exportPassthrough(built: VideoBuildResult, to outURL: URL,
                                          onProgress: @escaping (Double) -> Void) async throws {
        guard let session = AVAssetExportSession(asset: built.composition,
                                                 presetName: AVAssetExportPresetPassthrough) else {
            throw err("Couldn't start the export session.")
        }
        let ticker = Task { @MainActor in
            while !Task.isCancelled {
                onProgress(Double(session.progress))
                try? await Task.sleep(nanoseconds: 200_000_000)
            }
        }
        defer { ticker.cancel() }
        try await session.export(to: outURL, as: .mov)
        onProgress(1)
    }

    // MARK: - 2) Transcode (reader → writer), tự cầm bitrate/fps/kích thước

    private static func exportTranscoded(built: VideoBuildResult,
                                         options: VideoExportOptions,
                                         fps: Double,
                                         to outURL: URL,
                                         onProgress: @escaping (Double) -> Void) async throws {
        let comp = built.composition
        let videoTracks = try await comp.loadTracks(withMediaType: .video)
        let audioTracks = try await comp.loadTracks(withMediaType: .audio)
        guard !videoTracks.isEmpty else { throw err("This file has no video track.") }

        // Không có hiệu ứng nào thì vẫn cần một videoComposition "trơn" để
        // AVAssetReaderVideoCompositionOutput có cái mà dựa vào.
        let videoComposition: AVVideoComposition
        if let vc = built.videoComposition {
            videoComposition = vc
        } else {
            let vc = try await AVMutableVideoComposition.videoComposition(withPropertiesOf: comp)
            vc.renderSize = built.renderSize
            if options.frameRate.value != nil {
                vc.frameDuration = CMTime(value: 1, timescale: CMTimeScale(fps.rounded()))
            }
            videoComposition = vc
        }

        let reader = try AVAssetReader(asset: comp)
        let writer = try AVAssetWriter(outputURL: outURL, fileType: options.format.fileType)

        // ── Video ─────────────────────────────────────────────────────────
        let videoOut = AVAssetReaderVideoCompositionOutput(
            videoTracks: videoTracks,
            videoSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
        videoOut.videoComposition = videoComposition
        videoOut.alwaysCopiesSampleData = false
        guard reader.canAdd(videoOut) else { throw err("Couldn't read the composed video.") }
        reader.add(videoOut)

        let w = Int(built.renderSize.width), h = Int(built.renderSize.height)
        let bitrate = Int(Double(w * h) * fps * options.quality.bitsPerPixel)
        let videoIn = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: w,
            AVVideoHeightKey: h,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: max(200_000, min(bitrate, 80_000_000)),
                AVVideoExpectedSourceFrameRateKey: Int(fps.rounded()),
                AVVideoMaxKeyFrameIntervalKey: max(1, Int(fps.rounded()) * 2),
                AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
            ],
        ])
        videoIn.expectsMediaDataInRealTime = false
        guard writer.canAdd(videoIn) else { throw err("Couldn't write the video track.") }
        writer.add(videoIn)

        // ── Tiếng (gộp mọi track nguồn thành 1 track stereo) ──────────────
        var audioOut: AVAssetReaderAudioMixOutput?
        var audioIn: AVAssetWriterInput?
        if !audioTracks.isEmpty {
            let out = AVAssetReaderAudioMixOutput(audioTracks: audioTracks, audioSettings: [
                AVFormatIDKey: kAudioFormatLinearPCM,
                AVLinearPCMBitDepthKey: 16,
                AVLinearPCMIsFloatKey: false,
                AVLinearPCMIsBigEndianKey: false,
                AVLinearPCMIsNonInterleaved: false,
                AVSampleRateKey: 44_100,
                AVNumberOfChannelsKey: 2,
            ])
            out.audioMix = built.audioMix
            if reader.canAdd(out) {
                reader.add(out)
                let ain = AVAssetWriterInput(mediaType: .audio, outputSettings: [
                    AVFormatIDKey: kAudioFormatMPEG4AAC,
                    AVSampleRateKey: 44_100,
                    AVNumberOfChannelsKey: 2,
                    AVEncoderBitRateKey: 128_000,
                ])
                ain.expectsMediaDataInRealTime = false
                if writer.canAdd(ain) { writer.add(ain); audioIn = ain; audioOut = out }
            }
        }

        guard reader.startReading() else {
            throw reader.error ?? err("Couldn't read the video.")
        }
        guard writer.startWriting() else {
            reader.cancelReading()
            throw writer.error ?? err("Couldn't write the file.")
        }
        writer.startSession(atSourceTime: .zero)

        let total = built.timeMap.duration
        let queue = DispatchQueue(label: "com.thanglb.slopshot.export")

        // Bơm video: writer gọi lại mỗi khi sẵn sàng nhận thêm dữ liệu.
        async let videoDone: Void = withCheckedContinuation { cont in
            videoIn.requestMediaDataWhenReady(on: queue) {
                while videoIn.isReadyForMoreMediaData {
                    guard let sample = videoOut.copyNextSampleBuffer() else {
                        videoIn.markAsFinished(); cont.resume(); return
                    }
                    let t = CMSampleBufferGetPresentationTimeStamp(sample).seconds
                    videoIn.append(sample)
                    if total > 0 {
                        let p = min(0.99, t / total)
                        Task { @MainActor in onProgress(p) }
                    }
                }
            }
        }

        async let audioDone: Void = withCheckedContinuation { cont in
            guard let audioIn, let audioOut else { cont.resume(); return }
            audioIn.requestMediaDataWhenReady(on: queue) {
                while audioIn.isReadyForMoreMediaData {
                    guard let sample = audioOut.copyNextSampleBuffer() else {
                        audioIn.markAsFinished(); cont.resume(); return
                    }
                    audioIn.append(sample)
                }
            }
        }

        _ = await (videoDone, audioDone)

        if reader.status == .failed {
            writer.cancelWriting()
            throw reader.error ?? err("Reading the video failed.")
        }
        await writer.finishWriting()
        if writer.status != .completed {
            throw writer.error ?? err("Writing the file failed.")
        }
        onProgress(1)
    }

    // MARK: - 3) GIF

    private static func exportGIF(built: VideoBuildResult, fps: Double, to outURL: URL,
                                  onProgress: @escaping (Double) -> Void) async throws {
        let duration = built.timeMap.duration
        let step = 1.0 / max(1, fps)
        let count = max(1, Int(duration / step))
        guard count <= 3000 else {
            throw err("That clip is too long for a GIF — trim it or drop the frame rate.")
        }

        let gen = AVAssetImageGenerator(asset: built.composition)
        gen.appliesPreferredTrackTransform = true
        gen.videoComposition = built.videoComposition
        gen.requestedTimeToleranceBefore = .zero
        gen.requestedTimeToleranceAfter = CMTime(seconds: step / 2, preferredTimescale: 600)
        gen.maximumSize = built.renderSize

        guard let dest = CGImageDestinationCreateWithURL(
            outURL as CFURL, UTType.gif.identifier as CFString, count, nil) else {
            throw err("Couldn't create the GIF file.")
        }
        CGImageDestinationSetProperties(dest, [
            kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFLoopCount: 0],
        ] as CFDictionary)
        let frameProps = [
            kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFDelayTime: step,
                                            kCGImagePropertyGIFUnclampedDelayTime: step],
        ] as CFDictionary

        var written = 0
        for i in 0..<count {
            let t = CMTime(seconds: Double(i) * step, preferredTimescale: 600)
            guard let cg = try? await gen.image(at: t).image else { continue }
            CGImageDestinationAddImage(dest, cg, frameProps)
            written += 1
            let p = Double(i + 1) / Double(count)
            await MainActor.run { onProgress(p * 0.99) }
        }
        guard written > 0, CGImageDestinationFinalize(dest) else {
            throw err("Couldn't encode the GIF.")
        }
        onProgress(1)
    }

    // MARK: - tiện ích

    private static func err(_ msg: String) -> NSError {
        NSError(domain: "SlopShot", code: 22, userInfo: [NSLocalizedDescriptionKey: msg])
    }

    /// Tên file mặc định: "SlopShot 2026-08-10 at 09.41.12.mp4".
    static func defaultFilename(ext: String) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd 'at' HH.mm.ss"
        return "SlopShot \(f.string(from: Date())).\(ext)"
    }
}
