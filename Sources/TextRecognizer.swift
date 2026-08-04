import Vision   // framework OCR có sẵn của Apple — chạy on-device, không cần mạng
import CoreGraphics

// Kết quả 1 lần "soi" ảnh: chữ đọc được + mã QR nếu có.
struct OCRResult {
    let text: String
    let qrCodes: [String]

    var isEmpty: Bool { text.isEmpty && qrCodes.isEmpty }
    /// Nội dung để chép vào clipboard: ưu tiên chữ, không có chữ thì lấy QR.
    var copyText: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            ? qrCodes.joined(separator: "\n") : text
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Nhận chữ trong 1 ảnh (OCR). Vision tự lo phần máy học, ta chỉ gọi & gom kết quả.
//
// React analogy: như 1 hàm async tiện ích `await ocr(image)` trả về string,
// không giữ state gì — nên để static cho gọn.
// ─────────────────────────────────────────────────────────────────────────
enum TextRecognizer {

    // Trả về toàn bộ chữ đọc được (mỗi dòng cách nhau bằng "\n"). "" nếu không có chữ.
    static func recognize(in cgImage: CGImage) async -> String {
        await scan(in: cgImage).text
    }

    // Đọc chữ + dò mã QR trong cùng 1 lượt.
    static func scan(in cgImage: CGImage) async -> OCRResult {
        await withCheckedContinuation { cont in
            // 1) Tạo "yêu cầu" nhận chữ. Callback chạy khi Vision xử lý xong.
            let request = VNRecognizeTextRequest { req, _ in
                // Mỗi observation = 1 dòng chữ Vision tìm thấy; lấy ứng viên tốt nhất.
                let lines = (req.results as? [VNRecognizedTextObservation] ?? [])
                    .compactMap { $0.topCandidates(1).first?.string }
                cont.resume(returning: OCRResult(text: lines.joined(separator: "\n"),
                                                 qrCodes: detectQRCodes(in: cgImage)))
            }
            // .accurate = ưu tiên độ chính xác hơn tốc độ (ảnh tĩnh nên chấp nhận chậm hơn).
            request.recognitionLevel = .accurate
            request.usesLanguageCorrection = true
            // Không bật cái này thì Vision mặc định chỉ đọc tiếng Anh → chữ Việt/
            // Trung/Nhật ra rác. Bật lên, Vision tự dò ngôn ngữ trong ảnh
            // (bản .accurate hỗ trợ 30 thứ tiếng, có cả vi-VT).
            request.automaticallyDetectsLanguage = true

            // 2) Chạy request trên ảnh. Đẩy sang background để khỏi chặn main thread.
            let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    try handler.perform([request])
                } catch {
                    cont.resume(returning: OCRResult(text: "", qrCodes: detectQRCodes(in: cgImage)))
                }
            }
        }
    }

    // Mã QR trong vùng chụp (rẻ, chạy chung 1 lượt với OCR cho tiện).
    private static func detectQRCodes(in cgImage: CGImage) -> [String] {
        let request = VNDetectBarcodesRequest()
        request.symbologies = [.qr, .microQR]
        try? VNImageRequestHandler(cgImage: cgImage, options: [:]).perform([request])
        let found = (request.results ?? [])
            .compactMap { $0.payloadStringValue }
            .filter { !$0.isEmpty }
        // Bỏ trùng nhưng giữ thứ tự.
        var seen = Set<String>()
        return found.filter { seen.insert($0).inserted }
    }
}
