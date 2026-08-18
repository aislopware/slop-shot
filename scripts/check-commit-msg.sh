#!/usr/bin/env bash
#
# check-commit-msg.sh <file> — chặn một tiêu đề commit không đúng conventional commit.
#
#   scripts/check-commit-msg.sh .git/COMMIT_EDITMSG
#
# Chạy tự động ở hook commit-msg (xem .pre-commit-config.yaml). Toàn bộ luật là một regex và
# vài case, nên hook `language: system` — không có môi trường nào phải dựng, không có cache
# nào để trượt, giá của nó đúng bằng một lần spawn process.
#
# Quy ước này không phải trang trí. `cliff.toml` đọc TYPE để quyết định commit rơi vào mục
# nào của CHANGELOG.md, còn `git-cliff --bumped-version` đọc lại chính nó để quy feat/fix/`!`
# thành minor/patch/major. Một tiêu đề ngoài ngữ pháp thì không đóng góp gì cho version —
# đúng cái hỏng mà hook này sinh ra để làm cho nó ồn và sớm.
set -euo pipefail

FILE="${1:?dùng: check-commit-msg.sh <đường-dẫn-file-commit-message>}"
[[ -f "${FILE}" ]] || {
  echo "check-commit-msg: không thấy file: ${FILE}" >&2
  exit 2
}

# Tiêu đề là dòng đầu tiên không trống và không phải comment. Đọc thẳng dòng 1 sẽ hỏng với
# một `git commit` mà template đặt comment lên trước.
subject="$(grep -m 1 -vE '^\s*(#|$)' "${FILE}" || true)"

if [[ -z "${subject}" ]]; then
  echo "check-commit-msg: commit message rỗng" >&2
  exit 1
fi

# Mấy dòng này do chính git sinh, và chúng bị viết lại hoặc bị bỏ trước khi tới main. Bắt
# chúng theo ngữ pháp là chặn luôn `--fixup`, `--squash` và cả lúc giải xung đột.
case "${subject}" in
  "Merge "* | "Revert "* | "fixup!"* | "squash!"* | "amend!"*) exit 0 ;;
  *) ;; # còn lại đều phải qua ngữ pháp dưới đây
esac

TYPES='build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test'

# type(scope tuỳ chọn)! tuỳ chọn: tiêu đề
if [[ ! "${subject}" =~ ^(${TYPES})(\([a-z0-9][a-z0-9._/-]*\))?!?:\ .+ ]]; then
  cat >&2 << EOF
check-commit-msg: tiêu đề không phải conventional commit.

  đang là:  ${subject}

  phải là:  <type>[(scope)][!]: <tiêu đề>
  type:     ${TYPES//|/, }
            \`!\` hoặc trailer "BREAKING CHANGE:" đánh dấu thay đổi phá vỡ tương thích.

  ví dụ:    fix(timeline): keep two effects on one lane from overlapping
            feat(stickers): download packs on demand instead of bundling them
            feat(editor)!: store annotations in source space, not view space

Vì sao: cliff.toml đọc type để xếp commit này vào mục nào của CHANGELOG.md, và
\`git-cliff --bumped-version\` đọc nó để tính version kế tiếp. Tiêu đề ngoài ngữ
pháp thì không có mặt ở cả hai chỗ.
EOF
  exit 1
fi

# ── Văn phong ────────────────────────────────────────────────────────────────────────
# Phần dưới đây quản cái TEXT sau dấu hai chấm. Nó bị siết vì text đó được XUẤT BẢN:
# `changelog-section.sh` cắt đúng mấy dòng này ra khỏi CHANGELOG.md, và nội dung GitHub
# Release là mỗi tiêu đề một bullet, nguyên văn. Một tiêu đề viết cho người trong repo đọc
# sẽ thành ghi chú release cho người chưa từng thấy repo đọc.
#
# Luật: nói thay đổi này LÀM GÌ, ở thể mệnh lệnh, cho một người không có mặt ở đây.
text="${subject#*: }"
first="${text%% *}"

style_error() {
  cat >&2 << EOF
check-commit-msg: đúng conventional commit rồi, nhưng chưa phải một câu gọn.

  đang là:  ${subject}
  vướng:    $1
  nên:      $2

Nội dung release là mỗi tiêu đề một bullet, nguyên văn (scripts/changelog-section.sh).
Chi tiết không nhét vừa thì để ở THÂN commit — changelog không bao giờ đọc tới đó.
EOF
  exit 1
}

# Tiêu đề mở đầu bằng mạo từ là một câu KỂ VỀ code chứ không phải lệnh cho nó — "the pill
# stops sliding past its neighbour" là tả một cảnh, và người đọc ghi chú release phải tự
# suy ngược ra cái gì đã đổi và có ảnh hưởng tới mình không.
case "${first}" in
  [Tt]he | [Aa] | [Aa]n)
    style_error "mở đầu bằng mạo từ \"${first}\" — đó là lời tả, không phải thay đổi" \
      "bắt đầu bằng động từ: commit này LÀM GÌ? (\"stop the pill sliding past its neighbour\")"
    ;;
  *) ;;
esac

# Thể mệnh lệnh, đúng cái thể mà `git revert`/`git merge` tự viết sẵn cho bạn. Ngôi thứ ba
# là kiểu trượt hay gặp nhất và bắt máy móc được; gerund tách xuống dưới vì mệnh lệnh thật
# cũng có cái kết thúc bằng -ing ("bring", "string").
case "${first}" in
  [Aa]dds | [Bb]umps | [Cc]hanges | [Dd]rops | [Ff]ixes | [Kk]eeps | [Mm]akes | [Mm]oves | \
    [Rr]emoves | [Rr]enames | [Ss]tops | [Uu]pdates | [Uu]ses | [Aa]dded | [Ff]ixed | \
    [Cc]hanged | [Rr]emoved | [Uu]pdated)
    style_error "\"${first}\" không phải thể mệnh lệnh" \
      "viết như một câu lệnh: \"add\", \"fix\", \"drop\", \"rename\""
    ;;
  *) ;;
esac

# Tiêu đề là một cái tít. Dấu chấm không mua được gì mà changelog thì render nó vào giữa
# bullet.
case "${text}" in
  *.)
    style_error "kết thúc bằng dấu chấm" "bỏ dấu chấm cuối"
    ;;
  *) ;;
esac

# 72 là chỗ GitHub cắt "…" một tiêu đề trong danh sách commit, VÀ cũng là chỗ một bullet
# changelog hết đọc lướt được. Chặn cứng chứ không cảnh báo: cách sửa lúc nào cũng có sẵn —
# đẩy chi tiết xuống thân commit, chỗ mà lý lẽ cho một thay đổi vốn nên nằm.
if [[ "${#subject}" -gt 72 ]]; then
  style_error "tiêu đề dài ${#subject} ký tự; GitHub cắt từ 72" \
    "gọt còn 72 và đẩy phần dư xuống thân commit"
fi

# Gerund thường là trượt thể ("Adding X" thay vì "Add X"), nhưng "bring"/"ping"/"string" lại
# là mệnh lệnh kết thúc y hệt — nên cái này khuyên chứ không chặn.
if [[ "${first}" =~ ing$ ]] && [[ ! "${first}" =~ ^([Bb]ring|[Pp]ing|[Ss]tring|[Rr]ing|[Ss]ing)$ ]]; then
  echo "check-commit-msg: \"${first}\" đọc như gerund; thể mệnh lệnh thường ngắn và rõ hơn." >&2
fi
