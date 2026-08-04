#!/bin/bash
# Tải 1 bộ sticker Zalo về kho sticker của SlopShot, qua API PUBLIC của Sticker
# Store (stickers.zaloapp.com) — không cần đăng nhập, không đụng tới token Zalo.
#
#   ./tools/fetch_zalo_pack.sh --cid 00c28006bc43551d0c52 "Ami Bụng Bự 2"
#   ./tools/fetch_zalo_pack.sh --range 43516 43531        "Ami Bụng Bự"
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
# Ảnh lấy ở size=240, là mức cao nhất API trả (xin lớn hơn vẫn ra 240).
set -euo pipefail

API="https://zalo-api.zadn.vn/api/emoticon/sticker/webpc"
STORE="https://stickers.zaloapp.com"
DEST_ROOT="$HOME/Library/Application Support/SlopShot/Stickers"

MODE="${1:-}"; VALUE="${2:-}"
[ -n "$MODE" ] && [ -n "$VALUE" ] || {
    echo "Dùng: $0 --cid <cid> [tên bộ]"
    echo "      $0 --range <eid đầu> <eid cuối> [tên bộ]"; exit 1; }

# Tải 1 eid ra file. Trả 1 nếu không có sticker nào ở eid đó (biên của bộ).
fetch_eid() {   # $1=eid  $2=file đích
    local code size
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
export -f fetch_eid; export API
echo "$EIDS" | xargs -P 8 -I{} bash -c 'fetch_eid {} "$1/{}.png" || true' _ "$DEST"
echo "✅ $(ls "$DEST" | wc -l | tr -d ' ') sticker → $DEST"
