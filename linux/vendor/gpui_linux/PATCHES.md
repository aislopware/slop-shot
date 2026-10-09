# Local patches to gpui_linux

Vendored from aislopware/gpui-fast at the rev pinned in `linux/Cargo.toml` and wired in
through its `[patch."https://github.com/aislopware/gpui-fast.git"]`. The changes live in
`vendor/gpui_linux.patch`; search for `slopshot patch` to find each one.

1. **Wayland clipboard images** (`src/linux/wayland/clipboard.rs`, `client.rs`).
   Upstream only ever offers text mime types when writing the clipboard, so an image
   `ClipboardItem` reaches no other app. The data source now offers the mime type of every
   image entry and `send` answers those requests with the image bytes. An image followed
   by a `file://` string also offers `x-special/gnome-copied-files` (not `text/uri-list`,
   which Chromium apps such as Slack paste as a file instead of the pixels); a lone string
   tagged with the `"text/uri-list"` JSON metadata is offered only as a file, in both
   types (copying a recording).
2. **Fullscreen on a chosen output** (`src/linux/wayland/window.rs`).
   A `WindowKind::PopUp` opened with a `display_id` calls `xdg_toplevel.set_fullscreen(output)`
   before its first commit. xdg-shell gives a client no other way to place a window on a
   specific monitor, which the capture overlay needs on multi-monitor setups.
3. **Popups exactly where asked on X11** (`src/linux/x11/window.rs`).
   Upstream shifts every new X11 window 2 px right to dodge a window-manager placement
   bug. Override-redirect popups bypass the window manager, so the shift only misplaced
   them: the capture focus layer's outline landed inside the captured area.

To take a newer gpui-fast, change the rev in `linux/Cargo.toml` and run
`ci/vendor-gpui-linux.sh`. Drop a patch once gpui-fast ships the same behaviour.
