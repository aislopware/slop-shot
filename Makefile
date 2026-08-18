# Giống "scripts" trong package.json. Chạy: make run
APP     = SlopShot
CONFIG  = Debug
BUILD   = build
# Tự dò chữ ký: nếu Keychain có cert "SlopShot Dev" thì dùng (quyền macOS không
# bị reset mỗi lần build); không có (máy người khác clone về) thì tự rơi về ad-hoc
# "-" để build/cài được ngay, khỏi tạo cert. Ghi đè được: make install SIGN_ID="..."
SIGN_ID := $(shell security find-identity -v -p codesigning 2>/dev/null | grep -q "SlopShot Dev" && echo "SlopShot Dev" || echo "-")
APP_PATH = $(BUILD)/Build/Products/$(CONFIG)/$(APP).app

.PHONY: gen build run clean install

gen:                 # sinh SlopShot.xcodeproj từ project.yml
	xcodegen generate

build: gen           # build app rồi ký lại bằng cert cố định
	xcodebuild -project $(APP).xcodeproj -scheme $(APP) \
		-configuration $(CONFIG) -derivedDataPath $(BUILD) build
	codesign --force --deep --sign "$(SIGN_ID)" "$(APP_PATH)"
	@echo "Đã ký bằng: $(SIGN_ID)"

run: build           # build xong mở app luôn
	open "$(APP_PATH)"

# Cài "production" vào /Applications: build bản Release (tối ưu) rồi copy + ký.
# CONFIG=Release chỉ áp riêng cho lệnh này (target-specific), nên APP_PATH trỏ đúng Release.
install: CONFIG = Release
install: build
	@echo "→ Đóng SlopShot đang chạy (nếu có)…"
	-osascript -e 'quit app "SlopShot"' >/dev/null 2>&1 || true
	rm -rf "/Applications/$(APP).app"
	cp -R "$(APP_PATH)" "/Applications/$(APP).app"
	codesign --force --deep --sign "$(SIGN_ID)" "/Applications/$(APP).app"
	@echo "✅ Đã cài: /Applications/$(APP).app  (mở: open -a $(APP))"
	@echo "   Lần đầu nhớ cấp lại Screen Recording + Accessibility cho bản trong /Applications."

clean:
	rm -rf $(BUILD) $(APP).xcodeproj dist .work

# ── Hook ───────────────────────────────────────────────────────────────────
# Cài hook git. Cái quan trọng nhất là commit-msg: tiêu đề commit là ĐẦU VÀO của
# release (version và CHANGELOG đều sinh ra từ nó), nên nó bị siết ngay lúc viết.
.PHONY: hooks

hooks:
	@command -v prek >/dev/null 2>&1 && prek install \
		|| { command -v pre-commit >/dev/null 2>&1 && pre-commit install --install-hooks \
		|| { echo "❌ Chưa có prek. Cài: brew install prek"; exit 1; }; }

# ── Release ────────────────────────────────────────────────────────────────
# Bốn lệnh trên là "chạy ở máy mình"; mấy lệnh dưới là "ship cho người khác":
# ký bằng Developer ID của WEEBUILD, notarize qua Apple, ra DMG cho Homebrew.
# Toàn bộ quy trình: docs/release-pipeline.md
.PHONY: release release-preview package version

# Cắt release: quyết version từ commit log + render CHANGELOG + ghi ba chỗ + commit + tag.
# KHÔNG push. Không truyền VERSION thì git-cliff tự tính từ các commit sau tag gần nhất.
release:
	bash scripts/cut-release.sh $(VERSION)

# Xem trước version sắp ra và nội dung release, không ghi gì cả.
release-preview:
	bash scripts/cut-release.sh --dry-run $(VERSION)

# Chỉ ghi version, không commit/tag. Dùng khi muốn sửa thêm gì đó trước khi cắt.
version:
	bash scripts/bump-version.sh $(VERSION)

# Build + ký + notarize + đóng DMG vào dist/. CI chạy đúng script này.
# Cần Developer ID trong keychain và thông tin notarytool (xem header của script).
# Thử khô, chỉ ký không nộp Apple: make package VERSION=0.1.0 SKIP_NOTARIZE=1
package:
	SLOPSHOT_VERSION=$(VERSION) SLOPSHOT_SKIP_NOTARIZE=$(or $(SKIP_NOTARIZE),0) \
		bash scripts/package-release.sh
