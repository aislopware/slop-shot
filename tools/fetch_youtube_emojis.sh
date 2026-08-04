#!/bin/bash
# Tải bộ emoji live chat của YouTube (:hand-pink-waving:, :face-blue-smiling:,
# :yt: …) về kho sticker của SlopShot.
#
#   ./tools/fetch_youtube_emojis.sh ["Tên bộ"]
#
# YouTube KHÔNG có API công khai liệt kê bộ emoji này: catalog nằm trong picker
# của ô nhập chat nên chỉ hiện ra khi đã đăng nhập. Danh sách (nhãn + URL ảnh)
# lấy từ gist cộng đồng bên dưới — nếu gist chết thì thay GIST_URL, phần còn lại
# vẫn chạy nguyên.
#
# Ảnh nằm trên CDN ảnh của Google (yt3.ggpht.com), đuôi "=w24-h24-c-k-nd" là
# tham số resize. Xin tới 192 là mức cao nhất còn trả ảnh (240 trả về HTML lỗi),
# và ở mức này ảnh là PNG RGBA — nền trong suốt, dán lên ảnh chụp không lòi nền.
set -euo pipefail

GIST_URL="https://gist.githubusercontent.com/brainwo/8ea346ff73ace01aa5b7dd23014246e6/raw"
# "=s0" trên CDN ảnh của Google nghĩa là "trả bản GỐC". Với bộ emoji này bản gốc
# đúng 48×48 — đó là toàn bộ những gì YouTube phát hành.
#
# Đừng xin "=w192-h192-c-k-nd" cho nét: tham số -c ép CDN phóng lên cho đầy khung,
# ra file 192px nhưng chi tiết vẫn là 48px. Lấy về rồi phóng tiếp là nội suy hai
# lần, nát hơn hẳn so với phóng một lần từ bản gốc.
SIZE="s0"
NAME="${1:-YouTube Live Chat}"
DEST="$HOME/Library/Application Support/SlopShot/Stickers/$NAME"

mkdir -p "$DEST"
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
curl -sL --compressed "$GIST_URL" -o "$tmp"
[ -s "$tmp" ] || { echo "Không tải được danh sách emoji."; exit 1; }

# CSV: "Emoji label","Src" — bỏ dòng tiêu đề, tách nhãn (bỏ dấu :) và URL gốc.
python3 - "$tmp" "$DEST" "$SIZE" <<'PY'
import csv, re, sys, subprocess, os
from concurrent.futures import ThreadPoolExecutor
src, dest, size = sys.argv[1], sys.argv[2], sys.argv[3]

jobs = []
for row in list(csv.reader(open(src, encoding="utf-8")))[1:]:
    if len(row) < 2:
        continue
    label, url = row[0].strip().strip(":"), row[1].strip()
    if not label or "ggpht.com" not in url:
        continue
    base = re.sub(r"=.*$", "", url)                       # bỏ tham số resize cũ
    out = os.path.join(dest, re.sub(r"[^A-Za-z0-9._-]", "_", label) + ".png")
    jobs.append((f"{base}={size}", out))

def get(job):
    url, out = job
    if os.path.exists(out) and os.path.getsize(out) >= 500:
        return True                                       # đã có rồi, chạy lại không tải lại
    # CDN thỉnh thoảng rớt lẻ tẻ khi tải dồn → thử lại vài lần.
    for _ in range(3):
        subprocess.run(["curl", "-s", "--compressed", "-o", out, url])
        # tham số resize sai thì CDN trả HTML, không phải PNG → loại file rác.
        if os.path.exists(out) and os.path.getsize(out) >= 500 \
           and open(out, "rb").read(4) == b"\x89PNG":
            return True
    if os.path.exists(out):
        os.remove(out)
    return False

with ThreadPoolExecutor(max_workers=8) as pool:
    ok = sum(pool.map(get, jobs))
print(f"✅ {ok}/{len(jobs)} emoji → {dest}")
PY
