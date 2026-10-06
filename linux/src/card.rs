//! Floating preview card shown after a capture (ThumbnailController.swift).
//!
//! The card lives in a helper process that talks X11 to XWayland. Wayland gives a
//! client no way to put a window at a screen corner, keep it above other windows
//! without taking focus, or own the clipboard while unfocused; an override-redirect
//! X11 window can do all three. The helper also serves post-capture clipboard writes
//! for that last reason.

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, BoxShadow, Context, ImageSource, MouseButton,
    ObjectFit, RenderImage, StyledImage as _, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, div, hsla, img, point, prelude::*, px, size,
};
use gpui_component::Icon;
use gpui_kit_assets::IconName;
use serde::{Deserialize, Serialize};

use crate::settings::PreviewSide;

pub const HOST_ARG: &str = "--card-host";

const CARD_W: f32 = 210.;
const CARD_H: f32 = 150.;
/// Transparent padding that leaves room for the shadow.
const PAD: f32 = 14.;
const MARGIN: f32 = 24.;
const RADIUS: f32 = 14.;
const APPEAR: Duration = Duration::from_millis(320);
const LEAVE: Duration = Duration::from_millis(280);
const AUTO_DISMISS: Duration = Duration::from_secs(8);
const EXIT_DISMISS: Duration = Duration::from_millis(1500);
const TICK: Duration = Duration::from_millis(450);

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Show {
        /// A small PNG made for the card; the host deletes it once loaded.
        thumb: PathBuf,
        /// The capture's temp file, for Copy.
        file: PathBuf,
        /// Logical bounds of the display the capture came from, in the Wayland layout.
        display: [f32; 4],
        side: PreviewSide,
        /// A recording: Quick Look and Trim instead of Pin and Edit.
        #[serde(default)]
        video: bool,
    },
    Hide,
    /// White flash over `rect` (display-local logical pixels) on `display`
    /// (ScreenFlash.swift).
    Flash { display: [f32; 4], rect: [f32; 4] },
    /// Puts the image in `file` on the clipboard along with the file itself.
    Copy { file: PathBuf },
    /// Puts just the file on the clipboard (a recording).
    CopyFile { file: PathBuf },
    CopyText { text: String },
    /// Dims everything but `rect` and outlines it in red; clicks pass through
    /// (RecordingOverlayController.swift).
    Focus { display: [f32; 4], rect: [f32; 4] },
    /// The recording controls under `rect` (RecordingBarController.swift).
    RecordBar { display: [f32; 4], rect: [f32; 4], mic: bool },
    /// The scrolling capture's controls and live preview (ScrollCaptureController.swift).
    ScrollBar { display: [f32; 4], rect: [f32; 4] },
    ScrollStatus {
        text: String,
        slow: bool,
        auto: bool,
        /// A PNG of the stitched image so far, deleted once loaded.
        preview: Option<PathBuf>,
    },
    /// "Building image…" while the scrolling capture is stitched.
    Processing { display: [f32; 4] },
    HideChrome,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// Click on the card: the editor, or the player for a recording.
    Edit,
    Save,
    Share,
    Trim,
    RecordStop,
    RecordPause(bool),
    RecordRestart,
    RecordDiscard,
    RecordMute(bool),
    ScrollAuto(bool),
    ScrollDone,
    ScrollCancel,
}

// ── Main process side ────────────────────────────────────────────────────────

pub struct Host {
    child: Child,
    stdin: ChildStdin,
}

struct HostGlobal(Option<Host>);
impl gpui::Global for HostGlobal {}

/// The helper needs XWayland; without it SlopShot shows no card.
fn available() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())
}

fn spawn(events: async_channel::Sender<Event>) -> Option<Host> {
    if !available() {
        log::warn!("no X display: the preview card is off");
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let mut child = std::process::Command::new(exe)
        .arg(HOST_ARG)
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .inspect_err(|err| log::error!("starting the preview card helper: {err}"))
        .ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    std::thread::Builder::new()
        .name("slopshot-card".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match serde_json::from_str::<Event>(&line) {
                    Ok(event) => {
                        if events.send_blocking(event).is_err() {
                            break;
                        }
                    }
                    Err(err) => log::warn!("card helper said {line:?}: {err}"),
                }
            }
        })
        .ok()?;
    Some(Host { child, stdin })
}

/// Starts the helper and routes its events to `on_event` on the main thread.
pub fn init(on_event: fn(Event, &mut App), cx: &mut App) {
    let (tx, rx) = async_channel::unbounded();
    cx.set_global(HostGlobal(spawn(tx.clone())));
    cx.set_global(EventSender(tx));
    cx.spawn(async move |cx| {
        while let Ok(event) = rx.recv().await {
            cx.update(|cx| on_event(event, cx));
        }
    })
    .detach();
}

struct EventSender(async_channel::Sender<Event>);
impl gpui::Global for EventSender {}

/// Sends to the helper, restarting it once if it died. Returns false when there is
/// no helper.
pub fn send(request: &Request, cx: &mut App) -> bool {
    let line = serde_json::to_string(request).unwrap() + "\n";
    for attempt in 0..2 {
        let global = &mut cx.global_mut::<HostGlobal>().0;
        if let Some(host) = global
            && host.child.try_wait().ok().flatten().is_none()
            && host.stdin.write_all(line.as_bytes()).and_then(|_| host.stdin.flush()).is_ok()
        {
            return true;
        }
        if attempt == 0 {
            let tx = cx.global::<EventSender>().0.clone();
            cx.global_mut::<HostGlobal>().0 = spawn(tx);
        }
    }
    false
}

/// Hands a clipboard write to the helper: an X11 client may own the clipboard without
/// focus, unlike a Wayland one. False where there is no helper, or inside it.
pub fn relay_copy(request: &Request, cx: &mut App) -> bool {
    cx.has_global::<HostGlobal>() && send(request, cx)
}

pub fn has_host(cx: &App) -> bool {
    cx.try_global::<HostGlobal>().is_some_and(|h| h.0.is_some())
}

pub fn shutdown(cx: &mut App) {
    if let Some(mut host) = cx.has_global::<HostGlobal>().then(|| cx.global_mut::<HostGlobal>().0.take()).flatten() {
        let _ = host.child.kill();
        let _ = host.child.wait();
    }
}

/// Scaled to cover the card at 2× so it stays sharp on HiDPI.
pub fn write_thumbnail(image: &image::RgbaImage) -> anyhow::Result<PathBuf> {
    let (w, h) = (image.width() as f32, image.height() as f32);
    let scale = ((CARD_W * 2.) / w).max((CARD_H * 2.) / h).min(1.);
    let thumb = image::imageops::resize(
        image,
        ((w * scale).round() as u32).max(1),
        ((h * scale).round() as u32).max(1),
        image::imageops::FilterType::Triangle,
    );
    let path = crate::output::unique_path(&crate::output::temp_dir(), ".card", "png");
    std::fs::write(&path, crate::output::encode_png(&thumb)?)?;
    Ok(path)
}

// ── Helper process side ──────────────────────────────────────────────────────

pub(crate) fn emit(event: Event) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", serde_json::to_string(&event).unwrap());
    let _ = out.flush();
}

/// Entry point of `slopshot --card-host`. Exits when the main process closes stdin.
pub fn host_main() {
    let (tx, rx) = async_channel::unbounded::<Request>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            match serde_json::from_str(&line) {
                Ok(request) => {
                    if tx.send_blocking(request).is_err() {
                        return;
                    }
                }
                Err(err) => log::warn!("card request {line:?}: {err}"),
            }
        }
    });
    gpui_platform::application().with_assets(crate::Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        crate::preview::apply_ui_font(cx);
        cx.spawn(async move |cx| {
            while let Ok(request) = rx.recv().await {
                cx.update(|cx| handle(request, cx));
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

struct Current(AnyWindowHandle);
impl gpui::Global for Current {}

fn handle(request: Request, cx: &mut App) {
    match request {
        Request::Hide => hide(cx),
        Request::Copy { file } => copy_file(&file, cx),
        Request::CopyFile { file } => crate::output::set_clipboard_file(&file, cx),
        Request::CopyText { text } => crate::output::set_clipboard_text(text, cx),
        Request::Focus { display, rect } => crate::hud::focus(display, rect, cx),
        Request::RecordBar { display, rect, mic } => crate::hud::record_bar(display, rect, mic, cx),
        Request::ScrollBar { display, rect } => crate::hud::scroll_bar(display, rect, cx),
        Request::ScrollStatus { text, slow, auto, preview } => crate::hud::scroll_status(text, slow, auto, preview, cx),
        Request::Processing { display } => crate::hud::processing(display, cx),
        Request::HideChrome => crate::hud::hide(cx),
        Request::Flash { display, rect } => flash(display, rect, cx),
        Request::Show { thumb, file, display, side, video } => {
            hide(cx);
            let image = image::open(&thumb);
            let _ = std::fs::remove_file(&thumb);
            match image {
                Ok(image) => show(crate::preview::to_render_image(&image.to_rgba8()), file, display, side, video, cx),
                Err(err) => log::error!("loading the card thumbnail: {err}"),
            }
        }
    }
}

fn hide(cx: &mut App) {
    if let Some(Current(handle)) = cx.try_global::<Current>().map(|c| Current(c.0)) {
        cx.remove_global::<Current>();
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// The temp file is written in the background right after the capture, so a Copy
/// that comes very early may find it still empty.
fn copy_file(file: &Path, cx: &mut App) {
    let file = file.to_owned();
    cx.spawn(async move |cx| {
        for _ in 0..50 {
            match std::fs::read(&file) {
                Ok(png) if !png.is_empty() => {
                    cx.update(|cx| cx.write_to_clipboard(crate::output::clipboard_item(png, Some(&file))));
                    return;
                }
                _ => cx.background_executor().timer(Duration::from_millis(100)).await,
            }
        }
        log::error!("copying {}: the file never got written", file.display());
    })
    .detach();
}

/// `_NET_WORKAREA`: the screen minus the dock and top bar, in X pixels.
fn work_area() -> Option<[i32; 4]> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let atom = conn.intern_atom(true, b"_NET_WORKAREA").ok()?.reply().ok()?.atom;
    let reply = conn.get_property(false, root, atom, AtomEnum::CARDINAL, 0, 4).ok()?.reply().ok()?;
    let v: Vec<u32> = reply.value32()?.collect();
    (v.len() == 4).then(|| [v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32])
}

/// The X11 display that is the Wayland display with logical bounds `wanted`.
pub(crate) fn find_display(wanted: [f32; 4], cx: &App) -> Option<std::rc::Rc<dyn gpui::PlatformDisplay>> {
    let displays = cx.displays();
    let [wx, wy, ww, wh] = wanted;
    displays
        .iter()
        .find(|d| {
            let b = d.bounds();
            (f32::from(b.origin.x) - wx).abs() < 1. && (f32::from(b.origin.y) - wy).abs() < 1.
        })
        .or_else(|| {
            displays.iter().find(|d| {
                let b = d.bounds();
                (f32::from(b.size.width) - ww).abs() < 1. && (f32::from(b.size.height) - wh).abs() < 1.
            })
        })
        .cloned()
        .or_else(|| cx.primary_display())
}

fn show(thumb: Arc<RenderImage>, file: PathBuf, wanted: [f32; 4], side: PreviewSide, video: bool, cx: &mut App) {
    let Some(display) = find_display(wanted, cx) else { return };
    let usable = usable_area(display.bounds(), cx);

    let win = size(px(MARGIN + CARD_W + 2. * PAD), px(CARD_H + 2. * PAD));
    let x = match side {
        PreviewSide::Left => usable.left(),
        PreviewSide::Right => usable.right() - win.width,
    };
    let origin = point(x, usable.bottom() - px(MARGIN) - win.height);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, win))),
        titlebar: None,
        focus: false,
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
    match cx.open_window(options, |_, cx| cx.new(|cx| Card::new(thumb, file, side, video, cx))) {
        Ok(handle) => cx.set_global(Current(handle.into())),
        Err(err) => log::error!("opening the preview card: {err:#}"),
    }
}

/// `bounds` minus the dock and top bar.
pub(crate) fn usable_area(bounds: Bounds<gpui::Pixels>, cx: &App) -> Bounds<gpui::Pixels> {
    // The work area is in X pixels; GPUI lays windows out in logical pixels.
    let union_w = cx.displays().iter().map(|d| f32::from(d.bounds().right())).fold(0., f32::max);
    work_area()
        .and_then(|[x, y, w, h]| {
            let root_w = x11_root_width()? as f32;
            let s = if union_w > 0. { root_w / union_w } else { 1. };
            let area = Bounds::new(point(px(x as f32 / s), px(y as f32 / s)), size(px(w as f32 / s), px(h as f32 / s)));
            let clipped = area.intersect(&bounds);
            (clipped.size.width > px(0.) && clipped.size.height > px(0.)).then_some(clipped)
        })
        .unwrap_or(bounds)
}

fn x11_root_width() -> Option<u16> {
    use x11rb::connection::Connection as _;
    let (conn, screen) = x11rb::connect(None).ok()?;
    Some(conn.setup().roots[screen].width_in_pixels)
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Appearing(Instant),
    Shown,
    Leaving(Instant),
}

struct Card {
    thumb: Arc<RenderImage>,
    file: PathBuf,
    side: PreviewSide,
    video: bool,
    phase: Phase,
    hovering: bool,
    pinned: bool,
    ticked: Option<&'static str>,
    /// Bumped whenever the pending auto-dismiss is cancelled or rescheduled.
    dismiss_generation: u64,
}

/// Cubic bezier timing with control points (x1, 0) and (x2, 1): Core Animation's
/// ease-out is (0, 0.58) and ease-in (0.42, 1).
fn bezier(x1: f32, x2: f32, t: f32) -> f32 {
    let curve = |a: f32, b: f32, s: f32| 3. * a * s * (1. - s).powi(2) + 3. * b * s * s * (1. - s) + s.powi(3);
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if curve(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    curve(0., 1., (lo + hi) / 2.)
}

pub(crate) fn ease_out(t: f32) -> f32 {
    bezier(0., 0.58, t)
}

fn ease_in(t: f32) -> f32 {
    bezier(0.42, 1., t)
}

impl Card {
    fn new(thumb: Arc<RenderImage>, file: PathBuf, side: PreviewSide, video: bool, cx: &mut Context<Self>) -> Self {
        let mut card = Self {
            thumb,
            file,
            side,
            video,
            phase: Phase::Appearing(Instant::now()),
            hovering: false,
            pinned: false,
            ticked: None,
            dismiss_generation: 0,
        };
        card.schedule_dismiss(AUTO_DISMISS, cx);
        card
    }

    fn schedule_dismiss(&mut self, after: Duration, cx: &mut Context<Self>) {
        self.dismiss_generation += 1;
        let generation = self.dismiss_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                if this.dismiss_generation == generation {
                    this.dismiss(cx);
                }
            });
        })
        .detach();
    }

    fn cancel_dismiss(&mut self) {
        self.dismiss_generation += 1;
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.cancel_dismiss();
        if !matches!(self.phase, Phase::Leaving(_)) {
            self.phase = Phase::Leaving(Instant::now());
            cx.notify();
        }
    }

    fn set_hover(&mut self, inside: bool, cx: &mut Context<Self>) {
        if self.hovering == inside {
            return;
        }
        self.hovering = inside;
        if inside {
            self.cancel_dismiss();
        } else if !self.pinned {
            self.schedule_dismiss(EXIT_DISMISS, cx);
        }
        cx.notify();
    }

    /// Copy and Save turn into a tick for a beat, then the card slides away.
    fn fire(&mut self, key: &'static str, cx: &mut Context<Self>) {
        match key {
            "copy" if self.video => crate::output::set_clipboard_file(&self.file, cx),
            "copy" => copy_file(&self.file, cx),
            _ => emit(Event::Save),
        }
        self.ticked = Some(key);
        self.cancel_dismiss();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TICK).await;
            let _ = this.update(cx, |this, cx| this.dismiss(cx));
        })
        .detach();
        cx.notify();
    }

    /// Horizontal offset of the card from its resting place, and its opacity.
    fn motion(&mut self, window: &mut Window, cx: &mut App) -> (f32, f32) {
        let off = (CARD_W + 2. * PAD) + 40.;
        let sign = match self.side {
            PreviewSide::Left => -1.,
            PreviewSide::Right => 1.,
        };
        match self.phase {
            Phase::Shown => (0., 1.),
            Phase::Appearing(start) => {
                let t = (start.elapsed().as_secs_f32() / APPEAR.as_secs_f32()).min(1.);
                if t >= 1. {
                    self.phase = Phase::Shown;
                } else {
                    window.request_animation_frame();
                }
                let e = ease_out(t);
                (sign * off * (1. - e), e)
            }
            Phase::Leaving(start) => {
                let t = (start.elapsed().as_secs_f32() / LEAVE.as_secs_f32()).min(1.);
                if t >= 1. {
                    window.defer(cx, |window, _| window.remove_window());
                } else {
                    window.request_animation_frame();
                }
                let e = ease_in(t);
                (sign * off * e, 1. - e)
            }
        }
    }
}

fn circle(id: &'static str, icon: IconName, on: impl Fn(&mut Card, &mut Context<Card>) + 'static, cx: &mut Context<Card>) -> impl IntoElement {
    div()
            .id(id)
            .cursor_pointer()
            .size(px(24.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.55))
            .hover(|s| s.bg(hsla(0., 0., 0.12, 0.62)))
            .active(|s| s.bg(hsla(0., 0., 0., 0.7)))
            .text_color(gpui::white())
            .child(Icon::new(icon).size(px(11.)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                on(this, cx);
            }))
}

fn capsule(id: &'static str, label: &'static str, ticked: bool, cx: &mut Context<Card>) -> impl IntoElement {
    let content = if ticked {
        div().child(Icon::new(IconName::CircleCheck).size(px(15.))).into_any_element()
    } else {
        div().child(label).into_any_element()
    };
    div()
            .id(id)
            .cursor_pointer()
            .w(px(68.))
            .h(px(23.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 1., 0.92))
            .hover(|s| s.bg(hsla(0., 0., 1., 1.)))
            .active(|s| s.bg(hsla(0., 0., 0.84, 0.92)))
            .text_color(hsla(0., 0., 0., 0.85))
            .text_size(px(12.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(content)
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.fire(id, cx);
            }))
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (dx, alpha) = self.motion(window, cx);
        let left = match self.side {
            PreviewSide::Left => MARGIN,
            PreviewSide::Right => 0.,
        } + dx;
        let hovering = self.hovering;
        let pinned = self.pinned;
        let ticked = self.ticked;

        let controls = div()
            .absolute()
            .size_full()
            .bg(hsla(0., 0., 0., 0.42))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(capsule("copy", "Copy", ticked == Some("copy"), cx))
                    .child(capsule("save", "Save", ticked == Some("save"), cx)),
            )
            .child(div().absolute().top(px(6.)).left(px(6.)).child(circle("discard", IconName::X, |this, cx| this.dismiss(cx), cx)))
            .child(div().absolute().top(px(6.)).right(px(6.)).child(if self.video {
                circle("quick-look", IconName::Eye, |_, _| emit(Event::Edit), cx).into_any_element()
            } else {
                div()
                    .rounded_full()
                    .when(pinned, |d| d.bg(hsla(0., 0., 1., 0.35)))
                    .child(circle(
                        "pin",
                        if pinned { IconName::PinOff } else { IconName::Pin },
                        |this, cx| {
                            this.pinned = !this.pinned;
                            if this.pinned {
                                this.cancel_dismiss();
                            }
                            cx.notify();
                        },
                        cx,
                    ))
                    .into_any_element()
            }))
            .child(div().absolute().bottom(px(6.)).left(px(6.)).child(if self.video {
                circle(
                    "trim",
                    IconName::Scissors,
                    |this, cx| {
                        emit(Event::Trim);
                        this.dismiss(cx);
                    },
                    cx,
                )
                .into_any_element()
            } else {
                circle(
                    "edit",
                    IconName::SquarePen,
                    |this, cx| {
                        emit(Event::Edit);
                        this.dismiss(cx);
                    },
                    cx,
                )
                .into_any_element()
            }))
            .child(div().absolute().bottom(px(6.)).right(px(6.)).child(circle("share", IconName::Share2, |_, _| emit(Event::Share), cx)));

        let card = div()
            .id("card")
            .absolute()
            .left(px(left + PAD))
            .top(px(PAD))
            .w(px(CARD_W))
            .h(px(CARD_H))
            .rounded(px(RADIUS))
            .overflow_hidden()
            .bg(hsla(0., 0., 0.1, 1.))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.5),
                offset: point(px(0.), px(6.)),
                blur_radius: px(10.),
                spread_radius: px(0.),
                inset: false,
            }])
            .cursor_pointer()
            .on_hover(cx.listener(|this, inside: &bool, _, cx| this.set_hover(*inside, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|_, _, _, _| emit(Event::Edit)))
            .child(
                img(ImageSource::Render(self.thumb.clone()))
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .when(self.video && !hovering, |d| {
                d.child(
                    div().absolute().size_full().flex().items_center().justify_center().child(
                        div()
                            .size(px(44.))
                            .rounded_full()
                            .bg(hsla(0., 0., 0., 0.45))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(IconName::Play).size(px(17.)).text_color(hsla(0., 0., 1., 0.95))),
                    ),
                )
            })
            .when(hovering, |d| d.child(controls))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.22)),
            );

        div().size_full().opacity(alpha).child(card)
    }
}

const FLASH: Duration = Duration::from_millis(350);

fn flash(display: [f32; 4], [x, y, w, h]: [f32; 4], cx: &mut App) {
    let Some(display) = find_display(display, cx) else { return };
    let origin = display.bounds().origin + point(px(x), px(y));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(w), px(h))))),
        titlebar: None,
        focus: false,
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
    if let Err(err) = cx.open_window(options, |_, cx| cx.new(|_| Flash(Instant::now()))) {
        log::error!("opening the screen flash: {err:#}");
    }
}

struct Flash(Instant);

impl Render for Flash {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = (self.0.elapsed().as_secs_f32() / FLASH.as_secs_f32()).min(1.);
        if t >= 1. {
            window.defer(cx, |window, _| window.remove_window());
        } else {
            window.request_animation_frame();
        }
        div().size_full().bg(hsla(0., 0., 1., 0.45 * (1. - ease_out(t))))
    }
}
