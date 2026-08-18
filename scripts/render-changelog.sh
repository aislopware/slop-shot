#!/usr/bin/env bash
#
# render-changelog.sh — viết CHANGELOG.md từ commit log. THỨ DUY NHẤT được phép ghi file đó.
#
#   scripts/render-changelog.sh                # render lại đúng như cây hiện tại
#   scripts/render-changelog.sh --tag v0.2.0   # xếp các commit đang treo vào một version
#
# Mọi tham số được chuyển thẳng cho git-cliff.
#
# Nó tồn tại vì đúng một byte. Template trong `cliff.toml` kết mỗi release bằng một dòng
# trống để tiêu đề `## [x.y.z]` kế tiếp không bị dán vào bullet cuối của release trước —
# nên release cuối cùng kéo theo dòng trống đó ra tận cuối file. Hook `end-of-file-fixer`
# sẽ viết lại file, và `cut-release.sh` fail ngay chính cái commit nó vừa sinh ra vài giây
# trước, giữa chừng, khi ba chỗ giữ version đã bị ghi rồi.
#
# `postprocessors` trong cliff.toml không chữa được: chúng chạy trên TỪNG RELEASE chứ không
# trên văn bản hoàn chỉnh, nên một luật `\n+$` bóc dấu ngăn cách khỏi mọi thân release và
# dán các tiêu đề vào nhau, trong khi cuối-file thì vẫn y nguyên.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

command -v git-cliff > /dev/null || {
  echo "render-changelog: không có git-cliff trên PATH (brew install git-cliff)" >&2
  exit 1
}

# `$(...)` cắt sạch mọi newline cuối, printf trả lại đúng một cái. Render vào biến thay vì
# `--output` để một lần git-cliff fail thì CHANGELOG.md đã commit vẫn nguyên vẹn chứ không
# bị cụt.
rendered="$(git-cliff "$@")"

# So khớp bằng REGEX, không bao giờ dùng `${rendered//[[:space:]]/}`. Phép thay-toàn-cục của
# bash có độ phức tạp bậc hai theo độ dài chuỗi: riêng cái kiểm tra "có rỗng không" này đã
# đốt 57 giây CPU trên một changelog 32 KB, biến một lần render 1 giây thành một phút.
if [[ ! "${rendered}" =~ [^[:space:]] ]]; then
  echo "render-changelog: git-cliff render ra rỗng — không ghi đè" >&2
  exit 1
fi

printf '%s\n' "${rendered}" > CHANGELOG.md
