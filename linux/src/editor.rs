//! The editor window (EditorWindowController.swift + EditorView.windowBody).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Global,
    KeyDownEvent, MouseButton, Pixels, Size, Subscription, Window, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowOptions, canvas, div, prelude::*, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Sizable as _, window_border};
use gpui_kit_assets::IconName;
use image::RgbaImage;

use crate::board::{self, Board};
use crate::preview::white;
use crate::settings::Settings;
use crate::{alert, flow, output};

const TOP_BAR: f32 = 52.;
const BOTTOM_BAR: f32 = 44.;
const PAD: f32 = 28.;

struct EditorWindow {
    handle: AnyWindowHandle,
    editor: Entity<Editor>,
}

impl Global for EditorWindow {}

/// Opening an editor closes the previous one.
pub fn open(image: RgbaImage, source: Option<PathBuf>, cx: &mut App) {
    close(cx);
    let bounds = Bounds::centered(None, crate::preview::csd_size(1040., 740.), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("SlopShot — Editor".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_decorations: Some(WindowDecorations::Client),
        // The client-side shadow around the frame needs an alpha channel.
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(crate::APP_ID.into()),
        window_min_size: Some(crate::preview::csd_size(760., 500.)),
        focus: true,
        ..Default::default()
    };
    let mut created = None;
    let result = cx.open_window(options, |window, cx| {
        let editor = cx.new(|cx| Editor::new(image, source, window, cx));
        created = Some(editor.clone());
        cx.new(|cx| gpui_component::Root::new(editor, window, cx))
    });
    match result {
        Ok(handle) => {
            if let Some(editor) = created {
                cx.set_global(EditorWindow { handle: handle.into(), editor });
            }
        }
        Err(err) => alert::error("Couldn't open the editor", &format!("{err:#}"), cx),
    }
}

pub fn open_file(path: &Path, cx: &mut App) {
    match image::open(path) {
        Ok(img) => open(img.to_rgba8(), Some(path.to_owned()), cx),
        Err(err) => alert::error("Couldn't open the image", &format!("{}: {err}", path.display()), cx),
    }
}

pub fn close(cx: &mut App) {
    if let Some(w) = cx.try_global::<EditorWindow>() {
        let handle = w.handle;
        cx.remove_global::<EditorWindow>();
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// The editor is open with drawings nobody copied or saved.
pub fn has_unsaved_annotations(cx: &App) -> bool {
    cx.try_global::<EditorWindow>().is_some_and(|w| {
        let alive = cx.windows().iter().any(|h| h.window_id() == w.handle.window_id());
        alive && w.editor.read(cx).board.read(cx).dirty
    })
}

struct Editor {
    board: Entity<Board>,
    source: Option<PathBuf>,
    /// 1 = fit to the window.
    zoom: f32,
    /// The canvas area as laid out last frame; the fit is computed from it.
    area: Rc<Cell<Size<Pixels>>>,
    focus: FocusHandle,
    _board: Subscription,
}

impl Editor {
    fn new(image: RgbaImage, source: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let board = cx.new(|cx| Board::new(image, 1., focus.clone(), cx));
        let sub = cx.observe(&board, |_, _, cx| cx.notify());
        let area = Rc::new(Cell::new(size(px(1040.), px(740. - TOP_BAR - BOTTOM_BAR))));
        Self { board, source, zoom: 1., area, focus, _board: sub }
    }

    fn base_name(&self) -> String {
        self.source
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "SlopShot edited".into())
    }

    fn set_status(&self, status: String, cx: &mut Context<Self>) {
        self.board.update(cx, |b, cx| {
            b.status = status.into();
            cx.notify();
        });
    }

    fn flattened(&self, window: &mut Window, cx: &mut Context<Self>) -> RgbaImage {
        self.board.update(cx, |b, cx| b.flattened(window, cx))
    }

    fn animated(&self, cx: &App) -> bool {
        self.board.read(cx).has_animation()
    }

    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.read(cx).is_exporting() {
            return;
        }
        if self.animated(cx) {
            let name = self.base_name();
            self.board.update(cx, |b, cx| b.copy_gif(&name, window, cx, |_| {}));
            return;
        }
        let image = self.flattened(window, cx);
        output::copy_image(&image, None, cx);
        self.board.update(cx, |b, cx| {
            b.status = "Copied".into();
            b.mark_clean();
            cx.notify();
        });
    }

    fn done(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.read(cx).is_exporting() {
            return;
        }
        // The GIF takes seconds: close once it is on the clipboard, not before.
        if self.animated(cx) {
            let name = self.base_name();
            self.board.update(cx, |b, cx| b.copy_gif(&name, window, cx, close));
            return;
        }
        self.copy(window, cx);
        close_window(window, cx);
    }

    fn share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.read(cx).is_exporting() {
            return;
        }
        if self.animated(cx) {
            let path = output::temp_dir().join(format!("{}.gif", self.base_name()));
            self.board.update(cx, |b, cx| {
                b.build_gif(window, cx, move |b, result, cx| {
                    match result.and_then(|(gif, _)| Ok(output::write_file(&gif, &path).map(|_| gif.len())?)) {
                        Ok(len) => {
                            b.status = format!("Shared GIF ({})", output::byte_size(len as u64)).into();
                            flow::share_file(path, cx);
                        }
                        Err(err) => {
                            log::error!("sharing the GIF: {err:#}");
                            b.status = "GIF export failed".into();
                        }
                    }
                })
            });
            return;
        }
        let image = self.flattened(window, cx);
        let path = output::unique_path(&output::temp_dir(), &self.base_name(), "png");
        match output::encode_png(&image).and_then(|png| Ok(output::write_file(&png, &path)?)) {
            Ok(()) => flow::share_file(path, cx),
            Err(err) => self.set_status(format!("Export failed: {err:#}"), cx),
        }
    }

    fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.read(cx).is_exporting() {
            return;
        }
        let animated = self.animated(cx);
        let image = self.flattened(window, cx);
        let dir = self
            .source
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|d| *d != output::temp_dir())
            .map(Path::to_owned)
            .unwrap_or_else(|| Settings::get(cx).save_folder.clone());
        let _ = std::fs::create_dir_all(&dir);
        // With a moving layer GIF comes first; other formats keep the first frame.
        let name = format!("{}.{}", self.base_name(), if animated { "gif" } else { "png" });
        let receiver = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else { return };
            if animated && output::format_for(&path) == crate::settings::ImageFormat::Gif {
                let _ = this.update_in(cx, |this, window, cx| this.save_gif(path, window, cx));
                return;
            }
            let format = output::format_for(&path);
            let result = crate::raster::encode(&image, format).and_then(|bytes| {
                output::write_file(&bytes, &path)?;
                Ok(bytes.len())
            });
            let _ = this.update(cx, |this, cx| {
                let status = match result {
                    Ok(len) => {
                        this.board.update(cx, |b, _| b.mark_clean());
                        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        format!("Saved {file} ({})", output::byte_size(len as u64))
                    }
                    Err(err) => {
                        log::error!("saving {}: {err:#}", path.display());
                        "Save failed".into()
                    }
                };
                this.set_status(status, cx);
            });
        })
        .detach();
    }

    fn save_gif(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.board.update(cx, |b, cx| {
            b.build_gif(window, cx, move |b, result, _| {
                match result.and_then(|(gif, _)| Ok(output::write_file(&gif, &path).map(|_| gif.len())?)) {
                    Ok(len) => {
                        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        b.status = format!("Saved {file} ({})", output::byte_size(len as u64)).into();
                        b.mark_clean();
                    }
                    Err(err) => {
                        log::error!("saving {}: {err:#}", path.display());
                        b.status = "Export failed".into();
                    }
                }
            })
        });
    }

    fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.zoom = (self.zoom * 1.25).min(16.);
        cx.notify();
    }

    fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.zoom = (self.zoom / 1.25).max(0.25);
        cx.notify();
    }

    fn zoom_fit(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.;
        cx.notify();
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.update(cx, |b, cx| b.key_down(e, cx)) {
            cx.stop_propagation();
            return;
        }
        let k = &e.keystroke;
        let command = k.modifiers.control || k.modifiers.platform;
        match k.key.as_str() {
            "escape" if !command => {
                cx.stop_propagation();
                if self.board.update(cx, |b, cx| b.escape(window, cx)) {
                    return;
                }
                let (n, armed) = {
                    let b = self.board.read(cx);
                    (b.annotations.len(), b.esc_armed)
                };
                if n > 0 && !armed {
                    self.board.update(cx, |b, cx| {
                        b.esc_armed = true;
                        b.status = format!("Press Esc again to discard {n} annotation{} and close", if n == 1 { "" } else { "s" }).into();
                        cx.notify();
                    });
                } else {
                    close_window(window, cx);
                }
            }
            "enter" if !command && !self.board.read(cx).is_editing_text() => self.done(window, cx),
            "=" | "+" if command => self.zoom_in(cx),
            "-" | "_" if command => self.zoom_out(cx),
            "0" if command => self.zoom_fit(cx),
            _ => {}
        }
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let actions = board::pill()
            .child(board::redact_button(&self.board, cx))
            .child(board::divider())
            .child(board::undo_button(&self.board, cx))
            .child(board::clear_button(&self.board, cx))
            .child(board::divider())
            .child(board::transform_menu(&self.board, cx));
        div()
            .h(px(TOP_BAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .bg(gpui::hsla(0., 0., 0.13, 1.))
            .border_b_1()
            .border_color(white(0.08))
            .child(board::tools_pill(&self.board, cx))
            .child(board::style_pill(&self.board, cx))
            .child(actions)
            // The empty part of the bar moves the window, like a title bar.
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .min_w(px(8.))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(
                Button::new("save-as")
                    .label("Save as…")
                    .large()
                    .on_click(cx.listener(|this, _, window, cx| this.save_as(window, cx))),
            )
            .child(
                Button::new("done")
                    .label("Done")
                    .primary()
                    .large()
                    .on_click(cx.listener(|this, _, window, cx| this.done(window, cx))),
            )
    }

    fn bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let gray = gpui::hsla(0., 0., 0.85, 1.);
        let zoom_button = |id: &'static str, name: IconName, f: fn(&mut Self, &mut Context<Self>)| {
            board::hud_button(id, "", cx.listener(move |this, _, _, cx| f(this, cx)))
                .size(px(24.))
                .child(board::icon(name, 11., gray))
        };
        let zoom = div()
            .flex()
            .items_center()
            .gap(px(1.))
            .px(px(3.))
            .py(px(2.))
            .rounded(px(5.))
            .bg(white(0.08))
            .child(zoom_button("zoom-out", IconName::Minus, Self::zoom_out))
            .child(
                board::hud_button("zoom-fit", "Fit to window (Ctrl+0)", cx.listener(|this, _, _, cx| this.zoom_fit(cx)))
                    .w(px(42.))
                    .h(px(24.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gray)
                    .child(format!("{}%", (self.zoom * 100.).round() as i32)),
            )
            .child(zoom_button("zoom-in", IconName::Plus, Self::zoom_in));
        let icon_button = |id: &'static str, name: IconName, tip: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            board::hud_button(id, tip, cx.listener(move |this, _, window, cx| f(this, window, cx)))
                .w(px(34.))
                .h(px(30.))
                .bg(white(0.08))
                .child(board::icon(name, 13., gray))
        };
        div()
            .h(px(BOTTOM_BAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(16.))
            .bg(gpui::hsla(0., 0., 0.13, 1.))
            .border_t_1()
            .border_color(white(0.08))
            .child(
                div()
                    .flex_1()
                    .text_size(px(11.))
                    .text_color(gpui::hsla(0., 0., 0.55, 1.))
                    .child(self.board.read(cx).status_line()),
            )
            .child(zoom)
            .child(icon_button("share", IconName::Share, "Share", Self::share))
            .child(icon_button("copy", IconName::Copy, "Copy", Self::copy))
    }
}

fn close_window(window: &mut Window, cx: &mut App) {
    cx.remove_global::<EditorWindow>();
    window.remove_window();
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let area_size = self.area.get();
        let (avail_w, avail_h) = (f32::from(area_size.width), f32::from(area_size.height));
        let (iw, ih) = {
            let image = &self.board.read(cx).image;
            (image.width().max(1) as f32, image.height().max(1) as f32)
        };
        let factor = ((avail_w - PAD * 2.).max(50.) / iw).min((avail_h - PAD * 2.).max(50.) / ih);
        let (w, h) = (iw * factor * self.zoom, ih * factor * self.zoom);

        let measured = self.area.clone();
        let measure = canvas(
            move |bounds, window, _| {
                if measured.replace(bounds.size) != bounds.size {
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let area = div()
            .id("board-area")
            .size_full()
            .overflow_scroll()
            .bg(gpui::hsla(0., 0., 0.11, 1.))
            .child(
                div()
                    .min_w(px(avail_w))
                    .min_h(px(avail_h))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(PAD))
                    .child(
                        div()
                            .flex_none()
                            .w(px(w))
                            .h(px(h))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.5),
                                offset: gpui::point(px(0.), px(6.)),
                                blur_radius: px(16.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .child(self.board.clone()),
                    ),
            );

        window_border().child(
            div()
                .id("editor")
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key_down))
                .size_full()
                .flex()
                .flex_col()
                .bg(gpui::hsla(0., 0., 0.11, 1.))
                .text_color(gpui::white())
                .child(self.top_bar(cx))
                .child(div().relative().flex_1().min_h_0().child(measure).child(area))
                .child(self.bottom_bar(cx)),
        )
    }
}
