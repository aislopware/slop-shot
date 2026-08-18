#!/usr/bin/env bash
#
# bump-version.sh — ghi một marketing version vào tất cả các chỗ giữ nó, hoặc kiểm tra chúng khớp.
#
#   scripts/bump-version.sh 0.2.0            # ghi
#   scripts/bump-version.sh --check 0.2.0    # chỉ kiểm tra, không sửa gì (package-release.sh gọi)
#
# BA CHỖ, và vì sao chúng tách nhau:
#   project.yml  MARKETING_VERSION                       version của build setting
#   project.yml  info.properties.CFBundleShortVersionString
#                                                        XcodeGen ghi thẳng literal này ra plist —
#                                                        MARKETING_VERSION KHÔNG chạm tới nó
#   Support/Info.plist  CFBundleShortVersionString       file XcodeGen sinh ra nhưng vẫn commit,
#                                                        nên nó là cái lệch âm thầm khi quên `make gen`
#
# package-release.sh có đóng lại CFBundleShortVersionString bằng PlistBuddy trước khi ký, nên một
# bản release vẫn đúng version dù cây source lệch — đó chính là lý do phải có --check: không có nó,
# `make run` ở máy sẽ ra app khai sai version suốt nhiều tháng mà không ai thấy.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_YML="${REPO_ROOT}/project.yml"
INFO_PLIST="${REPO_ROOT}/Support/Info.plist"

CHECK_ONLY=0
if [[ "${1:-}" == "--check" ]]; then
  CHECK_ONLY=1
  shift
fi

VERSION="${1:-}"
[[ -n "${VERSION}" ]] || {
  echo "dùng: $(basename "$0") [--check] <version>   (vd 0.2.0, không có chữ v)" >&2
  exit 2
}
[[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
  echo "cảnh báo: '${VERSION}' không phải dạng x.y.z" >&2

read_project_yml() { sed -nE "s/^[[:space:]]*${1}: \"?([^\"]*)\"?$/\1/p" "${PROJECT_YML}"; }
read_plist() { /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "${INFO_PLIST}"; }

if [[ "${CHECK_ONLY}" == "0" ]]; then
  # sed trên macOS: -i cần một hậu tố, '' là "sửa tại chỗ, không backup".
  sed -i '' -E "s/^([[:space:]]*MARKETING_VERSION: ).*$/\1\"${VERSION}\"/" "${PROJECT_YML}"
  sed -i '' -E "s/^([[:space:]]*CFBundleShortVersionString: ).*$/\1\"${VERSION}\"/" "${PROJECT_YML}"

  # Support/Info.plist là output của XcodeGen. Sinh lại từ project.yml (thay vì sửa tay) để nó
  # giống hệt cái `make gen` sẽ tạo ra — sửa tay là lần sau `make gen` lại ra diff không đâu.
  if command -v xcodegen > /dev/null 2>&1; then
    (cd "${REPO_ROOT}" && xcodegen generate --quiet)
  else
    echo "không có xcodegen — sửa ${INFO_PLIST} bằng PlistBuddy thay thế" >&2
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${VERSION}" "${INFO_PLIST}"
  fi
fi

# Đọc ngược lại cả ba chỗ. sed thay-không-trúng là im lặng thành công, nên bước duy nhất thật sự
# chứng minh được gì là bước này.
fail=0
check() {
  local what="${1}" got="${2}"
  if [[ "${got}" != "${VERSION}" ]]; then
    echo "  ✗ ${what}: ${got:-<trống>} (cần ${VERSION})" >&2
    fail=1
  else
    echo "  ✓ ${what}: ${got}"
  fi
}

check "project.yml MARKETING_VERSION" "$(read_project_yml MARKETING_VERSION)"
check "project.yml CFBundleShortVersionString" "$(read_project_yml CFBundleShortVersionString)"
check "Support/Info.plist CFBundleShortVersionString" "$(read_plist)"

if [[ "${fail}" == "1" ]]; then
  if [[ "${CHECK_ONLY}" == "1" ]]; then
    echo "version trong cây source lệch nhau — chạy: scripts/bump-version.sh ${VERSION}" >&2
  fi
  exit 1
fi
