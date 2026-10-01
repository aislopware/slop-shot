# Changelog

Sinh từ commit log bằng [git-cliff](https://git-cliff.org). Mục nào nằm ở đâu là do
loại (type) của commit quyết định — tức quyết định lúc VIẾT commit, không phải lúc cắt
release. Quy ước được `scripts/check-commit-msg.sh` chặn ngay ở hook commit-msg.

## [0.8.1](https://github.com/aislopware/slop-shot/compare/v0.8.0...v0.8.1) — 2026-10-01

### Documentation

- Note brew trust needs Homebrew 6, upgrades reopen ([`f85497e`](https://github.com/aislopware/slop-shot/commit/f85497e4ed40ccee50b25ff860a3366550638dee))

## [0.8.0](https://github.com/aislopware/slop-shot/compare/v0.7.1...v0.8.0) — 2026-10-01

### Features

- Click any shape to select, move or delete it ([`e90c7b5`](https://github.com/aislopware/slop-shot/commit/e90c7b54c624194f40f85c3d167bc23d2c50f548))

## [0.7.1](https://github.com/aislopware/slop-shot/compare/v0.7.0...v0.7.1) — 2026-10-01

### Bug fixes

- Reopen after brew upgrades the bundle in place ([`297f450`](https://github.com/aislopware/slop-shot/commit/297f450474f45e3bca708c81e51de6ac95232dae))

## [0.7.0](https://github.com/aislopware/slop-shot/compare/v0.6.0...v0.7.0) — 2026-09-30

### Features

- Annotate right on the selection, ⌘C to copy ([`dc595df`](https://github.com/aislopware/slop-shot/commit/dc595dff84483ff26205a5d192ead7287f47ead0))

## [0.6.0](https://github.com/aislopware/slop-shot/compare/v0.5.0...v0.6.0) — 2026-09-30

### Features

- One look everywhere, glass surfaces, buttons react to hover ([`1fca51a`](https://github.com/aislopware/slop-shot/commit/1fca51a9603bb2b1f77a1cf8e1c262af6d555eb1))
- Pick tools with a single key; buttons react on hover ([`4cddd61`](https://github.com/aislopware/slop-shot/commit/4cddd61cbaf2b6dcbd495429c46c2d787a432316))
- Flash the captured area ([`ccd945d`](https://github.com/aislopware/slop-shot/commit/ccd945d70ec255d46db3aafe7a45026c2bedc696))

## [0.5.0](https://github.com/aislopware/slop-shot/compare/v0.4.1...v0.5.0) — 2026-09-30

### Features

- Show the active tool; hold Space, Shift, or press F ([`6f78afc`](https://github.com/aislopware/slop-shot/commit/6f78afc92e82fbacd4da80e0de89ae9b9e0e0371))
- Reopen SlopShot after brew upgrade replaces it ([`a992ddc`](https://github.com/aislopware/slop-shot/commit/a992ddc2036d39d58ecf3562888c5d43cc652b52))
- Record audio, shutter sound, faster and safer captures ([`d97f464`](https://github.com/aislopware/slop-shot/commit/d97f464227d671f87d90cfd553cd98e43b742340))

### Bug fixes

- Stop a crash when text recognition reports back twice ([`bc450ae`](https://github.com/aislopware/slop-shot/commit/bc450ae03783e5ace030a190268708e32a5c8470))
- Read colors correctly on P3 displays ([`eeee611`](https://github.com/aislopware/slop-shot/commit/eeee611b630c4a24e63e15626edcbaea24cba7cc))

## [0.4.1](https://github.com/aislopware/slop-shot/compare/v0.4.0...v0.4.1) — 2026-09-30

### Bug fixes

- List SlopShot in Screen Recording on a fresh install ([`6d32eec`](https://github.com/aislopware/slop-shot/commit/6d32eecfb043e4addd37c91c8472955fc4d88d7e))

## [0.4.0](https://github.com/aislopware/slop-shot/compare/v0.3.0...v0.4.0) — 2026-09-30

### Features

- Add a setting to show the preview card bottom-right ([`ce61712`](https://github.com/aislopware/slop-shot/commit/ce61712352d3e220f802d6230d06fe8db1a48cb9))
- Add an option to hide the menu bar icon ([`976f678`](https://github.com/aislopware/slop-shot/commit/976f6789f566a0e7827834b84581103a644d9cfd))
- Turn off the macOS screenshot shortcuts in one click ([`7737e27`](https://github.com/aislopware/slop-shot/commit/7737e2778fa3027cb50196d271780d522bd01194))
- Default to ⌘⇧1–6 in menu order ([`3ba462f`](https://github.com/aislopware/slop-shot/commit/3ba462feda00b18b4bbb85521b0d60cdbca987bf))
- Adjust the selection, then confirm to capture or record ([`3a3803f`](https://github.com/aislopware/slop-shot/commit/3a3803f85478990485c02f0bc4d674e682489a3b))

### Bug fixes

- Keep overlay buttons readable when macOS is in light mode ([`a909f36`](https://github.com/aislopware/slop-shot/commit/a909f36c38caa61b3687a8864cd41e7683d6ab62))
- Add SlopShot to the privacy list before opening it ([`eb75092`](https://github.com/aislopware/slop-shot/commit/eb750929b5e153bd29341daef1ad76abaf1f5772))
- Make ⌘Q close the open window instead of quitting ([`c28d9eb`](https://github.com/aislopware/slop-shot/commit/c28d9ebedaa099c0dc84254d8e197086c546de08))
- Show the result window in front of the active app ([`87af7b9`](https://github.com/aislopware/slop-shot/commit/87af7b9f49641336998d88f3184b201ee8d18a82))

### Tooling

- Install the better-update CLI with install.sh ([`8921345`](https://github.com/aislopware/slop-shot/commit/892134543aa1d67cadbe67a24f21130328f2ae05))

## [0.3.0](https://github.com/aislopware/slop-shot/compare/v0.2.0...v0.3.0) — 2026-08-25

### Features

- List the macOS permissions with a way to re-enable ([`388632b`](https://github.com/aislopware/slop-shot/commit/388632bd2660d4fd6b9c8592d736831ed9e225c2))

### Documentation

- Mention the permissions list in Settings ([`b8ea524`](https://github.com/aislopware/slop-shot/commit/b8ea52408d4e7f3b90c7538176c8291f0d7472fe))

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
