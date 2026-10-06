//! Floating chrome around a recording or scrolling capture, drawn by the X11 helper
//! (see card.rs for why): the dimmed focus layer, the recording bar
//! (RecordingBarController.swift) and the scrolling capture's bar, pill, preview and
//! spinner (ScrollCaptureController.swift).
//!
//! Every piece sits outside the captured rect, since a PipeWire feed, unlike
//! ScreenCaptureKit, can't leave windows out.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, Hsla, ObjectFit, Pixels, RenderImage,
    StyledImage as _, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, canvas, div,
    hsla, img, point, prelude::*, px, size,
};
use gpui_component::Icon;
use gpui_kit_assets::IconName;

use crate::board::tinted_button;
use crate::card::{Event, emit, find_display, usable_area};
use crate::preview::{hud, white};

/// Room around each HUD for its shadow.
const PAD: f32 = 16.;
const GAP: f32 = 12.;
const DIM: f32 = 0.35;
const SCROLL_BAR: (f32, f32) = (470., 44.);
const PREVIEW_W: f32 = 160.;

fn red() -> Hsla {
    gpui::rgb(0xFF453A).into()
}

fn yellow() -> Hsla {
    gpui::rgb(0xFFD60A).into()
}

#[derive(Default)]
struct Hud {
    windows: Vec<AnyWindowHandle>,
    scroll: Option<Entity<ScrollBar>>,
    preview: Option<Entity<Preview>>,
    /// Where "Please slow down" goes, and its window while shown.
    pill_at: Option<(Bounds<Pixels>, gpui::DisplayId)>,
    pill: Option<AnyWindowHandle>,
}

impl Global for Hud {}

fn hud_state(cx: &mut App) -> &mut Hud {
    if !cx.has_global::<Hud>() {
        cx.set_global(Hud::default());
    }
    cx.global_mut::<Hud>()
}

fn arr_bounds([x, y, w, h]: [f32; 4]) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

fn popup(bounds: Bounds<Pixels>, display: gpui::DisplayId) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(display),
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(crate::APP_ID.into()),
        ..Default::default()
    }
}

fn open<V: Render + 'static>(bounds: Bounds<Pixels>, display: gpui::DisplayId, cx: &mut App, view: impl FnOnce(&mut Window, &mut App) -> Entity<V>) -> Option<(AnyWindowHandle, Entity<V>)> {
    let mut made = None;
    let opened = cx.open_window(popup(bounds, display), |window, cx| {
        let v = view(window, cx);
        made = Some(v.clone());
        v
    });
    match (opened, made) {
        (Ok(handle), Some(v)) => {
            let handle: AnyWindowHandle = handle.into();
            hud_state(cx).windows.push(handle);
            Some((handle, v))
        }
        (Err(err), _) => {
            log::error!("opening capture chrome: {err:#}");
            None
        }
        _ => None,
    }
}

pub fn hide(cx: &mut App) {
    let hud = std::mem::take(hud_state(cx));
    for handle in hud.windows {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// Clicks and the wheel reach whatever is under the window: an empty X input shape.
fn click_through(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
    use x11rb::protocol::xproto::ClipOrdering;
    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(err) => return log::warn!("click-through: no window handle: {err}"),
    };
    let id = match handle.as_raw() {
        RawWindowHandle::Xcb(h) => h.window.get(),
        RawWindowHandle::Xlib(h) => h.window as u32,
        other => return log::warn!("click-through: not an X window: {other:?}"),
    };
    let (conn, _) = match x11rb::connect(None) {
        Ok(conn) => conn,
        Err(err) => return log::warn!("click-through: {err}"),
    };
    // Waiting for the reply: a flushed request was still lost when the connection dropped.
    match conn.shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, id, 0, 0, &[]).map(|c| c.check()) {
        Ok(Ok(())) => log::debug!("click-through: window {id:#x}"),
        Ok(Err(err)) => log::warn!("click-through {id:#x}: {err}"),
        Err(err) => log::warn!("click-through {id:#x}: {err}"),
    }
}

// ── Focus layer ──────────────────────────────────────────────────────────────

struct Focus {
    hole: Bounds<Pixels>,
    shaped: bool,
}

impl Render for Focus {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.shaped {
            self.shaped = true;
            click_through(window);
        }
        let hole = self.hole;
        canvas(|_, _, _| {}, move |bounds, _, window, _| {
            let dim = hsla(0., 0., 0., DIM);
            let o = bounds.origin;
            let (l, t, r, b) = (hole.left() + o.x, hole.top() + o.y, hole.right() + o.x, hole.bottom() + o.y);
            let strips = [
                Bounds::from_corners(bounds.origin, point(bounds.right(), t)),
                Bounds::from_corners(point(bounds.left(), b), bounds.bottom_right()),
                Bounds::from_corners(point(bounds.left(), t), point(l, b)),
                Bounds::from_corners(point(r, t), point(bounds.right(), b)),
            ];
            for strip in strips {
                if strip.size.width > px(0.) && strip.size.height > px(0.) {
                    window.paint_quad(gpui::fill(strip, dim));
                }
            }
            // 1.5 px outside the rect, so the outline never ends up in the video.
            let ring = Bounds::from_corners(point(l - px(2.5), t - px(2.5)), point(r + px(2.5), b + px(2.5)));
            window.paint_quad(gpui::outline(ring, red().opacity(0.9), gpui::BorderStyle::Solid).border_widths(px(2.)));
        })
        .size_full()
    }
}

pub fn focus(display: [f32; 4], rect: [f32; 4], cx: &mut App) {
    let Some(d) = find_display(display, cx) else { return };
    open(d.bounds(), d.id(), cx, |_, cx| cx.new(|_| Focus { hole: arr_bounds(rect), shaped: false }));
}

/// Where a bar of `w`×`h` goes: under `rect`, or over it when it would leave the
/// usable area, centred and kept on screen.
fn bar_bounds(display: Bounds<Pixels>, usable: Bounds<Pixels>, rect: Bounds<Pixels>, w: f32, h: f32) -> Bounds<Pixels> {
    let rect = rect + display.origin;
    let (w, h) = (px(w), px(h));
    let mut y = rect.bottom() + px(GAP);
    if y + h > usable.bottom() - px(6.) {
        y = rect.top() - px(GAP) - h;
    }
    y = y.clamp(usable.top() + px(6.), usable.bottom() - h - px(6.));
    let x = (rect.center().x - w / 2.).clamp(usable.left() + px(6.), usable.right() - w - px(6.));
    Bounds::new(point(x - px(PAD), y - px(PAD)), size(w + px(2. * PAD), h + px(2. * PAD)))
}

// ── Recording bar ────────────────────────────────────────────────────────────

struct RecordBar {
    seconds: u32,
    paused: bool,
    mic: bool,
    muted: bool,
    started: Instant,
}

impl RecordBar {
    fn new(mic: bool, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |bar, cx| {
                    if !bar.paused {
                        bar.seconds += 1;
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { seconds: 0, paused: false, mic, muted: false, started: Instant::now() }
    }

    const WIDTH: f32 = 252.;
    const MIC: f32 = 36.;
    const HEIGHT: f32 = 38.;
}

fn bar_icon(id: &'static str, icon: IconName, tip: &'static str, tint: Hsla, side: f32, on: impl Fn(&mut App) + 'static) -> impl IntoElement {
    tinted_button(id, tip, white(0.12), move |_, _, cx| on(cx))
        .size(px(side + 4.))
        .child(Icon::new(icon).size(px(side * 0.6)).text_color(tint))
}

impl Render for RecordBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The dot breathes while recording and holds still in yellow when paused.
        let dot = if self.paused {
            1.
        } else {
            window.request_animation_frame();
            let t = self.started.elapsed().as_secs_f32() / 0.8;
            0.3 + 0.7 * (0.5 + 0.5 * (t * std::f32::consts::PI).cos())
        };
        let weak = cx.entity().downgrade();
        let toggle_pause = {
            let weak = weak.clone();
            move |cx: &mut App| {
                let _ = weak.update(cx, |bar, cx| {
                    bar.paused = !bar.paused;
                    emit(Event::RecordPause(bar.paused));
                    cx.notify();
                });
            }
        };
        let restart = {
            let weak = weak.clone();
            move |cx: &mut App| {
                let _ = weak.update(cx, |bar, cx| {
                    bar.seconds = 0;
                    bar.paused = false;
                    emit(Event::RecordRestart);
                    cx.notify();
                });
            }
        };
        let mute = move |cx: &mut App| {
            let _ = weak.update(cx, |bar, cx| {
                bar.muted = !bar.muted;
                emit(Event::RecordMute(bar.muted));
                cx.notify();
            });
        };
        div().size_full().p(px(PAD)).child(
            hud(10.)
                .size_full()
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_color(gpui::white())
                .child(bar_icon("stop", IconName::Square, "Stop & save", red(), 22., |_| emit(Event::RecordStop)))
                .child(div().size(px(8.)).rounded_full().bg(if self.paused { yellow() } else { red() }).opacity(dot))
                .child(
                    div()
                        .w(px(50.))
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(format!("{}:{:02}", self.seconds / 60, self.seconds % 60)),
                )
                .child(div().w(px(1.)).h(px(20.)).bg(white(0.18)))
                .child(bar_icon(
                    "pause",
                    if self.paused { IconName::Play } else { IconName::Pause },
                    if self.paused { "Resume" } else { "Pause" },
                    gpui::white(),
                    24.,
                    toggle_pause,
                ))
                .child(bar_icon("restart", IconName::RotateCcw, "Restart", gpui::white(), 24., restart))
                .child(bar_icon("discard", IconName::Trash, "Discard (don't save)", gpui::white(), 24., |_| emit(Event::RecordDiscard)))
                .when(self.mic, |d| {
                    d.child(bar_icon(
                        "mic",
                        if self.muted { IconName::MicOff } else { IconName::Mic },
                        if self.muted { "Unmute microphone" } else { "Mute microphone" },
                        if self.muted { red() } else { gpui::white() },
                        24.,
                        mute,
                    ))
                })
                .child(Icon::new(IconName::GripVertical).size(px(12.)).text_color(white(0.35))),
        )
    }
}

pub fn record_bar(display: [f32; 4], rect: [f32; 4], mic: bool, cx: &mut App) {
    let Some(d) = find_display(display, cx) else { return };
    let w = RecordBar::WIDTH + if mic { RecordBar::MIC } else { 0. };
    let bounds = bar_bounds(d.bounds(), usable_area(d.bounds(), cx), arr_bounds(rect), w, RecordBar::HEIGHT);
    open(bounds, d.id(), cx, |_, cx| cx.new(|cx| RecordBar::new(mic, cx)));
}

// ── Scrolling capture ────────────────────────────────────────────────────────

struct ScrollBar {
    status: String,
    slow: bool,
    auto: bool,
}

fn text_button(id: &'static str, label: &'static str, primary: bool, on: impl Fn(&mut App) + 'static) -> impl IntoElement {
    div()
        .id(id)
        .px(px(12.))
        .h(px(26.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .bg(if primary { crate::preview::accent() } else { white(0.14) })
        .hover(move |d| d.bg(if primary { crate::preview::accent().opacity(0.85) } else { white(0.22) }))
        .on_click(move |_, _, cx| on(cx))
        .child(label)
}

impl Render for ScrollBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let auto = self.auto;
        div().size_full().p(px(PAD)).child(
            div()
                .size_full()
                .px(px(14.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(11.))
                .bg(hsla(0., 0., 0.13, 0.96))
                .text_color(gpui::white())
                .child(
                    div()
                        .w(px(130.))
                        .truncate()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .when(self.slow, |d| d.text_color(gpui::rgb(0xFF9F0A)))
                        .child(if self.slow { "Please slow down".to_owned() } else { self.status.clone() }),
                )
                .child(
                    div()
                        .id("auto")
                        .px(px(10.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .bg(if auto { crate::preview::accent() } else { white(0.14) })
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |bar, cx| {
                                bar.auto = !bar.auto;
                                emit(Event::ScrollAuto(bar.auto));
                                cx.notify();
                            });
                        })
                        .child("Auto-scroll"),
                )
                .child(div().flex_1())
                .child(text_button("cancel", "Cancel", false, |_| emit(Event::ScrollCancel)))
                .child(text_button("done", "Done", true, |_| emit(Event::ScrollDone))),
        )
    }
}

struct Pill;

impl Render for Pill {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(PAD)).child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(hsla(0., 0., 0.1, 0.92))
                .text_color(gpui::white())
                .text_size(px(14.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Please slow down"),
        )
    }
}

struct Preview {
    image: Option<(Arc<RenderImage>, f32)>,
    max_h: f32,
}

impl Render for Preview {
    /// Grows upward from a fixed bottom edge as the image gets longer.
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().justify_end().children(self.image.clone().map(|(image, aspect)| {
            let h = ((PREVIEW_W - 12.) * aspect + 12.).clamp(40., self.max_h);
            div()
                .w_full()
                .h(px(h))
                .p(px(6.))
                .rounded(px(12.))
                .bg(hsla(0., 0., 0.12, 0.96))
                .flex()
                .flex_col()
                .justify_end()
                .child(img(image).w_full().h_full().object_fit(ObjectFit::Contain))
        }))
    }
}

pub fn scroll_bar(display: [f32; 4], rect: [f32; 4], cx: &mut App) {
    let Some(d) = find_display(display, cx) else { return };
    let (db, usable, r) = (d.bounds(), usable_area(d.bounds(), cx), arr_bounds(rect));
    let bounds = bar_bounds(db, usable, r, SCROLL_BAR.0, SCROLL_BAR.1);
    if let Some((_, bar)) = open(bounds, d.id(), cx, |_, cx| cx.new(|_| ScrollBar { status: "Scroll through the area…".into(), slow: false, auto: false })) {
        hud_state(cx).scroll = Some(bar);
    }
    // The pill sits just above the rect: inside it, it would be stitched into the image.
    let (pw, ph) = (190., 42.);
    let abs = r + db.origin;
    if abs.top() - px(GAP + ph) > usable.top() {
        let at = Bounds::new(point(abs.center().x - px(pw / 2. + PAD), abs.top() - px(GAP + ph + PAD)), size(px(pw + 2. * PAD), px(ph + 2. * PAD)));
        hud_state(cx).pill_at = Some((at, d.id()));
    }
    // The live preview takes the right edge, or the left one when the rect is there.
    let max_h = (f32::from(usable.size.height) * 0.72).min(560.);
    let margin = 16.;
    let bottom = usable.bottom() - px(margin);
    for x in [usable.right() - px(PREVIEW_W + margin), usable.left() + px(margin)] {
        let column = Bounds::new(point(x, bottom - px(max_h)), size(px(PREVIEW_W), px(max_h)));
        if !column.intersects(&abs) {
            if let Some((_, view)) = open(column, d.id(), cx, |_, cx| cx.new(|_| Preview { image: None, max_h })) {
                hud_state(cx).preview = Some(view);
            }
            break;
        }
    }
}

pub fn scroll_status(text: String, slow: bool, auto: bool, preview: Option<PathBuf>, cx: &mut App) {
    let hud = hud_state(cx);
    let (bar, view, pill_at, pill) = (hud.scroll.clone(), hud.preview.clone(), hud.pill_at, hud.pill);
    match (slow, pill_at, pill) {
        (true, Some((at, display)), None) => {
            if let Some((handle, _)) = open(at, display, cx, |_, cx| cx.new(|_| Pill)) {
                hud_state(cx).pill = Some(handle);
            }
        }
        (false, _, Some(handle)) => {
            hud_state(cx).pill = None;
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        _ => {}
    }
    if let Some(bar) = bar {
        bar.update(cx, |bar, cx| {
            bar.status = text;
            bar.slow = slow;
            bar.auto = auto;
            cx.notify();
        });
    }
    let Some(path) = preview else { return };
    let image = image::open(&path);
    let _ = std::fs::remove_file(&path);
    if let (Ok(image), Some(view)) = (image, view) {
        let aspect = image.height() as f32 / image.width().max(1) as f32;
        let image = crate::preview::to_render_image(&image.to_rgba8());
        view.update(cx, |p, cx| {
            p.image = Some((image, aspect));
            cx.notify();
        });
    }
}

struct Processing(Instant);

impl Render for Processing {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.request_animation_frame();
        let turn = self.0.elapsed().as_secs_f32().fract();
        div().size_full().p(px(PAD)).child(
            div()
                .size_full()
                .rounded(px(14.))
                .bg(hsla(0., 0., 0.12, 0.96))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .text_color(gpui::white())
                .child(Icon::new(IconName::LoaderCircle).size(px(26.)).text_color(gpui::white()).rotate(gpui::radians(turn * std::f32::consts::TAU)))
                .child(div().text_size(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child("Building image…")),
        )
    }
}

pub fn processing(display: [f32; 4], cx: &mut App) {
    hide(cx);
    let Some(d) = find_display(display, cx) else { return };
    let (w, h) = (160. + 2. * PAD, 96. + 2. * PAD);
    let c = d.bounds().center();
    open(Bounds::new(point(c.x - px(w / 2.), c.y - px(h / 2.)), size(px(w), px(h))), d.id(), cx, |_, cx| cx.new(|_| Processing(Instant::now())));
}
