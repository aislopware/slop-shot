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
- **Color picker** — freeze the screen and magnify it 12× to land on the exact pixel (arrow keys nudge one pixel at a time), then click to copy as HEX, `rgb()`, `hsl()`, SwiftUI `Color` or `NSColor` — `←`/`→` switches format without leaving the picker.
- **Screen recording** — record a region to `.mov`, with pause/resume, restart, and discard. Optionally captures **system audio** and the **microphone** as two separate tracks (mute the mic mid-take from the recording bar), and quietly logs every click so the editor can turn them into zooms.
- **Annotation editor** — shapes, text, arrows; zoom via buttons, `⌘ +/-/0`, and trackpad pinch.
- **Floating preview card** — Copy / Save / Share / Pin, drag-out to other apps.
- **Video editor** — Quick Look (in-app player) plus a full editor built around a lane-based timeline: **trim** handles on a filmstrip, **cut** stretches out of the middle, **speed** ramps (0.25×–10×), **freeze** frames, eased **zoom**, **blur/pixelate censor** regions, and **text** captions. Pick a tool, drag on its lane, then drag the box straight on the video to place it. **Auto Zoom** turns the clicks you made while recording into zoom-ins in one press. Preview and export share one Core Image composition, so what you see is exactly what lands in the file. Exports MOV / MP4 / GIF with separate Quality, Resolution and FPS controls (a plain trim still takes the lossless passthrough path).
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
