#!/bin/bash
# Tải 1 bộ sticker Zalo về kho sticker của SlopShot, qua API PUBLIC của Sticker
# Store (stickers.zaloapp.com) — không cần đăng nhập, không đụng tới token Zalo.
#
#   ./tools/fetch_zalo_pack.sh --cid 00c28006bc43551d0c52 "Ami Bụng Bự 2"
#   ./tools/fetch_zalo_pack.sh --range 43516 43531        "Ami Bụng Bự"
#
# Sticker ĐỘNG lấy được luôn, không cần cài bộ đó trong Zalo: endpoint
# /api/emoticon/sprite trả về SPRITE SHEET ngang (các frame xếp cạnh nhau) cho
# bất kỳ eid nào. Script thử sprite trước, con nào vốn tĩnh thì rơi về ảnh 240px.
#
# Hai cách xác định bộ:
#   --cid    lấy từ URL trang store (stickers.zaloapp.com/oa/detail?cid=…);
#            endpoint /cate-stickers trả thẳng danh sách sticker của bộ. Đây là
#            cách CHÍNH XÁC, dùng được khi bộ đó còn trang store.
#   --range  tải thẳng một dải eid. Dùng cho bộ cũ không tra ra cid: quét một
#            vùng eid rồi nhìn ảnh để khoanh dải (xem ghi chú bên dưới).
#
# LƯU Ý về eid: eid là số thứ tự TOÀN CỤC, không reset theo bộ, và các bộ nằm
# liền nhau trong không gian eid. 404 chỉ là khoảng trống ngẫu nhiên, KHÔNG phải
# biên của bộ — đừng dò biên bằng cách bò tới khi gặp 404, sẽ nuốt luôn hàng
# nghìn sticker của các bộ khác. Muốn khoanh dải thì tải cả vùng rồi xem ảnh.
#
# Kích thước: ảnh tĩnh lấy size=240 (trần của API, xin lớn hơn vẫn ra 240).
# Sprite sheet CHỈ có ở size=130 — mọi size khác endpoint /sprite lại trả về ảnh
# tĩnh 240. Frame 130px là bản gốc cao nhất Zalo phát hành cho animation; muốn
# nét hơn thì phóng bằng tools/upscale_stickers.swift (nó giữ nguyên animation).
set -euo pipefail

API="https://zalo-api.zadn.vn/api/emoticon/sticker/webpc"
SPRITE="https://zalo-api.zadn.vn/api/emoticon/sprite"
STORE="https://stickers.zaloapp.com"
DEST_ROOT="$HOME/Library/Application Support/SlopShot/Stickers"

MODE="${1:-}"; VALUE="${2:-}"
[ -n "$MODE" ] && [ -n "$VALUE" ] || {
    echo "Dùng: $0 --cid <cid> [tên bộ]"
    echo "      $0 --range <eid đầu> <eid cuối> [tên bộ]"; exit 1; }

# Tải 1 eid ra file. Trả 1 nếu không có sticker nào ở eid đó (biên của bộ).
#
# Thử /sprite trước. Con nào có animation thì trả ảnh NGANG (frame xếp cạnh
# nhau); con tĩnh thì chính endpoint đó trả ảnh vuông 130 — lúc ấy bỏ đi, lấy
# lại bản 240 cho nét.
fetch_eid() {   # $1=eid  $2=file đích
    local code size w h
    code=$(curl -s --compressed -o "$2" -w "%{http_code}" "$SPRITE?eid=$1&size=130&version=5")
    size=$(stat -f%z "$2" 2>/dev/null || echo 0)
    if [ "$code" = "200" ] && [ "$size" -ge 500 ]; then
        w=$(sips -g pixelWidth  "$2" 2>/dev/null | tail -1 | awk '{print $2}')
        h=$(sips -g pixelHeight "$2" 2>/dev/null | tail -1 | awk '{print $2}')
        # Ngưỡng sprite sheet y hệt trong app: ngang chia hết cho cao và ≥3 lần.
        if [ -n "$w" ] && [ -n "$h" ] && [ "$h" -gt 0 ] \
           && [ "$w" -ge $((h * 3)) ] && [ $((w % h)) -eq 0 ]; then return 0; fi
    fi
    code=$(curl -s --compressed -o "$2" -w "%{http_code}" "$API?eid=$1&size=240&version=5")
    size=$(stat -f%z "$2" 2>/dev/null || echo 0)
    if [ "$code" != "200" ] || [ "$size" -lt 500 ]; then rm -f "$2"; return 1; fi
    return 0
}

case "$MODE" in
  --cid)
    NAME="${3:-Zalo $VALUE}"
    DEST="$DEST_ROOT/$NAME"; mkdir -p "$DEST"
    EIDS=$(curl -s --compressed "$STORE/cate-stickers?cid=$VALUE" \
           | python3 -c "import json,sys,re
for s in json.load(sys.stdin).get('value',[]):
    m=re.search(r'eid=(\d+)', s.get('url',''))
    if m: print(m.group(1))")
    [ -n "$EIDS" ] || { echo "cid $VALUE không trả về sticker nào."; exit 1; }
    ;;
  --range)
    HI="${3:-}"; [ -n "$HI" ] || { echo "--range cần cả eid đầu và eid cuối."; exit 1; }
    NAME="${4:-Zalo eid ${VALUE}-${HI}}"
    DEST="$DEST_ROOT/$NAME"; mkdir -p "$DEST"
    EIDS=$(seq "$VALUE" "$HI")
    ;;
  *) echo "Mode phải là --cid hoặc --range"; exit 1 ;;
esac

# Tải song song cho nhanh (mỗi eid 1 request độc lập).
export -f fetch_eid; export API SPRITE
echo "$EIDS" | xargs -P 8 -I{} bash -c 'fetch_eid {} "$1/{}.png" || true' _ "$DEST"

# Đếm xem bao nhiêu con ra được bản động, để biết ngay có gì hụt không.
anim=0
for f in "$DEST"/*.png; do
    [ -f "$f" ] || continue
    w=$(sips -g pixelWidth  "$f" 2>/dev/null | tail -1 | awk '{print $2}')
    h=$(sips -g pixelHeight "$f" 2>/dev/null | tail -1 | awk '{print $2}')
    [ -n "$w" ] && [ -n "$h" ] && [ "$h" -gt 0 ] && [ "$w" -ge $((h * 3)) ] && anim=$((anim + 1))
done
echo "✅ $(ls "$DEST" | wc -l | tr -d ' ') sticker → $DEST  (động: $anim)"
echo "   Phóng cho nét: xcrun swift tools/upscale_stickers.swift \"$(basename "$DEST")\""
echo "   Rồi nén lại:   xcrun swift tools/webp_stickers.swift \"$(basename "$DEST")\"  (nhỏ ~7 lần)"
