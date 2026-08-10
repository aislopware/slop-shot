import AVFoundation
import CoreImage
import CoreImage.CIFilterBuiltins
import AppKit

// ─────────────────────────────────────────────────────────────────────────
// Biến state của editor thành thứ AVFoundation hiểu được:
//
//   segments  ──►  AVMutableComposition   (cắt / tua nhanh / đứng hình)
//             ──►  AVVideoComposition     (zoom / che / chữ, qua Core Image)
//             ──►  AVAudioMix             (âm lượng từng track, mute)
//
// Cả PREVIEW lẫn EXPORT đều gọi đúng hàm này → xem sao thì file ra y vậy,
// không có chuyện preview một đằng file một nẻo.
//
// Cắt/tốc độ/đứng hình xử lý ở tầng composition (chèn từng khoảng giữ lại rồi
// scale thời gian). Zoom/che/chữ xử lý ở tầng Core Image (đọc pixel từng khung).
// ─────────────────────────────────────────────────────────────────────────

struct VideoBuildResult {
    let composition: AVComposition
    let videoComposition: AVVideoComposition?
    let audioMix: AVAudioMix?
    let timeMap: VideoTimeMap
    /// Cỡ khung hình của clip GỐC (pixel).
    let sourceSize: CGSize
    /// Cỡ khung hình xuất ra (đã nhân renderScale, ép chẵn cho H.264).
    let renderSize: CGSize
    /// Không có hiệu ứng nào ngoài trim? → xuất được bằng đường passthrough.
    let isPlainTrim: Bool
}

/// Cài đặt âm thanh cho lần dựng này.
struct VideoAudioSettings {
    var muted: Bool = false
    /// Âm lượng theo từng track nguồn (0…1). Thiếu phần tử thì coi như 1.
    var volumes: [Float] = []

    func volume(track index: Int) -> Float {
        guard !muted else { return 0 }
        return index < volumes.count ? volumes[index] : 1
    }
}

enum VideoCompositionBuilder {

    /// `renderScale` < 1 để xuất file nhỏ hơn; `fps` nil = giữ nhịp gốc.
    @MainActor
    static func build(asset: AVAsset,
                      trimStart: Double,
                      trimEnd: Double,
                      segments: [VideoSegment],
                      audio: VideoAudioSettings,
                      renderScale: CGFloat = 1,
                      fps: Double? = nil) async throws -> VideoBuildResult {

        guard let videoTrack = try await asset.loadTracks(withMediaType: .video).first else {
            throw NSError(domain: "SlopShot", code: 20,
                          userInfo: [NSLocalizedDescriptionKey: "This file has no video track."])
        }
        let audioTracks = try await asset.loadTracks(withMediaType: .audio)

        // Cỡ khung sau khi áp preferredTransform (clip quay màn hình thì luôn
        // là ma trận đơn vị, nhưng cứ tính đúng cho chắc).
        let natural = try await videoTrack.load(.naturalSize)
        let transform = try await videoTrack.load(.preferredTransform)
        let sourceSize = CGSize(width: abs(natural.applying(transform).width),
                                height: abs(natural.applying(transform).height))
        let nominalFPS = try await videoTrack.load(.nominalFrameRate)

        // ── 1) Tách segment theo loại + dựng bản đồ thời gian ─────────────
        let cuts    = segments.filter { $0.kind == .cut }
        let speeds  = segments.filter { $0.kind == .speed }
        let freezes = segments.filter { $0.kind == .freeze }

        let kept = VideoCuts.keptRanges(trimStart: trimStart, trimEnd: trimEnd, cuts: cuts)
        let timeMap = VideoTimeMap.build(keptRanges: kept, speeds: speeds, freezes: freezes)

        // ── 2) Composition: chèn từng mảnh rồi kéo/nén thời gian ──────────
        let comp = AVMutableComposition()
        guard let compVideo = comp.addMutableTrack(withMediaType: .video,
                                                   preferredTrackID: kCMPersistentTrackID_Invalid) else {
            throw NSError(domain: "SlopShot", code: 21,
                          userInfo: [NSLocalizedDescriptionKey: "Couldn't create a video track."])
        }
        compVideo.preferredTransform = transform

        var compAudio: [AVMutableCompositionTrack] = []
        for _ in audioTracks {
            if let t = comp.addMutableTrack(withMediaType: .audio,
                                            preferredTrackID: kCMPersistentTrackID_Invalid) {
                compAudio.append(t)
            }
        }

        let scale: CMTimeScale = 600
        var cursor = CMTime.zero
        for piece in timeMap.pieces {
            let srcRange = CMTimeRange(
                start: CMTime(seconds: piece.srcStart, preferredTimescale: scale),
                duration: CMTime(seconds: max(piece.srcDuration, 1.0 / 600), preferredTimescale: scale))
            let target = CMTime(seconds: piece.compDuration, preferredTimescale: scale)

            do {
                try compVideo.insertTimeRange(srcRange, of: videoTrack, at: cursor)
            } catch {
                continue   // mảnh hỏng (vượt cuối file) → bỏ qua, đừng làm sập cả clip
            }
            // Track tiếng thường ngắn hơn track hình một nhúm ở cuối; chèn hụt
            // thì chỉ đoạn đó mất tiếng, KHÔNG được bỏ luôn mảnh hình (bỏ là
            // mốc `cursor` đứng yên → mảnh sau ghi đè lên mảnh trước).
            for (i, track) in audioTracks.enumerated() where i < compAudio.count {
                try? compAudio[i].insertTimeRange(srcRange, of: track, at: cursor)
            }

            let inserted = CMTimeRange(start: cursor, duration: srcRange.duration)
            // Chỉ scale khi độ dài đích khác nguồn (speed / freeze).
            if abs(piece.compDuration - piece.srcDuration) > 0.001 {
                compVideo.scaleTimeRange(inserted, toDuration: target)
                for track in compAudio { track.scaleTimeRange(inserted, toDuration: target) }
                cursor = CMTimeAdd(cursor, target)
            } else {
                cursor = CMTimeAdd(cursor, srcRange.duration)
            }
        }

        // ── 3) Cỡ khung xuất ra (H.264 đòi cạnh chẵn) ─────────────────────
        var outW = Int((sourceSize.width * renderScale).rounded())
        var outH = Int((sourceSize.height * renderScale).rounded())
        if outW % 2 != 0 { outW -= 1 }
        if outH % 2 != 0 { outH -= 1 }
        let renderSize = CGSize(width: max(2, outW), height: max(2, outH))

        // ── 4) Snapshot hiệu ứng pixel → đưa xuống Core Image ─────────────
        let plan = renderPlan(segments: segments, sourceSize: sourceSize)
        let isPlainTrim = plan.isEmpty && speeds.isEmpty && freezes.isEmpty
            && abs(renderScale - 1) < 0.001

        var videoComposition: AVVideoComposition?
        if !plan.isEmpty || renderSize != sourceSize {
            let k = renderSize.width / max(sourceSize.width, 1)
            let outRect = CGRect(origin: .zero, size: renderSize)

            let vc = try await AVMutableVideoComposition.videoComposition(with: comp) { request in
                var img = request.sourceImage
                let extent = img.extent
                let t = timeMap.sourceTime(comp: request.compositionTime.seconds)

                // a) Che trước — che dán vào NỘI DUNG, nên zoom vào thì vết che
                //    phóng to theo, đúng như mong đợi.
                for c in plan.censors where t >= c.start && t <= c.end {
                    img = censor(img, spec: c, extent: extent)
                }

                // b) Zoom: nhiều đoạn chồng nhau thì lấy đoạn đang phóng mạnh nhất.
                var level: CGFloat = 1
                var center = CGPoint(x: 0.5, y: 0.5)
                for z in plan.zooms {
                    let l = z.level(at: t)
                    if l > level { level = l; center = z.center }
                }
                if level > 1.001 {
                    img = zoomed(img, level: level, center: center, extent: extent)
                }

                // c) Chữ nằm TRÊN CÙNG và không bị zoom kéo đi (như phụ đề).
                for tx in plan.texts where t >= tx.start && t <= tx.end {
                    guard let cg = tx.image else { continue }
                    let r = ciRect(tx.rect, in: extent)
                    let overlay = CIImage(cgImage: cg)
                    let sx = r.width / max(overlay.extent.width, 1)
                    let sy = r.height / max(overlay.extent.height, 1)
                    img = overlay
                        .transformed(by: CGAffineTransform(scaleX: sx, y: sy)
                            .concatenating(CGAffineTransform(translationX: r.minX, y: r.minY)))
                        .composited(over: img)
                }

                // d) Thu nhỏ về đúng cỡ xuất (nếu có đổi resolution).
                if abs(k - 1) > 0.001 {
                    img = img.transformed(by: CGAffineTransform(scaleX: k, y: k))
                }
                request.finish(with: img.cropped(to: outRect), context: nil)
            }
            vc.renderSize = renderSize
            if let fps, fps > 0 {
                vc.frameDuration = CMTime(value: 1, timescale: CMTimeScale(fps.rounded()))
            } else if nominalFPS > 0 {
                vc.frameDuration = CMTime(value: 1, timescale: CMTimeScale(nominalFPS.rounded()))
            }
            videoComposition = vc
        }

        // ── 5) Trộn tiếng ─────────────────────────────────────────────────
        var audioMix: AVAudioMix?
        if !compAudio.isEmpty {
            let mix = AVMutableAudioMix()
            mix.inputParameters = compAudio.enumerated().map { i, track in
                let p = AVMutableAudioMixInputParameters(track: track)
                p.setVolume(audio.volume(track: i), at: .zero)
                return p
            }
            audioMix = mix
        }

        // Đụng vào âm lượng thì không passthrough được nữa (phải nén lại tiếng).
        let audioUntouched = !audio.muted && audio.volumes.allSatisfy { $0 >= 0.999 }

        return VideoBuildResult(composition: comp,
                                videoComposition: videoComposition,
                                audioMix: audioMix,
                                timeMap: timeMap,
                                sourceSize: sourceSize,
                                renderSize: renderSize,
                                isPlainTrim: isPlainTrim && audioUntouched)
    }

    // MARK: - Snapshot state → plan bất biến

    @MainActor
    static func renderPlan(segments: [VideoSegment], sourceSize: CGSize) -> VideoRenderPlan {
        let zooms = segments.filter { $0.kind == .zoom }.map {
            VideoRenderPlan.Zoom(start: $0.start, end: $0.end,
                                 level: $0.zoom, center: $0.center, fade: $0.fade)
        }
        let censors = segments.filter { $0.kind == .censor }.map {
            VideoRenderPlan.Censor(start: $0.start, end: $0.end, rect: $0.rect,
                                   style: $0.censorStyle, strength: $0.strength)
        }
        // Chữ raster ngay tại đây (main thread) — Core Image chỉ việc dán ảnh.
        let texts = segments.filter { $0.kind == .text }.map { seg -> VideoRenderPlan.Text in
            let px = CGSize(width: max(4, seg.rect.width * sourceSize.width),
                            height: max(4, seg.rect.height * sourceSize.height))
            let img = VideoTextRasterizer.image(
                text: seg.text, pixelSize: px,
                fontPixels: CGFloat(seg.fontScale) * sourceSize.height,
                color: seg.textColor, shadow: seg.hasShadow)
            return VideoRenderPlan.Text(start: seg.start, end: seg.end, rect: seg.rect, image: img)
        }
        return VideoRenderPlan(zooms: zooms, censors: censors, texts: texts)
    }

    // MARK: - Mấy phép biến đổi ảnh

    /// Rect chuẩn hoá (gốc TRÊN-trái) → rect Core Image (gốc DƯỚI-trái).
    static func ciRect(_ r: CGRect, in extent: CGRect) -> CGRect {
        CGRect(x: extent.minX + r.minX * extent.width,
               y: extent.minY + (1 - r.minY - r.height) * extent.height,
               width: max(1, r.width * extent.width),
               height: max(1, r.height * extent.height))
    }

    private static func censor(_ img: CIImage, spec: VideoRenderPlan.Censor,
                               extent: CGRect) -> CIImage {
        let r = ciRect(spec.rect, in: extent).intersection(extent)
        guard !r.isNull, r.width > 1, r.height > 1 else { return img }

        let patch: CIImage
        switch spec.style {
        case .blur:
            // clampedToExtent trước khi blur, nếu không mép vùng che sẽ bị đen.
            let radius = max(4, min(r.width, r.height) * CGFloat(0.02 + spec.strength * 0.12))
            patch = img.cropped(to: r).clampedToExtent()
                .applyingGaussianBlur(sigma: Double(radius))
                .cropped(to: r)
        case .pixelate:
            let f = CIFilter.pixellate()
            f.inputImage = img.cropped(to: r).clampedToExtent()
            f.center = CGPoint(x: r.midX, y: r.midY)
            f.scale = Float(max(4, min(r.width, r.height) * CGFloat(0.03 + spec.strength * 0.15)))
            patch = (f.outputImage ?? img).cropped(to: r)
        }
        return patch.composited(over: img)
    }

    /// Phóng `level` lần quanh `center`, kẹp lại để không lòi ra ngoài khung
    /// (nếu không sẽ thấy viền đen ở mép khi tâm zoom sát cạnh).
    private static func zoomed(_ img: CIImage, level: CGFloat,
                               center: CGPoint, extent: CGRect) -> CIImage {
        let half = 1 / (2 * level)
        let cx = min(max(center.x, half), 1 - half)
        let cy = min(max(center.y, half), 1 - half)

        // Điểm tâm trong toạ độ Core Image (gốc dưới-trái).
        let px = extent.minX + cx * extent.width
        let py = extent.minY + (1 - cy) * extent.height
        let target = CGPoint(x: extent.midX, y: extent.midY)

        let t = CGAffineTransform(scaleX: level, y: level)
            .concatenating(CGAffineTransform(translationX: target.x - level * px,
                                             y: target.y - level * py))
        return img.transformed(by: t).cropped(to: extent)
    }
}
