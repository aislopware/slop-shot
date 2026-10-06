//! The video editor window (VideoEditorWindowController, VideoEditorView, VideoTimeline
//! and VideoRegionOverlay in the macOS app).
//!
//!   top bar: tools · Auto Zoom · Undo ········ Export As… · Export · Done
//!   stage (player + region box)            │ inspector
//!   transport                              │
//!   timeline: filmstrip + trim + 5 lanes
//!   bottom bar: Format · Quality · Resolution · Frame rate

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _, Global, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, PathBuilder, Pixels, Point, RenderImage, SharedString,
    Subscription, Task, Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, canvas,
    div, img, point, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::searchable_list::SearchableVec;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::{Disableable as _, IndexPath, Sizable as _, window_border};
use gpui_kit_assets::IconName;
use image::RgbaImage;

use super::export::{self, Audio, Format, FrameRate, Options, Quality, Resolution};
use super::model::{CensorStyle, Edit, Kind, NRect, SPEEDS, Segment, speed_label};
use super::player::{self, Info, Player};
use super::render::{self, Plan};
use crate::annotate::{PALETTE, Rgba};
use crate::board;
use crate::preview::{self, white};
use crate::settings::Settings;
use crate::{alert, flow, output};

const TOP_BAR: f32 = 52.;
const TRANSPORT: f32 = 36.;
const INSPECTOR: f32 = 250.;
const BOTTOM_BAR: f32 = 52.;
const INSET: f32 = 12.;
const STRIP_TOP: f32 = 4.;
const STRIP_H: f32 = 56.;
const LANE_H: f32 = 18.;
const LANE_GAP: f32 = 4.;
const LANES_TOP: f32 = STRIP_TOP + STRIP_H + 8.;
const TIMELINE: f32 = LANES_TOP + Kind::LANES as f32 * (LANE_H + LANE_GAP) + 4.;
const PREVIEW_MAX: (u32, u32) = (1280, 720);
const THUMBS: usize = 26;
const UNDO_LIMIT: usize = 40;

fn surface() -> Hsla {
    gpui::hsla(0., 0., 0.11, 1.)
}

fn raised() -> Hsla {
    gpui::hsla(0., 0., 0.13, 1.)
}

fn gray(l: f32) -> Hsla {
    gpui::hsla(0., 0., l, 1.)
}

/// The system colours of each effect's pill on macOS.
fn tint(kind: Kind) -> Hsla {
    let (r, g, b) = match kind {
        Kind::Cut => (255, 59, 48),
        Kind::Speed => (255, 149, 0),
        Kind::Freeze => (48, 176, 199),
        Kind::Zoom => (255, 204, 0),
        Kind::Censor => (175, 82, 222),
        Kind::Text => (52, 199, 89),
    };
    gpui::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. }.into()
}

fn icon(kind: Kind) -> IconName {
    match kind {
        Kind::Cut => IconName::Scissors,
        Kind::Speed => IconName::Gauge,
        Kind::Freeze => IconName::Snowflake,
        Kind::Zoom => IconName::ZoomIn,
        Kind::Censor => IconName::EyeOff,
        Kind::Text => IconName::Type,
    }
}

fn hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Cut => "Drop a Cut at the playhead — the clip skips that stretch",
        Kind::Speed => "Drop a Speed ramp at the playhead — slow it down or speed it up",
        Kind::Freeze => "Drop a Freeze at the playhead — hold that frame still",
        Kind::Zoom => "Drop a Zoom at the playhead, then draw the region on the video",
        Kind::Censor => "Drop a Censor box at the playhead, then draw what to blur out",
        Kind::Text => "Drop a caption at the playhead, then place it on the video",
    }
}

/// "1:04.2": effects are often shorter than a second.
pub fn timecode(t: f64, decimals: bool) -> String {
    if !t.is_finite() || t < 0. {
        return if decimals { "0:00.0".into() } else { "0:00".into() };
    }
    let s = t as u64;
    if decimals { format!("{}:{:02}.{}", s / 60, s % 60, ((t - s as f64) * 10.) as u64) } else { format!("{}:{:02}", s / 60, s % 60) }
}

struct VideoEditorWindow {
    handle: AnyWindowHandle,
}

impl Global for VideoEditorWindow {}

/// Opening an editor closes the previous one.
pub fn open(file: PathBuf, cx: &mut App) {
    close(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, preview::csd_size(1160., 780.), cx))),
        titlebar: Some(gpui::TitlebarOptions { title: Some("SlopShot — Video Editor".into()), appears_transparent: true, ..Default::default() }),
        window_decorations: Some(WindowDecorations::Client),
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(crate::APP_ID.into()),
        window_min_size: Some(preview::csd_size(940., 640.)),
        focus: true,
        ..Default::default()
    };
    let result = cx.open_window(options, |window, cx| {
        let editor = cx.new(|cx| VideoEditor::new(file, window, cx));
        cx.new(|cx| gpui_component::Root::new(editor, window, cx))
    });
    match result {
        Ok(handle) => cx.set_global(VideoEditorWindow { handle: handle.into() }),
        Err(err) => alert::error("Couldn't open the video editor", &format!("{err:#}"), cx),
    }
}

/// Closes without exporting, as a new capture does.
pub fn close(cx: &mut App) {
    if let Some(w) = cx.try_global::<VideoEditorWindow>() {
        let handle = w.handle;
        cx.remove_global::<VideoEditorWindow>();
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    TrimStart,
    TrimEnd,
    Scrub,
    Move { id: u64, grab: f64 },
    ResizeLeft(u64),
    ResizeRight(u64),
    /// Region box drags, in window points.
    RegionMove { grab: Point<Pixels>, origin: NRect },
    RegionCorner { fixed: Point<Pixels> },
    RegionDraw { anchor: Point<Pixels> },
}

/// A slider in the inspector bound to one field of the selected segment.
#[derive(Clone, Copy)]
struct Knob {
    label: &'static str,
    range: (f64, f64),
    get: fn(&Segment) -> f64,
    set: fn(&mut Segment, f64),
    show: fn(f64) -> String,
}

fn knobs(kind: Kind) -> Vec<Knob> {
    match kind {
        Kind::Zoom => vec![
            Knob { label: "Zoom", range: (1.2, 5.), get: |s| s.zoom, set: |s, v| s.zoom = v, show: |v| format!("{v:.1}×") },
            Knob { label: "Ease in/out", range: (0., 1.2), get: |s| s.fade, set: |s, v| s.fade = v, show: |v| format!("{v:.2}s") },
        ],
        Kind::Freeze => vec![Knob { label: "Hold", range: (0.2, 6.), get: |s| s.duration(), set: |s, v| s.end = s.start + v, show: |v| format!("{v:.1}s") }],
        Kind::Censor => vec![Knob { label: "Strength", range: (0., 1.), get: |s| s.strength, set: |s, v| s.strength = v, show: |v| format!("{}%", (v * 100.).round()) }],
        Kind::Text => vec![Knob { label: "Size", range: (0.03, 0.25), get: |s| s.font_scale, set: |s, v| s.font_scale = v, show: |v| format!("{}%", (v * 100.).round()) }],
        _ => vec![],
    }
}

type Choice = SelectState<SearchableVec<SharedString>>;

/// The inspector's widgets for the selected segment; rebuilt when it changes.
struct Inspector {
    id: u64,
    sliders: Vec<(Knob, Entity<SliderState>)>,
    speed: Option<Entity<Choice>>,
    caption: Option<Entity<InputState>>,
    _subs: Vec<Subscription>,
}

struct Exporting {
    progress: Arc<Mutex<f64>>,
    label: &'static str,
    done: async_channel::Receiver<anyhow::Result<PathBuf>>,
}

struct VideoEditor {
    file: PathBuf,
    info: Option<Info>,
    load_error: Option<String>,
    player: Option<Player>,
    preview_size: (u32, u32),
    edit: Edit,
    undo: Vec<Edit>,
    selected: Option<u64>,
    /// The newest decoded frame and its source time, before effects.
    raw: Option<Arc<(RgbaImage, f64)>>,
    frame: Option<Arc<RenderImage>>,
    /// Textures replaced since the last tick, freed with the window at hand.
    stale: Vec<Arc<RenderImage>>,
    composing: bool,
    compose_again: bool,
    plan: Arc<Plan>,
    thumbs: Vec<Arc<RenderImage>>,
    audio: Audio,
    options: Options,
    exporting: Option<Exporting>,
    drag: Drag,
    /// The drag already pushed its one undo step.
    drag_snapshot: bool,
    knob_live: bool,
    stage: Rc<Cell<Bounds<Pixels>>>,
    timeline: Rc<Cell<Bounds<Pixels>>>,
    inspector: Option<Inspector>,
    audio_sliders: Vec<Entity<SliderState>>,
    selects: Vec<Entity<Choice>>,
    focus: FocusHandle,
    _select_subs: Vec<Subscription>,
    _audio_subs: Vec<Subscription>,
    _rebuild: Option<Task<()>>,
    _loop: Task<()>,
}

impl VideoEditor {
    fn new(file: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let ticker = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(15)).await;
                if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                    break;
                }
            }
        });
        let path = file.clone();
        cx.spawn_in(window, async move |this, cx| {
            let opened = cx
                .background_spawn(async move {
                    let info = player::probe(&path)?;
                    let size = player::fit(&info, PREVIEW_MAX);
                    let player = Player::open(&path, size)?;
                    anyhow::Ok((info, size, player))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| match opened {
                Ok((info, size, player)) => this.loaded(info, size, player, window, cx),
                Err(err) => {
                    this.load_error = Some(format!("Couldn't open the clip — {err:#}"));
                    cx.notify();
                }
            });
        })
        .detach();
        let mut this = Self {
            file,
            info: None,
            load_error: None,
            player: None,
            preview_size: (16, 9),
            edit: Edit::new(0.),
            undo: vec![],
            selected: None,
            raw: None,
            frame: None,
            stale: vec![],
            composing: false,
            compose_again: false,
            plan: Arc::new(Plan::new(&[], 16, 9)),
            thumbs: vec![],
            audio: Audio::default(),
            options: Options::default(),
            exporting: None,
            drag: Drag::None,
            drag_snapshot: false,
            knob_live: false,
            stage: Rc::default(),
            timeline: Rc::default(),
            inspector: None,
            audio_sliders: vec![],
            selects: vec![],
            focus,
            _select_subs: vec![],
            _audio_subs: vec![],
            _rebuild: None,
            _loop: ticker,
        };
        this.build_selects(window, cx);
        this
    }

    fn loaded(&mut self, info: Info, size: (u32, u32), mut player: Player, window: &mut Window, cx: &mut Context<Self>) {
        self.edit = Edit::new(info.duration);
        self.preview_size = size;
        self.audio.volumes = vec![1.; info.audio_tracks];
        player.set_map(self.edit.time_map());
        self.player = Some(player);
        self.plan = Arc::new(Plan::new(&[], size.0, size.1));
        self.audio_sliders = (0..info.audio_tracks)
            .map(|_| cx.new(|_| SliderState::new().min(0.).max(1.).step(0.01).default_value(1.)))
            .collect();
        self._audio_subs = self
            .audio_sliders
            .iter()
            .enumerate()
            .map(|(i, state)| {
                cx.subscribe(state, move |this: &mut Self, _, event: &SliderEvent, cx| {
                    let SliderEvent::Change(v) = event else { return };
                    if let Some(volume) = this.audio.volumes.get_mut(i) {
                        *volume = v.start() as f64;
                    }
                    this.apply_audio();
                    cx.notify();
                })
            })
            .collect();
        let (file, duration) = (self.file.clone(), info.duration);
        self.info = Some(info);
        cx.spawn_in(window, async move |this, cx| {
            let thumbs = cx.background_spawn(async move { thumbnails(&file, duration) }).await;
            let _ = this.update(cx, |this, cx| {
                this.thumbs = thumbs.iter().map(preview::to_render_image).collect();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for image in self.stale.drain(..) {
            window.drop_image(image).ok();
        }
        if let Some(exporting) = &self.exporting {
            if let Ok(result) = exporting.done.try_recv() {
                self.exporting = None;
                self.exported(result, cx);
            }
            cx.notify();
        }
        let Some(player) = &mut self.player else { return };
        let (comp, playing) = (player.comp, player.is_playing());
        if let Some(frame) = player.tick() {
            self.raw = Some(Arc::new(frame));
            self.recompose(cx);
        }
        let player = self.player.as_ref().unwrap();
        if player.comp != comp || player.is_playing() != playing {
            cx.notify();
        }
    }

    fn recompose(&mut self, cx: &mut Context<Self>) {
        let Some(raw) = self.raw.clone() else { return };
        if self.composing {
            self.compose_again = true;
            return;
        }
        self.composing = true;
        let plan = self.plan.clone();
        let task = cx.background_spawn(async move {
            let (frame, t) = &*raw;
            let out = if plan.is_empty() || !plan.fits(frame) { frame.clone() } else { render::compose(frame.clone(), &plan, *t) };
            preview::to_render_image(&out)
        });
        cx.spawn(async move |this, cx| {
            let image = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(old) = this.frame.replace(image) {
                    this.stale.push(old);
                }
                this.composing = false;
                if std::mem::take(&mut this.compose_again) {
                    this.recompose(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ── state changes ─────────────────────────────────────────────────────

    /// After any edit: the effects redraw now; the timing reaches the player shortly,
    /// so a slider drag doesn't seek on every step.
    fn touch(&mut self, cx: &mut Context<Self>) {
        self.plan = Arc::new(Plan::new(&self.edit.segments, self.preview_size.0, self.preview_size.1));
        self.recompose(cx);
        let map = self.edit.time_map();
        if self.player.as_ref().is_some_and(|p| *p.map() != map) {
            self._rebuild = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let _ = this.update(cx, |this, cx| {
                    let map = this.edit.time_map();
                    if let Some(player) = &mut this.player {
                        player.set_map(map);
                    }
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn push_undo(&mut self) {
        self.undo.push(self.edit.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.undo.pop() else { return };
        self.edit = edit;
        self.selected = None;
        self.inspector = None;
        self.touch(cx);
    }

    /// A tool drops a segment at the playhead, selected for the inspector.
    fn drop_segment(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if self.exporting.is_some() || self.info.is_none() {
            return;
        }
        let len = if kind == Kind::Freeze { 1. } else { 2. };
        let start = self.playhead().clamp(self.edit.trim_start, self.edit.trim_end);
        let Some((s, e)) = self.edit.fit(kind, start, start + len) else { return };
        self.push_undo();
        let seg = Segment::new(kind, s, e);
        self.selected = Some(seg.id);
        self.edit.segments.push(seg);
        self.seek_source((s + e) / 2.);
        self.touch(cx);
    }

    fn delete(&mut self, id: u64, cx: &mut Context<Self>) {
        self.push_undo();
        self.edit.segments.retain(|s| s.id != id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.touch(cx);
    }

    fn select(&mut self, id: Option<u64>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.selected = id;
            cx.notify();
        }
    }

    fn apply_audio(&self) {
        if let Some(player) = &self.player {
            player.set_levels(&self.audio.volumes, self.audio.muted);
        }
    }

    // ── playback ──────────────────────────────────────────────────────────

    fn playhead(&self) -> f64 {
        self.player.as_ref().map_or(0., |p| p.source())
    }

    fn seek_source(&mut self, t: f64) {
        if let Some(player) = &mut self.player {
            player.seek_source(t);
        }
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &mut self.player {
            player.toggle();
            cx.notify();
        }
    }

    fn step(&mut self, by: f64, cx: &mut Context<Self>) {
        if let Some(player) = &mut self.player {
            player.pause();
            let t = player.comp + by;
            player.seek_comp(t);
            cx.notify();
        }
    }

    fn go_to_start(&mut self, cx: &mut Context<Self>) {
        let start = self.edit.trim_start;
        self.seek_source(start);
        cx.notify();
    }

    // ── export ────────────────────────────────────────────────────────────

    fn export(&mut self, ask_where: bool, cx: &mut Context<Self>) {
        let Some(info) = self.info.clone() else { return };
        if self.exporting.is_some() {
            return;
        }
        if let Some(player) = &mut self.player {
            player.pause();
        }
        let folder = Settings::get(cx).save_folder.clone();
        let name = output::base_name();
        let ext = self.options.format.ext();
        if !ask_where {
            let _ = std::fs::create_dir_all(&folder);
            return self.start_export(info, output::unique_path(&folder, &name, ext), cx);
        }
        let _ = std::fs::create_dir_all(&folder);
        let receiver = cx.prompt_for_new_path(&folder, Some(&format!("{name}.{ext}")));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else { return };
            let _ = this.update(cx, |this, cx| this.start_export(info, path, cx));
        })
        .detach();
    }

    fn start_export(&mut self, info: Info, target: PathBuf, cx: &mut Context<Self>) {
        let progress = Arc::new(Mutex::new(0.));
        let (tx, rx) = async_channel::bounded(1);
        let (file, edit, audio, options) = (self.file.clone(), self.edit.clone(), self.audio.clone(), self.options);
        let report = progress.clone();
        std::thread::spawn(move || {
            let result = export::export(&file, &info, &edit, &audio, &options, &target, &|p| *report.lock().unwrap() = p).map(|()| target);
            let _ = tx.send_blocking(result);
        });
        self.exporting = Some(Exporting { progress, label: self.options.format.label(), done: rx });
        cx.notify();
    }

    fn exported(&mut self, result: anyhow::Result<PathBuf>, cx: &mut Context<Self>) {
        match result {
            Ok(path) => {
                log::info!("exported {}", path.display());
                if cx.has_global::<flow::Last>() {
                    cx.global_mut::<flow::Last>().file = path.clone();
                }
                cx.reveal_path(&path);
            }
            Err(err) => {
                log::error!("video export: {err:#}");
                alert::error("Export failed", &format!("{err:#}"), cx);
            }
        }
    }

    // ── keys ──────────────────────────────────────────────────────────────

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let typing = self.inspector.as_ref().and_then(|i| i.caption.as_ref()).is_some_and(|c| c.read(cx).focus_handle(cx).is_focused(window));
        if typing {
            return;
        }
        let k = &e.keystroke;
        let command = k.modifiers.control || k.modifiers.platform;
        match k.key.as_str() {
            "z" if command => self.undo(cx),
            "space" if !command => self.toggle_play(cx),
            "left" if !command => self.step(-1. / 30., cx),
            "right" if !command => self.step(1. / 30., cx),
            "backspace" | "delete" if self.selected.is_some() => {
                let id = self.selected.unwrap();
                self.delete(id, cx);
            }
            "escape" => self.select(None, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // ── timeline drags ────────────────────────────────────────────────────

    fn timeline_width(&self) -> f32 {
        (f32::from(self.timeline.get().size.width) - INSET * 2.).max(1.)
    }

    fn x_of(&self, t: f64) -> f32 {
        let d = self.edit.duration;
        if d <= 0. { INSET } else { INSET + (t / d) as f32 * self.timeline_width() }
    }

    fn time_at(&self, x: f32) -> f64 {
        let d = self.edit.duration;
        (((x - INSET) / self.timeline_width()) as f64 * d).clamp(0., d.max(0.))
    }

    fn lane_top(lane: usize) -> f32 {
        LANES_TOP + lane as f32 * (LANE_H + LANE_GAP)
    }

    /// A freeze is a moment on the source axis: it gets a fixed width to be clickable.
    fn pill(&self, s: &Segment) -> (f32, f32) {
        let a = self.x_of(s.start);
        let b = if s.kind == Kind::Freeze { a + 26. } else { self.x_of(s.end).max(a + 8.) };
        (a, b - a)
    }

    fn timeline_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.edit.duration <= 0. || self.exporting.is_some() {
            return;
        }
        let local = e.position - self.timeline.get().origin;
        let (x, y) = (f32::from(local.x), f32::from(local.y));
        self.drag_snapshot = false;
        if y <= STRIP_TOP + STRIP_H + 4. {
            let (sx, ex) = (self.x_of(self.edit.trim_start), self.x_of(self.edit.trim_end));
            self.drag = if (x - sx).abs() <= 8. {
                Drag::TrimStart
            } else if (x - ex).abs() <= 8. {
                Drag::TrimEnd
            } else {
                self.seek_source(self.time_at(x));
                Drag::Scrub
            };
            cx.notify();
            return;
        }
        // Topmost first: pills drawn later sit above.
        let hit = self.edit.segments.iter().rev().find(|s| {
            let top = Self::lane_top(s.kind.lane());
            let (px, pw) = self.pill(s);
            y >= top && y <= top + LANE_H && x >= px - 2. && x <= px + pw + 2.
        });
        if let Some(s) = hit {
            let (px, pw) = self.pill(s);
            let resizable = s.kind != Kind::Freeze;
            self.drag = if resizable && (x - px).abs() <= 5. {
                Drag::ResizeLeft(s.id)
            } else if resizable && (x - (px + pw)).abs() <= 5. {
                Drag::ResizeRight(s.id)
            } else {
                Drag::Move { id: s.id, grab: self.time_at(x) - s.start }
            };
            let id = s.id;
            self.select(Some(id), cx);
            return;
        }
        self.select(None, cx);
        self.drag = Drag::Scrub;
        self.seek_source(self.time_at(x));
        cx.notify();
    }

    fn snapshot_once(&mut self) {
        if !self.drag_snapshot {
            self.drag_snapshot = true;
            self.push_undo();
        }
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag == Drag::None {
            return;
        }
        if e.pressed_button != Some(MouseButton::Left) {
            return self.mouse_up(cx);
        }
        let t = self.time_at(f32::from((e.position - self.timeline.get().origin).x));
        match self.drag {
            Drag::None => {}
            Drag::TrimStart => {
                self.snapshot_once();
                self.edit.trim_start = t.min(self.edit.trim_end - 0.15);
                let at = self.edit.trim_start;
                self.seek_source(at);
            }
            Drag::TrimEnd => {
                self.snapshot_once();
                self.edit.trim_end = t.max(self.edit.trim_start + 0.15);
                let at = self.edit.trim_end;
                self.seek_source(at);
            }
            Drag::Scrub => self.seek_source(t),
            Drag::Move { id, grab } => {
                self.snapshot_once();
                let start = self.edit.clamped_start(id, t - grab);
                if let Some(s) = self.edit.get_mut(id) {
                    let len = s.duration();
                    s.start = start;
                    s.end = start + len;
                }
                self.touch(cx);
            }
            Drag::ResizeLeft(id) => {
                self.snapshot_once();
                let (lo, _) = self.edit.resize_bounds(id);
                if let Some(s) = self.edit.get_mut(id) {
                    s.start = t.max(lo).min(s.end - s.kind.min_duration());
                }
                self.touch(cx);
            }
            Drag::ResizeRight(id) => {
                self.snapshot_once();
                let (_, hi) = self.edit.resize_bounds(id);
                if let Some(s) = self.edit.get_mut(id) {
                    s.end = t.min(hi).max(s.start + s.kind.min_duration());
                }
                self.touch(cx);
            }
            Drag::RegionMove { .. } | Drag::RegionCorner { .. } | Drag::RegionDraw { .. } => self.region_update(e.position, cx),
        }
        cx.notify();
    }

    fn mouse_up(&mut self, cx: &mut Context<Self>) {
        let drag = std::mem::replace(&mut self.drag, Drag::None);
        match drag {
            Drag::None | Drag::Scrub => {}
            Drag::RegionMove { .. } | Drag::RegionCorner { .. } | Drag::RegionDraw { .. } => {
                // The zoom slider shows the level the drawn box set.
                self.inspector = None;
                self.touch(cx);
            }
            _ => self.touch(cx),
        }
        self.drag_snapshot = false;
    }

    // ── region box ────────────────────────────────────────────────────────

    fn selected_region(&self) -> Option<&Segment> {
        self.selected.and_then(|id| self.edit.get(id)).filter(|s| s.kind.has_region())
    }

    /// Where the frame shows inside the stage, in window points.
    fn video_rect(&self) -> Bounds<Pixels> {
        let stage = self.stage.get();
        let Some(info) = &self.info else { return stage };
        let (sw, sh) = (f32::from(stage.size.width), f32::from(stage.size.height));
        let fit = (sw / info.width as f32).min(sh / info.height as f32);
        let (w, h) = (info.width as f32 * fit, info.height as f32 * fit);
        Bounds::new(stage.origin + point(px((sw - w) / 2.), px((sh - h) / 2.)), gpui::size(px(w), px(h)))
    }

    fn to_view(&self, r: NRect) -> Bounds<Pixels> {
        let v = self.video_rect();
        let (vw, vh) = (f32::from(v.size.width) as f64, f32::from(v.size.height) as f64);
        Bounds::new(v.origin + point(px((r.x * vw) as f32), px((r.y * vh) as f32)), gpui::size(px((r.w * vw) as f32), px((r.h * vh) as f32)))
    }

    fn to_norm(&self, b: Bounds<Pixels>) -> NRect {
        let v = self.video_rect();
        let (vw, vh) = (f32::from(v.size.width).max(1.) as f64, f32::from(v.size.height).max(1.) as f64);
        let o = b.origin - v.origin;
        NRect { x: f32::from(o.x) as f64 / vw, y: f32::from(o.y) as f64 / vh, w: f32::from(b.size.width) as f64 / vw, h: f32::from(b.size.height) as f64 / vh }
    }

    fn corners(b: Bounds<Pixels>) -> [Point<Pixels>; 4] {
        [b.origin, b.top_right(), b.bottom_left(), b.bottom_right()]
    }

    fn region_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(seg) = self.selected_region() else { return };
        let Some(norm) = seg.region() else { return };
        let r = self.to_view(norm);
        let p = e.position;
        self.push_undo();
        let corners = Self::corners(r);
        self.drag = if let Some(i) = corners.iter().position(|c| (c.x - p.x).abs() <= px(7.) && (c.y - p.y).abs() <= px(7.)) {
            // The opposite corner stays put.
            Drag::RegionCorner { fixed: corners[3 - i] }
        } else if r.contains(&p) {
            Drag::RegionMove { grab: p - r.origin, origin: norm }
        } else {
            Drag::RegionDraw { anchor: p }
        };
        cx.stop_propagation();
        cx.notify();
    }

    fn region_update(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        let between = |a: Point<Pixels>, b: Point<Pixels>| {
            Bounds::new(point(a.x.min(b.x), a.y.min(b.y)), gpui::size((b.x - a.x).abs().max(px(8.)), (b.y - a.y).abs().max(px(8.))))
        };
        let rect = match self.drag {
            Drag::RegionMove { grab, origin } => {
                let v = self.video_rect();
                let at = p - grab - v.origin;
                let mut r = origin;
                r.x = (f32::from(at.x) as f64 / f32::from(v.size.width).max(1.) as f64).clamp(0., 1. - r.w);
                r.y = (f32::from(at.y) as f64 / f32::from(v.size.height).max(1.) as f64).clamp(0., 1. - r.h);
                r
            }
            Drag::RegionCorner { fixed } => self.to_norm(between(fixed, p)),
            Drag::RegionDraw { anchor } => self.to_norm(between(anchor, p)),
            _ => return,
        };
        if let Some(s) = self.edit.get_mut(id) {
            s.set_region(rect);
        }
        self.touch(cx);
    }

    // ── widgets ───────────────────────────────────────────────────────────

    fn choice<T: Copy + PartialEq + 'static>(
        all: &[T],
        label: fn(T) -> &'static str,
        current: T,
        apply: fn(&mut Self, T, &mut Window, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<Choice>, Subscription) {
        let all = all.to_vec();
        let items: Vec<SharedString> = all.iter().map(|v| label(*v).into()).collect();
        let selected = all.iter().position(|v| *v == current).map(IndexPath::new);
        let state = cx.new(|cx| SelectState::new(SearchableVec::new(items), selected, window, cx));
        let sub = cx.subscribe_in(&state, window, move |this, _, event: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
            let SelectEvent::Confirm(Some(value)) = event else { return };
            if let Some(v) = all.iter().find(|v| label(**v) == value.as_ref()) {
                apply(this, *v, window, cx);
                cx.notify();
            }
        });
        (state, sub)
    }

    fn build_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let o = self.options;
        let (format, s1) = Self::choice(&Format::ALL, Format::label, o.format, |this, v, window, cx| {
            this.options.format = v;
            // GIF only takes low rates.
            let allowed = FrameRate::choices(v);
            if !allowed.contains(&this.options.frame_rate) {
                this.options.frame_rate = allowed[0];
            }
            this.build_selects(window, cx);
        }, window, cx);
        let (quality, s2) = Self::choice(&Quality::ALL, Quality::label, o.quality, |this, v, _, _| this.options.quality = v, window, cx);
        let (resolution, s3) = Self::choice(&Resolution::ALL, Resolution::label, o.resolution, |this, v, _, _| this.options.resolution = v, window, cx);
        let (rate, s4) = Self::choice(FrameRate::choices(o.format), FrameRate::label, o.frame_rate, |this, v, _, _| this.options.frame_rate = v, window, cx);
        self.selects = vec![format, quality, resolution, rate];
        self._select_subs = vec![s1, s2, s3, s4];
    }

    fn sync_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(seg) = self.selected.and_then(|id| self.edit.get(id)).cloned() else {
            self.inspector = None;
            return;
        };
        if self.inspector.as_ref().is_some_and(|i| i.id == seg.id) {
            return;
        }
        let id = seg.id;
        let mut subs = vec![];
        let sliders = knobs(seg.kind)
            .into_iter()
            .map(|knob| {
                let state = cx.new(|_| {
                    SliderState::new()
                        .min(knob.range.0 as f32)
                        .max(knob.range.1 as f32)
                        .step(((knob.range.1 - knob.range.0) / 200.) as f32)
                        .default_value((knob.get)(&seg).clamp(knob.range.0, knob.range.1) as f32)
                });
                subs.push(cx.subscribe(&state, move |this: &mut Self, _, event: &SliderEvent, cx| match event {
                    SliderEvent::Change(v) => {
                        // One undo step per drag, taken as it starts.
                        if !this.knob_live {
                            this.knob_live = true;
                            this.push_undo();
                        }
                        if let Some(s) = this.edit.get_mut(id) {
                            (knob.set)(s, v.start() as f64);
                        }
                        this.touch(cx);
                    }
                    SliderEvent::Release(_) => this.knob_live = false,
                }));
                (knob, state)
            })
            .collect();
        let speed = (seg.kind == Kind::Speed).then(|| {
            let items: Vec<SharedString> = SPEEDS.iter().map(|f| speed_label(*f).into()).collect();
            let selected = SPEEDS.iter().position(|f| (f - seg.speed).abs() < 0.001).map(IndexPath::new);
            let state = cx.new(|cx| SelectState::new(SearchableVec::new(items), selected, window, cx));
            subs.push(cx.subscribe(&state, move |this: &mut Self, _, event: &SelectEvent<SearchableVec<SharedString>>, cx| {
                let SelectEvent::Confirm(Some(value)) = event else { return };
                if let Some(f) = SPEEDS.iter().find(|f| speed_label(**f) == value.as_ref()) {
                    this.push_undo();
                    if let Some(s) = this.edit.get_mut(id) {
                        s.speed = *f;
                    }
                    this.touch(cx);
                }
            }));
            state
        });
        let caption = (seg.kind == Kind::Text).then(|| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("Caption").default_value(seg.text.clone()));
            subs.push(cx.subscribe(&state, move |this: &mut Self, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    if let Some(s) = this.edit.get_mut(id) {
                        s.text = text;
                    }
                    this.touch(cx);
                }
            }));
            state
        });
        self.inspector = Some(Inspector { id, sliders, speed, caption, _subs: subs });
    }

    // ── layout ────────────────────────────────────────────────────────────

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.exporting.is_some();
        let tool = |kind: Kind| {
            board::hud_button(SharedString::from(format!("tool-{}", kind.label())), hint(kind), cx.listener(move |this, _, _, cx| this.drop_segment(kind, cx)))
                .h(px(28.))
                .px(px(9.))
                .gap(px(5.))
                .text_color(gray(0.86))
                .child(board::icon(icon(kind), 12., gray(0.86)))
                .child(div().text_size(px(11.)).font_weight(gpui::FontWeight::MEDIUM).child(kind.label()))
        };
        let tools = board::pill()
            .child(tool(Kind::Cut))
            .child(tool(Kind::Speed))
            .child(tool(Kind::Freeze))
            .child(board::divider())
            .child(tool(Kind::Zoom))
            .child(tool(Kind::Censor))
            .child(tool(Kind::Text));
        // Linux records no click log, so there is nothing to zoom in on.
        let auto_zoom = div()
            .id("auto-zoom")
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(7.))
            .bg(white(0.06))
            .opacity(0.4)
            .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("No clicks were recorded for this take").build(window, cx))
            .child(board::icon(IconName::WandSparkles, 12., gray(0.85)))
            .child(div().text_size(px(11.)).font_weight(gpui::FontWeight::MEDIUM).text_color(gray(0.85)).child("Auto Zoom"));
        let undo = board::hud_button("undo", "Undo (Ctrl+Z)", cx.listener(|this, _, _, cx| this.undo(cx)))
            .size(px(30.))
            .bg(white(0.06))
            .when(self.undo.is_empty(), |d| d.opacity(0.4))
            .child(board::icon(IconName::Undo2, 13., gray(0.85)));
        div()
            .h(px(TOP_BAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .bg(raised())
            .border_b_1()
            .border_color(white(0.08))
            .child(tools)
            .child(auto_zoom)
            .child(undo)
            .child(div().flex_1().h_full().min_w(px(8.)).on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()))
            .child(Button::new("export-as").label("Export As…").large().disabled(busy).on_click(cx.listener(|this, _, _, cx| this.export(true, cx))))
            .child(Button::new("export").label("Export").primary().large().disabled(busy).on_click(cx.listener(|this, _, _, cx| this.export(false, cx))))
            .child(Button::new("done").label("Done").large().disabled(busy).on_click(|_, window, cx| {
                cx.remove_global::<VideoEditorWindow>();
                window.remove_window();
            }))
    }

    fn stage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = self.stage.clone();
        let measure = canvas(
            move |bounds, window, _| {
                if measured.replace(bounds) != bounds {
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let origin = self.stage.get().origin;
        let local = |b: Bounds<Pixels>| (b.origin - origin, b.size);
        let v = self.video_rect();
        let mut stage = div().id("stage").relative().flex_1().min_h_0().overflow_hidden().bg(gpui::black()).child(measure);
        if let Some(frame) = &self.frame {
            let (at, size) = local(v);
            stage = stage.child(img(frame.clone()).absolute().left(at.x).top(at.y).w(size.width).h(size.height).object_fit(ObjectFit::Fill));
        }
        if let Some(seg) = self.selected_region()
            && let Some(norm) = seg.region()
        {
            let r = self.to_view(norm);
            let playhead = self.playhead();
            let live = playhead >= seg.start - 0.001 && playhead <= seg.end + 0.001;
            let alpha = if live { 1. } else { 0.35 };
            let color = tint(seg.kind).opacity(alpha);
            let (at, size) = local(r);
            let mut overlay = div().id("region").absolute().size_full().cursor_crosshair().on_mouse_down(MouseButton::Left, cx.listener(Self::region_down));
            if seg.kind == Kind::Zoom {
                // Dims what the zoom crops away.
                let (vo, vs) = local(v);
                let shade = gpui::black().opacity(0.30 * alpha);
                let band = |l: Pixels, t: Pixels, w: Pixels, h: Pixels| div().absolute().left(l).top(t).w(w.max(px(0.))).h(h.max(px(0.))).bg(shade);
                overlay = overlay
                    .child(band(vo.x, vo.y, vs.width, at.y - vo.y))
                    .child(band(vo.x, at.y + size.height, vs.width, vo.y + vs.height - at.y - size.height))
                    .child(band(vo.x, at.y, at.x - vo.x, size.height))
                    .child(band(at.x + size.width, at.y, vo.x + vs.width - at.x - size.width, size.height));
            }
            let mut boxed = div().absolute().left(at.x).top(at.y).w(size.width).h(size.height).border_2().border_color(color);
            if seg.kind == Kind::Censor {
                boxed = boxed.border_dashed().bg(tint(seg.kind).opacity(0.18 * alpha));
            }
            overlay = overlay.child(boxed);
            for c in Self::corners(r) {
                let c = c - origin;
                overlay = overlay.child(div().absolute().left(c.x - px(4.)).top(c.y - px(4.)).size(px(8.)).bg(color));
            }
            if !live {
                let (vo, _) = local(v);
                overlay = overlay.child(
                    div()
                        .absolute()
                        .left(vo.x + px(8.))
                        .top(vo.y + px(8.))
                        .text_size(px(10.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(white(0.75))
                        .child(format!("Playhead is outside this {} — move it to preview", seg.kind.label().to_lowercase())),
                );
            }
            stage = stage.child(overlay);
        }
        if let Some(exporting) = &self.exporting {
            let p = *exporting.progress.lock().unwrap();
            stage = stage.child(
                div().absolute().size_full().flex().items_center().justify_center().child(
                    preview::hud(14.)
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::MEDIUM).child(format!("Exporting {}… {}%", exporting.label, (p * 100.) as u32)))
                        .child(div().w(px(260.)).h(px(4.)).rounded_full().bg(white(0.15)).child(div().h_full().rounded_full().w(px(260. * p as f32)).bg(preview::accent()))),
                ),
            );
        }
        if let Some(error) = &self.load_error {
            stage = stage.child(div().absolute().size_full().flex().items_center().justify_center().p(px(24.)).text_size(px(13.)).text_color(gray(0.75)).child(error.clone()));
        }
        stage
    }

    fn transport(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (comp, total, playing) = self.player.as_ref().map_or((0., 0., false), |p| (p.comp, p.map().duration(), p.is_playing()));
        let button = |id: &'static str, icon: IconName, tip: &'static str, f: fn(&mut Self, &mut Context<Self>)| {
            board::hud_button(id, tip, cx.listener(move |this, _, _, cx| f(this, cx))).size(px(26.)).child(board::icon(icon, 12., gray(0.85)))
        };
        div()
            .h(px(TRANSPORT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .bg(raised())
            .border_t_1()
            .border_color(white(0.08))
            .child(button("play", if playing { IconName::Pause } else { IconName::Play }, "Play / Pause (Space)", Self::toggle_play))
            .child(button("to-start", IconName::SkipBack, "Back to the start of the trim", Self::go_to_start))
            .child(div().text_size(px(11.)).text_color(gray(0.75)).child(format!("{} / {}", timecode(comp, false), timecode(total, false))))
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(gray(0.5)).child("A tool drops in at the playhead — drag it on its lane to place it"))
    }

    fn section_title(s: &str) -> impl IntoElement {
        div().text_size(px(10.)).font_weight(gpui::FontWeight::BOLD).text_color(gray(0.55)).child(s.to_uppercase())
    }

    fn caption(s: impl Into<SharedString>) -> impl IntoElement {
        div().text_size(px(11.)).text_color(gray(0.68)).child(s.into())
    }

    fn slider_row(label: &str, value: String, state: &Entity<SliderState>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(div().text_size(px(11.)).text_color(gray(0.75)).child(label.to_owned()))
                    .child(div().text_size(px(10.)).text_color(gray(0.55)).child(value)),
            )
            .child(Slider::new(state))
    }

    fn inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().id("inspector").size_full().overflow_y_scroll().p(px(14.)).flex().flex_col().gap(px(14.));
        match (self.selected.and_then(|id| self.edit.get(id)), &self.inspector) {
            (Some(seg), Some(widgets)) => {
                let id = seg.id;
                body = body
                    .child(div().flex().items_center().gap(px(6.)).child(board::icon(icon(seg.kind), 11., tint(seg.kind))).child(Self::section_title(seg.kind.label())))
                    .child(Self::caption(format!("{} → {}  ({:.2}s)", timecode(seg.start, true), timecode(seg.end, true), seg.duration())));
                if let Some(speed) = &widgets.speed {
                    body = body.child(Select::new(speed).small()).child(Self::caption(format!(
                        "{:.2}s of the clip plays in {:.2}s.",
                        seg.duration(),
                        seg.duration() / seg.speed.max(0.01)
                    )));
                }
                if seg.kind == Kind::Censor {
                    let style = seg.censor_style;
                    let mut toggle = div().flex().p(px(2.)).gap(px(2.)).rounded(px(6.)).bg(white(0.06));
                    for s in CensorStyle::ALL {
                        toggle = toggle.child(
                            board::hud_button(SharedString::from(format!("censor-{}", s.label())), s.label(), cx.listener(move |this, _, _, cx| {
                                this.push_undo();
                                if let Some(seg) = this.edit.get_mut(id) {
                                    seg.censor_style = s;
                                }
                                this.touch(cx);
                            }))
                            .flex_1()
                            .h(px(22.))
                            .text_size(px(11.))
                            .when(s == style, |d| d.bg(white(0.16)))
                            .child(s.label()),
                        );
                    }
                    body = body.child(toggle);
                }
                if let Some(caption) = &widgets.caption {
                    body = body.child(Input::new(caption).small());
                }
                for (knob, state) in &widgets.sliders {
                    body = body.child(Self::slider_row(knob.label, (knob.show)((knob.get)(seg)), state));
                }
                if seg.kind == Kind::Text {
                    let mut swatches = div().flex().flex_wrap().gap(px(5.));
                    for (name, color) in PALETTE {
                        swatches = swatches.child(swatch(id, name, color, seg.text_color == color, cx));
                    }
                    body = body.child(swatches).child(Checkbox::new("shadow").label("Shadow").checked(seg.shadow).on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.push_undo();
                        if let Some(s) = this.edit.get_mut(id) {
                            s.shadow = *checked;
                        }
                        this.touch(cx);
                    })));
                }
                body = body.child(Self::caption(match seg.kind {
                    Kind::Zoom => "Drag the yellow box on the video to choose what to zoom into.",
                    Kind::Censor => "Drag the purple box on the video to cover what should stay private.",
                    Kind::Text => "Drag the green box on the video to place the caption.",
                    Kind::Cut => "These frames are dropped from the exported clip.",
                    _ => "",
                }));
                body = body.child(
                    Button::new("delete")
                        .icon(IconName::Trash)
                        .label("Delete")
                        .danger()
                        .small()
                        .on_click(cx.listener(move |this, _, _, cx| this.delete(id, cx))),
                );
            }
            _ => {
                body = body
                    .child(Self::section_title("Clip"))
                    .child(Self::caption(format!("Trim: {} → {}", timecode(self.edit.trim_start, true), timecode(self.edit.trim_end, true))))
                    .child(Self::caption("Select a clip on the timeline to edit it, or pick a tool above and drag on its lane."));
            }
        }
        if !self.audio_sliders.is_empty() {
            let n = self.audio_sliders.len();
            let mut audio = div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .pt(px(14.))
                .border_t_1()
                .border_color(white(0.08))
                .child(Self::section_title("Audio"))
                .child(Checkbox::new("mute").label("Mute all").checked(self.audio.muted).on_click(cx.listener(|this, checked: &bool, _, cx| {
                    this.audio.muted = *checked;
                    this.apply_audio();
                    cx.notify();
                })));
            if !self.audio.muted {
                for (i, state) in self.audio_sliders.iter().enumerate() {
                    let label = if n == 1 { "Volume" } else if i == 0 { "System" } else { "Microphone" };
                    let v = self.audio.volumes.get(i).copied().unwrap_or(1.);
                    audio = audio.child(Self::slider_row(label, format!("{}%", (v * 100.).round()), state));
                }
            }
            body = body.child(audio);
        }
        div().w(px(INSPECTOR)).flex_none().h_full().bg(raised()).border_l_1().border_color(white(0.08)).child(body)
    }

    fn timeline(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = self.timeline.clone();
        let measure = canvas(
            move |bounds, window, _| {
                if measured.replace(bounds) != bounds {
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let mut root = div()
            .id("timeline")
            .relative()
            .h(px(TIMELINE))
            .flex_none()
            .bg(surface())
            .border_t_1()
            .border_color(white(0.08))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::timeline_down))
            .child(measure);
        if self.edit.duration <= 0. {
            return root;
        }
        let w = self.timeline_width();
        let (sx, ex) = (self.x_of(self.edit.trim_start), self.x_of(self.edit.trim_end));

        let mut strip = div().absolute().left(px(INSET)).top(px(STRIP_TOP)).w(px(w)).h(px(STRIP_H)).rounded(px(6.)).overflow_hidden().bg(gray(0.07));
        let mut thumbs = div().absolute().size_full().flex();
        let tw = w / self.thumbs.len().max(1) as f32;
        for thumb in &self.thumbs {
            thumbs = thumbs.child(img(thumb.clone()).w(px(tw)).h(px(STRIP_H)).flex_none().object_fit(ObjectFit::Cover));
        }
        let dim = gpui::black().opacity(0.62);
        strip = strip
            .child(thumbs)
            .child(div().absolute().left_0().top_0().h_full().w(px((sx - INSET).max(0.))).bg(dim))
            .child(div().absolute().left(px(ex - INSET)).top_0().h_full().w(px((INSET + w - ex).max(0.))).bg(dim));
        for cut in self.edit.segments.iter().filter(|s| s.kind == Kind::Cut) {
            let (a, b) = (self.x_of(cut.start), self.x_of(cut.end));
            strip = strip.child(div().absolute().left(px(a - INSET)).top_0().h_full().w(px((b - a).max(2.))).bg(tint(Kind::Cut).opacity(0.28)));
        }
        let yellow = tint(Kind::Zoom);
        root = root.child(strip).child(div().absolute().left(px(sx)).top(px(STRIP_TOP)).w(px((ex - sx).max(1.))).h(px(STRIP_H)).border_2().border_color(yellow));
        for hx in [sx, ex] {
            let grip = || div().w(px(1.)).h(px(12.)).rounded_full().bg(gpui::black().opacity(0.45));
            root = root.child(
                div()
                    .absolute()
                    .left(px(hx - 5.))
                    .top(px(STRIP_TOP - 3.))
                    .w(px(10.))
                    .h(px(STRIP_H + 6.))
                    .rounded(px(3.))
                    .bg(yellow)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .child(grip())
                    .child(grip()),
            );
        }
        for lane in 0..Kind::LANES {
            let name = if lane == 1 { "Speed" } else { Kind::ALL.iter().find(|k| k.lane() == lane).map_or("", |k| k.label()) };
            let top = Self::lane_top(lane);
            root = root
                .child(div().absolute().left(px(INSET)).top(px(top)).w(px(w)).h(px(LANE_H)).rounded(px(4.)).bg(white(0.04)))
                .child(div().absolute().left(px(3.)).top(px(top)).h(px(LANE_H)).flex().items_center().text_size(px(9.)).font_weight(gpui::FontWeight::MEDIUM).text_color(gray(0.42)).child(name));
        }
        for s in &self.edit.segments {
            let (x, pw) = self.pill(s);
            let on = Some(s.id) == self.selected;
            let color = tint(s.kind);
            root = root.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(Self::lane_top(s.kind.lane())))
                    .w(px(pw))
                    .h(px(LANE_H))
                    .rounded(px(4.))
                    .bg(color.opacity(if on { 0.85 } else { 0.6 }))
                    .border_1()
                    .when(on, |d| d.border_color(gpui::white()))
                    .when(!on, |d| d.border_color(color))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(3.))
                    .text_size(px(9.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::white())
                    .whitespace_nowrap()
                    .child(s.badge()),
            );
        }
        let head = self.x_of(self.playhead());
        root.child(div().absolute().left(px(head - 1.)).top_0().w(px(2.)).h_full().bg(gpui::white())).child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, _| {
                    let o = bounds.origin;
                    let mut path = PathBuilder::fill();
                    path.move_to(o);
                    path.line_to(o + point(px(10.), px(0.)));
                    path.line_to(o + point(px(5.), px(7.)));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, gpui::white());
                    }
                },
            )
            .absolute()
            .left(px(head - 5.))
            .top_0()
            .w(px(10.))
            .h(px(7.)),
        )
    }

    fn bottom_bar(&self) -> impl IntoElement {
        let labeled = |title: &'static str, select: &Entity<Choice>| {
            div().flex().flex_col().gap(px(2.)).child(div().text_size(px(9.)).text_color(gray(0.5)).child(title)).child(Select::new(select).small().w(px(104.)))
        };
        let n = self.edit.segments.len();
        let len = timecode(self.player.as_ref().map_or(0., |p| p.map().duration()), true);
        let summary = if n == 0 { format!("{len} · no edits") } else { format!("{len} · {n} edit{}", if n == 1 { "" } else { "s" }) };
        let mut bar = div().h(px(BOTTOM_BAR)).flex_none().flex().items_center().gap(px(16.)).px(px(16.)).bg(raised()).border_t_1().border_color(white(0.08));
        if self.selects.len() == 4 {
            bar = bar.child(labeled("Format", &self.selects[0]));
            if self.options.format != Format::Gif {
                bar = bar.child(labeled("Quality", &self.selects[1]));
            }
            bar = bar.child(labeled("Resolution", &self.selects[2])).child(labeled("Frame rate", &self.selects[3]));
        }
        bar.when(self.exporting.is_some(), |d| d.opacity(0.5)).child(div().flex_1()).child(div().text_size(px(11.)).text_color(gray(0.55)).child(summary))
    }
}

fn swatch(id: u64, name: &'static str, color: Rgba, on: bool, cx: &mut Context<VideoEditor>) -> impl IntoElement {
    div()
        .id(name)
        .size(px(18.))
        .rounded_full()
        .bg(preview::hsla(color))
        .border_2()
        .border_color(if on { gpui::white() } else { white(0.15) })
        .cursor_pointer()
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(name).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.push_undo();
            if let Some(s) = this.edit.get_mut(id) {
                s.text_color = color;
            }
            this.touch(cx);
        }))
}

impl Render for VideoEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_inspector(window, cx);
        window_border().child(
            div()
                .id("video-editor")
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key_down))
                // Drags keep following the pointer anywhere in the window.
                .on_mouse_move(cx.listener(Self::mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.mouse_up(cx)))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.mouse_up(cx)))
                .size_full()
                .flex()
                .flex_col()
                .bg(surface())
                .text_color(gpui::white())
                .child(self.top_bar(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(div().flex_1().min_w_0().flex().flex_col().child(self.stage(cx)).child(self.transport(cx)))
                        .child(self.inspector(cx)),
                )
                .child(self.timeline(cx))
                .child(self.bottom_bar()),
        )
    }
}

/// Frames spread over the clip for the filmstrip, 160 px at most.
fn thumbnails(file: &std::path::Path, duration: f64) -> Vec<RgbaImage> {
    use gstreamer as gst;
    use gstreamer::prelude::*;
    let result = (|| {
        let pipeline = gst::parse::launch(&format!(
            "uridecodebin uri=\"{}\" ! videoconvert ! videoscale ! video/x-raw,format=RGBA,pixel-aspect-ratio=1/1 ! appsink name=sink",
            crate::media::file_uri(file)
        ))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
        let sink = pipeline.by_name("sink").and_then(|e| e.downcast::<gstreamer_app::AppSink>().ok()).ok_or_else(|| anyhow::anyhow!("no sink"))?;
        crate::media::wait_state(&pipeline, gst::State::Paused, Duration::from_secs(5))?;
        let mut out = vec![];
        for i in 0..THUMBS {
            // Just short of the end: asking for the very last frame often fails.
            let t = (duration * i as f64 / (THUMBS - 1) as f64).min(duration - 0.05).max(0.);
            if pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, gst::ClockTime::from_nseconds((t * 1e9) as u64)).is_err() {
                continue;
            }
            let _ = pipeline.state(gst::ClockTime::from_seconds(5));
            let Some(sample) = sink.try_pull_preroll(gst::ClockTime::from_seconds(2)) else { continue };
            let Some(image) = crate::media::sample_image(&sample) else { continue };
            let (w, h) = image.dimensions();
            let scale = (160. / w.max(h) as f32).min(1.);
            out.push(image::imageops::resize(&image, ((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1), image::imageops::FilterType::Triangle));
        }
        let _ = pipeline.set_state(gst::State::Null);
        anyhow::Ok(out)
    })();
    result.unwrap_or_else(|err| {
        log::warn!("filmstrip: {err:#}");
        vec![]
    })
}
