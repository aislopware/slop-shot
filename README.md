<p align="center">
  <img src="docs/icon.png" width="120" alt="SlopShot icon">
</p>

<h1 align="center">SlopShot</h1>

<p align="center">
  A native macOS screenshot &amp; screen-recording tool, built from scratch in Swift.<br>
  UX inspired by <a href="https://cleanshot.com">CleanShot X</a> — original code, not a fork.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%2015%2B-blue" alt="macOS 15+">
  <img src="https://img.shields.io/badge/Swift-SwiftUI%20%C2%B7%20AppKit-orange" alt="Swift">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT">
</p>

> Menu-bar app (no Dock icon). Everything is driven from the menu-bar `S` icon and global hotkeys.

## Features

- **Capture** — full screen, drag-to-select region, **scrolling capture** (image stitching + Accessibility scroll-offset), and **text capture** (Vision OCR).
- **OCR result window** — recognized text next to the captured image, editable, with **QR code** detection and one-click **translation** into 24 languages via Apple's on-device Translation framework (no API key, works offline once the language pack is downloaded).
- **Smart region selection** — the screen freezes the instant you hit the hotkey (so nothing under the overlay can change, and apps never lose their focused state), with a pixel loupe, **window snapping**, and **item snapping**: hover to outline the button / card / panel under the cursor and click to grab it. Hold `⌥` for a free selection.
- **Screen recording** — record a region to `.mov`, with pause/resume, restart, and discard.
- **Annotation editor** — shapes, text, arrows; zoom via buttons, `⌘ +/-/0`, and trackpad pinch.
- **Floating preview card** — Copy / Save / Share / Pin, drag-out to other apps.
- **Video preview** — Quick Look (in-app player) + **Trim** tool with a custom filmstrip range slider, exporting *Trim Only* (`.mov`, passthrough) or *Trim & Convert* (MP4 / GIF).
- **Capture history**, **settings**, and configurable **global hotkeys**.

## Tech stack

Swift · SwiftUI · AppKit · ScreenCaptureKit · AVFoundation / AVKit · Vision (OCR) · CoreGraphics/CoreText · [XcodeGen](https://github.com/yonaskolb/XcodeGen)

## Build & run

Requires Xcode (macOS 15+ SDK) and XcodeGen (`brew install xcodegen`).

```bash
make run        # build Debug + launch
make install    # build Release → sign → copy to /Applications
```

### Code signing

The `Makefile` **auto-detects** the signing identity, so a fresh clone just works:

- if a self-signed certificate named `SlopShot Dev` exists in your Keychain, it's used
  (this keeps the macOS Screen-Recording / Accessibility permissions across rebuilds);
- otherwise it falls back to **ad-hoc** signing (`-`) — builds and runs fine, but the
  permissions reset on each rebuild. For a one-time install that's a non-issue.

Override anytime: `make install SIGN_ID="Your Identity"`.

Optional (to keep permissions stable across rebuilds): create a self-signed *Code Signing*
certificate named `SlopShot Dev` in **Keychain Access** → Certificate Assistant →
*Create a Certificate…*.

The app is **not** sandboxed and is **not** notarized — it's intended for personal local use.
A locally-built app has no quarantine flag, so Gatekeeper lets it run without warnings.

## App icon

The logo (an `S` inside viewfinder crop-corners) is generated programmatically — no image files:

```bash
swift tools/make_icon.swift   # regenerates AppIcon + the menu-bar template
```

## License

Personal learning project. Provided as-is.
