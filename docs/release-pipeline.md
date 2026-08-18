# Release pipeline

Từ một commit ở máy tới `brew install --cask aislopware/tap/slopshot` trên máy người lạ.

```
  tiêu đề commit          ┐
  (conventional)          ├─► git-cliff ─► version + CHANGELOG.md
                          ┘
                                 │  make release  →  commit + tag v0.1.0 (KHÔNG push)
                                 ▼
             git push origin main && git push origin v0.1.0
                                 │
                                 ▼
  .github/workflows/release.yml
     package ─► build universal ─► ký Developer ID ─► notarize ─► staple ─► DMG
     publish ─► GitHub Release, nội dung = một lát CHANGELOG.md
     tap     ─► aislopware/homebrew-tap: Casks/slopshot.rb ← version + sha256
```

Cả ba job đều chạy đúng thứ ở trong repo này: `scripts/package-release.sh` là đường đi
duy nhất từ checkout ra file cài được, nên CI và người cắt release bằng tay dùng chung một
đoạn code. Không có bước nào chỉ tồn tại trong giao diện GitHub.

---

## 1. Tiêu đề commit là đầu vào của release

Version và ghi chú release không viết tay. Cả hai sinh ra từ tiêu đề commit, đọc hai lần:

| đọc bởi | để làm gì |
|---|---|
| `git-cliff --bumped-version` | `feat` → minor, `fix`/`perf`/`refactor` → patch, `!` → major |
| `scripts/render-changelog.sh` | render mục của release trong `CHANGELOG.md` |

Cấu hình ở `cliff.toml`. Ngữ pháp bị chặn ngay lúc viết commit:

```bash
brew install prek git-cliff     # prek = pre-commit viết bằng Rust
make hooks                      # cài hook pre-commit + commit-msg
```

```
<type>[(scope)][!]: <tiêu đề>

type: build, chore, ci, docs, feat, fix, perf, refactor, revert, style, test
```

`scripts/check-commit-msg.sh` chặn thêm mấy thứ về văn phong, vì tiêu đề này **được xuất
bản** — nội dung GitHub Release là mỗi tiêu đề một bullet, nguyên văn:

- mở đầu bằng `The`/`A`/`An` → đó là lời tả, không phải thay đổi;
- `adds`, `fixed`, `updates`… → không phải thể mệnh lệnh;
- kết thúc bằng dấu chấm; dài quá 72 ký tự (chỗ GitHub cắt `…`).

Bỏ qua một lần: `git commit --no-verify`. Nhưng commit đó sẽ không có mặt trong changelog
lẫn trong phép tính version, nên bỏ qua là tự chọn cho nó biến mất.

> **Lịch sử trước hook.** 20 commit đầu của repo đều là văn xuôi. `cliff.toml` ở đây để
> `filter_unconventional = false` (slop-desk để `true`) nên chúng rơi vào mục *Other
> changes* của v0.1.0 thay vì bị vứt — một release rỗng thì git-cliff xoá luôn cả tiêu đề,
> kéo theo `changelog-section.sh` fail. Chuyện này chỉ xảy ra đúng một lần.

## 2. Cắt release

```bash
make release-preview            # version sắp ra + nội dung release, không ghi gì
make release                    # cắt thật
make release VERSION=0.2.0      # ép version
```

`scripts/cut-release.sh` làm theo thứ tự: kiểm nhánh `main` + cây sạch → hỏi git-cliff
version → render `CHANGELOG.md --tag v<x>` → kiểm mục đó cắt ra được → ghi version vào ba
chỗ → commit `chore(release): v<x>` → tag.

**Nó không push.** Cú push tag là cái đốt ~15 phút CI và một lượt nộp Apple, nên nó ở lại
là một phím bấm riêng:

```bash
git push origin main && git push origin v0.2.0
gh run watch --repo aislopware/slop-shot
```

Huỷ khi lỡ tay: `git tag -d v0.2.0 && git reset --hard HEAD~1`.

Lần release **đầu tiên** phải ép `VERSION=`: git-cliff suy tiền tố `v` từ tag gần nhất, mà
lúc đó chưa có tag nào.

### Ba chỗ giữ version

`scripts/bump-version.sh` ghi và đọc ngược lại cả ba — `sed` thay không trúng là im lặng
thành công, nên bước đọc lại mới là bước chứng minh được điều gì.

| chỗ | vì sao tách ra |
|---|---|
| `project.yml` → `MARKETING_VERSION` | build setting |
| `project.yml` → `info.properties.CFBundleShortVersionString` | XcodeGen ghi thẳng literal này ra plist, `MARKETING_VERSION` **không** chạm tới |
| `Support/Info.plist` | file XcodeGen sinh ra nhưng vẫn commit → chỗ lệch âm thầm khi quên `make gen` |

`scripts/package-release.sh` gọi `bump-version.sh --check` ở bước preflight: lệch thì fail
trong vài giây thay vì sau một bản build đã ký.

## 3. Ký và notarize

`scripts/package-release.sh` — chạy được cả ở CI lẫn ở máy.

- **Universal (arm64 + x86_64).** App thuần Swift/AppKit, target macOS 15, mà máy Intel vẫn
  chạy được macOS 15. Xong build thì `lipo -archs` kiểm lại chứ không tin `ARCHS=`: một
  setting bị đè là bản "universal" âm thầm còn một slice, và người dùng Intel phát hiện ra
  thay cho mình.
- **Build unsigned trước, đóng version sau, ký sau cùng.** Sửa plist bên trong một bundle đã
  ký là hỏng chữ ký, mà `CFBundleVersion` phải đợi số build của CI.
- **Hardened Runtime + secure timestamp** (`--options runtime --timestamp`) là điều kiện
  notarization. `Support/SlopShot.entitlements` mở lại đúng một thứ Hardened Runtime khoá mà
  app cần: `com.apple.security.device.audio-input` (mic trong screen recording).
- **Notarize + staple `.app` TRƯỚC khi nó vào DMG.** Cask copy `SlopShot.app` *ra khỏi* ảnh
  đĩa, nên ticket chỉ dán trên DMG không bao giờ tới được app người dùng bấm vào —
  Gatekeeper phải hỏi Apple qua mạng, và lần mở đầu tiên khi không có mạng là fail. App bên
  trong DMG là một bản copy. Đẩy khối này xuống sau bước tạo DMG thì pipeline vẫn xanh và
  âm thầm ship app không ticket.
- **DMG cần lượt notarize riêng.** Apple không suy ticket của ảnh đĩa từ ruột nó.

Ra `dist/SlopShot-<version>-universal.dmg` và `dist/SHA256SUMS`.

Identity mặc định: `Developer ID Application: WEEBUILD VIET NAM COMPANY LIMITED (AJ4R8GWM7A)`.

### Thử ở máy mình

```bash
make package VERSION=0.1.0 SKIP_NOTARIZE=1
```

Vẫn ký Developer ID đầy đủ, chỉ không nộp Apple. File ra **không** qua được Gatekeeper trên
máy khác — chỉ để chứng minh đường build + ký còn chạy.

Muốn notarize thật ở máy, đăng ký profile một lần rồi trỏ vào nó:

```bash
xcrun notarytool store-credentials slopshot-notary \
  --apple-id "…" --team-id AJ4R8GWM7A --password "<app-specific-password>"

SLOPSHOT_NOTARY_PROFILE=slopshot-notary make package VERSION=0.1.0
```

## 4. Secret

Repo này **public**, và mọi dòng workflow in ra là công khai vĩnh viễn. Hình dạng bắt buộc,
giữ nguyên:

- **Một step duy nhất** chạm vào vật liệu ký: pull → mask → source → import → build → ký →
  notarize. Không secret nào vượt ranh giới step, nên không có `$GITHUB_ENV` hay `set -x`
  của step sau nào làm lộ được.
- **Mask TRƯỚC.** `better-update env pull --stdout` in ra các dòng `export KEY='value'`; mỗi
  giá trị được `::add-mask::` trước khi bất cứ thứ gì kịp echo nó. Một giá trị kịp tới log
  trước khi mask của nó được đăng ký thì mask sau đó vô nghĩa.
- **Keychain dùng một lần** cho mỗi job, `security list-keychains` *thêm* vào đầu search
  list chứ không thay thế — thay thế là hất login keychain ra khỏi danh sách mà toolchain
  Xcode vẫn đang đọc.

### Vì sao là better-update chứ không phải GitHub secrets

Developer ID là của WEEBUILD VIET NAM và dùng chung cho nhiều sản phẩm (slop-desk, idealabs
desktop, và đây). Nó nằm một chỗ duy nhất trong vault E2E của better-update; mọi pipeline
kéo từ đó. Xoay chứng chỉ là sửa một chỗ, không phải đi sửa N repo.

**GitHub secret duy nhất của repo này:**

| tên | là gì |
|---|---|
| `BETTER_UPDATE_ROBOT` | token robot để mở vault |

Đặt: `gh secret set BETTER_UPDATE_ROBOT --repo aislopware/slop-shot`.

**Trong vault** (environment `production`) — `eas.json` ở gốc repo link project bằng
`projectId`, nên `env pull` phải chạy *bên trong* checkout, nếu không nó thoát mã 4 với
"Project not linked":

| biến | dùng ở job |
|---|---|
| `APPLE_CERTIFICATE_P12_BASE64` | package — .p12 Developer ID, base64 |
| `APPLE_CERTIFICATE_PASSWORD` | package — mật khẩu của .p12 đó |
| `APPLE_ID` | package — tài khoản notarytool |
| `APPLE_TEAM_ID` | package — `AJ4R8GWM7A` |
| `APPLE_APP_SPECIFIC_PASSWORD` | package — app-specific password của `APPLE_ID` |
| `HOMEBREW_TAP_TOKEN` | tap — PAT có quyền push vào `aislopware/homebrew-tap` |

CI cài `@better-update/cli@latest` **kèm `--minimum-release-age=0`**, và cái cờ đó là bắt
buộc chứ không phải trang trí: mặc định bun giữ lại các bản publish trong 24h, nên `@latest`
trần có thể âm thầm rơi về một build cũ hơn mức server chấp nhận (server từ chối mọi bản
dưới `0.72.0`). Chính rủi ro đó là lý do trước đây phải ghim cứng một version; tắt cái
giữ-lại đi thì bỏ được ghim mà không dính lại cái bẫy.

## 5. Homebrew tap

`aislopware/homebrew-tap` → `Casks/slopshot.rb`.

```bash
brew trust aislopware/tap                    # Homebrew 6+ chặn tap bên thứ ba
brew install --cask aislopware/tap/slopshot
```

Job `tap` `sed` đúng hai dòng của file cask:

```ruby
  version "0.1.0"
  sha256 "…"
```

Nên **giữ mỗi cái đúng một dòng, thụt vào hai dấu cách**. Viết lại thành `version("0.1.0")`
hay đổi thụt lề là `sed` không khớp nữa, job vẫn xanh, và tap kẹt ở version cũ. Có comment
cảnh báo ngay đầu file cask.

Cask **không** khai `depends_on arch:` vì DMG là universal. Nó khai
`depends_on macos: ">= :sequoia"` khớp `LSMinimumSystemVersion 15.0` trong `Support/Info.plist`.

## 6. Chạy tay khi cần

`workflow_dispatch` nhận `version` và `dry-run`:

```bash
# vẫn ký Developer ID rồi dừng: không notarize, không tạo Release, không bump tap
gh workflow run release.yml --repo aislopware/slop-shot \
  -f version=0.1.0 -f dry-run=true
```

`concurrency: release` — một release một lúc. **Đừng bao giờ cancel một lượt notarize đang
chạy:** Apple tính lượt nộp đó dù sao đi nữa, và huỷ giữa chừng để lại artifact đã ký nhưng
chưa staple.

Chạy lại một release đã có (tag lại, retry job) là an toàn: job `publish` `edit` + `upload
--clobber` thay vì fail, job `tap` coi "không có gì đổi" là no-op đúng chứ không phải lỗi.

## 7. Hỏng hay gặp

| triệu chứng | nguyên nhân |
|---|---|
| `CHANGELOG.md không có mục cho 0.2.0` | tag được push mà không qua `make release`. Chạy `scripts/render-changelog.sh --tag v0.2.0`, commit, tag lại. |
| `version trong cây source lệch nhau` | ai đó sửa `project.yml` mà quên `make gen`. `scripts/bump-version.sh <version>`. |
| `identity không có trong keychain nào đang mở` | ở máy: chưa cài .p12 Developer ID. Ở CI: `APPLE_CERTIFICATE_*` sai hoặc thiếu trong vault. |
| `binary thiếu slice x86_64` | có setting đè `ARCHS` trong `project.yml`. |
| codesign đứng chờ mãi ở CI | thiếu `security set-key-partition-list` — nó đang đợi một hộp thoại UI không ai trả lời. |
| `Project not linked` (mã 4) | `env pull` chạy ngoài checkout; job `tap` phải `checkout` **trước** `download-artifact`. |
| `git-cliff không tính được version` | không có conventional commit nào sau tag gần nhất. Ép: `make release VERSION=x.y.z`. |
| Gatekeeper chặn app tải về dù CI xanh | ticket chưa staple vào `.app` trước khi dựng DMG. Xem lại thứ tự ở mục 3. |
