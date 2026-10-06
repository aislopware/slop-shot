//! The frozen-screen overlay: Capture Area with its inline editor
//! (RegionSelectionController.swift, OverlayChrome.swift) and Pick Color
//! (ColorPickerController.swift). One window per display shows that display's part
//! of the capture taken the instant the command arrived.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, CursorStyle, Entity,
    FocusHandle, Hsla, KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, RenderImage, Size,
    Window, WindowBackgroundAppearance, WindowKind, WindowOptions, canvas, div, fill, point,
    prelude::*, px, size,
};
use gpui_kit_assets::IconName;
use image::RgbaImage;

use crate::board::{self, Board};
use crate::preview::{accent, black, hud, to_render_image, white};
use crate::settings::Settings;
use crate::snap::{RectF, SnapEngine};
use crate::{alert, flow, output};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Area,
    Color,
}

/// What an area selection is for; all but Capture always go through the adjust step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purpose {
    Capture,
    Text,
    Scroll,
    Record,
}

impl Purpose {
    fn badge(self) -> (IconName, &'static str) {
        match self {
            Purpose::Capture => (IconName::Scan, "Capture Area"),
            Purpose::Text => (IconName::ScanText, "Capture Text"),
            Purpose::Scroll => (IconName::ChevronsUpDown, "Scrolling Capture"),
            Purpose::Record => (IconName::CircleDot, "Record Area"),
        }
    }

    fn confirm(self) -> (IconName, &'static str) {
        match self {
            Purpose::Capture => (IconName::Camera, "Capture"),
            Purpose::Text => (IconName::ScanText, "Read Text"),
            Purpose::Scroll => (IconName::ChevronsUpDown, "Start Scrolling Capture"),
            Purpose::Record => (IconName::CircleDot, "Start Recording"),
        }
    }
}

const DRAG_SLOP: f32 = 4.;
const MIN_SIZE: f32 = 5.;
const SNAP_RADIUS: f32 = 9.;
const EDGE_GRAB: f32 = 8.;
const CHROME_FADE: f32 = 0.12;
const HINT_FADE: f32 = 0.14;

type P = (f32, f32);

#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    fn from_points(a: P, b: P) -> Self {
        R { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs(), h: (a.1 - b.1).abs() }
    }
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
    fn contains(&self, p: P) -> bool {
        p.0 >= self.x && p.0 <= self.right() && p.1 >= self.y && p.1 <= self.bottom()
    }
    fn inset(&self, d: f32) -> R {
        R { x: self.x + d, y: self.y + d, w: self.w - 2. * d, h: self.h - 2. * d }
    }
    fn intersect(&self, o: &R) -> R {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        R { x, y, w: (self.right().min(o.right()) - x).max(0.), h: (self.bottom().min(o.bottom()) - y).max(0.) }
    }
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(point(px(self.x), px(self.y)), size(px(self.w), px(self.h)))
    }
    fn big_enough(&self) -> bool {
        self.w >= MIN_SIZE && self.h >= MIN_SIZE
    }
}

impl From<RectF> for R {
    fn from(r: RectF) -> Self {
        R { x: r.x, y: r.y, w: r.w, h: r.h }
    }
}

/// An opacity easing towards on or off since the last change.
#[derive(Clone, Copy)]
struct Fade {
    on: bool,
    since: Instant,
    secs: f32,
}

impl Fade {
    fn new(on: bool, secs: f32) -> Self {
        Fade { on, since: Instant::now() - Duration::from_secs(1), secs }
    }
    fn set(&mut self, on: bool) {
        if self.on != on {
            self.on = on;
            self.since = Instant::now();
        }
    }
    fn progress(&self) -> f32 {
        (self.since.elapsed().as_secs_f32() / self.secs).min(1.)
    }
    fn value(&self, low: f32) -> f32 {
        let t = ease_out(self.progress());
        if self.on { low + (1. - low) * t } else { 1. - (1. - low) * t }
    }
    fn animating(&self) -> bool {
        self.progress() < 1.
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(2)
}

struct Display {
    /// Global logical bounds.
    bounds: Bounds<Pixels>,
    texture: Arc<RenderImage>,
    pixels: Arc<RgbaImage>,
    engine: Option<Arc<SnapEngine>>,
    window: Option<AnyWindowHandle>,
    focus: Option<FocusHandle>,
}

impl Display {
    fn size(&self) -> (f32, f32) {
        (f32::from(self.bounds.size.width), f32::from(self.bounds.size.height))
    }
    fn full(&self) -> R {
        let (w, h) = self.size();
        R { x: 0., y: 0., w, h }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Grab {
    New,
    Move,
    Resize { l: bool, t: bool, r: bool, b: bool },
}

struct Drag {
    display: usize,
    grab: Grab,
    start: P,
    last: P,
    anchor: P,
    base: R,
    moved: bool,
    /// The selection a click outside it replaced, restored if the click selects nothing.
    previous: Option<R>,
}

struct Inline {
    display: usize,
    rect: R,
    board: Entity<Board>,
    bar: Rc<Cell<Option<Size<Pixels>>>>,
}

struct Session {
    mode: Mode,
    purpose: Purpose,
    /// Image pixels per logical point.
    scale: f32,
    displays: Vec<Display>,
    pointer: Option<(usize, P)>,
    selection: Option<(usize, R)>,
    hover: Option<(usize, R)>,
    /// Snapped left, top, right, bottom edges of the selection.
    snapped: [bool; 4],
    drag: Option<Drag>,
    adjust: bool,
    space: bool,
    alt: bool,
    shift: bool,
    hint: Fade,
    chrome: Fade,
    badge_dim: Fade,
    bar: Fade,
    closing: Option<Instant>,
    inline: Option<Inline>,
    nudge: (i32, i32),
    done: bool,
}

pub fn open(mode: Mode, image: RgbaImage, cx: &mut App) {
    open_for(mode, Purpose::Capture, image, cx);
}

pub fn open_for(mode: Mode, purpose: Purpose, image: RgbaImage, cx: &mut App) {
    let displays = cx.displays();
    if displays.is_empty() {
        cx.remove_global::<flow::Busy>();
        alert::error("Capture failed", "SlopShot could not find a display.", cx);
        return;
    }
    let desktop = displays.iter().map(|d| d.bounds()).reduce(|a, b| a.union(&b)).unwrap();
    // The portal image spans every display; HiDPI only changes this ratio.
    let scale = image.width() as f32 / f32::from(desktop.size.width);
    let snap = mode == Mode::Area && Settings::get(cx).snap;

    let mut states = Vec::new();
    for display in &displays {
        let b = display.bounds();
        let x = (f32::from(b.origin.x - desktop.origin.x) * scale).round() as u32;
        let y = (f32::from(b.origin.y - desktop.origin.y) * scale).round() as u32;
        let w = ((f32::from(b.size.width) * scale).round() as u32).min(image.width().saturating_sub(x));
        let h = ((f32::from(b.size.height) * scale).round() as u32).min(image.height().saturating_sub(y));
        let pixels = Arc::new(image::imageops::crop_imm(&image, x, y, w, h).to_image());
        states.push(Display {
            bounds: b,
            texture: to_render_image(&pixels),
            pixels,
            engine: None,
            window: None,
            focus: None,
        });
    }
    let session = cx.new(|_| Session {
        mode,
        purpose,
        scale,
        displays: states,
        pointer: None,
        selection: None,
        hover: None,
        snapped: [false; 4],
        drag: None,
        adjust: false,
        space: false,
        alt: false,
        shift: false,
        hint: Fade::new(true, HINT_FADE),
        chrome: Fade { on: true, since: Instant::now(), secs: CHROME_FADE },
        badge_dim: Fade::new(false, CHROME_FADE),
        bar: Fade::new(false, HINT_FADE),
        closing: None,
        inline: None,
        nudge: (0, 0),
        done: false,
    });

    for (i, display) in displays.iter().enumerate() {
        let options = WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Fullscreen(display.bounds())),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            display_id: Some(display.id()),
            window_background: WindowBackgroundAppearance::Transparent,
            app_id: Some(crate::APP_ID.into()),
            ..Default::default()
        };
        let model = session.clone();
        match cx.open_window(options, |window, cx| cx.new(|cx| OverlayView::new(model, i, window, cx))) {
            Ok(handle) => {
                session.update(cx, |s, _| s.displays[i].window = Some(handle.into()));
                log::debug!("overlay opened ({mode:?}, display {i})");
            }
            Err(err) => log::error!("opening the overlay window: {err:#}"),
        }

        if snap {
            let pixels = session.read(cx).displays[i].pixels.clone();
            let task = cx.background_spawn(async move { SnapEngine::build(&pixels, scale) });
            let session = session.clone();
            cx.spawn(async move |cx| {
                if let Some(engine) = task.await {
                    cx.update(|cx| session.update(cx, |s, _| s.displays[i].engine = Some(Arc::new(engine))));
                }
            })
            .detach();
        }
    }
}

/// Capture Fullscreen: the primary display, flashed, then the post-capture flow.
/// macOS takes the display under the pointer; Wayland doesn't tell where that is.
pub fn capture_fullscreen(image: RgbaImage, cx: &mut App) {
    let displays = cx.displays();
    let Some(desktop) = displays.iter().map(|d| d.bounds()).reduce(|a, b| a.union(&b)) else {
        alert::error("Capture failed", "SlopShot could not find a display.", cx);
        return;
    };
    let target = cx.primary_display().map(|d| d.bounds()).unwrap_or(desktop);
    let scale = image.width() as f32 / f32::from(desktop.size.width);
    let x = (f32::from(target.origin.x - desktop.origin.x) * scale).round() as u32;
    let y = (f32::from(target.origin.y - desktop.origin.y) * scale).round() as u32;
    let w = ((f32::from(target.size.width) * scale).round() as u32).min(image.width().saturating_sub(x));
    let h = ((f32::from(target.size.height) * scale).round() as u32).min(image.height().saturating_sub(y));
    let crop = image::imageops::crop_imm(&image, x, y, w, h).to_image();
    flow::flash(target, Bounds::new(Point::default(), target.size), cx);
    flow::finish_image(crop, target, false, cx);
}

impl Session {
    fn inline_edit(&self, cx: &App) -> bool {
        self.purpose == Purpose::Capture && Settings::get(cx).edit_after_select
    }

    fn display_rect_px(&self, display: usize, r: R) -> (u32, u32, u32, u32) {
        let pixels = &self.displays[display].pixels;
        let (iw, ih) = (pixels.width() as f32, pixels.height() as f32);
        let x0 = (r.x * self.scale).round().clamp(0., iw);
        let y0 = (r.y * self.scale).round().clamp(0., ih);
        let x1 = (r.right() * self.scale).round().clamp(0., iw);
        let y1 = (r.bottom() * self.scale).round().clamp(0., ih);
        (x0 as u32, y0 as u32, (x1 - x0).max(1.) as u32, (y1 - y0).max(1.) as u32)
    }

    fn crop(&self, display: usize, r: R) -> RgbaImage {
        let (x, y, w, h) = self.display_rect_px(display, r);
        image::imageops::crop_imm(self.displays[display].pixels.as_ref(), x, y, w, h).to_image()
    }

    /// The pixel under a display-local point, nudged by the arrow keys.
    fn sample_point(&self, display: usize, p: P) -> (u32, u32) {
        let pixels = &self.displays[display].pixels;
        let x = ((p.0 * self.scale).floor() as i32 + self.nudge.0).clamp(0, pixels.width() as i32 - 1);
        let y = ((p.1 * self.scale).floor() as i32 + self.nudge.1).clamp(0, pixels.height() as i32 - 1);
        (x as u32, y as u32)
    }

    fn sample(&self, display: usize, p: P) -> ([u8; 3], (u32, u32)) {
        let (x, y) = self.sample_point(display, p);
        let [r, g, b, _] = self.displays[display].pixels.get_pixel(x, y).0;
        ([r, g, b], (x, y))
    }

    fn snap_on(&self, cx: &App) -> bool {
        Settings::get(cx).snap && !self.alt
    }

    fn update_hover(&mut self, cx: &App) {
        self.hover = None;
        if self.mode != Mode::Area || self.selection.is_some() || self.drag.is_some() || self.inline.is_some() || !self.snap_on(cx) {
            return;
        }
        let Some((d, p)) = self.pointer else { return };
        let display = &self.displays[d];
        let Some(engine) = &display.engine else { return };
        let full = display.full();
        let limit = RectF { x: full.x, y: full.y, w: full.w, h: full.h };
        if let Some(found) = engine.element(p, limit) {
            self.hover = Some((d, found.into()));
        }
    }

    /// Snaps the moving edges to pixel lines within 9 pt (SnapEngine.swift).
    fn snap_edges(&mut self, display: usize, mut r: R, edges: [bool; 4], cx: &App) -> R {
        self.snapped = [false; 4];
        if !self.snap_on(cx) || self.shift || r.w < 3. || r.h < 3. {
            return r;
        }
        let Some(engine) = self.displays[display].engine.clone() else { return r };
        let (mut l, mut t, mut rt, mut b) = (r.x, r.y, r.right(), r.bottom());
        if edges[0]
            && let Some(x) = engine.snap_x(l, t, b, SNAP_RADIUS)
        {
            l = x;
            self.snapped[0] = true;
        }
        if edges[2]
            && let Some(x) = engine.snap_x(rt, t, b, SNAP_RADIUS)
        {
            rt = x;
            self.snapped[2] = true;
        }
        if edges[1]
            && let Some(y) = engine.snap_y(t, l, rt, SNAP_RADIUS)
        {
            t = y;
            self.snapped[1] = true;
        }
        if edges[3]
            && let Some(y) = engine.snap_y(b, l, rt, SNAP_RADIUS)
        {
            b = y;
            self.snapped[3] = true;
        }
        if rt > l && b > t {
            r = R { x: l, y: t, w: rt - l, h: b - t };
        }
        r
    }

    fn grab_at(&self, r: R, p: P) -> Option<Grab> {
        let near = |a: f32, b: f32| (a - b).abs() <= EDGE_GRAB;
        let in_x = p.0 >= r.x - EDGE_GRAB && p.0 <= r.right() + EDGE_GRAB;
        let in_y = p.1 >= r.y - EDGE_GRAB && p.1 <= r.bottom() + EDGE_GRAB;
        let (l, rt) = (in_y && near(p.0, r.x), in_y && near(p.0, r.right()));
        let (t, b) = (in_x && near(p.1, r.y), in_x && near(p.1, r.bottom()));
        if l || rt || t || b {
            return Some(Grab::Resize { l, t, r: rt, b });
        }
        r.contains(p).then_some(Grab::Move)
    }
}

struct OverlayView {
    session: Entity<Session>,
    index: usize,
    focus: FocusHandle,
}

impl OverlayView {
    fn new(session: Entity<Session>, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        session.update(cx, |s, _| s.displays[index].focus = Some(focus.clone()));
        Self { session, index, focus }
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let p = local(e.position);
        let i = self.index;
        let mode = self.session.read(cx).mode;
        if mode == Mode::Color {
            self.session.update(cx, |s, cx| {
                s.pointer = Some((i, p));
                s.nudge = (0, 0);
                cx.notify();
            });
            return;
        }
        let edit_after = self.session.read(cx).inline_edit(cx);
        self.session.update(cx, |s, cx| {
            if s.inline.is_some() || s.closing.is_some() {
                return;
            }
            s.hint.set(false);
            s.pointer = Some((i, p));
            if s.adjust
                && let Some((d, sel)) = s.selection
                && d == i
            {
                if e.click_count == 2 && sel.contains(p) {
                    let session = cx.entity();
                    cx.defer(move |cx| confirm(&session, cx));
                    return;
                }
                if let Some(grab) = s.grab_at(sel, p) {
                    s.drag = Some(Drag { display: i, grab, start: p, last: p, anchor: p, base: sel, moved: false, previous: None });
                    s.bar.set(false);
                    cx.notify();
                    return;
                }
            }
            // A press on one display discards the others' selections.
            let previous = s.selection.filter(|(d, _)| *d == i).map(|(_, r)| r);
            if s.selection.is_some_and(|(d, _)| d != i) {
                s.selection = None;
            }
            s.drag = Some(Drag { display: i, grab: Grab::New, start: p, last: p, anchor: p, base: R::default(), moved: false, previous });
            let _ = edit_after;
            cx.notify();
        });
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let p = local(e.position);
        let i = self.index;
        self.session.update(cx, |s, cx| {
            if s.closing.is_some() {
                return;
            }
            let moved_display = s.pointer.map(|(d, _)| d) != Some(i);
            s.pointer = Some((i, p));
            // The badge fades back when the pointer comes near it.
            let (w, _) = s.displays[i].size();
            s.badge_dim.set((p.0 - w / 2.).abs() <= 140. && (p.1 - 58.).abs() <= 50.);
            s.alt = e.modifiers.alt;
            s.shift = e.modifiers.shift;
            if s.mode == Mode::Color {
                s.nudge = (0, 0);
                cx.notify();
                return;
            }
            if moved_display {
                s.hover = None;
            }
            let pressed = e.pressed_button == Some(MouseButton::Left);
            if pressed && s.drag.as_ref().is_some_and(|d| d.display == i) {
                drag_to(s, p, cx);
            } else {
                s.update_hover(cx);
            }
            cx.notify();
        });
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let mode = self.session.read(cx).mode;
        if mode == Mode::Color {
            let session = self.session.clone();
            cx.defer(move |cx| pick_color(&session, cx));
            return;
        }
        let Some(drag) = self.session.update(cx, |s, _| s.drag.take()) else { return };
        let session = self.session.clone();
        let edit_after = self.session.read(cx).inline_edit(cx);
        enum Then {
            Finish(usize, R),
            Adjust,
            Cancel,
            Nothing,
        }
        let then = self.session.update(cx, |s, cx| {
            cx.notify();
            let current = s.selection.filter(|(d, _)| *d == drag.display).map(|(_, r)| r);
            match drag.grab {
                Grab::New if drag.moved => match current {
                    Some(r) if r.big_enough() => {
                        if edit_after { Then::Finish(drag.display, r) } else { Then::Adjust }
                    }
                    _ => {
                        s.selection = drag.previous.map(|r| (drag.display, r));
                        Then::Nothing
                    }
                },
                Grab::New => {
                    if let Some((d, r)) = s.hover.take() {
                        s.selection = Some((d, r));
                        if edit_after { Then::Finish(d, r) } else { Then::Adjust }
                    } else if let Some(prev) = drag.previous.filter(|_| s.adjust) {
                        s.selection = Some((drag.display, prev));
                        Then::Adjust
                    } else {
                        Then::Cancel
                    }
                }
                Grab::Resize { .. } => {
                    if current.is_some_and(|r| !r.big_enough()) {
                        s.selection = Some((drag.display, drag.base));
                    }
                    Then::Adjust
                }
                Grab::Move => Then::Adjust,
            }
        });
        match then {
            Then::Finish(d, r) => cx.defer(move |cx| finish_selection(&session, d, r, cx)),
            Then::Adjust => self.session.update(cx, |s, cx| {
                s.adjust = true;
                s.bar.set(true);
                cx.notify();
            }),
            Then::Cancel => cx.defer(move |cx| cancel(&session, cx)),
            Then::Nothing => {}
        }
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let k = &e.keystroke;
        let command = k.modifiers.control || k.modifiers.platform;

        if let Some(board) = self.session.read(cx).inline.as_ref().map(|i| i.board.clone()) {
            let (typing, popover) = {
                let b = board.read(cx);
                (b.is_editing_text(), b.popover.is_some())
            };
            if !typing && !popover {
                match k.key.as_str() {
                    "c" if command => return finish_inline(&session, false, window, cx),
                    "s" if command => return finish_inline(&session, true, window, cx),
                    "enter" if !command => return finish_inline(&session, false, window, cx),
                    _ => {}
                }
            }
            if board.update(cx, |b, cx| b.key_down(e, cx)) {
                cx.stop_propagation();
                return;
            }
            if k.key == "escape" {
                cx.stop_propagation();
                if !board.update(cx, |b, cx| b.escape(window, cx)) {
                    cancel(&session, cx);
                }
            }
            return;
        }

        let mode = self.session.read(cx).mode;
        match (k.key.as_str(), mode) {
            ("escape", _) => cancel(&session, cx),
            ("enter" | "space", Mode::Color) => pick_color(&session, cx),
            ("left" | "right", Mode::Color) if !k.modifiers.shift => {
                let by = if k.key == "left" { -1 } else { 1 };
                Settings::update(cx, |s| s.color_format = s.color_format.step(by));
                cx.notify();
            }
            (key @ ("left" | "right" | "up" | "down"), Mode::Color) => {
                let (dx, dy) = match key {
                    "left" => (-1, 0),
                    "right" => (1, 0),
                    "up" => (0, -1),
                    _ => (0, 1),
                };
                self.session.update(cx, |s, cx| {
                    s.nudge.0 += dx;
                    s.nudge.1 += dy;
                    cx.notify();
                });
            }
            ("space", Mode::Area) => self.session.update(cx, |s, _| s.space = true),
            ("enter", Mode::Area) => {
                if self.session.read(cx).adjust {
                    confirm(&session, cx);
                }
            }
            ("f", Mode::Area) if !command => {
                let i = self.index;
                let edit_after = self.session.read(cx).inline_edit(cx);
                let full = self.session.update(cx, |s, cx| {
                    if s.drag.is_some() {
                        return None;
                    }
                    let full = s.displays[i].full();
                    s.selection = Some((i, full));
                    s.hover = None;
                    s.hint.set(false);
                    s.snapped = [false; 4];
                    cx.notify();
                    Some(full)
                });
                if let Some(full) = full {
                    if edit_after {
                        finish_selection(&session, i, full, cx);
                    } else {
                        self.session.update(cx, |s, _| {
                            s.adjust = true;
                            s.bar.set(true);
                        });
                    }
                }
            }
            (key @ ("left" | "right" | "up" | "down"), Mode::Area) => {
                let step = if k.modifiers.shift { 10. } else { 1. };
                let (dx, dy) = match key {
                    "left" => (-step, 0.),
                    "right" => (step, 0.),
                    "up" => (0., -step),
                    _ => (0., step),
                };
                self.session.update(cx, |s, cx| {
                    if !s.adjust || s.drag.is_some() {
                        return;
                    }
                    if let Some((d, r)) = s.selection {
                        let (w, h) = s.displays[d].size();
                        let x = (r.x + dx).clamp(0., (w - r.w).max(0.));
                        let y = (r.y + dy).clamp(0., (h - r.h).max(0.));
                        s.selection = Some((d, R { x, y, ..r }));
                        cx.notify();
                    }
                });
            }
            _ => {}
        }
    }

    fn key_up(&mut self, e: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key == "space" {
            self.session.update(cx, |s, _| s.space = false);
        }
    }

    fn modifiers_changed(&mut self, e: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.session.update(cx, |s, cx| {
            s.alt = e.modifiers.alt;
            s.shift = e.modifiers.shift;
            if s.drag.is_none() {
                s.update_hover(cx);
            }
            cx.notify();
        });
    }
}

fn local(p: Point<Pixels>) -> P {
    (f32::from(p.x), f32::from(p.y))
}

/// Updates the rubber band, move or resize for the pointer at `p`.
fn drag_to(s: &mut Session, p: P, cx: &App) {
    let Some(mut drag) = s.drag.take() else { return };
    if !drag.moved {
        if (p.0 - drag.start.0).hypot(p.1 - drag.start.1) < DRAG_SLOP {
            s.drag = Some(drag);
            return;
        }
        drag.moved = true;
        s.hover = None;
    }
    let display = drag.display;
    let full = s.displays[display].full();
    let (dx, dy) = (p.0 - drag.last.0, p.1 - drag.last.1);
    drag.last = p;
    let rect = match drag.grab {
        Grab::New => {
            // Holding Space moves the whole band instead of sizing it.
            if s.space {
                drag.anchor.0 += dx;
                drag.anchor.1 += dy;
            }
            let a = drag.anchor;
            let mut end = p;
            if s.shift {
                let side = (p.0 - a.0).abs().max((p.1 - a.1).abs());
                end = (a.0 + side * (p.0 - a.0).signum(), a.1 + side * (p.1 - a.1).signum());
            }
            let r = R::from_points(a, end).intersect(&full);
            if s.space || s.shift {
                s.snapped = [false; 4];
                r
            } else {
                // Only the edges following the pointer snap.
                let edges = [end.0 < a.0, end.1 < a.1, end.0 >= a.0, end.1 >= a.1];
                s.snap_edges(display, r, edges, cx)
            }
        }
        Grab::Move => {
            let total = (p.0 - drag.start.0, p.1 - drag.start.1);
            let b = drag.base;
            s.snapped = [false; 4];
            R {
                x: (b.x + total.0).clamp(0., (full.w - b.w).max(0.)),
                y: (b.y + total.1).clamp(0., (full.h - b.h).max(0.)),
                ..b
            }
        }
        Grab::Resize { l, t, r, b } => {
            let total = (p.0 - drag.start.0, p.1 - drag.start.1);
            let base = drag.base;
            let (mut x0, mut y0, mut x1, mut y1) = (base.x, base.y, base.right(), base.bottom());
            if l {
                x0 += total.0;
            }
            if r {
                x1 += total.0;
            }
            if t {
                y0 += total.1;
            }
            if b {
                y1 += total.1;
            }
            // Dragging past the opposite edge flips the rect.
            let rect = R::from_points((x0, y0), (x1, y1)).intersect(&full);
            let flip_x = x1 < x0;
            let flip_y = y1 < y0;
            let edges = [l != flip_x && (l || r), t != flip_y && (t || b), r != flip_x && (l || r), b != flip_y && (t || b)];
            s.snap_edges(display, rect, edges, cx)
        }
    };
    s.selection = Some((display, rect));
    s.drag = Some(drag);
}

/// Removes every overlay window; `fade` animates it out like a cancel.
fn close(session: &Entity<Session>, fade: bool, cx: &mut App) {
    let windows: Vec<_> = session.update(cx, |s, cx| {
        if s.done {
            return vec![];
        }
        s.done = true;
        if fade {
            s.closing = Some(Instant::now());
            cx.notify();
        }
        s.displays.iter_mut().filter_map(|d| d.window.take()).collect()
    });
    if windows.is_empty() {
        return;
    }
    cx.remove_global::<flow::Busy>();
    let remove = move |cx: &mut App| {
        for handle in windows {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    };
    if fade {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs_f32(CHROME_FADE)).await;
            cx.update(remove);
        })
        .detach();
    } else {
        // Deferred: this runs inside one of these windows' event handlers.
        cx.defer(remove);
    }
}

fn cancel(session: &Entity<Session>, cx: &mut App) {
    close(session, true, cx);
}

/// The selection is done: inline editor by default, else crop and finish.
fn finish_selection(session: &Entity<Session>, display: usize, rect: R, cx: &mut App) {
    if session.read(cx).inline_edit(cx) {
        start_inline(session, display, rect, cx);
    } else {
        confirm(session, cx);
    }
}

/// Capture (adjust step): crop, flash the rect, post-capture flow.
fn confirm(session: &Entity<Session>, cx: &mut App) {
    let Some((d, r)) = session.read(cx).selection else { return };
    if !r.big_enough() {
        return;
    }
    let (crop, bounds, purpose, scale) = {
        let s = session.read(cx);
        (s.crop(d, r), s.displays[d].bounds, s.purpose, s.scale)
    };
    match purpose {
        Purpose::Capture => {
            // Before closing: without the X11 helper only a focused window can copy.
            flow::finish_image(crop, bounds, false, cx);
            close(session, false, cx);
            flow::flash(bounds, r.bounds(), cx);
        }
        Purpose::Text => {
            close(session, false, cx);
            crate::ocr::capture_text(crop, cx);
        }
        Purpose::Scroll => {
            close(session, false, cx);
            crate::scroll::start(bounds, r.bounds(), scale, cx);
        }
        Purpose::Record => {
            close(session, false, cx);
            crate::record::start(bounds, r.bounds(), scale, cx);
        }
    }
}

fn start_inline(session: &Entity<Session>, display: usize, rect: R, cx: &mut App) {
    let (crop, focus, others) = {
        let s = session.read(cx);
        let others: Vec<_> = s.displays.iter().enumerate().filter(|(i, _)| *i != display).filter_map(|(_, d)| d.window).collect();
        (s.crop(display, rect), s.displays[display].focus.clone(), others)
    };
    let Some(focus) = focus else { return };
    let unit = (900. / rect.w.max(1.)).max(1.);
    let board = cx.new(|cx| Board::new(crop, unit, focus, cx));
    session.update(cx, |s, cx| {
        cx.observe(&board, |_, _, cx| cx.notify()).detach();
        s.inline = Some(Inline { display, rect, board, bar: Rc::new(Cell::new(None)) });
        s.selection = Some((display, rect));
        s.hover = None;
        s.drag = None;
        s.snapped = [false; 4];
        for (i, d) in s.displays.iter_mut().enumerate() {
            if i != display {
                d.window = None;
            }
        }
        cx.notify();
    });
    cx.defer(move |cx| {
        for handle in others {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    });
}

/// Inline Copy (forced clipboard) or Save (post-capture flow plus the save folder).
fn finish_inline(session: &Entity<Session>, save: bool, window: &mut Window, cx: &mut App) {
    let Some((board, display)) = session.read(cx).inline.as_ref().map(|i| (i.board.clone(), i.display)) else { return };
    if board.read(cx).is_exporting() {
        return;
    }
    // A moving sticker: copy the GIF (takes seconds), then close.
    if !save && board.read(cx).has_animation() {
        let session = session.clone();
        board.update(cx, |b, cx| b.copy_gif(&output::base_name(), window, cx, move |cx| close(&session, false, cx)));
        return;
    }
    let image = board.update(cx, |b, cx| b.flattened(window, cx));
    let bounds = session.read(cx).displays[display].bounds;
    flow::finish_image(image, bounds, !save, cx);
    if save {
        flow::save_last(cx);
    }
    close(session, false, cx);
}

fn pick_color(session: &Entity<Session>, cx: &mut App) {
    let Some((d, p)) = session.read(cx).pointer else { return };
    let (rgb, _) = session.read(cx).sample(d, p);
    let text = Settings::get(cx).color_format.format(rgb);
    output::copy_text(text.clone(), cx);
    close(session, false, cx);
    crate::history::add_color(text, rgb, cx);
}

// ── Painting ────────────────────────────────────────────────────────────────

fn teal() -> Hsla {
    gpui::rgb(0x40C8E0).into()
}

fn bar(window: &mut Window, r: R) {
    let halo = r.inset(-1.).bounds();
    window.paint_quad(gpui::quad(halo, px(2.), black(0.4), px(0.), gpui::transparent_black(), Default::default()));
    window.paint_quad(gpui::quad(r.bounds(), px(1.5), gpui::white(), px(0.), gpui::transparent_black(), Default::default()));
}

/// Corner brackets and mid-edge bars drawn inside the selection.
fn paint_handles(window: &mut Window, r: R) {
    if r.w <= 6. || r.h <= 6. {
        return;
    }
    let t = 3.;
    let arm = 22f32.min(8f32.max(r.w.min(r.h) / 3.));
    let len = 18f32.min(10f32.max(r.w.min(r.h) / 4.));
    for (cx_, cy_, sx, sy) in [(r.x, r.y, 1., 1.), (r.right(), r.y, -1., 1.), (r.right(), r.bottom(), -1., -1.), (r.x, r.bottom(), 1., -1.)] {
        let hx = if sx > 0. { cx_ } else { cx_ - arm };
        let hy = if sy > 0. { cy_ } else { cy_ - t };
        bar(window, R { x: hx, y: hy, w: arm, h: t });
        let vx = if sx > 0. { cx_ } else { cx_ - t };
        let vy = if sy > 0. { cy_ } else { cy_ - arm };
        bar(window, R { x: vx, y: vy, w: t, h: arm });
    }
    if r.w > 2. * arm + len + 12. {
        let x = r.x + (r.w - len) / 2.;
        bar(window, R { x, y: r.y, w: len, h: t });
        bar(window, R { x, y: r.bottom() - t, w: len, h: t });
    }
    if r.h > 2. * arm + len + 12. {
        let y = r.y + (r.h - len) / 2.;
        bar(window, R { x: r.x, y, w: t, h: len });
        bar(window, R { x: r.right() - t, y, w: t, h: len });
    }
}

/// Teal lines running 130 pt outward from each snapped edge, fading out.
fn paint_guides(window: &mut Window, r: R, snapped: [bool; 4]) {
    let reach = 130.;
    let solid = teal().opacity(0.85);
    let clear = teal().opacity(0.);
    let grad = |angle: f32| gpui::linear_gradient(angle, gpui::linear_color_stop(solid, 0.), gpui::linear_color_stop(clear, 1.));
    for (i, on) in snapped.into_iter().enumerate() {
        if !on {
            continue;
        }
        if i % 2 == 0 {
            let x = if i == 0 { r.x } else { r.right() } - 0.5;
            window.paint_quad(fill(R { x, y: r.y - reach, w: 1., h: reach }.bounds(), grad(0.)));
            window.paint_quad(fill(R { x, y: r.bottom(), w: 1., h: reach }.bounds(), grad(180.)));
        } else {
            let y = if i == 1 { r.y } else { r.bottom() } - 0.5;
            window.paint_quad(fill(R { x: r.x - reach, y, w: reach, h: 1. }.bounds(), grad(270.)));
            window.paint_quad(fill(R { x: r.right(), y, w: reach, h: 1. }.bounds(), grad(90.)));
        }
    }
}

fn paint_dim(window: &mut Window, full: R, hole: Option<R>, color: Hsla) {
    let Some(h) = hole else {
        window.paint_quad(fill(full.bounds(), color));
        return;
    };
    let quads = [
        R { x: 0., y: 0., w: full.w, h: h.y },
        R { x: 0., y: h.bottom(), w: full.w, h: full.h - h.bottom() },
        R { x: 0., y: h.y, w: h.x, h: h.h },
        R { x: h.right(), y: h.y, w: full.w - h.right(), h: h.h },
    ];
    for q in quads {
        if q.w > 0. && q.h > 0. {
            window.paint_quad(fill(q.bounds(), color));
        }
    }
}

/// Sutherland–Hodgman: `subject` clipped to the convex `clip` polygon.
fn clip_convex(subject: Vec<P>, clip: &[P]) -> Vec<P> {
    let n = clip.len();
    let area: f32 = (0..n).map(|i| clip[i].0 * clip[(i + 1) % n].1 - clip[(i + 1) % n].0 * clip[i].1).sum();
    let winding = area.signum();
    let mut out = subject;
    for i in 0..n {
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let inside = |p: P| winding * ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) >= 0.;
        let cross = |p: P, q: P| {
            let (x1, y1, x2, y2) = (p.0, p.1, q.0, q.1);
            let (x3, y3, x4, y4) = (a.0, a.1, b.0, b.1);
            let den = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
            if den.abs() < 1e-6 {
                return p;
            }
            let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / den;
            (x1 + t * (x2 - x1), y1 + t * (y2 - y1))
        };
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (cur, prev) = (input[j], input[(j + input.len() - 1) % input.len()]);
            match (inside(cur), inside(prev)) {
                (true, true) => out.push(cur),
                (true, false) => {
                    out.push(cross(prev, cur));
                    out.push(cur);
                }
                (false, true) => out.push(cross(prev, cur)),
                (false, false) => {}
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// Nearest-neighbour zoom of the pixels around `(sx, sy)` in a rounded square.
fn paint_loupe(window: &mut Window, frame: R, pixels: &RgbaImage, (sx, sy): (u32, u32), zoom: f32, scale: f32) {
    let radius = 14.;
    window.paint_quad(gpui::quad(frame.bounds(), px(radius), gpui::hsla(0., 0., 0.08, 1.), px(0.), gpui::transparent_black(), Default::default()));
    let outline = crate::annotate::rounded_rect(
        crate::annotate::Rect { x: frame.x, y: frame.y, w: frame.w, h: frame.h },
        radius,
    );
    let clip = outline;
    let cell = zoom / scale;
    let n = (frame.w / cell).ceil() as i32 | 1;
    let half = n / 2;
    let (iw, ih) = (pixels.width() as i32, pixels.height() as i32);
    let origin = (frame.x + frame.w / 2. - cell / 2. - half as f32 * cell, frame.y + frame.h / 2. - cell / 2. - half as f32 * cell);
    let safe = frame.inset(radius);
    for dy in 0..n {
        for dx in 0..n {
            let (x, y) = (sx as i32 + dx - half, sy as i32 + dy - half);
            if x < 0 || y < 0 || x >= iw || y >= ih {
                continue;
            }
            let [r, g, b, _] = pixels.get_pixel(x as u32, y as u32).0;
            let color: Hsla = gpui::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. }.into();
            let c = R { x: origin.0 + dx as f32 * cell, y: origin.1 + dy as f32 * cell, w: cell, h: cell }.intersect(&frame);
            if c.w <= 0. || c.h <= 0. {
                continue;
            }
            let corners_inside = [(c.x, c.y), (c.right(), c.y), (c.x, c.bottom()), (c.right(), c.bottom())]
                .iter()
                .all(|p| safe.contains(*p) || (p.0 >= safe.x && p.0 <= safe.right()) || (p.1 >= safe.y && p.1 <= safe.bottom()));
            if corners_inside {
                window.paint_quad(fill(c.bounds(), color));
            } else {
                let poly = clip_convex(vec![(c.x, c.y), (c.right(), c.y), (c.right(), c.bottom()), (c.x, c.bottom())], &clip);

                if poly.len() >= 3 {
                    let mut pb = PathBuilder::fill();
                    let pts: Vec<_> = poly.iter().map(|p| point(px(p.0), px(p.1))).collect();
                    pb.add_polygon(&pts, true);
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, color);
                    }
                }
            }
        }
    }
    if cell >= 3. {
        let line = white(0.1);
        for k in 0..=n {
            let x = origin.0 + k as f32 * cell;
            let y = origin.1 + k as f32 * cell;
            let v = R { x: x - 0.25, y: frame.y, w: 0.5, h: frame.h }.intersect(&frame.inset(radius / 2.));
            let h = R { x: frame.x, y: y - 0.25, w: frame.w, h: 0.5 }.intersect(&frame.inset(radius / 2.));
            if v.w > 0. && v.h > 0. {
                window.paint_quad(fill(v.bounds(), line));
            }
            if h.w > 0. && h.h > 0. {
                window.paint_quad(fill(h.bounds(), line));
            }
        }
    }
    let center = R { x: origin.0 + half as f32 * cell, y: origin.1 + half as f32 * cell, w: cell, h: cell };
    window.paint_quad(gpui::outline(center.inset(-1.).bounds(), black(0.9), gpui::BorderStyle::Solid));
    window.paint_quad(gpui::outline(center.bounds(), gpui::white(), gpui::BorderStyle::Solid));
    window.paint_quad(gpui::quad(frame.bounds(), px(radius), gpui::transparent_black(), px(1.), white(0.18), Default::default()));
}

/// The loupe's drop shadow, laid under its canvas.
fn loupe_shadow(frame: R) -> AnyElement {
    div()
        .absolute()
        .left(px(frame.x))
        .top(px(frame.y))
        .w(px(frame.w))
        .h(px(frame.h))
        .rounded(px(14.))
        .shadow(vec![gpui::BoxShadow {
            color: black(0.55),
            offset: point(px(0.), px(2.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        }])
        .into_any_element()
}

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn chip(text: String, bg: Hsla) -> gpui::Div {
    div()
        .px(px(8.))
        .py(px(5.))
        .rounded(px(8.))
        .bg(bg)
        .border_1()
        .border_color(white(0.12))
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .font_family("monospace")
        .text_color(gpui::white())
        .whitespace_nowrap()
        .child(text)
}

fn chip_fill() -> Hsla {
    gpui::hsla(0., 0., 0.12, 0.88)
}

fn hint_panel(title: &'static str, subtitle: &'static str, opacity: f32, height: f32) -> AnyElement {
    div()
        .absolute()
        .top(px(height - 96.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .relative()
                .bottom(px(0.))
                .child(
                    hud(14.)
                        .px(px(22.))
                        .py(px(15.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .opacity(opacity)
                        .child(div().text_size(px(15.)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(gpui::white()).child(title))
                        .child(div().text_size(px(12.)).text_color(white(0.62)).child(subtitle)),
                ),
        )
        .into_any_element()
}

fn badge(icon: IconName, title: &'static str, opacity: f32) -> AnyElement {
    div()
        .absolute()
        .top(px(58. - 15.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            hud(16.)
                .opacity(opacity)
                .px(px(14.))
                .py(px(7.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::white())
                .child(board::icon(icon, 13., gpui::white()))
                .child(title),
        )
        .into_any_element()
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.session.read(cx);
        let i = self.index;
        let texture = s.displays[i].texture.clone();
        let full = s.displays[i].full();
        let (sw, sh) = (full.w, full.h);
        let mode = s.mode;
        let snap_setting = Settings::get(cx).snap;
        let pointer = s.pointer.filter(|(d, _)| *d == i).map(|(_, p)| p);
        let selection = s.selection.filter(|(d, _)| *d == i).map(|(_, r)| r);
        let hover = s.hover.filter(|(d, _)| *d == i).map(|(_, r)| r);
        let snapped = s.snapped;
        let dragging = s.drag.as_ref().is_some_and(|d| d.display == i && d.moved);
        let inline = s.inline.as_ref().filter(|x| x.display == i);
        let closing = s.closing.map(|t| (t.elapsed().as_secs_f32() / CHROME_FADE).min(1.));
        let chrome = s.chrome.value(0.) * closing.map_or(1., |t| 1. - ease_out(t));
        let window_alpha = closing.map_or(1., |t| 1. - ease_out(t));

        let animating = s.chrome.animating() || s.hint.animating() || s.badge_dim.animating() || s.bar.animating() || closing.is_some();

        let badge_dim = s.badge_dim;
        let badge_alpha = if badge_dim.on { 1. - 0.85 * ease_out(badge_dim.progress()) } else { 0.15 + 0.85 * ease_out(badge_dim.progress()) };

        let hint_alpha = if mode == Mode::Color { 1. } else { s.hint.value(0.) };
        let bar_alpha = s.bar.value(0.);
        let show_bar = mode == Mode::Area && s.adjust && !dragging && s.drag.is_none() && inline.is_none() && selection.is_some();

        let dim = gpui::rgba(0x05050ACC).into();
        let dim = Hsla { a: 0.42 * chrome, ..dim };
        let paint_state = (selection, hover, pointer, snapped, inline.is_some());
        let backdrop = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let _ = window.paint_image(bounds, bounds, Default::default(), texture, 0, false);
                if mode == Mode::Color {
                    return;
                }
                let (selection, hover, pointer, snapped, inline) = paint_state;
                paint_dim(window, full, selection.or(hover), dim);
                if let Some(r) = selection {
                    if !inline {
                        paint_guides(window, r, snapped);
                        window.paint_quad(gpui::outline(r.inset(-2.).bounds(), black(0.45 * chrome), gpui::BorderStyle::Solid));
                        window.paint_quad(gpui::outline(r.inset(-1.).bounds(), white(chrome), gpui::BorderStyle::Solid));
                        paint_handles(window, r);
                    }
                } else if let Some(h) = hover {
                    let r = h.inset(-1.);
                    window.paint_quad(gpui::quad(r.bounds(), px(0.), accent().opacity(0.12), px(2.), accent(), Default::default()));
                } else if let Some(p) = pointer {
                    let line = white(0.2 * chrome);
                    window.paint_quad(fill(R { x: 0., y: p.1.floor() + 0.5, w: full.w, h: 1. }.bounds(), line));
                    window.paint_quad(fill(R { x: p.0.floor() + 0.5, y: 0., w: 1., h: full.h }.bounds(), line));
                }
            },
        )
        .absolute()
        .size_full();

        let mut root = div()
            .id("overlay")
            .track_focus(&self.focus)
            .size_full()
            // No gpui-component Root here to apply the theme's font.
            .font_family(gpui_component::Theme::global(cx).font_family.clone())
            .relative()
            .opacity(window_alpha)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| cancel(&this.session, cx)))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .child(backdrop);

        root = root.cursor(match (mode, s.adjust, selection, pointer) {
            (Mode::Area, true, Some(r), Some(p)) if inline.is_none() => match s.grab_at(r, p) {
                Some(Grab::Move) => if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand },
                Some(Grab::Resize { l, t, r: rt, b }) => match ((l || rt), (t || b)) {
                    (true, true) if (l && t) || (rt && b) => CursorStyle::ResizeUpLeftDownRight,
                    (true, true) => CursorStyle::ResizeUpRightDownLeft,
                    (true, false) => CursorStyle::ResizeLeftRight,
                    _ => CursorStyle::ResizeUpDown,
                },
                Some(Grab::New) | None => CursorStyle::Crosshair,
            },
            (_, _, _, _) if inline.is_some() => CursorStyle::Arrow,
            _ => CursorStyle::Crosshair,
        });

        let scale = s.scale;
        let pixels = s.displays[i].pixels.clone();

        match mode {
            Mode::Area => {
                if let Some(inl) = inline {
                    root = root.child(inline_editor(inl, sw, sh, cx));
                }
                let (title, subtitle) = if snap_setting {
                    (
                        "Drag to capture · click a highlighted area · ⌥ free · esc",
                        "click a highlighted area  ·  ⌥ free select  ·  ⇧ square  ·  hold space to move  ·  F full screen  ·  esc to cancel",
                    )
                } else {
                    ("Drag to capture · esc to cancel", "⇧ square  ·  hold space to move  ·  F full screen  ·  esc to cancel")
                };
                if hint_alpha > 0. && inline.is_none() {
                    root = root.child(div().absolute().size_full().opacity(chrome).child(hint_panel(title, subtitle, hint_alpha, sh)));
                }
                root = root.child(div().absolute().size_full().opacity(chrome).child(badge(s.purpose.badge().0, s.purpose.badge().1, badge_alpha)));

                // Size chip.
                let chip_for = selection.map(|r| (r, None)).or(hover.map(|r| (r, Some("Item"))));
                if let (Some((r, label)), None) = (chip_for, inline) {
                    let (pw, ph) = ((r.w * scale).round() as i32, (r.h * scale).round() as i32);
                    let text = match label {
                        Some(l) => format!("{l}   {pw} × {ph}"),
                        None => format!("{pw} × {ph}"),
                    };
                    let chip_w = text.chars().count() as f32 * 7.3 + 16.;
                    let chip_h = 25.;
                    let mut y = r.y - 8. - chip_h;
                    if y < 6. {
                        y = r.bottom() + 8.;
                        if y + chip_h > sh - 6. {
                            y = r.y + 8.;
                        }
                    }
                    let x = r.x.clamp(6., (sw - chip_w - 6.).max(6.));
                    let bg = if label.is_some() { accent().opacity(0.95) } else { chip_fill() };
                    root = root.child(div().absolute().left(px(x)).top(px(y)).opacity(chrome).child(chip(text, bg)));
                }

                if show_bar && let Some(r) = selection {
                    root = root.child(action_bar(&self.session, r, sw, sh, bar_alpha, cx));
                }

                // Loupe while choosing or resizing.
                let show_loupe = inline.is_none() && (selection.is_none() || s.drag.is_some());
                if let (true, Some(p)) = (show_loupe, pointer) {
                    let side = 128.;
                    let mut x = p.0 + 22.;
                    if x + side > sw - 12. {
                        x = p.0 - 22. - side;
                    }
                    let mut y = p.1 + 22.;
                    if y + side > sh - 44. {
                        y = p.1 - 22. - side;
                    }
                    let frame = R { x: x.max(12.), y: y.max(12.), w: side, h: side };
                    let (rgb, (px_x, px_y)) = s.sample(i, p);
                    let pixels = pixels.clone();
                    let sample = s.sample_point(i, p);
                    root = root
                        .child(loupe_shadow(frame))
                        .child(
                            canvas(|_, _, _| {}, move |_, _, window, _| paint_loupe(window, frame, &pixels, sample, 8., scale))
                                .absolute()
                                .size_full(),
                        )
                        .child(
                            div()
                                .absolute()
                                .top(px(frame.bottom() + 6.))
                                .left(px(frame.x))
                                .w(px(side))
                                .flex()
                                .justify_center()
                                .child(loupe_chip(Some(rgb), px_x, px_y)),
                        );
                }
            }
            Mode::Color => {
                root = root.child(hint_panel(
                    "Click to copy the color",
                    "← → change format  ·  arrow keys with ⇧ nudge by 1px  ·  esc to cancel",
                    1.,
                    sh,
                ));
                root = root.child(badge(IconName::Pipette, "Pick Color", badge_alpha));
                if let Some(p) = pointer {
                    let side = 152.;
                    let mut x = p.0 + 26.;
                    if x + side > sw - 12. {
                        x = p.0 - 26. - side;
                    }
                    let mut y = p.1 + 26.;
                    if y + side > sh - 96. {
                        y = p.1 - 26. - side;
                    }
                    let frame = R { x, y, w: side, h: side };
                    let sample = s.sample_point(i, p);
                    let (rgb, (px_x, px_y)) = s.sample(i, p);
                    let format = Settings::get(cx).color_format;
                    let pixels = pixels.clone();
                    root = root.child(loupe_shadow(frame)).child(
                        canvas(|_, _, _| {}, move |_, _, window, _| paint_loupe(window, frame, &pixels, sample, 12., scale))
                            .absolute()
                            .size_full(),
                    );
                    let panel_w = 260.;
                    let panel_h = 50.;
                    let mut py = frame.bottom() + 8.;
                    if py + panel_h > sh - 8. {
                        py = frame.y - 8. - panel_h;
                    }
                    let pxl = (frame.x + side / 2. - panel_w / 2.).clamp(8., (sw - panel_w - 8.).max(8.));
                    let [r, g, b] = rgb;
                    let swatch: Hsla = gpui::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. }.into();
                    root = root.child(
                        div().absolute().left(px(pxl)).top(px(py)).w(px(panel_w)).flex().justify_center().child(
                            hud(12.)
                                .px(px(10.))
                                .py(px(9.))
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .child(div().size(px(30.)).rounded(px(6.)).bg(swatch).border_1().border_color(white(0.35)))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .font_family("monospace")
                                                .text_color(gpui::white())
                                                .whitespace_nowrap()
                                                .child(format.format(rgb)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(10.))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(white(0.55))
                                                .child(format!("{}  ·  {px_x}, {px_y}", format.short_label())),
                                        ),
                                ),
                        ),
                    );
                }
            }
        }

        if animating {
            window.request_animation_frame();
        }
        root
    }
}

fn loupe_chip(rgb: Option<[u8; 3]>, x: u32, y: u32) -> AnyElement {
    let text = match rgb {
        Some(c) => format!("{}  {x}, {y}", hex(c)),
        None => format!("{x}, {y}"),
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(4.))
        .rounded(px(7.))
        .bg(chip_fill())
        .border_1()
        .border_color(white(0.12));
    if let Some([r, g, b]) = rgb {
        let color: Hsla = gpui::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. }.into();
        row = row.child(div().size(px(10.)).rounded(px(2.)).bg(color).border_1().border_color(white(0.5)));
    }
    row.child(
        div()
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .font_family("monospace")
            .text_color(gpui::white())
            .whitespace_nowrap()
            .child(text),
    )
    .into_any_element()
}

/// Cancel and Capture under the selection in the adjust step.
fn action_bar(session: &Entity<Session>, r: R, sw: f32, sh: f32, alpha: f32, cx: &App) -> AnyElement {
    let (confirm_icon, confirm_title) = session.read(cx).purpose.confirm();
    let confirm_w = 7.5 * confirm_title.len() as f32 + 50.;
    let total = 32. + 6. + confirm_w;
    let mut y = r.bottom() + 8.;
    if y + 32. > sh - 6. {
        y = r.y - 8. - 32.;
        if y < 6. {
            y = r.bottom() - 8. - 32.;
        }
    }
    let x = (r.right() - total).clamp(6., (sw - total - 6.).max(6.));
    let scale = 0.96 + 0.04 * alpha;
    let cancel_button = {
        let session = session.clone();
        div()
            .id("cancel")
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .bg(chip_fill().opacity(0.92))
            .border_1()
            .border_color(white(0.12))
            .hover(|s| s.bg(chip_fill()).border_color(white(0.5)))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| cancel(&session, cx))
            .child(board::icon(IconName::X, 13., gpui::white()))
    };
    let confirm_button = {
        let session = session.clone();
        div()
            .id("confirm")
            .h(px(32.))
            .w(px(confirm_w))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded(px(8.))
            .bg(accent().opacity(0.88))
            .border_1()
            .border_color(white(0.25))
            .hover(|s| s.bg(accent()).border_color(white(0.6)))
            .cursor(CursorStyle::PointingHand)
            .text_size(px(13.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(gpui::white())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| confirm(&session, cx))
            .child(board::icon(confirm_icon, 13., gpui::white()))
            .child(confirm_title)
    };
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .flex()
        .gap(px(6.))
        .opacity(alpha)
        .w(px(total * scale))
        .child(cancel_button)
        .child(confirm_button)
        .into_any_element()
}

/// The board laid exactly over the selection, with its floating toolbar.
fn inline_editor(inl: &Inline, sw: f32, sh: f32, cx: &App) -> AnyElement {
    let r = inl.rect;
    let board_entity = inl.board.clone();
    let b = board_entity.read(cx);
    let measured = inl.bar.get();
    let bar_cell = inl.bar.clone();

    let session_actions = {
        let board = board_entity.clone();
        let cancel_b = board::hud_button("inline-cancel", "Cancel (esc)", {
            let board = board.clone();
            move |_, window, cx| {
                let _ = &board;
                window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
            }
        })
        .w(px(30.))
        .h(px(28.))
        .child(board::icon(IconName::X, 13., gpui::hsla(0., 0., 0.85, 1.)));
        let save_b = board::hud_button("inline-save", "Save (Ctrl+S)", move |_, window, cx| {
            window.dispatch_keystroke(gpui::Keystroke::parse("ctrl-s").unwrap(), cx);
        })
        .w(px(30.))
        .h(px(28.))
        .child(board::icon(IconName::Download, 13., gpui::hsla(0., 0., 0.85, 1.)));
        let copy_b = board::tinted_button("inline-copy", "Copy and close (Ctrl+C or ↩)", accent().opacity(0.85), move |_, window, cx| {
            window.dispatch_keystroke(gpui::Keystroke::parse("ctrl-c").unwrap(), cx);
        })
        .h(px(28.))
        .px(px(10.))
        .gap(px(5.))
        .bg(accent())
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(gpui::white())
        .child(board::icon(IconName::Copy, 12., gpui::white()))
        .child("Copy");
        board::pill().child(cancel_b).child(save_b).child(copy_b)
    };

    let toolbar = hud(14.)
        .relative()
        .p(px(6.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(board::tools_pill(&board_entity, cx))
        .child(board::style_pill(&board_entity, cx))
        .child(
            board::pill()
                .child(board::redact_button(&board_entity, cx))
                .child(board::divider())
                .child(board::undo_button(&board_entity, cx))
                .child(board::clear_button(&board_entity, cx)),
        )
        .child(session_actions)
        .child(
            canvas(move |bounds, _, _| bar_cell.set(Some(bounds.size)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        );

    let (bw, bh) = measured.map_or((0., 0.), |s| (f32::from(s.width), f32::from(s.height)));
    let (gap, m) = (10., 8.);
    let mut cy = r.bottom() + gap + bh / 2.;
    if cy + bh / 2. > sh - m {
        cy = r.y - gap - bh / 2.;
    }
    if cy - bh / 2. < m {
        cy = r.bottom() - gap - bh / 2.;
    }
    let cx_ = (r.x + r.w / 2.).clamp(bw / 2. + m, (sw - bw / 2. - m).max(bw / 2. + m));
    let (left, top) = (cx_ - bw / 2., cy - bh / 2.);

    let status = (!b.status.is_empty()).then(|| {
        div()
            .absolute()
            .left(px(left))
            .top(px(top - 30.))
            .w(px(bw.max(1.)))
            .flex()
            .justify_center()
            .child(
                hud(6.)
                    .px(px(10.))
                    .py(px(4.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::white())
                    .whitespace_nowrap()
                    .child(b.status.clone()),
            )
    });

    div()
        .absolute()
        .size_full()
        .child(div().absolute().left(px(r.x)).top(px(r.y)).w(px(r.w)).h(px(r.h)).child(board_entity.clone()))
        .child(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .when(measured.is_none(), |d| d.opacity(0.))
                .occlude()
                .child(toolbar),
        )
        .children(status)
        .into_any_element()
}
