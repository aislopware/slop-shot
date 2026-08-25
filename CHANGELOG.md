# Changelog

Sinh từ commit log bằng [git-cliff](https://git-cliff.org). Mục nào nằm ở đâu là do
loại (type) của commit quyết định — tức quyết định lúc VIẾT commit, không phải lúc cắt
release. Quy ước được `scripts/check-commit-msg.sh` chặn ngay ở hook commit-msg.

## [0.2.0](https://github.com/aislopware/slop-shot/compare/v0.1.0...v0.2.0) — 2026-08-25

### Features

- Blur the emails, tokens and card numbers it can see ([`e03548e`](https://github.com/aislopware/slop-shot/commit/e03548ed7bfca99e4df7222250514a730a8b7018))
- Search captures by the text written inside them ([`d30b590`](https://github.com/aislopware/slop-shot/commit/d30b590c45beba0ea880fa48c6d1b55fbf63e56b))
- Redact the rest of the card, not just its number ([`4bf16cf`](https://github.com/aislopware/slop-shot/commit/4bf16cf7a0cd22b88b075259c2b20a2a05e211e0))
- Cover redactions with a solid black box, not a blur ([`1bdb9b0`](https://github.com/aislopware/slop-shot/commit/1bdb9b04930a1ca4c674e30d562f535dfe023a35))

### Bug fixes

- Copy the hex when Copy is pressed on a colour row ([`8c927ab`](https://github.com/aislopware/slop-shot/commit/8c927ab2f13357e7a0f13d266557753673721e6b))
- Warn again when ⌘Z pulls the redactions back off ([`ecef891`](https://github.com/aislopware/slop-shot/commit/ecef891a98b6a31371f537642172141f0509d4bf))
- Split the overloaded Settings tab and un-dim History buttons ([`61638e3`](https://github.com/aislopware/slop-shot/commit/61638e38e36057f5f29a17f21d0ba0abf1c92083))
- Don't let escape close the window mid-export ([`d0bb710`](https://github.com/aislopware/slop-shot/commit/d0bb7106b8b7210d6961e741b36a973c3214f8d3))
- Only look for a cardholder name on the card itself ([`ea63cc3`](https://github.com/aislopware/slop-shot/commit/ea63cc396ecfbbf974dfa1a35ef37b9a7d394455))
- Widen the window so all five tabs fit ([`da44bea`](https://github.com/aislopware/slop-shot/commit/da44bea90b26736763a47183b390ba472afa43cb))

## [0.1.0](https://github.com/aislopware/slop-shot/releases/tag/v0.1.0) — 2026-08-18

### Documentation

- Write up the pipeline and how to install from brew ([`1e90fb3`](https://github.com/aislopware/slop-shot/commit/1e90fb37ea2a2742079855ee73144b93e06fbc95))

### Tooling

- Reformat the shell scripts with shfmt ([`ba34852`](https://github.com/aislopware/slop-shot/commit/ba34852bfe4fefba147d94f7d8071248f2c03367))
- Sign, notarize and ship a DMG to the Homebrew tap ([`173ed0f`](https://github.com/aislopware/slop-shot/commit/173ed0f7c7010c53d740c1736b1b1303f3751963))
- Install the better-update CLI from latest, not a pin ([`563830d`](https://github.com/aislopware/slop-shot/commit/563830d585bf1ccc24512acf7eed433176cf63d5))

### Other changes

- Initial commit: SlopShot — native macOS capture & recording tool ([`74e4a06`](https://github.com/aislopware/slop-shot/commit/74e4a0608edeadcc4a6c66b14cc854f2f453ae7b))
- Add MIT license, README hero icon + badges ([`b2e7269`](https://github.com/aislopware/slop-shot/commit/b2e72690893a3847372774510cd9007d8a284bf8))
- Auto-detect signing identity (ad-hoc fallback) so clone+make install just works ([`4ce0535`](https://github.com/aislopware/slop-shot/commit/4ce05351783b0485f42d551aa21e7bfcf31caa40))
- Pinch-to-zoom works anywhere in the canvas, not just over the image ([`5acaa60`](https://github.com/aislopware/slop-shot/commit/5acaa607a79d75852aca0ef7210c3b165e43232b))
- Add Clear button to wipe all annotations in one click (Undo restores) ([`07b5e6c`](https://github.com/aislopware/slop-shot/commit/07b5e6cf2fc9ebbd3e8b9572656a8c2c1225cc98))
- Region select: fix app crash when Esc fires the cancel callback twice ([`d6e8d93`](https://github.com/aislopware/slop-shot/commit/d6e8d932087be88b4ae57f315f7a0dcc3be189da))
- Add Launch at Login toggle ([`0681b25`](https://github.com/aislopware/slop-shot/commit/0681b25bf6fc2d55004eda62c71244a120ae7bd5))
- Text capture: show an OCR result window with QR detection and translation ([`9efbc96`](https://github.com/aislopware/slop-shot/commit/9efbc96f6d1d7235df5c0071b88af9523b5e0507))
- Region select: freeze the screen, snap to window and item edges, redraw the overlay ([`254175d`](https://github.com/aislopware/slop-shot/commit/254175d9a36d1bc26b641b7eced9dd7001e34cf0))
- Extract the shared chrome (chip, loupe, pixel read) into OverlayChrome ([`b131508`](https://github.com/aislopware/slop-shot/commit/b131508d362b3cd5da4ca8b33e9b11f27707d40f))
- Color picker: magnify to the exact pixel, click to copy (⌃⌥⌘8) ([`3a77c6c`](https://github.com/aislopware/slop-shot/commit/3a77c6cc7279d311f32e823b568240c97fbc3643))
- Region select: a click that jitters a pixel no longer cancels the capture ([`e82c61b`](https://github.com/aislopware/slop-shot/commit/e82c61bda88a20fed1d85b11dc8101f29a65b410))
- Pick from packs on disk and drop them in as image layers ([`7f94d54`](https://github.com/aislopware/slop-shot/commit/7f94d5447a0525e2938fc8c805a9bff54eda19c3))
- Fetch sticker packs and upscale them ([`554edd7`](https://github.com/aislopware/slop-shot/commit/554edd72724ae168248ef28af2a0817fc8926242))
- Video editor: cut, speed, zoom, censor, text and freeze on a timeline ([`37e02bc`](https://github.com/aislopware/slop-shot/commit/37e02bc4085abe7b3f76e031891c868701d25ff5))
- Rewrite the last AppKit views — overlays, panels, settings ([`71aaa2e`](https://github.com/aislopware/slop-shot/commit/71aaa2ebcd5a72ebb12b62bd2e1bf7f1f3a07bf8))
- Blur out anything you don't want in the shot ([`b9fb055`](https://github.com/aislopware/slop-shot/commit/b9fb055da7148bad7850b689ca9b2f0fd02e30e8))
- Keep the animation, and fetch packs instead of shipping them ([`bc45ed9`](https://github.com/aislopware/slop-shot/commit/bc45ed91a0f068b653ad28312c5bc8c5b071bf41))
- Video editor: one press drops the effect in, and lanes stay untangled ([`48afeb2`](https://github.com/aislopware/slop-shot/commit/48afeb22fecc8eeb642da3ca947a13298b07dd29))
