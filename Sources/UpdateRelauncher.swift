import Foundation

// ─────────────────────────────────────────────────────────────────────────
// `brew upgrade` xong thì tự mở lại bản mới.
//
// Cask tắt SlopShot bằng SIGTERM sau khi đã chép bản mới vào /Applications,
// nhưng KHÔNG mở lại được: bước cài của Homebrew chạy trong sandbox, `open` ở
// đó bị LaunchServices từ chối. Nên app tự lo: nhận SIGTERM → nếu file .app ở
// đường dẫn cũ đã là một bundle KHÁC (bị thay) thì mở bundle mới rồi thoát.
//
// "Khác" = file chạy (Contents/MacOS/SlopShot) khác inode, không so số version:
// cài lại cùng bản cũng thay bundle và cũng cần mở lại. Phải so file chạy chứ
// không so thư mục .app: khi terminal chạy brew có quyền App Management, brew
// GIỮ thư mục .app cũ và chỉ chuyển ruột mới vào — inode thư mục không đổi.
// SIGTERM vì lý do khác (tắt máy, `kill` tay, gỡ cài đặt — file vẫn nguyên
// hoặc biến mất) thì thoát bình thường như trước.
// ─────────────────────────────────────────────────────────────────────────
@MainActor
enum UpdateRelauncher {
    private static var source: DispatchSourceSignal?

    static func install() {
        let path = Bundle.main.bundlePath
        guard let executable = Bundle.main.executablePath else { return }
        let launchedInode = inode(of: executable)
        signal(SIGTERM, SIG_IGN)   // tắt xử lý mặc định để DispatchSource nhận được
        let src = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        src.setEventHandler {
            if let launchedInode, let now = inode(of: executable), now != launchedInode {
                let open = Process()
                open.executableURL = URL(fileURLWithPath: "/usr/bin/open")
                // -n: bản này còn sống tới lúc exit, `open` thường sẽ chỉ gọi nó lên.
                open.arguments = ["-n", path]
                // Chờ `open` gửi xong yêu cầu rồi mới thoát, kẻo tiến trình con bị dọn
                // theo app khi app chết.
                if (try? open.run()) != nil { open.waitUntilExit() }
            }
            exit(0)
        }
        src.resume()
        source = src
    }

    private static func inode(of path: String) -> UInt64? {
        (try? FileManager.default.attributesOfItem(atPath: path)[.systemFileNumber] as? NSNumber)?
            .uint64Value
    }
}
