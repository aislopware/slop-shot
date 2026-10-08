# SlopShot macOS: behavioral spec (parity reference for the Linux port)

Source of truth: the Swift app in `Sources/` (v0.9.1). Every fact cites `File.swift:line`.
All paths are relative to `Sources/`. "pt" = logical point; "px" = physical pixel
(pt × display scale). Coordinates are top-left origin unless stated otherwise.

Common visual tokens (used below by name):

| Token | Value | Ref |
|---|---|---|
| Radius.small / control / bar / panel | 6 / 8 / 12 / 14 | Theme.swift:13-19 |
| Edge stroke | white 14%, 1pt | Theme.swift:23 |
| Hover fill / press fill | white 10% / white 18% | Theme.swift:25-26 |
| Editor surface / raised surface | gray 0.11 / gray 0.13 | Theme.swift:28-29 |
| `quick` animation | ease-out 0.14 s | Theme.swift:31 |
| HUD background | blurred "hudWindow" material + black 28% + edge stroke | Theme.swift:60-71 |
| HUD button | hover: white 10% fill; press: white 18% + scale 0.94 | Theme.swift:95-106 |
| Overlay dim | rgb(0.02,0.02,0.04) @ 0.42 (0.34 if no frozen image) | OverlayChrome.swift:12, RegionSelectionController.swift:189-192 |
| Chip fill / chip edge | rgb(0.08,0.09,0.11) @ 0.92 / white 14% | OverlayChrome.swift:13-14 |
| Snap guide color | rgb(0.24,0.86,0.79) (teal) | OverlayChrome.swift:15 |
| Accent | system accent color (`controlAccentColor`) | many |

All overlay/HUD windows force a dark appearance regardless of system theme
(RegionSelectionController.swift:1049, ThumbnailController.swift:268, ColorPickerController.swift:274, EditorWindowController.swift:35).

---

## 1. App lifecycle

### 1.1 Process model
- Background agent app: `LSUIElement = true` -> no Dock icon, not in Cmd-Tab (Support/Info.plist; project.yml).
- No main window. The only scene is a menu-bar item (SlopShotApp.swift:116-130).
- When the annotation editor opens, the app temporarily becomes a regular app (Dock icon appears) and returns to accessory when the editor closes (EditorWindowController.swift:48, 67).
- Single instance: no explicit code; macOS LaunchServices guarantees it. Launching the app again while running triggers "reopen", which opens the **Settings** window (SlopShotApp.swift:71-74). This is the only way back to Settings when the menu-bar icon is hidden (AppSettings.swift:148-150, SettingsWindowController.swift:72-76).
- Launch sequence (SlopShotApp.swift:11-25): build hidden app menu -> register all global hotkeys -> install SIGTERM relauncher (UpdateRelauncher.swift; Homebrew upgrade relaunch, macOS-only) -> if Screen Recording is not granted, silently request it (SystemPermissions.swift:106-111) -> listen for hotkey changes and re-register.
- Warm-up: if permission already granted, the shareable-content list is pre-fetched so the first hotkey is fast; it is invalidated when displays change (ScreenCapturer.swift:47-58).
- First run: no onboarding window, no welcome. Only the OS permission prompt (from the request above). Nothing else.

### 1.2 Hidden application menu (keyboard only; app has no visible menu bar)
Exists so standard editing keys work in text fields (SlopShotApp.swift:34-66):
- App menu: "Close Window" **Cmd+Q** -> closes the key window (via normal close path), does **not** quit the app (SlopShotApp.swift:41-47, 76-81).
- Edit menu: Undo Cmd+Z, Redo Cmd+Shift+Z, separator, Cut Cmd+X, Copy Cmd+C, Paste Cmd+V, Select All Cmd+A (SlopShotApp.swift:55-63).

### 1.3 Menu-bar item
- Title "SlopShot", template image asset `MenuBarIcon` (an "S" glyph; 16/32/48 px PNGs in Assets.xcassets/MenuBarIcon.imageset), auto-tinted black/white by the OS (SlopShotApp.swift:119, 124).
- Visibility bound to setting `showMenuBarIcon` (default on) (SlopShotApp.swift:124-127).
- Click behavior: standard `MenuBarExtra` menu. Left click opens the dropdown menu. There is no separate right-click behavior and no click-to-capture.

### 1.4 Menu contents, in order (ContentView.swift:12-95)
Shortcut hints are the **current** configured global hotkeys, right-aligned, and update live after rebinding (ContentView.swift:8-10, 98-107; Hotkey.swift:91-117).

Section 1 (capture):
1. icon `crop`, **"Capture Area"**, hint = Capture Area hotkey (default Cmd+Shift+1)
2. icon `display`, **"Capture Fullscreen"**, hint default Cmd+Shift+2
3. icon `rectangle.and.arrow.up.right.and.arrow.down.left`, **"Capture Scrolling Area"**, default Cmd+Shift+3
4. icon `text.viewfinder`, **"Capture Text (OCR)"**, default Cmd+Shift+4
5. icon `eyedropper`, **"Pick Color"**, default Cmd+Shift+5
6. If recording: icon `stop.circle`, **"Stop Recording"** (no hint). Else icon `video`, **"Record Area"**, default Cmd+Shift+6 (ContentView.swift:40-49)

Divider.

Section 2 (only if a capture/recording happened this session, i.e. `lastSavedURL != nil`) (ContentView.swift:55-67):
- Only if last capture was an image: icon `pencil.tip.crop.circle`, **"Edit Last Screenshot"** -> opens editor on last image.
- icon `arrow.up.forward.app`, **"Open Last File"** -> opens the file with the system default app. The "last file" is the temp PNG (or temp .mov), or the saved file if the user clicked Save afterwards (ScreenCapturer.swift:513, 528).
- Divider.

Section 3:
- icon `clock.arrow.circlepath`, **"Capture History…"** (no shortcut)
- icon `gearshape`, **"Settings…"**, Cmd+, (ContentView.swift:74-77)

Divider.
- icon `info.circle`, **"About SlopShot"** -> standard About panel (app icon, name, version), app activated first (ContentView.swift:84-89).
- icon `power`, **"Quit SlopShot"**, Cmd+Q, destructive role (ContentView.swift:91-94).

Note: `lastStatus` strings (e.g. "Selection cancelled.") are set throughout but are **never displayed** anywhere in the UI (ScreenCapturer.swift:12; not referenced in ContentView.swift).

### 1.5 Global session rules (apply to every capture action)
- `beginSession` gate (ScreenCapturer.swift:87-92):
  1. If another capture session is running (`busy`), the new hotkey/menu action is **ignored**.
  2. If Screen Recording permission is missing: alert (see 1.6), action aborted.
  3. If the annotation editor window is open with unsaved annotations: modal alert **"Discard your annotations?"** / "The editor is still open with drawings you haven't copied or saved. Starting a new capture closes it." Buttons **"Discard & Capture"** (default) / **"Keep Editing"** (aborts the capture) (ScreenCapturer.swift:68-77).
  4. Then closes editor, video viewer, video editor, OCR window without saving (ScreenCapturer.swift:78-81).
  5. Hides the current preview card immediately (no animation) (ScreenCapturer.swift:90).
- Exception: Record Area while recording = stop recording (ScreenCapturer.swift:339).

### 1.6 Errors and permission alerts
- Missing Screen Recording (ScreenCapturer.swift:104-133): warning alert, title **"SlopShot needs Screen Recording permission"**, body:
  "macOS is blocking screen capture, so every screenshot comes out empty.\n\nOpen System Settings › Privacy & Security › Screen & System Audio Recording, turn SlopShot on, then quit and reopen SlopShot.\n\nIf SlopShot is already listed and enabled, remove it with the “–” button and add it again — reinstalling the app invalidates the old entry." Buttons **"Open System Settings"** / **"Later"**.
- Any capture error: warning alert **"Capture failed"**, body = error text, button **"OK"** (ScreenCapturer.swift:136-145).
- No system notifications are ever posted.

---

## 2. Global hotkeys

Actions, order (= menu order = Settings order), titles and defaults (Hotkey.swift:52-80):

| Action | Title | Default |
|---|---|---|
| captureArea | Capture Area | Cmd+Shift+1 |
| captureFullscreen | Capture Fullscreen | Cmd+Shift+2 |
| captureScrolling | Capture Scrolling Area | Cmd+Shift+3 |
| captureText | Capture Text (OCR) | Cmd+Shift+4 |
| pickColor | Pick Color | Cmd+Shift+5 |
| recordArea | Record Area | Cmd+Shift+6 (also stops an active recording) |

- Registered system-wide, fire even when the app is in background (HotKeyManager.swift:49-68). Registration failure (taken by another app) is only logged, no UI (HotKeyManager.swift:64-67).
- Persistence: dictionary action -> {keyCode, modifiers} JSON in defaults key `hotkeys`; missing entry = default (AppSettings.swift:186-200, 241-246).
- Changing or resetting any binding unregisters all and re-registers all (SlopShotApp.swift:21-24, 84-94).
- Display format: modifiers in order Ctrl Opt Shift Cmd (`⌃⌥⇧⌘`) then the key label, e.g. `⇧⌘1`; arrows `←→↓↑`, Return `↩`, Tab `⇥`, Space `Space`, Delete `⌫`, Esc `⎋`, F1-F12; unknown keys `key<n>` (Hotkey.swift:25-45).
- Recording a new binding: see Settings > Shortcuts (section 7.5): requires at least one modifier; bare key beeps; Esc cancels.
- macOS-specific: Cmd+Shift+3/4/5 conflict with system screenshot keys; Settings offers to disable the system ones (SystemScreenshotShortcuts.swift). Linux equivalent: detect/offer to release conflicting desktop bindings.

---

## 3. Capture Area overlay

Entry: `captureRegion` (ScreenCapturer.swift:174-203) -> `selectArea` with tool "Capture Area", tool icon `camera.viewfinder`, confirm title "Capture", confirm icon `camera.fill`, `editInline = settings.editAfterSelect` (default **true**) (ScreenCapturer.swift:753-765).

### 3.1 Freeze
- On hotkey, **every** display is captured in parallel (cursor excluded, SlopShot's own windows excluded) *before* any overlay is shown (ScreenCapturer.swift:750-777, 813-822).
- Each display gets its own overlay window showing the frozen image as backdrop, pixel-exact, shown instantly (no animation) so the user sees no "window appearing" (RegionSelectionController.swift:1031-1035, 1052, 1107-1122).
- If freezing failed for a display (fallback), overlay is transparent over the live screen, fades in over 0.10 s, dim is lighter (0.34), no loupe, no pixel snapping; capture happens after the overlay closes with a 150 ms wait (RegionSelectionController.swift:1109-1118; ScreenCapturer.swift:194-197).
- Overlay window: borderless, above everything including the menu bar (screen-saver level), does not activate SlopShot / does not steal app activation (so other apps' lightboxes don't close) (RegionSelectionController.swift:17-26, 1043-1058, 1006-1012).
- The SwiftUI chrome (dim, hint, badges) fades in 0.12 s ease-out on top of the instantly-shown frozen image (RegionSelectionController.swift:134-135).

### 3.2 What is drawn (paint order) (RegionSelectionController.swift:110-207)
1. **Dim** over the whole screen with an even-odd hole: the hole is the current selection, or (if none) the hovered snap target (RegionSelectionController.swift:186-192).
2. If a selection exists: snap guides, selection outline, handles. Else if a hover target exists: accent fill 12% + accent 2 pt stroke on rect inset by -1. Else, if pointer is on this display: **crosshair** (RegionSelectionController.swift:194-206).
3. Hint panel (bottom), tool badge (top), size chip, action buttons (adjust step only), loupe.

Crosshair: two full-screen 1 pt lines (vertical and horizontal) through cursor (+0.5 pt offset), white 20% (RegionSelectionController.swift:293-300). Only when nothing is hovered/selected.

Selection outline (RegionSelectionController.swift:211-216): black 45% 1 pt stroke on rect inset -1.5, then white 1 pt stroke on rect inset -0.5 (i.e. drawn just outside the captured area).

Handles (RegionSelectionController.swift:220-260), drawn **inside** the rect, only if rect > 6×6:
- Corner L-brackets: thickness 3, arm length `min(22, max(8, min(w,h)/3))`.
- Mid-edge bars: length `min(18, max(10, min(w,h)/4))`, thickness 3; top/bottom bars only if `w > 2*arm + bar + 12`, left/right only if `h > 2*arm + bar + 12`.
- Each bar: black 40% halo (inset -1, corner 2) then white fill (corner 1.5).

Snap guides (RegionSelectionController.swift:264-289): for each snapped edge, a 1 pt teal line extending 130 pt **outward** from the rect on both ends (not across the selection), gradient 85% -> 0% opacity.

### 3.3 Hint panel and tool badge
- Hint panel: bottom-centered, 96 pt above the bottom edge; title 15 pt semibold white, subtitle 12 pt white 62%, padding 22 h / 15 v, HUD background radius 14 (OverlayChrome.swift:199-216; RegionSelectionController.swift:117-121).
  - Title, snap ON: **"Drag to capture · click a highlighted area · ⌥ free · esc"**; snap OFF: **"Drag to capture · esc to cancel"** (RegionSelectionController.swift:1024-1026).
  - Subtitle, snap ON: **"click a highlighted area  ·  scroll to grow / shrink it  ·  ⌥ free select  ·  ⇧ square  ·  hold space to move  ·  F full screen  ·  esc to cancel"**; snap OFF: **"⇧ square  ·  hold space to move  ·  F full screen  ·  esc to cancel"** (two spaces around each `·`) (RegionSelectionController.swift:173-177).
  - Hidden (opacity -> 0, 0.14 s) once the user presses the mouse (`interacted`) or presses F; reset when following a desktop switch (RegionSelectionController.swift:120-121, 681, 799, 997).
- Tool badge: label with SF icon + title (e.g. camera icon + "Capture Area"), 13 pt semibold white, padding 14 h / 7 v, HUD background radius 16, shadow black 40% r6, centered horizontally at y = 58 pt from top. Fades to 15% opacity (0.12 s) when the cursor is within ±140 pt horizontally and ±50 pt vertically of its center. Stays for the whole session (OverlayChrome.swift:223-244).

### 3.4 Loupe (area mode) (RegionSelectionController.swift:340-359; OverlayChrome.swift:85-97, 135-194)
- Shown only when a frozen image exists, the pointer is on this display, and (no selection yet OR currently dragging/resizing).
- Size 128×128 pt, zoom ×8 (source square = `round(128/8 × scale)` px, e.g. 32 px on 2×).
- Placement: top-left at cursor + (22, 22); if it would pass right edge − 12, place to the left (cursor.x − 22 − 128); if it would pass bottom − 44, place above; clamp to ≥ 12 from top/left.
- Content: nearest-neighbour pixels; pixel grid lines white 10% 0.5 pt drawn only if each source pixel is ≥ 3 pt wide; the pixel under the cursor outlined black 90% 1 pt (outer) + white 1 pt; background gray 0.08; corner radius 14; edge stroke; shadow black 55% r8.
- Chip under loupe, centered, 6 pt below: swatch (11-1=10 pt square, radius 2, white 50% border) + text **`#RRGGBB  X, Y`** (two spaces; uppercase hex in sRGB; X,Y = integer **pixel** coords = `Int(pt × scale)`), 11 pt semibold monospaced digits. If color unreadable: just `X, Y`.

### 3.5 Size label (chip) (RegionSelectionController.swift:306-334; OverlayChrome.swift:37-42, 103-129)
- Text: **`W × H`** in **pixels** (pt × scale, rounded), with spaces around `×`. Font 12 pt semibold monospaced digits, white, padding 8 h / 5 v, chip fill, radius 8, edge stroke.
- Hover (no selection): **`Window   W × H`** or **`Item   W × H`** (three spaces), accent-colored background (accent 95%).
- Position: left-aligned to rect.minX, 8 pt above rect top. If that's < 6 pt from top, 8 pt below rect bottom; if that overflows bottom − 6, inside at rect.minY + 8. X clamped to [6, screenWidth − chipWidth − 6].
- Hidden while the inline editor is active.

### 3.6 Snapping and hover (only if setting "Snap to window & item edges" is on, default on)
- Window list snapshot taken per display **before** the overlay shows: on-screen windows at layer 0, plus windows of regular (Dock-icon) apps at layers 1 up to below the screen-saver level (e.g. Telegram's media viewer at layer 101), not SlopShot's, alpha > 0.05, clipped to the display, ≥ 24×24 pt; front-to-back order (SnapEngine.swift:52-86).
- Pixel edge engine built off-main-thread from the frozen image (~20-40 ms); until ready, window geometry alone is used (RegionSelectionController.swift:1093-1104; SnapEngine.swift:134-240).
- **Hover** (no drag in progress, ⌥ not held) (RegionSelectionController.swift:549-596):
  - Window under cursor = top-most window containing it.
  - Edge pixels: max per-channel difference between neighbours ≥ a **fixed** threshold of 16 (not adaptive: text-heavy screens used to push an adaptive threshold up and lose real 1 px UI borders). Only straight runs ≥ 20 pt (gaps ≤ 2 px) become candidate lines (SnapEngine.swift:140-230).
  - Engine collects every "box" around the cursor inside that window (or whole screen), up to 20 candidate lines per side plus the window's own edges, box sides ≥ 16 pt (SnapEngine.swift:347-470):
    - Window edges are known boundaries: they always count as a side (even at the screen edge, where there are no pixels), and an inner line meeting one must reach it within 2 px.
    - Candidate lines: a line counts if it passes the cursor row/column, or covers ≥ half the span between the nearest perpendicular candidates (an image edge that blends into the backdrop right at the cursor). Of 3 adjacent lines (thick or double border), the longest is kept.
    - Rounded corners: at each corner, measure how far each side's line starts from the corner (cap 24 pt). Both gaps must be within the cap, and at most one corner may have gaps that disagree (|a − b| > max(3 px, max(a, b)/3)) — a real rounded corner has equal gaps. Coverage (≥ 0.80 per side, mean ≥ 0.86) is measured on the straight part only, which must be ≥ 1/4 of the side.
    - Large boxes (≥ 160 pt both ways) may miss one corner (a dark photo blending into a dark viewer backdrop) if the other three agree; then coverage needs min ≥ 0.50 and mean ≥ 0.78, and the score drops by 0.15.
    - Stacks rejected: a line spanning ≥ 90% of the box through its middle 40% (either axis), at least 60% as strong as the box's own border, means "several items" (two list rows), not one. Weaker inner lines (a grid inside a picture) are fine.
    - Crossing boxes (overlap, neither contains the other) can't both be real: keep the one with the higher score (mean coverage − 0.05 per disagreeing corner).
    - Remaining boxes sorted by area form a nested chain, smallest first. A box that only adds a thin strip on ≤ 2 sides (≤ max(6 px, 1/8) of the first box of that item), or ≤ 2 px all round, replaces the previous level instead of adding one; an even margin all round (a picture inside a chat bubble) is a separate level.
  - Levels = that chain, minus boxes within 4 pt of the window, plus the **Window** as the last level. Default = the smallest. No window -> just the chain (or nothing).
  - When the cursor falls into a gap with no box (a few pt between two list rows), the previous item stays highlighted while the cursor is within 6 pt of it, instead of flashing to the Window and back.
  - **Scroll wheel** steps through levels: physical scroll up = bigger, down = smaller (respecting natural scrolling). Trackpad deltas are accumulated to 24 per step; momentum is ignored. A level reached by scrolling stays selected while the cursor moves as long as it is still in the new chain; otherwise it falls back to the smallest (RegionSelectionController.swift:573-591).
  - Hover is primed at overlay open from the current mouse position, so a highlight appears without moving the mouse (RegionSelectionController.swift:1084-1091).
  - Leaving a display clears that display's hover (RegionSelectionController.swift:607-610).
- **Edge snapping while dragging/resizing** (RegionSelectionController.swift:564-599): radius 9 pt. Each of the 4 edges independently snaps first to the nearest window edge within 9 pt, else to the best pixel edge (coverage ≥ 0.55 along the selection's span, score = coverage − 0.25×distance/radius) (SnapEngine.swift:312-349). Snapped edges produce teal guides. Rects < 3 pt in either dimension are not snapped.
- **⌥ (Option)**: "free mode" - no hover highlight, no snapping, while held (checked on every mouse move/drag and on modifier change) (RegionSelectionController.swift:628, 633-643, 686).
- **⇧ (Shift)** while dragging: square selection (side = larger of |dx|,|dy|), no snapping (RegionSelectionController.swift:731-746).
- Selection is clipped to the display bounds (RegionSelectionController.swift:745).

### 3.7 Mouse flow (RegionSelectionController.swift:645-783)
- Mouse down: other displays' overlays reset (discard their selections) (RegionSelectionController.swift:953-956). Hint hides.
- Drag threshold 4 pt: below it, it's still a click (hover kept) (RegionSelectionController.swift:396, 716-724).
- Mouse up:
  - Dragged and rect ≥ 5×5 pt -> selection done (enter "adjust" or finish, see 3.8).
  - Else if a hover target exists -> select exactly that rect (click-to-capture window/item).
  - Else if this was a mis-click outside a selection in adjust mode -> restore the previous selection.
  - Else -> **cancel** the whole capture (click on empty area cancels) (RegionSelectionController.swift:767-780).
- Right mouse down anywhere -> cancel (RegionSelectionController.swift:783).
- Hold **Space** while dragging: the whole rubber-band moves with the mouse (anchor follows); release Space to resume sizing (RegionSelectionController.swift:707-713, 785-787, 795-796).

### 3.8 After the drag: two modes
**Default (`editAfterSelect` = true, requires frozen image):** `skipAdjust` - the selection finishes immediately on mouse-up / click / F (RegionSelectionController.swift:419-427, 1079). Other displays' overlays close instantly; the chosen display's overlay stays and the **inline editor** is placed on it (section 6.9). No adjust handles step, no Capture button.

**`editAfterSelect` = false ("adjust" step, also used for Text, Scrolling and Record):**
- The rect stays with handles. Cursor: resize cursors within 8 pt of an edge/corner (corner = both edges), open hand inside, closed hand while moving, pointing hand over buttons, crosshair elsewhere (RegionSelectionController.swift:438-483).
- Drag an edge/corner: resize (only grabbed edges move, those edges snap; dragging past the opposite edge flips). Loupe shows during resize. If the result is < 5 pt in a dimension on release, revert (RegionSelectionController.swift:487-506, 760-763).
- Drag inside: move, clamped to the display (RegionSelectionController.swift:508-513).
- Double-click inside: confirm (RegionSelectionController.swift:662).
- Click outside: start a new selection; if it ends as a click on nothing, the old one is restored (RegionSelectionController.swift:668-674, 775-776).
- **Action bar** (RegionSelectionController.swift:76-96, 138-171): two buttons, height 32, gap 6 between, 8 pt below the rect, right-aligned to rect.maxX; if no room below (bottom − 6) -> above; if no room above (< 6) -> inside bottom. X clamped to [6, width − total − 6].
  - Cancel: 32×32, `xmark` 13 pt bold white, chip fill 92% (100% hovered), stroke edge (white 50% hovered), radius 8.
  - Confirm: width = text width + 50, icon + title (e.g. camera.fill + "Capture"), 13 pt semibold white, accent 88% (100% hovered), white 25% stroke (60% hovered), scales to 1.03 hovered, radius 8.
  - The bar is hidden during dragging; appears with opacity+scale (0.96 -> 1) in 0.14 s.
  - Press-and-release on the same button triggers it; dragging off cancels the press.
- Confirm requires rect ≥ 5×5 pt (RegionSelectionController.swift:515-519).

### 3.9 Keyboard during selection (RegionSelectionController.swift:789-811, 960-971)
| Key | Effect |
|---|---|
| Esc | Cancel immediately (no step-back). Also caught by a local and a global key monitor so Esc always works even if focus is lost. |
| Return / keypad Enter | Confirm, only in adjust step |
| Space (hold) | Move rubber-band while dragging |
| F | Select the whole display (when not dragging); in default mode this goes straight to the inline editor on the full screen |
| Arrows | Adjust step only: move selection 1 pt, with Shift 10 pt (clamped) |
| ⇧ / ⌥ | Square / free select (modifiers) |

Keyboard focus follows the pointer between displays (only when no mouse button is held) (RegionSelectionController.swift:1004-1012).

### 3.10 Multi-display
- Overlay on every display; a selection cannot span displays (RegionSelectionController.swift:822-830).
- Only the display under the pointer shows crosshair/loupe (`pointerInside`) (RegionSelectionController.swift:46-48).
- Pixel scale for each display is derived from its frozen image width / point width (RegionSelectionController.swift:1073-1076).
- Result = (rect, display, that display's frozen image).
- Virtual-desktop (Space) switch during selection: after 150 ms, all displays are re-frozen, selections reset, hint shown again, overlays brought to the new desktop (RegionSelectionController.swift:972-1002).

### 3.11 Closing
- Cancel: overlays fade out 0.12 s (mouse ignored during fade) (OverlayChrome.swift:20-27, RegionSelectionController.swift:931).
- Selection done: overlays removed instantly (so subsequent live captures don't include them) (RegionSelectionController.swift:1124-1126).
- Cancel produces no sound, no message.

### 3.12 Result (non-inline)
Crop the frozen image to the rect (pt × image-derived scale, intersected, integral) (ScreenCapturer.swift:831-843), white flash on that rect (section 4.3), then post-capture flow (section 4.2).

---

## 4. Other capture modes and the post-capture flow

### 4.1 Capture Fullscreen (ScreenCapturer.swift:156-169)
- No overlay. Captures the display **under the mouse cursor** (not the primary display) (ScreenCapturer.swift:161-163, 779-783), at native pixel resolution, cursor hidden, SlopShot windows excluded.
- White flash over the whole display, then post-capture flow. Subtitle `W×Hpx`.

There is **no** window-capture mode and **no** "capture previous area" mode. (Window capture is done by hovering + clicking in Capture Area.)

### 4.2 Post-capture flow `finishImage` (ScreenCapturer.swift:594-643) - exact order
1. Base name = **`SlopShot yyyy-MM-dd 'at' HH.mm.ss`**, e.g. `SlopShot 2026-10-06 at 14.03.27` (ScreenCapturer.swift:598, 684-687).
2. Reserve a unique **temp** file `<tmpdir>/<base>.png` (adds ` (2)`, ` (3)` … on collision) by creating it empty (TempFiles.swift:14-25).
3. Play capture sound if enabled (system "Screen Capture.aif", fallback "Tink"; restarts if already playing) (ScreenCapturer.swift:603, 646-654).
4. Remember as "last image"/"last file" (enables menu items).
5. If "Show preview thumbnail" (default on): show the preview card **immediately** (section 4.4).
6. In background: encode **PNG** (always PNG, regardless of the format setting) and write to the temp file; on failure delete it.
7. Then, if "Also copy to clipboard" (default **on**) or the action forced a copy: clipboard = **image + file URL** of the temp PNG (image only if the write failed).
8. Add a history entry (kind image, subtitle e.g. `1440×900px`), with thumbnail and background OCR indexing.

**Nothing is written to the save folder automatically.** Files land in the save folder only when the user clicks **Save** on the preview card (or ⌘S in the inline editor, or Save in History). The Settings caption states this (SettingsWindowController.swift:87).
No notification, no auto-open editor in this flow (editor opens only from the card/menu).

### 4.3 Screen flash (ScreenFlash.swift:11-32)
White borderless panel over the captured rect (or entire display), click-through, top-most, starts at alpha 0.45 and fades to 0 over 0.35 s ease-out. Used by: Capture Fullscreen (whole display), Capture Area non-inline mode (the rect). **Not** used by: inline-editor captures, scrolling capture, OCR, color pick, recording.

### 4.4 Floating preview card (ThumbnailController.swift)
- **One card at a time**: showing a new card removes the old one instantly; starting any new capture session also removes it (ThumbnailController.swift:226; ScreenCapturer.swift:90). No stacking.
- Display: the display where the capture happened (ThumbnailController.swift:234-236).
- Geometry: card 210×150 pt, with 14 pt transparent padding all around for the shadow -> window 238×178. Positioned 24 pt from the bottom of the usable area (above Dock) and 24 pt from the left (setting "Bottom left", default) or right ("Bottom right") edge of the usable area (ThumbnailController.swift:52-53, 229-241).
- Look: image aspect-**fill** (cropped, not letterboxed) on gray 0.1, corner radius 14, 1 pt white 22% inner border, shadow black 50% r10 y+6 (ThumbnailController.swift:62-80).
- Window: floating level, all desktops, does not activate the app (ThumbnailController.swift:263-272).
- Appear: starts shifted off-screen by (width + 40) toward its side, alpha 0, slides in + fades in over **0.32 s ease-out** (ThumbnailController.swift:274-285).
- Auto-dismiss: **8 s** after appearing. Mouse enter cancels the timer. Mouse exit (if not pinned) re-schedules dismiss after **1.5 s**. Dismiss = slide out toward the same edge + fade over **0.28 s ease-in** (ThumbnailController.swift:255-261, 288, 304-333). Hover works even when the app is inactive.
- Hover state (fade 0.14 s): black 42% overlay over the image plus controls (ThumbnailController.swift:101-141):
  - Center, vertical stack spacing 6: white capsules 68×23 pt, white 92% bg, black 85% text 12 pt semibold: **"Copy"**, **"Save"**.
  - Top-left circle: `xmark`, tooltip **"Discard"**.
  - Top-right circle: `pin` (`pin.fill` when pinned), tooltip **"Pin"**.
  - Bottom-left circle: `pencil.tip.crop.circle`, tooltip **"Edit"**.
  - Bottom-right circle: `square.and.arrow.up`, tooltip **"Share"**.
  - Circles: 24×24, black 55%, icon 10 pt bold white; 6 pt inset from card corners.
  - Button hover: brightness +0.12, scale 1.06; press: brightness −0.08, scale 0.92 (ThumbnailController.swift:185-201).
- Actions:
  - **Click the card body**: open the annotation editor on this image (temp file as source). The card stays (ThumbnailController.swift:82).
  - **Copy**: clipboard = image + temp file URL (ScreenCapturer.swift:618-622). Button turns into a `checkmark.circle.fill` for 0.45 s, then the card slides out (ThumbnailController.swift:144-149).
  - **Save**: write the image to the save folder in the configured format (JPEG/HEIC at quality 0.9) as `<base>.<ext>` (`jpg` for JPEG), never overwriting (adds ` (2)`, ` (3)`…), creating the folder if needed; becomes the "last file". Same tick + slide out (ScreenCapturer.swift:510-519, 656-682; AppSettings.swift:28-58). Error -> "Capture failed" alert.
  - **Discard (x)**: slide out. Temp file and history entry are kept.
  - **Pin**: toggles pinned; pinned cancels auto-dismiss and mouse-exit dismiss (card stays until Discard/Copy/Save/Edit/new capture) (ThumbnailController.swift:249-253). There is no separate "pinned floating screenshot" window.
  - **Edit**: open editor, then slide the card out (ThumbnailController.swift:128).
  - **Share**: system share menu for the temp file URL.
  - **Drag the card out**: drags the temp PNG file into other apps; drag preview is the 210×150 aspect-filled image (ThumbnailController.swift:83-89).
  - Haptic tick on buttons (trackpad only; ignore on Linux).
- Video variant (after recording): play badge (44 pt circle, black 45%, `play.fill`) when not hovered; top-right becomes `eye` "Quick Look" (opens in-app player); bottom-left becomes `scissors` "Trim" (opens video editor, card slides out); Copy copies the .mov file; Save copies the .mov into the save folder (ThumbnailController.swift:92-130; ScreenCapturer.swift:463-487, 522-534).

### 4.5 Sounds
Shutter sound (if "Play a sound", default on) at: every image capture (`finishImage`), and when a recording stops successfully (ScreenCapturer.swift:439, 603). Never on OCR, color pick, cancel.

### 4.6 Capture Text (OCR) (ScreenCapturer.swift:208-243)
Selection overlay with tool title **"Capture Text"**, icon `text.viewfinder`, confirm **"Read Text"** (always uses the adjust step, never inline). OCR on the crop. If no text: nothing visible happens. Else: clipboard = recognized text (always, regardless of the clipboard setting), history entry `<n> chars`, and the **Text Recognition** window opens (section 9).

### 4.7 Capture Scrolling Area / Record Area
See section 9.

---

## 5. Color picker (ColorPickerController.swift; ScreenCapturer.swift:248-276)

- Captures **only the display under the cursor**; overlay only on that display. If capture fails: "Capture failed" alert with "Couldn't read the screen to pick a color from." (ScreenCapturer.swift:251-259).
- **No dimming**: frozen image shown as-is (ColorPickerController.swift:7-9). Cursor: crosshair (ColorPickerController.swift:131-133). Window on all desktops.
- Hint panel (bottom, 96 pt up): title **"Click to copy the color"**, subtitle **"← → change format  ·  arrow keys with ⇧ nudge by 1px  ·  esc to cancel"**. Always visible (no fade on interaction); drawn below the loupe (ColorPickerController.swift:41-47).
- Tool badge: eyedropper + **"Pick Color"** (top center, same behavior as 3.3).
- Loupe: **152×152 pt, zoom ×12**, offset 26 from cursor, flips when within 12 pt of right edge or 96 pt of bottom (ColorPickerController.swift:35-37, 52-54); same grid/center-cell rendering as 3.4.
- Readout panel (ColorPickerController.swift:62-107): centered under the loupe, 8 pt gap; if it would overflow bottom − 8, placed 8 pt above the loupe; x clamped to 8 pt margins. HUD background radius 12, padding 10 h / 9 v, spacing 10:
  - Swatch 30×30, radius 6, white 35% 1 pt border.
  - Line 1: color value in the current format, 13 pt semibold monospaced digits, white.
  - Line 2: **`<SHORT>  ·  X, Y`** (short label + pixel coords), 10 pt medium, white 55%.
- Formats, in cycle order (AppSettings.swift:62-105), default **HEX**:

| # | Settings label | Short | Output example |
|---|---|---|---|
| 1 | `HEX  #A4773F` | HEX | `#A4773F` |
| 2 | `hex  #a4773f` | hex | `#a4773f` |
| 3 | `CSS  rgb(…)` | RGB | `rgb(164, 119, 63)` |
| 4 | `CSS  hsl(…)` | HSL | `hsl(33, 44%, 45%)` (h deg, s/l % rounded) |
| 5 | `SwiftUI Color` | SwiftUI | `Color(red: 0.643, green: 0.467, blue: 0.247)` |
| 6 | `AppKit NSColor` | NSColor | `NSColor(srgbRed: 0.643, green: 0.467, blue: 0.247, alpha: 1)` |

  Values converted to sRGB; 8-bit values `round(c×255)`; 3-decimal floats for the code formats.
- Keys (ColorPickerController.swift:172-223):
  - Esc -> cancel (fade 0.12 s). Right click -> cancel.
  - Return or Space -> pick.
  - ← / → (no Shift) -> previous/next format, wraps; persisted immediately to settings (ColorPickerController.swift:286).
  - ↑ / ↓ (any), and ⇧+←/→ -> nudge exactly 1 device pixel, clamped, and the real mouse cursor is warped to match.
- Mouse: move updates color; mouse down/drag also updates; **mouse up picks** (ColorPickerController.swift:155-168).
- After pick (ScreenCapturer.swift:264-276): overlay closes instantly; clipboard = string in the format shown at that moment; history entry (kind color, subtitle = HEX, 56×40 swatch thumbnail). **No toast/HUD, no sound, no flash.**

---

## 6. Annotation editor

### 6.1 Window (EditorWindowController.swift:15-51)
- 1040×740 pt, centered, min 760×500, title "SlopShot — Editor" (hidden), transparent title bar, full-size content, dark appearance, **traffic-light buttons hidden** (no visible close button). Resizable, miniaturizable.
- Opening one closes any previous editor. App becomes a regular app while open; floats above other apps until the user activates SlopShot (WindowPresenting.swift:16-38).
- Background drag does not move the window; the empty part of the top bar does (EditorWindowController.swift:31-34).
- Entry points: preview card click / Edit, menu "Edit Last Screenshot", History "Edit".

### 6.2 Layout (EditorView.swift:222-234, 674-698, 990-1010)
- Top bar 52 pt, bg gray 0.13, 1 pt white 8% hairline at bottom, horizontal padding 14, spacing 10:
  `[tools pill] [style pill] [actions pill] <spacer> [Save as…] [Done]`
- Canvas area: gray 0.11; image fitted inside with 28 pt padding, centered, shadow black 50% r16 y6; scrollable when zoomed (EditorView.swift:591-609, 664-670).
- Bottom bar 44 pt, bg gray 0.13, hairline at top, padding 16: `[status text] <spacer> [zoom controls] [Share] [Copy]`.
- Pills: HStack spacing 3, padding 5, bg white 6%, white 8% stroke, radius 12; dividers 1×22 white 12% (EditorView.swift:703-743).

### 6.3 Tools, order, keys (EditorView.swift:9-71, 711-721, 745-758)
Tools pill: `Select | Rectangle Ellipse Line Arrow | Highlight Blur Pen | Text Counter`

| Tool | Icon (SF) | Key | Tooltip |
|---|---|---|---|
| Select | cursorarrow | V | Select (V) |
| Rectangle | rectangle | R | Rectangle (R) |
| Ellipse | circle | O | Ellipse (O) |
| Line | line.diagonal | L | Line (L) |
| Arrow | arrow.up.right | A | Arrow (A) |
| Highlight | highlighter | H | Highlight (H) |
| Blur | drop.fill | B | Blur (B) |
| Pen | pencil.tip | P | Pen (P) |
| Text | textformat | T | Text (T) |
| Counter | 1.circle | N | Counter (N) |

- Buttons 30×28, icon 14 pt medium; selected = accent fill radius 7 + white icon; else gray 0.80 icon.
- Single-letter keys work without modifiers, not while editing text or in any text field (EditorView.swift:373-380).
- Default tool = **last tool the user picked** (persisted, key `editor.lastTool`), first-ever default **Arrow** (EditorView.swift:150, 544-551). Auto-switch to Select after paste/sticker is not persisted.
- Default color **Red**, default width **Normal (0.005)** - not persisted (EditorView.swift:151-152).
- Hidden tools: Image (from paste/sticker) and Redaction (from Redact).

### 6.4 Style pill (EditorView.swift:724-848)
- **Color** button: 18 pt circle of current color, white 55% 1.5 pt ring; tooltip "Color". Popover: 5-column grid of 24 pt circles, spacing 10, padding 14: **Red, Orange, Yellow, Green, Blue, Purple, Pink, White, Black** (EditorView.swift:184-194). These are SwiftUI semantic system colors, not fixed hex values. On screen (dark appearance) they resolve to Apple's dark variants: red #FF453A, orange #FF9F0A, yellow #FFD60A, green #30D158, blue #0A84FF, purple #BF5AF2, pink #FF375F. The exported image is rendered by `ImageRenderer` and may resolve to the light variants: #FF3B30, #FF9500, #FFCC00, #34C759, #007AFF, #AF52DE, #FF2D55. This is unverified; check a real export before you fix the Linux values. White #FFFFFF, black #000000. Selected swatch ring white 95% 2.5 pt, others white 25% 1 pt. Choosing closes the popover.
- **Width** button: `lineweight` icon; tooltip "Stroke width · blur strength". Popover list, rows 200 pt wide: preview bar 64 × max(w×600, 1.5) (accent if current), name, checkmark if current: **Thin 0.003, Normal 0.005, Bold 0.008, Heavy 0.012** (fractions of image width) (EditorView.swift:195-197, 791-824).
- Changing color/width while a layer is selected also changes that layer (color applies to rect, ellipse, line, arrow, highlight, pen, text, counter, redaction; width to rect, ellipse, line, arrow, pen, blur) with status "Recolored the selected <tool>" / "Changed stroke width on the selected <tool>" / "Changed blur strength on the selected layer" (EditorView.swift:462-490).
- **Stickers** button (`face.smiling`, tooltip "Stickers"): sticker-pack picker; inserting adds an image layer 28% of image width, centered, selected, tool -> Select; status `Added sticker “<name>”` or `Added animated sticker “<name>” — Copy/Save gives a GIF` (EditorView.swift:828-848).

### 6.5 Actions pill (EditorView.swift:729-738, 850-987)
`[Redact] | [Undo] [Clear] | [Flip & rotate menu]`
- **Redact** (`eye.slash`, `hourglass` while scanning). On open, if setting on, image is scanned for sensitive data; if found: icon turns orange with an orange count badge and status `<n> sensitive item(s) (<summary>) — hit Redact to black them out`. Pressing Redact places solid **black** boxes (padded by 0.4% / 0.6% of image size) over each match; status `Blacked out <n> item(s) (<summary>) — ⌘Z to undo, or Select + ⌫ for one`. If none: `No emails, phone numbers, card numbers or tokens found`. Pressing again: `Already redacted — ⌘Z to take it back out`. Undo removes the whole batch at once (EditorView.swift:884-960, 508-525). Summary order: email, phone, card, CVV, expiry, name on card, key/token, password, ID number (SensitiveScanner.swift:95-107).
- **Undo** (`arrow.uturn.backward`, disabled when no annotations).
- **Clear** (`trash`, tooltip "Clear all annotations"): removes all; status `Cleared <n> annotation(s) — Undo (⌘Z) to restore`; Undo restores all (EditorView.swift:568-585).
- **Flip & rotate** menu (`crop.rotate`): "Flip horizontal", "Flip vertical", divider, "Rotate left", "Rotate right". Applies to the selected image layer, otherwise to the base image (annotations transformed with it) (EditorView.swift:853-880, 1202-1250).

### 6.6 Drawing behavior (EditorView.swift:1264-1368, 1381-1500)
- Coordinates normalized to the image, so annotations scale with zoom/window and export at full resolution. Stroke width = fraction × image width (min 1 px).
- Drag (min distance 4 pt): if the press is on a selected layer's handle -> resize/move endpoint; else if it hits a layer -> select it and move it (works in **every** tool); else draw a new shape with the current tool. Shapes smaller than 0.4% of the image in both axes are discarded (except Pen). A newly drawn shape becomes selected.
- Click: Select tool selects the layer whose bounds (+6 pt) contain the point, or deselects. Other tools select a layer hit on its stroke; Text tool clicking an existing text starts editing it; Counter places a number; Text places a text field; empty click deselects.
- Hit tolerance: half stroke + 6 pt; filled shapes (blur, redaction, image, text, counter) hit inside (+4 pt).
- Rendering:
  - Rectangle: stroked rounded rect, corner radius = stroke width.
  - Ellipse: stroked.
  - Line: round caps.
  - Arrow: line + two head strokes at ±π/7, head length = 0.02 × W + 2 × stroke, round caps.
  - Highlight: straight line, color 35% opacity, width 0.03 × W, round caps (width setting ignored).
  - Blur: Gaussian blur of the base image, radius = max(width×4, 0.004) × image px width, edge-clamped, clipped to the dragged rect with corner radius min(6, min(w,h)/4); sharp edges (EditorView.swift:1370-1375, 1426-1442, 1717-1745).
  - Pen: freehand polyline, round caps/joins.
  - Text: 0.022 × W semibold, current color, anchored top-left at click point. While editing: a plain single-line field 240 pt wide with placeholder "Text…"; Return ends editing (EditorView.swift:631-641).
  - Counter: filled circle radius 0.013 × W in current color with white bold number (size r×1.2); number = max existing + 1 (starts at 1) (EditorView.swift:199-201).
  - Redaction: solid fill, corner radius min(3, min(w,h)/6).
  - Image: pasted/sticker image with flip/rotation.
- Selection visuals: dashed white 90% 1 pt box (dash 4,3) around the layer (+3 pt), not for lines/arrows/highlights; 9 pt white rounded squares with accent 1.5 pt border at handles: 4 corners for rect/ellipse/blur/redaction/image, 2 endpoints for line/arrow/highlight, none for pen/text/counter (EditorView.swift:1152-1191). Handle grab radius 9 pt.
- **Paste (Cmd+V)**: image from clipboard (file URL of an image, or GIF/PNG/TIFF data) becomes an image layer 50% of the width (max 80% height), centered, selected, tool -> Select; status "Pasted image" / "Pasted GIF (<n> frames)". If editing text, normal text paste (EditorView.swift:384-452).

### 6.7 Keyboard (EditorView.swift:328-400)
| Key | Effect |
|---|---|
| Tool letters | see 6.3 |
| Delete / Forward Delete | delete selected layer (not while typing); status `Deleted <tool> — ⌘Z to undo` |
| Cmd+Z / Cmd+Shift+Z | undo / redo annotations (text undo while typing) |
| Cmd+= or Cmd++ / Cmd+- / Cmd+0 | zoom in / out / fit |
| Cmd+V | paste image |
| Return | **Done** (default button) |
| Esc | chain below |

**Esc chain** (window editor): popover open -> closes popover; editing text -> stop editing; a layer selected -> deselect; annotations exist and not yet armed -> arm and show status `Press ⎋ again to discard <n> annotation(s) and close`; otherwise close the window (no copy, no save). Any annotation add/remove disarms. During GIF export Esc is ignored (EditorView.swift:352-370, 213-217).

### 6.8 Undo semantics (EditorView.swift:492-542)
- Undo after Clear restores all; undo after Delete re-inserts the layer at its old index (only if nothing was added since); undo of a Redact batch removes the whole batch and restores the warning; otherwise removes the last annotation. Redo mirrors. Drawing anything new clears redo.
- Moving/resizing a layer and recoloring are **not** undoable.

### 6.9 Zoom (EditorView.swift:1012-1056)
Bottom bar: `[-] [NN%] [+]` on white 8% pill radius 5; % label 11 pt mono, click = fit (tooltip "Fit to window (⌘0)"). 100% = fit. In ×1.25 / ÷1.25, clamped 25%-1600%. Pinch also zooms.

### 6.10 Output buttons (window editor)
- **Done** (prominent, Return): copy flattened image to clipboard (image only, no file), then close (EditorView.swift:1663-1668).
- **Copy** (bottom-right, `doc.on.doc`): copy image only; status "Copied"; marks clean (EditorView.swift:1610-1619).
- **Share** (`square.and.arrow.up`): system share picker with the flattened image (EditorView.swift:1640-1661).
- **Save as…**: standard save dialog with a "Format:" popup: **PNG, JPEG, TIFF, BMP, GIF** (static) or **GIF (animated), PNG (first frame), JPEG (first frame)** when animated stickers exist. Default filename = source file's base name (e.g. `SlopShot 2026-10-06 at 14.03.27.png`) or `SlopShot edited`; the extension follows the format. JPEG quality 0.9. Overwrite confirmation is the OS dialog's. Status `Saved <file> (<size>)` or "Save failed"/"Export failed" (EditorView.swift:1670-1707, 1793-1867). Note: Save as… does **not** use the configured save folder or image format.
- Output is rendered at the image's full pixel size; sizes formatted `X.Y MB` or `N KB`.
- Animated stickers: Copy/Done/Share produce a GIF (frames rendered; status `Building GIF… i/n`, then `Copied GIF — <n> frames, <size>`); buttons disabled while exporting (EditorView.swift:1543-1638).
- Status line default text: `<n> annotation(s)` (EditorView.swift:992-994).

### 6.11 Close with unsaved changes
- No save prompt on close (Esc twice, Cmd+Q "Close Window"). The only guard is the "Discard your annotations?" alert when a **new capture** starts while the editor has annotations not yet copied/saved (ScreenCapturer.swift:68-77). "Dirty" = annotation count changed to > 0; Copy/Done/Save as reset it.

### 6.12 Inline editor (default Capture Area flow) (EditorView.swift:106-120, 236-322; RegionSelectionController.swift:885-906; ScreenCapturer.swift:558-582)
- The selected rect on the frozen overlay becomes the canvas (same pixels, exact position). Dim stays around it; tool badge stays at top; size chip hidden.
- Stroke/text/counter sizes are scaled as if the canvas were ~900 pt wide: `unit = max(1, 900 / rectWidth)` (EditorView.swift:116-119).
- Floating toolbar (HUD, radius 14, padding 6, spacing 8): `[tools pill] [style pill] [Redact | Undo Clear] [Cancel(x) Save(⬇) Copy]`. No zoom, no share, no flip/rotate, no Save as…
  - Cancel `xmark` tooltip "Cancel (esc)"; Save `square.and.arrow.down` tooltip "Save (⌘S)"; **Copy** label button (doc.on.doc + "Copy", accent bg, 28 pt high) tooltip "Copy and close (⌘C or ↩)".
- Toolbar position: centered on the rect horizontally (clamped 8 pt from screen edges), 10 pt below the rect; if no room, 10 pt above; if still no room, inside the rect bottom.
- Status messages appear in a small HUD 30 pt above the toolbar.
- Keys: **Cmd+C or Return/Enter = Copy**, **Cmd+S = Save**, **Esc = cancel immediately** (no arming; still first clears text editing/selection), plus all editor keys (EditorView.swift:332-339, 360-361).
- Copy -> overlay closes instantly, then post-capture flow (4.2) with **forced clipboard copy**. Save -> post-capture flow (clipboard only if setting on) **and** immediately write to the save folder (configured format). Cancel -> overlay fades out, nothing captured. No flash in this path.
- Rendered output converted to 8-bit in the capture's color space (ScreenCapturer.swift:575-592).

---

## 7. Settings window (SettingsWindowController.swift)

Window 640×400 (fixed), title **"Settings"**, closable only, centered, app activated; re-opening focuses the existing one (SettingsWindowController.swift:12-34). Tabs in order: **General** (gearshape), **Capture** (camera.viewfinder), **Privacy** (hand.raised), **Shortcuts** (keyboard), **About** (info.circle). All changes apply and persist immediately (AppSettings.swift:133-160).

### 7.1 General
- Section **"Startup"**:
  - Toggle **"Launch SlopShot at login"** - reflects the OS login-item state (not stored); default off (AppSettings.swift:162-183).
  - Toggle **"Show icon in menu bar"** - default **on**. When off, caption: "Hotkeys keep working. To get back here, open SlopShot again from Spotlight or Finder."
- Section **"Save location"**: folder icon + path (home shown as `~`, middle-truncated) + **"Choose…"** button (folder picker starting at current folder). Default **~/Desktop**. Caption: "Captures are kept temporarily until you click Save on the preview — then they're written to the folder above." (AppSettings.swift:202-210, 224-226)
- Section **"Image format"**: popup **"Format"**: PNG, JPEG, HEIC, TIFF, GIF, BMP; default **PNG**. Affects only the preview-card Save / inline ⌘S (AppSettings.swift:15-58).

### 7.2 Capture
- Section **"Selection"**: Toggle **"Snap to window & item edges"**, default **on**. Caption: "Hover to outline the window or item under the cursor, then click to grab it. Hold ⌥ while dragging for a free selection."
- Section **"After capture"**:
  - **"Annotate right after selecting an area"**, default **on** (tooltip "Drawing tools appear on the selection; ⌘C copies, ⌘S saves, esc cancels.")
  - **"Also copy to clipboard"**, default **on**
  - **"Play a sound"**, default **on**
  - **"Show preview thumbnail"**, default **on**
  - Popup **"Preview position"**: "Bottom left" (default) / "Bottom right"; disabled when thumbnail is off. Caption: "Right-handed? Put the preview on the right, closer to where your mouse already is."
- Section **"Recording"**: **"Record system audio"** (default off), **"Record microphone"** (default off). Caption: "System audio is what your Mac plays (apps, videos, calls). The microphone asks for permission the first time."
- Section **"Color picker"**: popup **"Copy color as"** (6 formats, labels in section 5), default HEX. Caption: "Press ← → while picking to switch format without leaving the screen."

### 7.3 Privacy
- Section **"Permissions"** (refreshes every 2 s and on app activation): rows for **"Screen & System Audio Recording"** ("Every screenshot and screen recording. Without it captures come out empty.") and **"Accessibility"** ("Scrolling capture: auto-scroll, and reading how far the page has moved."); each shows green "Granted" (checkmark.circle.fill) or orange "Not granted" (exclamationmark.triangle.fill) and an **"Open…"** button. Screen Recording when not granted adds: "Turn it on, then quit and reopen SlopShot — macOS only hands this permission to a freshly launched app." (SettingsWindowController.swift:212-267; SystemPermissions.swift)
- Section **"Sensitive data"**: Toggle **"Scan captures for sensitive data"**, default **on**, + caption (SettingsWindowController.swift:178-183).
- Section **"Search"**: Toggle **"Index text inside screenshots"**, default **on**. Caption: "Reads each screenshot in the background so History can be searched by what's written in the image."
- Footer: lock icon + "Both run on-device with Apple's Vision framework. No image, and no text read out of one, ever leaves your Mac."

### 7.4 Shortcuts
- Section **"Global shortcuts"**: one row per action (order of section 2): title, recorder box 110×22, reset button (`arrow.uturn.backward`, tooltip "Reset to default").
- Recorder: shows the binding (or "—"); click -> **"Type shortcut…"** in accent color with accent 18% bg and accent border; only one recorder listens at a time; next key press with ≥ 1 modifier is saved; no modifier -> system beep, keeps listening; Esc -> cancel. All keys are swallowed while listening (SettingsWindowController.swift:375-426).
- Caption: "Click a shortcut, then press the new key combo (needs at least one of ⌃⌥⇧⌘). Press ⎋ to cancel."
- No duplicate/conflict detection between SlopShot's own actions.
- Section **"macOS screenshot shortcuts"** (macOS-specific): status text "macOS still owns ⌘⇧3, ⌘⇧4 and ⌘⇧5" / "⌘⇧3, ⌘⇧4 and ⌘⇧5 go to SlopShot"; orange warning listing blocked actions; buttons "Turn off macOS shortcuts" / "Restore macOS" (SettingsWindowController.swift:312-366).

### 7.5 About
App icon 84×84, "SlopShot" (title2 bold), "Version X.Y.Z" caption, "A CleanShot-style capture tool, built native on macOS." (SettingsWindowController.swift:429-447).

### 7.6 Stored settings summary (AppSettings.swift:212-247)
| Key | Default |
|---|---|
| save.folder | ~/Desktop |
| save.copy | true |
| save.thumb | true |
| save.format | png |
| select.snap | true |
| pick.color.format | hex |
| editor.redact.scan | true |
| history.index.text | true |
| preview.side | left |
| menubar.icon | true |
| capture.sound | true |
| capture.inlineEdit | true |
| record.audio.system | false |
| record.audio.mic | false |
| hotkeys | {} (all defaults) |
| editor.lastTool | (unset -> arrow) (EditorView.swift:546) |
| translate.target | system language if supported else en (TranslationService.swift:60-64) |

---

## 8. Pin
There is no standalone pinned-screenshot window. "Pin" exists only on the preview card and keeps the card on screen (disables auto-dismiss) until dismissed (ThumbnailController.swift:118-120, 249-253).

---

## 9. Lower-priority features (entry points + core behavior)

- **History** - menu "Capture History…". Window 420×520 (min 360×320) titled "Capture History"; header "Recent captures" with Settings and "Clear all" buttons; search field "Search text inside captures…"; rows show thumbnail, `<subtitle> · <relative time>`; per-row Copy, Save as… (copies file into save folder without asking), Edit (image -> editor), Play (video), Open & translate (text -> OCR window), Delete. Empty states "No captures yet" / `No captures match “<q>”`. Max 100 items, stored in Application Support/SlopShot/history.json + Thumbnails/. Images are OCR-indexed in the background when the setting is on (HistoryWindowController.swift; CaptureHistory.swift:60-150; ScreenCapturer.swift:690-743).
- **OCR window** - after Capture Text. 960×620 (min 760×440), title "Text Recognition" or "Text & QR Recognition"; image vs. editable text; QR codes listed with Copy (http(s) links openable); "<n> characters · <m> words"; "Copy text" -> "Copied"; Original/translated toggle, target-language menu, "Translate" (OCRWindowController.swift; TranslationService.swift).
- **Scrolling capture** - selection overlay ("Scrolling Capture", confirm "Start Scrolling Capture", adjust step). Then dim + focus border around the rect and a 470×44 bar 12 pt below (or above) the rect: status text (130 pt, e.g. "Scroll through the area…", "Captured <n> px"), "Auto-scroll" toggle button, "Cancel" (Esc) and "Done" (Return). Frames captured ~5 fps and stitched; "Please slow down" pill if scrolling too fast; "Building image…" while finishing. Result goes through the normal post-capture flow (no flash), subtitle `W×Hpx (scrolling)` (ScreenCapturer.swift:291-332; ScrollCaptureController.swift).
- **Record Area** - selection overlay ("Record Area", confirm "Start Recording", adjust step); hotkey again or menu "Stop Recording" stops. 60 fps H.264 .mov in temp dir, optional system audio/mic. Dim + focus border, click-ripple effects, and a recording bar below the rect: Stop & save, timer `m:ss`, Pause/Resume, Restart, "Discard (don't save)", mic mute (if mic on), drag handle. On stop: sound, clipboard = .mov file (if setting), history (duration `m:ss`), video preview card (4.4). In-app player and video editor (trim/zoom/export) available from the card (ScreenCapturer.swift:337-507; RecordingBarController.swift; ScreenRecorder.swift).
