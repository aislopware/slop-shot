#!/bin/bash
# Copy 1 bộ sticker Zalo đã cache sẵn dưới máy vào kho sticker của SlopShot.
#
#   ./tools/import_zalo_pack.sh 11212 "Ami bụng bự"
#
# Zalo cache sticker ở  ~/Library/Application Support/ZaloData/media/<uid>/sticker/
# theo cấu trúc <packId>/<stickerId>/<hash>.(png|webp), và CHỈ cache những
# sticker bạn đã thực sự nhìn thấy. Muốn đủ cả bộ thì mở Zalo, vào bộ đó, kéo
# hết một lượt cho client tải về, rồi chạy lại script này.
#
# Mỗi sticker Zalo thường có 2 file: 1 ảnh vuông tĩnh và 1 sprite sheet ngang
# (các frame animation xếp cạnh nhau). Script ưu tiên bản vuông; không có mới
# lấy sprite sheet (app tự cắt frame đầu khi hiển thị).
set -euo pipefail

PACK_ID="${1:-}"
PACK_NAME="${2:-Zalo $PACK_ID}"
[ -n "$PACK_ID" ] || { echo "Dùng: $0 <packId> [tên bộ]"; exit 1; }

SRC_ROOT="$HOME/Library/Application Support/ZaloData/media"
DEST="$HOME/Library/Application Support/SlopShot/Stickers/$PACK_NAME"

# uid nằm giữa đường dẫn, dò ra thay vì bắt gõ tay.
PACK_DIR=$(find "$SRC_ROOT" -maxdepth 3 -type d -name "$PACK_ID" -path "*/sticker/*" 2>/dev/null | head -1)
[ -n "$PACK_DIR" ] || { echo "Không thấy bộ $PACK_ID trong cache Zalo."; exit 1; }

mkdir -p "$DEST"
n=0
for sticker_dir in "$PACK_DIR"/*/; do
    [ -d "$sticker_dir" ] || continue
    best=""
    for f in "$sticker_dir"*; do
        [ -f "$f" ] || continue
        w=$(sips -g pixelWidth  "$f" 2>/dev/null | tail -1 | awk '{print $2}')
        h=$(sips -g pixelHeight "$f" 2>/dev/null | tail -1 | awk '{print $2}')
        [ -n "$w" ] && [ -n "$h" ] || continue
        [ -z "$best" ] && best="$f"
        if [ "$w" = "$h" ]; then best="$f"; break; fi   # bản vuông là bản muốn
    done
    [ -n "$best" ] || continue
    cp "$best" "$DEST/$(basename "$sticker_dir").${best##*.}"
    n=$((n + 1))
done

echo "✅ $n sticker → $DEST"
[ "$n" -gt 0 ] || echo "   (bộ này chưa được cache — mở Zalo xem qua bộ đó rồi chạy lại)"
