#!/bin/bash
# Copy 1 bộ sticker Zalo đã cache sẵn dưới máy vào kho sticker của SlopShot.
#
# THƯỜNG THÌ DÙNG tools/fetch_zalo_pack.sh THAY CHO SCRIPT NÀY: nó tải sprite
# sheet thẳng từ CDN cho bất kỳ eid nào, không cần cài bộ đó trong Zalo và không
# phải cuộn cho client cache về. Script này chỉ còn đáng dùng khi bạn KHÔNG biết
# dải eid của bộ nhưng bộ đó đã nằm sẵn trong Zalo của bạn.
#
#   ./tools/import_zalo_pack.sh 11786 "Ami Bụng Bự"
#   ./tools/import_zalo_pack.sh 11786,11788,11901,11903,11906 "Ami Bụng Bự"
#
# Nhận nhiều packId ngăn cách bằng dấu phẩy → gộp hết vào MỘT bộ. Cần thế vì
# Zalo hay xé một nhân vật ra nhiều bộ nhỏ trên store (Ami Bụng Bự là 5 bộ),
# trong khi ở đây ta chỉ muốn một thư mục "Ami Bụng Bự" duy nhất. Tên file là
# stickerId (eid) — vốn là số toàn cục nên gộp cũng không đụng tên nhau.
#
# Zalo cache sticker ở  ~/Library/Application Support/ZaloData/media/<uid>/sticker/
# theo cấu trúc <packId>/<stickerId>/<hash>.(png|webp), và CHỈ cache những
# sticker bạn đã thực sự nhìn thấy. Muốn đủ cả bộ thì mở Zalo, vào bộ đó, kéo
# hết một lượt cho client tải về, rồi chạy lại script này.
#
# Mỗi sticker Zalo động có 2 file: 1 ảnh vuông TĨNH và 1 SPRITE SHEET ngang (các
# frame animation xếp cạnh nhau, ví dụ 3250×130 = 25 frame 130×130). Script lấy
# sprite sheet nếu có — app tự cắt ra frame và chạy animation, copy/xuất ra GIF
# vẫn nhúc nhích. Không có sprite mới rơi về bản vuông tĩnh.
#
# API công khai của Zalo (tools/fetch_zalo_pack.sh) CHỈ trả ảnh tĩnh, nên đây là
# đường duy nhất lấy được bản động.
set -euo pipefail

PACK_IDS="${1:-}"
PACK_NAME="${2:-Zalo $PACK_IDS}"
[ -n "$PACK_IDS" ] || { echo "Dùng: $0 <packId[,packId…]> [tên bộ]"; exit 1; }

SRC_ROOT="$HOME/Library/Application Support/ZaloData/media"
DEST="$HOME/Library/Application Support/SlopShot/Stickers/$PACK_NAME"

mkdir -p "$DEST"
n=0; anim=0; missing=""
for pack_id in ${PACK_IDS//,/ }; do
    # uid nằm giữa đường dẫn, dò ra thay vì bắt gõ tay.
    pack_dir=$(find "$SRC_ROOT" -maxdepth 3 -type d -name "$pack_id" -path "*/sticker/*" 2>/dev/null | head -1)
    [ -n "$pack_dir" ] || { missing="$missing $pack_id"; continue; }

    for sticker_dir in "$pack_dir"/*/; do
        [ -d "$sticker_dir" ] || continue
        sprite=""; square=""; any=""
        for f in "$sticker_dir"*; do
            [ -f "$f" ] || continue
            w=$(sips -g pixelWidth  "$f" 2>/dev/null | tail -1 | awk '{print $2}')
            h=$(sips -g pixelHeight "$f" 2>/dev/null | tail -1 | awk '{print $2}')
            [ -n "$w" ] && [ -n "$h" ] && [ "$h" -gt 0 ] || continue
            [ -z "$any" ] && any="$f"
            # Sprite sheet: ngang chia hết cho cao và ≥3 lần (ngưỡng y hệt trong
            # app, để không nhận nhầm ảnh banner ngang thành animation).
            if [ "$w" -ge $((h * 3)) ] && [ $((w % h)) -eq 0 ]; then sprite="$f"
            elif [ "$w" = "$h" ]; then square="$f"; fi
        done
        best="${sprite:-${square:-$any}}"
        [ -n "$best" ] || continue
        eid=$(basename "$sticker_dir")
        # Bản động thay luôn bản tĩnh cùng eid đã nằm sẵn trong bộ (đuôi có thể khác).
        [ -n "$sprite" ] && rm -f "$DEST/$eid".*
        cp "$best" "$DEST/$eid.${best##*.}"
        n=$((n + 1))
        [ -n "$sprite" ] && anim=$((anim + 1))
    done
done

echo "✅ $n sticker → $DEST  (trong đó $anim con động)"
[ -z "$missing" ] || echo "⚠️  chưa có trong cache Zalo:$missing"
[ "$n" -gt 0 ] || echo "   (chưa cache con nào — mở Zalo xem qua bộ đó rồi chạy lại)"
[ "$anim" -eq "$n" ] || echo "   Thiếu bản động? Mở Zalo, cuộn hết bộ cho client tải về rồi chạy lại."
echo "   Sprite sheet gốc chỉ 130px. Muốn nét hơn: xcrun swift tools/upscale_stickers.swift \"$PACK_NAME\""
echo "   Rồi nén lại:   xcrun swift tools/webp_stickers.swift \"$PACK_NAME\"  (nhỏ ~7 lần)"
