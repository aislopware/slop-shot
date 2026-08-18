#!/usr/bin/env bash
#
# changelog-section.sh <version> — in ra đúng phần của một version trong CHANGELOG.md.
#
# Job publish trong .github/workflows/release.yml lấy đoạn này làm nội dung GitHub Release, và
# THIẾU thì fail chứ không rơi về đoạn văn mẫu: một release không có ghi chú thì v0.2.1 và v0.2.2
# dạy người đọc đúng một lượng thông tin như nhau — không gì cả.
#
# Job package cũng gọi nó ngay sau khi resolve version, để một tag quên viết changelog fail sau
# vài giây thay vì sau ~15 phút build đã ký và đã notarize.
set -euo pipefail

VERSION="${1:?dùng: $(basename "$0") <version>   (vd 0.1.0, không có chữ v)}"
VERSION="${VERSION#v}"
CHANGELOG="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/CHANGELOG.md}"

[[ -f "${CHANGELOG}" ]] || {
  echo "không thấy ${CHANGELOG}" >&2
  exit 1
}

# git-cliff render tiêu đề dạng `## [0.1.0](compare-url) — 2026-08-18`. Bắt theo riêng phần
# version trong ngoặc vuông: ngày thì đổi theo tag còn URL đổi theo tag TRƯỚC đó, không cái nào
# neo được. Vẫn nhận cả `## 0.1.0` trần để một changelog viết tay cũng cắt được.
#
# In từ sau dòng tiêu đề tới ngay trước `## ` kế tiếp (bỏ chính dòng tiêu đề — GitHub Release đã
# có version ở title rồi), rồi cắt các dòng trống thừa hai đầu.
section="$(awk -v want="## [${VERSION}]" -v plain="## ${VERSION}" '
  /^## / {
    if (found) exit
    # index(), không phải regex: dấu chấm trong một version là ký tự đại diện của regex, và
    # "0.2.3" sẽ khớp cả một tiêu đề ghi "0x2y3".
    if (index($0, want) == 1 || $0 == plain) { found = 1 }
    next
  }
  found { lines[n++] = $0 }
  END {
    start = 0
    while (start < n && lines[start] ~ /^[[:space:]]*$/) start++
    stop = n - 1
    while (stop >= start && lines[stop] ~ /^[[:space:]]*$/) stop--
    for (i = start; i <= stop; i++) print lines[i]
  }
' "${CHANGELOG}")"

# So khớp bằng regex chứ không `${section//[[:space:]]/}`: phép thay-toàn-cục của bash có độ
# phức tạp bậc hai theo độ dài chuỗi, mà một mục release thì dài ra theo chính release đó.
if [[ ! "${section}" =~ [^[:space:]] ]]; then
  echo "CHANGELOG.md không có mục cho ${VERSION} (hoặc mục đó rỗng)" >&2
  echo "  Sinh lại:  scripts/render-changelog.sh --tag v${VERSION}" >&2
  echo "  Hoặc cắt release cho đúng bài:  make release" >&2
  exit 1
fi

printf '%s\n' "${section}"
