//! Draws SlopShot's windows to PNGs with no desktop session: GPUI's test platform with the
//! platform's own text system and the wgpu headless renderer, so a UI library update can be
//! looked at without a GNOME session or a portal.
//!
//!   ci/vm.sh env SLOPSHOT_RENDER_DIR=\$HOME/render cargo test render -- --ignored
//!
//! PNGs go to `$SLOPSHOT_RENDER_DIR` (default `target/render`).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, Render, Size, Window, px, size};
use gpui_component::Root;
use image::{Rgba, RgbaImage};

use crate::settings::Settings;

fn app() -> HeadlessAppContext {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_platform::text_system(),
        Arc::new(crate::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::theme::install(cx);
        cx.set_global(Settings::default());
    });
    cx
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("SLOPSHOT_RENDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/render"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Opens `view` inside the same `Root` the app wraps every window in, draws it, and writes
/// `<name>.png`.
fn render<V: Render + 'static>(
    name: &str,
    content: Size<gpui::Pixels>,
    build: impl FnOnce(&mut Window, &mut gpui::App) -> Entity<V>,
) {
    render_in(app(), name, content, build);
}

fn render_in<V: Render + 'static>(
    mut cx: HeadlessAppContext,
    name: &str,
    content: Size<gpui::Pixels>,
    build: impl FnOnce(&mut Window, &mut gpui::App) -> Entity<V>,
) {
    let handle = cx
        .open_window(crate::preview::csd_size(f32::from(content.width), f32::from(content.height)), |window, cx| {
            let view = build(window, cx);
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap();
    let handle: AnyWindowHandle = handle.into();
    cx.run_until_parked();
    let image = cx
        .update_window(handle, |_, window, cx| {
            // Next-frame callbacks run before a frame is drawn: draw it as the app would.
            window.draw(cx).clear(cx);
            window.render_to_image()
        })
        .unwrap()
        .unwrap();
    let path = out_dir().join(format!("{name}.png"));
    image.save(&path).unwrap();
    eprintln!("rendered {}", path.display());
}

/// A light screenshot with a dark panel, so marks show against both.
fn screenshot() -> RgbaImage {
    RgbaImage::from_fn(960, 600, |x, y| match (x, y) {
        (_, 0..48) => Rgba([236, 238, 242, 255]),
        (600.., _) => Rgba([24, 26, 31, 255]),
        (40..560, 90..130) | (40..420, 150..170) | (40..480, 185..205) => Rgba([200, 206, 216, 255]),
        _ => Rgba([255, 255, 255, 255]),
    })
}

#[test]
#[ignore = "needs a GPU adapter (lavapipe will do); run with --ignored"]
fn render_editor() {
    render("editor", size(px(1040.), px(740.)), |window, cx| {
        cx.new(|cx| crate::editor::Editor::new(screenshot(), None, window, cx))
    });
}

fn settings(name: &str, tab: crate::prefs::Tab) {
    render(name, size(px(820.), px(560.)), move |window, cx| cx.new(|cx| crate::prefs::Prefs::new(tab, window, cx)));
}

#[test]
#[ignore = "needs a GPU adapter (lavapipe will do); run with --ignored"]
fn render_settings() {
    use crate::prefs::Tab;
    settings("settings-general", Tab::General);
    settings("settings-capture", Tab::Capture);
    settings("settings-privacy", Tab::Privacy);
    settings("settings-shortcuts", Tab::Shortcuts);
    settings("settings-about", Tab::About);
}

#[test]
#[ignore = "needs a GPU adapter (lavapipe will do); run with --ignored"]
fn render_history() {
    use crate::history::{Item, Kind};
    let now = chrono::Local::now().timestamp();
    let item = |id: &str, kind, age: i64, text: Option<&str>, subtitle: &str| Item {
        id: id.into(),
        kind,
        date: now - age,
        file: None,
        text: text.map(Into::into),
        subtitle: subtitle.into(),
        ocr_text: None,
    };
    let items = vec![
        item("shot", Kind::Image, 40, None, "1920 × 1080 · Area"),
        item("clip", Kind::Video, 900, None, "0:42 · 1280 × 720"),
        item("ocr", Kind::Text, 7200, Some("Lỗi kết nối máy chủ"), "19 chars"),
        item("pick", Kind::Color, 200_000, Some("#8B7CF6"), "#8B7CF6"),
    ];
    let thumbs = vec![
        ("shot".to_owned(), image::imageops::thumbnail(&screenshot(), 240, 150)),
        ("pick".to_owned(), RgbaImage::from_pixel(56, 40, Rgba([0x8b, 0x7c, 0xf6, 255]))),
    ];
    let mut cx = app();
    cx.update(|cx| crate::history::seed(items, thumbs, cx));
    render_in(cx, "history", size(px(440.), px(560.)), |window, cx| cx.new(|cx| crate::history::HistoryView::new(window, cx)));
    render("history-empty", size(px(440.), px(560.)), |window, cx| cx.new(|cx| crate::history::HistoryView::new(window, cx)));
}
