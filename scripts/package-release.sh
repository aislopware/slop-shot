#!/usr/bin/env bash
#
# package-release.sh — build, ký Developer ID, notarize và đóng gói một bản SlopShot.
#
# VÌ SAO CÓ FILE NÀY: `make install` chỉ đủ cho máy mình (ad-hoc sign, không notarize —
# máy khác tải về sẽ bị Gatekeeper chặn). Đây là chỗ DUY NHẤT biết cách đi từ một checkout
# sạch ra file người lạ cài được, nên CI (.github/workflows/release.yml) và người ngồi cắt
# release bằng tay chạy đúng cùng một đoạn code.
#
# SẢN PHẨM (vào dist/):
#   SlopShot-<version>-universal.dmg   SlopShot.app đã ký + stapled, kèm symlink /Applications
#   SHA256SUMS                         cái mà Casks/slopshot.rb bên homebrew-tap ghim
#
# UNIVERSAL (arm64 + x86_64): app thuần Swift/AppKit, không link thư viện chỉ-có-arm64 nào,
# deployment target macOS 15 — máy Intel vẫn chạy được macOS 15, nên không có lý do gì bỏ họ.
# Cask vì thế không khai `depends_on arch:`.
#
# ĐẦU VÀO ký/notarize (biến môi trường — CI kéo từ vault better-update, xem
# docs/release-pipeline.md; chạy ở máy mình thì dựa vào login keychain là đủ):
#   SLOPSHOT_VERSION              BẮT BUỘC. Marketing version, không có "v" đằng trước (vd 0.1.0).
#   SLOPSHOT_BUILD_NUMBER         CFBundleVersion. Mặc định: 1.
#   SLOPSHOT_SIGN_IDENTITY        Identity cho codesign. Mặc định: Developer ID của WEEBUILD.
#   SLOPSHOT_NOTARY_PROFILE       Tên profile `notarytool --keychain-profile`. Ưu tiên hơn bộ dưới.
#   APPLE_ID / APPLE_TEAM_ID / APPLE_APP_SPECIFIC_PASSWORD
#                                 Thông tin notarytool khi không có keychain profile (đường CI đi).
#   SLOPSHOT_SKIP_NOTARIZE=1      Ký + đóng gói nhưng KHÔNG nộp Apple. CHỈ dùng cho dry-run —
#                                 file ra sẽ KHÔNG qua được Gatekeeper trên máy khác.
#
# Chạy từ thư mục nào cũng được: mọi đường dẫn tính từ gốc repo.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${REPO_ROOT}/dist"
WORK="${REPO_ROOT}/.work/package-release"
DD="${WORK}/DerivedData"
STAGE="${WORK}/stage"

VERSION="${SLOPSHOT_VERSION:?SLOPSHOT_VERSION là bắt buộc (vd 0.1.0, không có chữ v)}"
BUILD_NUMBER="${SLOPSHOT_BUILD_NUMBER:-1}"
SIGN_IDENTITY="${SLOPSHOT_SIGN_IDENTITY:-Developer ID Application: WEEBUILD VIET NAM COMPANY LIMITED (AJ4R8GWM7A)}"
SKIP_NOTARIZE="${SLOPSHOT_SKIP_NOTARIZE:-0}"

APP="SlopShot.app"
ENTITLEMENTS="${REPO_ROOT}/Support/SlopShot.entitlements"
DMG="${DIST}/SlopShot-${VERSION}-universal.dmg"

die() {
  echo "LỖI: $*" >&2
  exit 1
}

step() { echo "── $* ──"; }

# ── 1. Preflight ────────────────────────────────────────────────────────────────────────────
step "Preflight"

for tool in xcodegen xcodebuild codesign hdiutil; do
  command -v "${tool}" > /dev/null 2>&1 || die "thiếu tool: ${tool}"
done

[[ -f "${ENTITLEMENTS}" ]] || die "không thấy ${ENTITLEMENTS}"

# Ký trước khi biết version có khớp không là phí 20 phút build + một lượt notarize. Ba chỗ ghi
# version đều do scripts/bump-version.sh sửa; nếu chúng lệch nhau thì đang cắt release dở dang.
"${REPO_ROOT}/scripts/bump-version.sh" --check "${VERSION}"

security find-identity -v -p codesigning | grep -qF "${SIGN_IDENTITY}" ||
  die "identity không có trong keychain nào đang mở: ${SIGN_IDENTITY}"

if [[ "${SKIP_NOTARIZE}" != "1" ]]; then
  if [[ -z "${SLOPSHOT_NOTARY_PROFILE:-}" ]]; then
    : "${APPLE_ID:?đặt SLOPSHOT_NOTARY_PROFILE, hoặc APPLE_ID + APPLE_TEAM_ID + APPLE_APP_SPECIFIC_PASSWORD}"
    : "${APPLE_TEAM_ID:?APPLE_TEAM_ID bắt buộc khi không có notary keychain profile}"
    : "${APPLE_APP_SPECIFIC_PASSWORD:?APPLE_APP_SPECIFIC_PASSWORD bắt buộc khi không có notary keychain profile}"
  fi
fi

rm -rf "${WORK}"
mkdir -p "${DIST}" "${DD}" "${STAGE}"

echo "version=${VERSION} build=${BUILD_NUMBER}"
echo "identity=${SIGN_IDENTITY}"

# ── 2. Build (chưa ký) ──────────────────────────────────────────────────────────────────────
# Build UNSIGNED có chủ đích: version phải đóng vào Info.plist SAU khi build (XcodeGen ghi
# CFBundleShortVersionString ra file, còn CFBundleVersion thì mình đóng số build của CI vào),
# mà sửa plist bên trong bundle đã ký là hỏng chữ ký. Nên thứ tự bắt buộc là: build → đóng
# version → ký.
step "Build Release (universal, chưa ký)"

(cd "${REPO_ROOT}" && xcodegen generate --quiet)

xcodebuild \
  -project "${REPO_ROOT}/SlopShot.xcodeproj" \
  -scheme SlopShot \
  -configuration Release \
  -destination 'generic/platform=macOS' \
  -derivedDataPath "${DD}" \
  ARCHS="arm64 x86_64" \
  ONLY_ACTIVE_ARCH=NO \
  CODE_SIGNING_ALLOWED=NO \
  build

BUILT="${DD}/Build/Products/Release/${APP}"
[[ -d "${BUILT}" ]] || die "xcodebuild không sinh ra ${BUILT}"
cp -R "${BUILT}" "${STAGE}/"

# `lipo` chứ không tin ARCHS: một setting bị project.yml đè lại thì bản "universal" âm thầm
# chỉ còn một slice, và người dùng Intel là người phát hiện ra thay vì mình.
slices="$(lipo -archs "${STAGE}/${APP}/Contents/MacOS/SlopShot")"
for arch in arm64 x86_64; do
  case " ${slices} " in
    *" ${arch} "*) ;;
    *) die "binary thiếu slice ${arch} (lipo thấy: ${slices})" ;;
  esac
done
echo "slices=${slices}"

# ── 3. Đóng version + ký ────────────────────────────────────────────────────────────────────
step "Đóng version + ký ${APP}"

PLIST="${STAGE}/${APP}/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${VERSION}" "${PLIST}"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${BUILD_NUMBER}" "${PLIST}"

# --options runtime (Hardened Runtime) + --timestamp (secure timestamp) là hai điều kiện
# notarization; --entitlements mở lại đúng một thứ Hardened Runtime khoá mà app cần: micro.
codesign --force --sign "${SIGN_IDENTITY}" --options runtime --timestamp \
  --entitlements "${ENTITLEMENTS}" "${STAGE}/${APP}"
codesign --verify --strict --deep --verbose=1 "${STAGE}/${APP}"

notarize() {
  local artifact="${1}"
  if [[ -n "${SLOPSHOT_NOTARY_PROFILE:-}" ]]; then
    xcrun notarytool submit "${artifact}" \
      --keychain-profile "${SLOPSHOT_NOTARY_PROFILE}" --wait
  else
    xcrun notarytool submit "${artifact}" \
      --apple-id "${APPLE_ID}" --team-id "${APPLE_TEAM_ID}" \
      --password "${APPLE_APP_SPECIFIC_PASSWORD}" --wait
  fi
}

# ── 4. Notarize + staple APP, TRƯỚC khi nó vào DMG ──────────────────────────────────────────
# Thứ tự là toàn bộ mẹo ở đây: cask copy SlopShot.app RA KHỎI ảnh đĩa, nên ticket chỉ dán trên
# DMG không bao giờ tới được app người dùng bấm vào — Gatekeeper phải hỏi Apple qua mạng, và
# lần mở đầu tiên khi không có mạng là fail. App bên trong DMG là một BẢN COPY, nên staple phải
# xong trước `cp -R … "${DMG_ROOT}"` ở dưới. Đẩy khối này xuống sau bước tạo DMG thì pipeline
# vẫn xanh và âm thầm ship app không ticket.
if [[ "${SKIP_NOTARIZE}" == "1" ]]; then
  echo "SLOPSHOT_SKIP_NOTARIZE=1 — app đã ký nhưng CHƯA notarize/staple."
else
  step "Notarize ${APP}"
  APP_ZIP="${WORK}/SlopShot-${VERSION}.zip"
  # ditto --sequesterRsrc là cách Apple khuyến nghị đóng zip đi notarize: resource fork bị tách
  # riêng thay vì làm hỏng gói upload.
  ditto -c -k --keepParent --sequesterRsrc "${STAGE}/${APP}" "${APP_ZIP}"
  notarize "${APP_ZIP}"

  # Staple bản gốc trong STAGE — đó mới là bản dùng để dựng DMG. Validate luôn thay vì tin exit
  # code: ticket không dán được thì phải fail ở đây, chứ không ship một app trông ổn cho tới lúc
  # ai đó mở nó offline.
  step "Staple ${APP}"
  xcrun stapler staple "${STAGE}/${APP}"
  xcrun stapler validate "${STAGE}/${APP}" ||
    die "không có ticket nào dán được vào ${APP} — DMG sẽ ship app fail ở lần mở đầu tiên offline"
fi

# ── 5. DMG ──────────────────────────────────────────────────────────────────────────────────
step "Dựng DMG"

DMG_ROOT="${WORK}/dmg"
mkdir -p "${DMG_ROOT}"
cp -R "${STAGE}/${APP}" "${DMG_ROOT}/"
ln -s /Applications "${DMG_ROOT}/Applications"

rm -f "${DMG}"
hdiutil create -srcfolder "${DMG_ROOT}" -volname "SlopShot ${VERSION}" \
  -fs HFS+ -format UDZO -quiet "${DMG}"
codesign --force --sign "${SIGN_IDENTITY}" --timestamp "${DMG}"

if [[ "${SKIP_NOTARIZE}" == "1" ]]; then
  echo "SLOPSHOT_SKIP_NOTARIZE=1 — DMG đã ký nhưng CHƯA notarize (chỉ hợp lệ cho dry run)."
else
  # DMG cần lượt notarize riêng: Apple không suy ra ticket của ảnh đĩa từ ruột nó được.
  step "Notarize DMG"
  notarize "${DMG}"
  xcrun stapler staple "${DMG}"
  xcrun stapler validate "${DMG}"
fi

# ── 6. Checksum ─────────────────────────────────────────────────────────────────────────────
step "Checksum"
(cd "${DIST}" && shasum -a 256 "$(basename "${DMG}")" > SHA256SUMS)
cat "${DIST}/SHA256SUMS"

echo "OK: ${DIST}"
