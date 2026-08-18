#!/usr/bin/env bash
#
# cut-release.sh — cắt một bản release: quyết version, viết ghi chú, ghi ba chỗ, commit, tag.
#
#   make release                    # version tính từ các commit sau tag gần nhất
#   make release VERSION=0.2.0      # ép version
#   make release-preview            # in kế hoạch + ghi chú, không đụng file nào
#
# Trước đây version là tham số bắt buộc và CHANGELOG viết tay. Giờ cả hai đều ra từ cùng một
# nguồn: tiêu đề commit. `git-cliff --bumped-version` đọc feat/fix/`!` để ra minor/patch/major,
# `render-changelog.sh` đọc lại chính chúng để render ghi chú mà GitHub Release đăng. Cái giữ
# cho hai đường đọc đó có nghĩa là hook commit-msg (scripts/check-commit-msg.sh).
#
# KHÔNG push. Cú push tag mới là cái khởi động pipeline ký + notarize (~15 phút và một lượt nộp
# Apple), nên nó ở lại là một thao tác riêng, cố ý:
#
#   git push origin main && git push origin v0.2.0
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

die() {
  echo "cut-release: $*" >&2
  exit 1
}
step() { echo "── $* ────────────────────────────────────────"; }

DRY_RUN=0
VERSION=""
for arg in "$@"; do
  case "${arg}" in
    --dry-run) DRY_RUN=1 ;;
    -*) die "cờ lạ: ${arg}" ;;
    *) VERSION="${arg#v}" ;;
  esac
done

command -v git-cliff > /dev/null || die "không có git-cliff trên PATH (brew install git-cliff)"
command -v xcodegen > /dev/null || die "không có xcodegen trên PATH (brew install xcodegen)"

# Release cắt TỪ một nhánh, không cắt từ checkout rời, và cắt từ main vì tag phải với tới được
# từ đúng cái nhánh mà tap và tài liệu đang trỏ vào.
branch="$(git rev-parse --abbrev-ref HEAD)"
[[ "${branch}" == "main" ]] || die "đang ở nhánh '${branch}' — release cắt từ main"

# Cây bẩn nghĩa là commit release sẽ mang theo phần việc không ai duyệt như một phần của nó, và
# bump sẽ đè lên những sửa đổi chưa từng được build.
if [[ -n "$(git status --porcelain)" ]]; then
  git status --short >&2
  die "cây làm việc còn thay đổi chưa commit — commit hoặc stash trước đã"
fi

step "Quyết version"
if [[ -z "${VERSION}" ]]; then
  # Chưa có tag nào thì git-cliff không tính được: nó ra "0.1.0" trần rồi tự loại vì không
  # khớp `tag_pattern = "v[0-9]*"` — nó suy tiền tố `v` TỪ tag gần nhất, mà đây chưa có cái
  # nào. Chỉ đúng một lần trong đời repo, và câu trả lời là ép version trên dòng lệnh.
  [[ -n "$(git tag --list 'v[0-9]*')" ]] ||
    die "repo chưa có tag v* nào — lần đầu phải ép: make release VERSION=0.1.0"

  VERSION="$(git-cliff --bumped-version 2> /dev/null)" ||
    die "git-cliff không tính được version (không có conventional commit nào sau tag gần nhất?)"
  VERSION="${VERSION#v}"
  echo "tính từ các commit sau tag gần nhất: ${VERSION}"
else
  echo "ép trên dòng lệnh: ${VERSION}"
fi
[[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] ||
  die "không phải semver: ${VERSION}"

git rev-parse -q --verify "refs/tags/v${VERSION}" > /dev/null &&
  die "tag v${VERSION} đã tồn tại — chọn version khác hoặc xoá tag"

step "Render CHANGELOG.md"
# `--tag` bảo git-cliff xếp các commit đang treo XUỐNG DƯỚI version sắp tag, thay vì để chúng ở
# mục "Unreleased" mà release không cắt ra được.
scripts/render-changelog.sh --tag "v${VERSION}"
notes="$(scripts/changelog-section.sh "${VERSION}")" ||
  die "changelog vừa render không có mục ${VERSION}"

if [[ "${DRY_RUN}" == "1" ]]; then
  step "Chạy khô — nội dung release sẽ là"
  printf '%s\n' "${notes}"
  git checkout -- CHANGELOG.md 2> /dev/null || rm -f CHANGELOG.md
  echo
  echo "cut-release: chưa ghi gì cả. Chạy lại không có --dry-run để cắt v${VERSION}."
  exit 0
fi

step "Ghi version vào cả ba chỗ"
scripts/bump-version.sh "${VERSION}"

step "Commit và tag"
git add CHANGELOG.md project.yml Support/Info.plist

# `chore(release)` là tiêu đề DUY NHẤT mà `cliff.toml` skip, nên commit release không bao giờ
# xuất hiện trong ghi chú của release sau nó.
git commit -m "chore(release): v${VERSION}"
git tag -a "v${VERSION}" -m "SlopShot ${VERSION}"

cat << EOF

✅ v${VERSION} đã commit và tag ở local, chưa gì rời khỏi máy này.

   Xem lại:  git show --stat HEAD
   Đẩy đi:   git push origin main && git push origin v${VERSION}
   Huỷ:      git tag -d v${VERSION} && git reset --hard HEAD~1

Theo dõi pipeline: gh run watch --repo aislopware/slop-shot
EOF
