//! The annotation canvas shared by the editor window and the inline editor on the
//! capture overlay, plus the toolbar pieces both show (EditorView.swift).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, CursorStyle, Div, Entity,
    FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, RenderImage, SharedString, Stateful, Subscription, Window, canvas, deferred, div, point,
    prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::Icon;
use gpui_kit_assets::IconName;
use image::RgbaImage;

use crate::annotate::{self, Annotation, Layer, Metrics, PALETTE, Pt, Rgba, Tool, WIDTHS};
use crate::frames::Frames;
use crate::preview::{self, Texture, View, accent, to_render_image, white};
use crate::raster::{self, BlurCache};
use crate::settings::Settings;
use crate::stickers::StickerPicker;
use crate::{gifwriter, output, sensitive};

/// Below this many points a press is a click, not a drag.
const DRAG_SLOP: f32 = 4.;
/// How often moving stickers redraw.
const ANIMATION_TICK: Duration = Duration::from_millis(1000 / 15);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pop {
    Color,
    Width,
    Transform,
    Sticker,
}

enum LayerDrag {
    Move(u64),
    Corner(u64, usize),
    End(u64, usize),
    Draw,
}

struct Press {
    /// Window position of the press.
    at: Point<Pixels>,
    drag: Option<LayerDrag>,
    last: Pt,
}

struct Editing {
    id: u64,
    input: Entity<InputState>,
    _events: Subscription,
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
enum TexKey {
    Blur(u32),
    Layer(usize, bool, bool, u8),
}

/// Redact (EditorView.swift): what the scan found, and the boxes the last press laid.
#[derive(Default)]
struct Redaction {
    /// Found and not covered yet; the badge on the Redact button counts them.
    pending: Vec<sensitive::Match>,
    scanning: bool,
    /// The last press's boxes: undo takes them all out at once, not one by one.
    batch: HashSet<u64>,
    redo_batch: HashSet<u64>,
    /// What the batch covers, so taking it off warns again.
    covered: Vec<sensitive::Match>,
    /// Bumped when the base image flips or turns: a scan of the old one is dropped.
    generation: u32,
}

pub struct Board {
    pub image: Arc<RgbaImage>,
    base: Arc<RenderImage>,
    blur_cache: BlurCache,
    textures: HashMap<TexKey, Arc<RenderImage>>,
    pub annotations: Vec<Annotation>,
    redo: Vec<Annotation>,
    cleared: Option<Vec<Annotation>>,
    /// Layers removed with Delete: the layer, its index, and the top layer right after
    /// removal. Undo only puts it back while nothing was drawn since.
    deleted: Vec<(Annotation, usize, Option<u64>)>,
    current: Option<Annotation>,
    pub tool: Tool,
    pub color: Rgba,
    pub width: f32,
    selected: Option<u64>,
    editing: Option<Editing>,
    press: Option<Press>,
    /// Stroke, text and counter size scale; 1 in the editor window.
    pub unit: f32,
    pub status: SharedString,
    /// The first Escape with annotations only arms; the second closes.
    pub esc_armed: bool,
    /// Has annotations nobody copied or saved yet.
    pub dirty: bool,
    pub popover: Option<Pop>,
    popover_closed: Option<(Pop, Instant)>,
    view: Rc<Cell<View>>,
    host_focus: FocusHandle,
    redaction: Redaction,
    stickers: Option<Entity<StickerPicker>>,
    /// Animated layers play from here.
    clock: Instant,
    ticking: bool,
    /// Building a GIF takes seconds; other copies and saves wait for it.
    exporting: bool,
}

impl Board {
    pub fn new(image: RgbaImage, unit: f32, host_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        let tool = Settings::get(cx).editor_tool.as_deref().and_then(Tool::from_id).unwrap_or(Tool::Arrow);
        let board = Self {
            base: to_render_image(&image),
            image: Arc::new(image),
            blur_cache: BlurCache::new(),
            textures: HashMap::new(),
            annotations: Vec::new(),
            redo: Vec::new(),
            cleared: None,
            deleted: Vec::new(),
            current: None,
            tool,
            color: PALETTE[0].1,
            width: WIDTHS[1].1,
            selected: None,
            editing: None,
            press: None,
            unit,
            status: SharedString::default(),
            esc_armed: false,
            dirty: false,
            popover: None,
            popover_closed: None,
            view: Rc::new(Cell::new(View::default())),
            host_focus,
            redaction: Redaction::default(),
            stickers: None,
            clock: Instant::now(),
            ticking: false,
            exporting: false,
        };
        board.scan_on_open(cx);
        board
    }

    pub fn is_editing_text(&self) -> bool {
        self.editing.is_some()
    }

    fn metrics(&self) -> Metrics {
        let scale = self.view.get().scale;
        Metrics {
            image_w: self.image.width() as f32,
            unit: self.unit,
            pt: if scale > 0. { 1. / scale } else { 1. },
        }
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.annotations.iter().position(|a| a.id == id)
    }

    /// Mirrors SwiftUI's `onChange(of: annotations.count)`.
    fn count_changed(&mut self, before: usize) {
        let n = self.annotations.len();
        if n != before {
            self.esc_armed = false;
            self.dirty = n > 0;
        }
    }

    fn push(&mut self, a: Annotation) {
        let before = self.annotations.len();
        self.annotations.push(a);
        self.redo.clear();
        self.cleared = None;
        self.count_changed(before);
    }

    pub fn status_line(&self) -> SharedString {
        if self.status.is_empty() {
            let n = self.annotations.len();
            format!("{n} annotation{}", if n == 1 { "" } else { "s" }).into()
        } else {
            self.status.clone()
        }
    }

    pub fn pick(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.tool = tool;
        Settings::update(cx, |s| s.editor_tool = Some(tool.id().into()));
        cx.notify();
    }

    fn selected_index_where(&self, f: impl Fn(Tool) -> bool) -> Option<usize> {
        let i = self.index_of(self.selected?)?;
        f(self.annotations[i].tool).then_some(i)
    }

    pub fn apply_color(&mut self, color: Rgba, cx: &mut Context<Self>) {
        self.color = color;
        if let Some(i) = self.selected_index_where(Tool::recolorable) {
            self.annotations[i].color = color;
            self.status = format!("Recolored the selected {}", self.annotations[i].tool.label().to_lowercase()).into();
        }
        cx.notify();
    }

    pub fn apply_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.width = width;
        if let Some(i) = self.selected_index_where(Tool::resizable_stroke) {
            self.annotations[i].width = width;
            let tool = self.annotations[i].tool;
            self.status = if tool == Tool::Blur {
                "Changed blur strength on the selected layer".into()
            } else {
                format!("Changed stroke width on the selected {}", tool.label().to_lowercase()).into()
            };
        }
        cx.notify();
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let before = self.annotations.len();
        if self.annotations.is_empty()
            && let Some(backup) = self.cleared.take()
        {
            self.annotations = backup;
            self.status = SharedString::default();
        } else if let Some((_, _, top)) = self.deleted.last()
            && self.annotations.last().map(|a| a.id) == *top
        {
            let (layer, index, _) = self.deleted.pop().unwrap();
            self.selected = Some(layer.id);
            self.annotations.insert(index.min(self.annotations.len()), layer);
            self.status = SharedString::default();
        } else if let Some(last) = self.annotations.last()
            && self.redaction.batch.contains(&last.id)
        {
            let r = &mut self.redaction;
            let (batch, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.annotations).into_iter().partition(|a| r.batch.contains(&a.id));
            self.annotations = kept;
            let n = batch.len();
            self.redo.extend(batch);
            r.redo_batch = std::mem::take(&mut r.batch);
            // The data is showing again, so warn again as when the editor opened.
            r.pending = r.covered.clone();
            let removed = format!("Removed {n} redaction{}", plural(n));
            self.status = if r.covered.is_empty() {
                removed.into()
            } else {
                format!("{removed} — {} visible again", sensitive::summary(&r.covered)).into()
            };
        } else if let Some(last) = self.annotations.pop() {
            self.redo.push(last);
        }
        self.count_changed(before);
        cx.notify();
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        let before = self.annotations.len();
        if let Some(last) = self.redo.last()
            && self.redaction.redo_batch.contains(&last.id)
        {
            let r = &mut self.redaction;
            let (batch, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.redo).into_iter().partition(|a| r.redo_batch.contains(&a.id));
            self.redo = kept;
            let n = batch.len();
            self.annotations.extend(batch);
            r.batch = std::mem::take(&mut r.redo_batch);
            r.pending.clear();
            self.status = format!("Redacted {n} item{} again", plural(n)).into();
        } else if let Some(a) = self.redo.pop() {
            self.annotations.push(a);
        }
        self.count_changed(before);
        cx.notify();
    }

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.selected.and_then(|id| self.index_of(id)) else { return };
        let before = self.annotations.len();
        let removed = self.annotations.remove(i);
        // Deleted on its own, it no longer belongs to a batch.
        self.redaction.batch.remove(&removed.id);
        self.redaction.redo_batch.remove(&removed.id);
        let label = removed.tool.label().to_lowercase();
        self.deleted.push((removed, i, self.annotations.last().map(|a| a.id)));
        self.selected = None;
        self.status = format!("Deleted {label} — Ctrl+Z to undo").into();
        self.count_changed(before);
        cx.notify();
    }

    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.annotations.is_empty() {
            return;
        }
        let n = self.annotations.len();
        self.cleared = Some(std::mem::take(&mut self.annotations));
        self.redo.clear();
        let r = &mut self.redaction;
        if !r.batch.is_empty() {
            r.pending = r.covered.clone();
        }
        r.batch.clear();
        r.redo_batch.clear();
        self.deleted.clear();
        self.selected = None;
        self.editing = None;
        self.current = None;
        self.status = format!("Cleared {n} annotation{} — Undo (Ctrl+Z) to restore", if n == 1 { "" } else { "s" }).into();
        self.count_changed(n);
        cx.notify();
    }

    fn next_counter(&self) -> u32 {
        self.annotations.iter().filter(|a| a.tool == Tool::Counter).map(|a| a.number).max().unwrap_or(0) + 1
    }

    /// The image with every annotation drawn in, at full resolution.
    pub fn flattened(&mut self, window: &mut Window, cx: &mut Context<Self>) -> RgbaImage {
        self.end_editing(window, cx);
        raster::flatten(&self.image, &self.annotations, self.unit, &mut self.blur_cache)
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    // ── Flip and rotate: the selected image layer, else the base image ────────

    fn selected_layer(&mut self) -> Option<&mut Layer> {
        let i = self.selected_index_where(|t| t == Tool::Image)?;
        self.annotations[i].layer.as_mut()
    }

    pub fn flip(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        if let Some(layer) = self.selected_layer() {
            if horizontal { layer.flip_h ^= true } else { layer.flip_v ^= true }
        } else {
            let (w, h) = (self.image.width() as f32, self.image.height() as f32);
            let flipped = if horizontal {
                image::imageops::flip_horizontal(self.image.as_ref())
            } else {
                image::imageops::flip_vertical(self.image.as_ref())
            };
            for a in &mut self.annotations {
                for p in &mut a.points {
                    *p = if horizontal { (w - p.0, p.1) } else { (p.0, h - p.1) };
                }
                if let Some(layer) = &mut a.layer {
                    if horizontal { layer.flip_h ^= true } else { layer.flip_v ^= true }
                }
            }
            self.replace_base(flipped);
        }
        cx.notify();
    }

    pub fn rotate(&mut self, clockwise: bool, cx: &mut Context<Self>) {
        let turn = if clockwise { 1 } else { 3 };
        if let Some(layer) = self.selected_layer() {
            layer.rotation = (layer.rotation + turn) % 4;
        } else {
            let (w, h) = (self.image.width() as f32, self.image.height() as f32);
            let rotated = if clockwise {
                image::imageops::rotate90(self.image.as_ref())
            } else {
                image::imageops::rotate270(self.image.as_ref())
            };
            for a in &mut self.annotations {
                for p in &mut a.points {
                    *p = if clockwise { (h - p.1, p.0) } else { (p.1, w - p.0) };
                }
                if let Some(layer) = &mut a.layer {
                    layer.rotation = (layer.rotation + turn) % 4;
                }
                // Box shapes keep [top-left, bottom-right] for resizing.
                if a.points.len() == 2 && matches!(a.tool, Tool::Rect | Tool::Ellipse | Tool::Blur | Tool::Image | Tool::Censor) {
                    let r = a.bounding_rect();
                    a.points = vec![(r.x, r.y), (r.x + r.w, r.y + r.h)];
                }
            }
            self.replace_base(rotated);
        }
        cx.notify();
    }

    fn replace_base(&mut self, image: RgbaImage) {
        // Found boxes no longer line up with the text.
        let r = &mut self.redaction;
        r.pending.clear();
        r.covered.clear();
        r.generation += 1;
        self.base = to_render_image(&image);
        self.image = Arc::new(image);
        self.blur_cache.clear();
        self.textures.clear();
    }

    // ── Redact ──────────────────────────────────────────────────────────────

    pub fn redact_hint(&self) -> usize {
        self.redaction.pending.len()
    }

    pub fn is_scanning(&self) -> bool {
        self.redaction.scanning
    }

    /// Reads the image off the main thread; `done` gets what it found, unless the base
    /// image changed meanwhile.
    fn scan(&self, cx: &mut Context<Self>, done: impl FnOnce(&mut Self, anyhow::Result<Vec<sensitive::Match>>, &mut Context<Self>) + 'static) {
        let image = self.image.clone();
        let generation = self.redaction.generation;
        cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async move { sensitive::scan(&image) }).await;
            let _ = this.update(cx, |b, cx| {
                if b.redaction.generation == generation {
                    done(b, found, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Only counts and tells; changing someone's image behind their back is not on.
    fn scan_on_open(&self, cx: &mut Context<Self>) {
        if !Settings::get(cx).redact_scan_on_open {
            return;
        }
        self.scan(cx, |b, found, _| match found {
            Ok(found) if !found.is_empty() && b.redaction.pending.is_empty() => {
                let n = found.len();
                b.status = format!("{n} sensitive item{} ({}) — hit Redact to black them out", plural(n), sensitive::summary(&found)).into();
                b.redaction.pending = found;
            }
            Ok(_) => {}
            Err(err) => log::warn!("scanning for sensitive data: {err:#}"),
        });
    }

    pub fn redact(&mut self, cx: &mut Context<Self>) {
        let r = &self.redaction;
        if r.scanning {
            return;
        }
        // The boxes aren't in the base image, so scanning again would find the same
        // places and lay a second batch on top.
        if !r.batch.is_empty() && r.batch.iter().all(|id| self.index_of(*id).is_some()) {
            self.status = "Already redacted — Ctrl+Z to take it back out".into();
            cx.notify();
            return;
        }
        if !r.pending.is_empty() {
            let found = std::mem::take(&mut self.redaction.pending);
            self.cover(found);
            cx.notify();
            return;
        }
        self.redaction.scanning = true;
        cx.notify();
        self.scan(cx, |b, found, _| {
            b.redaction.scanning = false;
            match found {
                Ok(found) if found.is_empty() => b.status = "No emails, phone numbers, card numbers or tokens found".into(),
                Ok(found) => b.cover(found),
                Err(err) => b.status = format!("Couldn't read the text: {err:#}").into(),
            }
        });
    }

    fn cover(&mut self, found: Vec<sensitive::Match>) {
        let (w, h) = (self.image.width() as f32, self.image.height() as f32);
        // OCR boxes hug the glyphs; a little margin hides the strokes poking out.
        let (dx, dy) = (0.004 * w, 0.006 * h);
        let before = self.annotations.len();
        let mut batch = HashSet::new();
        for m in &found {
            let r = m.rect;
            let tl = ((r.x - dx).max(0.), (r.y - dy).max(0.));
            let br = ((r.x + r.w + dx).min(w), (r.y + r.h + dy).min(h));
            let a = Annotation::new(Tool::Censor, Rgba::BLACK, 0., vec![tl, br]);
            batch.insert(a.id);
            self.annotations.push(a);
        }
        self.redo.clear();
        self.cleared = None;
        self.count_changed(before);
        let n = found.len();
        self.status = format!(
            "Blacked out {n} item{} ({}) — Ctrl+Z to undo, or Select + Delete for one",
            plural(n),
            sensitive::summary(&found)
        )
        .into();
        self.redaction = Redaction { batch, covered: found, generation: self.redaction.generation, ..Redaction::default() };
    }

    // ── Paste ───────────────────────────────────────────────────────────────

    /// Ctrl+V: an image (or an image file) on the clipboard becomes a layer half the
    /// canvas wide, centred and selected. A GIF stays animated.
    pub fn paste_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(frames) = clipboard_frames(cx) else { return false };
        let note = if frames.is_animated() { format!("Pasted GIF ({} frames)", frames.frames.len()) } else { "Pasted image".into() };
        self.place(frames, 0.5, note, cx);
        true
    }

    /// Lays an image `width_fraction` of the canvas wide in the middle, keeping its
    /// aspect, and selects it so it can be dragged and resized right away.
    pub fn place(&mut self, frames: Frames, width_fraction: f32, note: String, cx: &mut Context<Self>) {
        let (bw, bh) = (self.image.width() as f32, self.image.height() as f32);
        let (iw, ih) = frames.first().dimensions();
        let (iw, ih) = (iw.max(1) as f32, ih.max(1) as f32);
        let mut nw = width_fraction;
        let mut nh = nw * bw * (ih / iw) / bh;
        if nh > 0.8 {
            let k = 0.8 / nh;
            nw *= k;
            nh *= k;
        }
        let tl = ((0.5 - nw / 2.) * bw, (0.5 - nh / 2.) * bh);
        let br = ((0.5 + nw / 2.) * bw, (0.5 + nh / 2.) * bh);
        let mut a = Annotation::new(Tool::Image, Rgba::BLACK, 0., vec![tl, br]);
        a.layer = Some(Layer::new(frames));
        let id = a.id;
        self.push(a);
        // Not persisted: the user didn't pick Select themselves.
        self.tool = Tool::Select;
        self.selected = Some(id);
        self.editing = None;
        self.status = note.into();
        cx.notify();
    }

    // ── Stickers and GIF ────────────────────────────────────────────────────

    pub fn has_animation(&self) -> bool {
        self.annotations.iter().any(|a| annotate::animation(a).is_some())
    }

    pub fn is_exporting(&self) -> bool {
        self.exporting
    }

    fn sticker_picker(&mut self, cx: &mut Context<Self>) -> Entity<StickerPicker> {
        if let Some(picker) = &self.stickers {
            return picker.clone();
        }
        let board = cx.weak_entity();
        let on_pick: crate::stickers::OnPick = Rc::new(move |frames, name, _, cx| {
            let _ = board.update(cx, |b, cx| {
                let note = if frames.is_animated() {
                    format!("Added animated sticker \u{201c}{name}\u{201d} — Copy/Save gives a GIF")
                } else {
                    format!("Added sticker \u{201c}{name}\u{201d}")
                };
                b.place(frames, 0.28, note, cx);
                b.popover = None;
            });
        });
        let picker = cx.new(|_| StickerPicker::new(on_pick));
        self.stickers = Some(picker.clone());
        picker
    }

    /// Redraws while a layer moves; stops by itself once none does.
    fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !self.has_animation() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(ANIMATION_TICK).await;
                let going = this
                    .update(cx, |b, cx| {
                        b.ticking = b.has_animation();
                        if b.ticking {
                            cx.notify();
                        }
                        b.ticking
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        })
        .detach();
    }

    /// Renders every frame of the GIF off the main thread, counting them in the status
    /// line, then hands the bytes (or the failure) to `done`.
    pub fn build_gif(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, anyhow::Result<(Vec<u8>, usize)>, &mut Context<Self>) + 'static,
    ) {
        if self.exporting {
            return;
        }
        self.end_editing(window, cx);
        self.exporting = true;
        let (image, annotations, unit) = (self.image.clone(), self.annotations.clone(), self.unit);
        let plan = gifwriter::timeline(&annotations);
        let count = plan.len();
        let (tx, rx) = async_channel::unbounded::<usize>();
        let job = cx.background_spawn(async move {
            gifwriter::animate(&image, &annotations, unit, &plan, |i| {
                let _ = tx.try_send(i);
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok(i) = rx.recv().await {
                let _ = this.update(cx, |b, cx| {
                    b.status = format!("Building GIF… {i}/{count}").into();
                    cx.notify();
                });
            }
            let result = job.await.map(|gif| (gif, count));
            let _ = this.update(cx, |b, cx| {
                b.exporting = false;
                done(b, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Copies the GIF as a file and as GIF data, deliberately without a PNG: an app
    /// offered a still bitmap takes it and the animation is lost. `then` runs after.
    pub fn copy_gif(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>, then: impl FnOnce(&mut App) + 'static) {
        let path = output::temp_dir().join(format!("{name}.gif"));
        self.build_gif(window, cx, move |b, result, cx| {
            let written = result.and_then(|(gif, frames)| {
                output::write_file(&gif, &path)?;
                Ok((gif, frames))
            });
            match written {
                Ok((gif, frames)) => {
                    let size = output::byte_size(gif.len() as u64);
                    output::copy_gif(gif, &path, cx);
                    b.status = format!("Copied GIF — {frames} frames, {size}").into();
                    b.dirty = false;
                    cx.defer(then);
                }
                Err(err) => {
                    log::error!("building the GIF: {err:#}");
                    b.status = "GIF export failed".into();
                }
            }
        });
    }

    pub fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.popover.take().is_some() {
        } else if self.editing.is_some() {
            self.end_editing(window, cx);
        } else if self.selected.take().is_some() {
        } else {
            return false;
        }
        self.esc_armed = false;
        cx.notify();
        true
    }

    /// Delete, tool letters, Ctrl+Z / Ctrl+Shift+Z and Ctrl+V. Returns whether the key
    /// was used.
    pub fn key_down(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.editing.is_some() {
            return false;
        }
        let k = &e.keystroke;
        let m = &k.modifiers;
        let command = m.control || m.platform;
        match k.key.as_str() {
            "delete" | "backspace" if !command && self.selected.is_some() => {
                self.delete_selected(cx);
                true
            }
            key if !command && !m.alt => match Tool::pickable().find(|t| t.key() == Some(key)) {
                Some(tool) => {
                    self.pick(tool, cx);
                    true
                }
                None => false,
            },
            "v" if command => self.paste_image(cx),
            "z" if command && m.shift => {
                self.redo(cx);
                true
            }
            "z" if command => {
                self.undo(cx);
                true
            }
            _ => false,
        }
    }

    // ── Popovers ────────────────────────────────────────────────────────────

    pub fn toggle_popover(&mut self, pop: Pop, cx: &mut Context<Self>) {
        // The click that closed it from outside also lands on its button.
        let just_closed = self
            .popover_closed
            .is_some_and(|(p, at)| p == pop && at.elapsed() < Duration::from_millis(300));
        self.popover = if self.popover == Some(pop) || just_closed { None } else { Some(pop) };
        if self.popover == Some(Pop::Sticker) {
            let picker = self.sticker_picker(cx);
            picker.update(cx, |p, cx| p.rescan(cx));
        }
        cx.notify();
    }

    fn close_popover(&mut self, cx: &mut Context<Self>) {
        if let Some(pop) = self.popover.take() {
            self.popover_closed = Some((pop, Instant::now()));
            cx.notify();
        }
    }

    // ── Text ────────────────────────────────────────────────────────────────

    fn start_editing(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.index_of(id).map(|i| self.annotations[i].text.clone()).unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Text…").default_value(text));
        let events = cx.subscribe_in(&input, window, move |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => {
                let value = input.read(cx).value().to_string();
                if let Some(i) = this.index_of(id) {
                    this.annotations[i].text = value;
                }
            }
            InputEvent::PressEnter { .. } => this.end_editing(window, cx),
            _ => {}
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.editing = Some(Editing { id, input, _events: events });
        cx.notify();
    }

    pub fn end_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else { return };
        if let Some(i) = self.index_of(edit.id)
            && self.annotations[i].text.trim().is_empty()
        {
            let before = self.annotations.len();
            self.annotations.remove(i);
            self.count_changed(before);
        }
        window.focus(&self.host_focus, cx);
        cx.notify();
    }

    // ── Pointer ─────────────────────────────────────────────────────────────

    fn hit(&self, p: Pt, loose: bool) -> Option<u64> {
        let m = self.metrics();
        self.annotations
            .iter()
            .rev()
            .find(|a| {
                if loose {
                    annotate::frame(a, &m, raster::text_width).inflate(6. * m.pt).contains(p)
                } else {
                    annotate::hits(a, p, &m, raster::text_width)
                }
            })
            .map(|a| a.id)
    }

    fn begin_drag(&mut self, start: Pt) -> LayerDrag {
        let m = self.metrics();
        if let Some(id) = self.selected
            && let Some(i) = self.index_of(id)
            && let Some(g) = annotate::grip_at(&self.annotations[i], start, &m)
        {
            let a = &mut self.annotations[i];
            if annotate::grips(a).len() == 2 {
                return LayerDrag::End(id, g);
            }
            // `resize` takes points[0] as the top-left corner; shapes drawn
            // right-to-left don't start there.
            let r = a.bounding_rect();
            a.points = vec![(r.x, r.y), (r.x + r.w, r.y + r.h)];
            return LayerDrag::Corner(id, g);
        }
        if let Some(hit) = self.hit(start, self.tool == Tool::Select) {
            self.selected = Some(hit);
            return LayerDrag::Move(hit);
        }
        self.selected = None;
        LayerDrag::Draw
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.close_popover(cx);
        if self.editing.is_some() {
            self.end_editing(window, cx);
        } else {
            window.focus(&self.host_focus, cx);
        }
        let p = self.view.get().to_image(e.position);
        self.press = Some(Press { at: e.position, drag: None, last: p });
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let p = self.view.get().to_image(e.position);
        let Some(press) = self.press.as_ref() else { return };
        if press.drag.is_none() {
            let d = e.position - press.at;
            if f32::from(d.x).hypot(f32::from(d.y)) < DRAG_SLOP {
                return;
            }
            let start = self.view.get().to_image(press.at);
            let drag = self.begin_drag(start);
            let press = self.press.as_mut().unwrap();
            press.drag = Some(drag);
            press.last = start;
        }
        let press = self.press.as_mut().unwrap();
        let prev = std::mem::replace(&mut press.last, p);
        let start = self.view.get().to_image(press.at);
        match press.drag {
            Some(LayerDrag::Move(id)) => {
                if let Some(i) = self.index_of(id) {
                    self.annotations[i].translate(p.0 - prev.0, p.1 - prev.1);
                }
            }
            Some(LayerDrag::Corner(id, corner)) => {
                if let Some(i) = self.index_of(id) {
                    annotate::resize(&mut self.annotations[i], corner, p);
                }
            }
            Some(LayerDrag::End(id, end)) => {
                if let Some(i) = self.index_of(id) {
                    let a = &mut self.annotations[i];
                    let last = a.points.len() - 1;
                    a.points[if end == 0 { 0 } else { last }] = p;
                }
            }
            Some(LayerDrag::Draw) | None => {
                if !self.tool.draws_on_drag() {
                    return;
                }
                match &mut self.current {
                    None => self.current = Some(Annotation::new(self.tool, self.color, self.width, vec![start, p])),
                    Some(c) if c.tool == Tool::Pen => c.points.push(p),
                    Some(c) => c.points[1] = p,
                }
            }
        }
        cx.notify();
    }

    fn mouse_up(&mut self, e: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(press) = self.press.take() else { return };
        match press.drag {
            Some(LayerDrag::Draw) => {
                let (iw, ih) = (self.image.width() as f32, self.image.height() as f32);
                if let Some(c) = self.current.take()
                    && (c.tool == Tool::Pen || c.is_big_enough(iw, ih))
                {
                    let id = c.id;
                    self.push(c);
                    self.selected = Some(id);
                }
            }
            Some(_) => {}
            None => self.tap(self.view.get().to_image(e.position), window, cx),
        }
        self.current = None;
        cx.notify();
    }

    fn tap(&mut self, p: Pt, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool == Tool::Select {
            self.selected = self.hit(p, true);
            return;
        }
        if let Some(hit) = self.hit(p, false) {
            let is_text = self.index_of(hit).is_some_and(|i| self.annotations[i].tool == Tool::Text);
            if self.tool == Tool::Text && is_text {
                self.selected = None;
                self.start_editing(hit, window, cx);
            } else {
                self.selected = Some(hit);
            }
            return;
        }
        self.selected = None;
        match self.tool {
            Tool::Counter => {
                let mut a = Annotation::new(Tool::Counter, self.color, self.width, vec![p]);
                a.number = self.next_counter();
                let id = a.id;
                self.push(a);
                self.selected = Some(id);
            }
            Tool::Text => {
                let a = Annotation::new(Tool::Text, self.color, self.width, vec![p]);
                let id = a.id;
                self.push(a);
                self.start_editing(id, window, cx);
            }
            _ => {}
        }
    }

    fn texture(&mut self, key: TexKey, make: impl FnOnce(&mut Self) -> RgbaImage) -> Arc<RenderImage> {
        if let Some(t) = self.textures.get(&key) {
            return t.clone();
        }
        let texture = to_render_image(&make(self));
        self.textures.insert(key, texture.clone());
        texture
    }

    fn texture_key(t: &Texture) -> TexKey {
        match t {
            Texture::Blur(r) => TexKey::Blur(r.round() as u32),
            Texture::Layer(l) => TexKey::Layer(Arc::as_ptr(&l.image) as usize, l.flip_h, l.flip_v, l.rotation % 4),
        }
    }
}

/// A copied file first: its pixels, not its icon. Then raw image data, GIF first since
/// only it keeps the animation.
fn clipboard_frames(cx: &App) -> Option<Frames> {
    let item = cx.read_from_clipboard()?;
    let entries = item.entries();
    let from_file = entries.iter().find_map(|entry| match entry {
        gpui::ClipboardEntry::String(s) => s.text().lines().find_map(|line| {
            let path = url::Url::parse(line.trim()).ok()?.to_file_path().ok()?;
            Frames::load(&path).ok()
        }),
        _ => None,
    });
    let mut images: Vec<&gpui::Image> = entries
        .iter()
        .filter_map(|entry| match entry {
            gpui::ClipboardEntry::Image(img) => Some(img),
            _ => None,
        })
        .collect();
    images.sort_by_key(|img| img.format != gpui::ImageFormat::Gif);
    from_file.or_else(|| images.into_iter().find_map(|img| Frames::decode(&img.bytes).ok()))
}

impl Render for Board {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing_id = self.editing.as_ref().map(|e| e.id);
        let visible: Vec<Annotation> = self
            .annotations
            .iter()
            .filter(|a| Some(a.id) != editing_id)
            .chain(self.current.as_ref())
            .cloned()
            .collect();
        self.keep_ticking(cx);
        let shown = annotate::at_time(&visible, self.clock.elapsed().as_secs_f64());

        // Upload what the primitives need now; paint can't borrow `self`.
        let probe = self.metrics();
        let mut textures = HashMap::new();
        for a in &shown {
            for prim in annotate::prims(a, &probe) {
                let req = match &prim {
                    annotate::Prim::Blur { radius, .. } => Texture::Blur(*radius),
                    annotate::Prim::Image { layer, .. } => Texture::Layer(layer),
                    _ => continue,
                };
                let key = Self::texture_key(&req);
                let texture = match req {
                    Texture::Blur(r) => self.texture(key, |b| raster::blurred(&mut b.blur_cache, &b.image, r).as_ref().clone()),
                    Texture::Layer(l) => self.texture(key, |_| annotate::oriented(l)),
                };
                textures.insert(key, texture);
            }
        }

        let selection = self
            .selected
            .filter(|id| Some(*id) != editing_id)
            .and_then(|id| self.index_of(id))
            .map(|i| self.annotations[i].clone());
        let base = self.base.clone();
        let iw = self.image.width() as f32;
        let unit = self.unit;
        let view_cell = self.view.clone();

        let text_field = self.editing.as_ref().and_then(|edit| {
            let i = self.index_of(edit.id)?;
            let a = &self.annotations[i];
            let view = self.view.get();
            let at = point(px(a.start().0 * view.scale), px(a.start().1 * view.scale));
            let size = annotate::text_size(&self.metrics()) * view.scale;
            Some(
                div().absolute().left(at.x).top(at.y).w(px(240.)).child(
                    Input::new(&edit.input)
                        .appearance(false)
                        .p_0()
                        .h(px(size * raster::LINE_HEIGHT))
                        .text_size(px(size))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(preview::hsla(a.color)),
                ),
            )
        });

        let _ = window;
        div()
            .id("board")
            .relative()
            .size_full()
            .cursor(if self.tool == Tool::Select { CursorStyle::Arrow } else { CursorStyle::Crosshair })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let scale = f32::from(bounds.size.width) / iw;
                        let view = View { origin: bounds.origin, scale };
                        view_cell.set(view);
                        let _ = window.paint_image(bounds, bounds, Default::default(), base, 0, false);
                        let m = Metrics { image_w: iw, unit, pt: 1. / scale };
                        let mut texture = |t: Texture| textures[&Board::texture_key(&t)].clone();
                        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                            for a in &shown {
                                for prim in annotate::prims(a, &m) {
                                    preview::paint(&prim, view, bounds, &mut texture, window, cx);
                                }
                            }
                        });
                        if let Some(a) = selection {
                            let frame = view.rect(annotate::frame(&a, &m, raster::text_width));
                            let grips: Vec<_> = annotate::grips(&a).into_iter().map(|g| view.to_window(g)).collect();
                            preview::paint_selection(frame, &grips, window);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(text_field)
    }
}

// ── Toolbar pieces shared by the window and inline editors ─────────────────

pub fn pill() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(3.))
        .p(px(5.))
        .rounded(px(12.))
        .bg(white(0.06))
        .border_1()
        .border_color(white(0.08))
}

pub fn divider() -> Div {
    div().w(px(1.)).h(px(22.)).mx(px(2.)).bg(white(0.12))
}

/// A borderless HUD button: highlight on hover, tooltip, click handler.
pub fn hud_button(
    id: impl Into<gpui::ElementId>,
    tip: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    tinted_button(id, tip, white(0.08), on_click)
}

/// A HUD button whose background is `hover` under the pointer.
pub fn tinted_button(
    id: impl Into<gpui::ElementId>,
    tip: impl Into<SharedString>,
    hover: gpui::Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let tip: SharedString = tip.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .on_click(on_click)
}

pub fn icon(name: IconName, size: f32, color: gpui::Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

fn tool_icon(tool: Tool) -> AnyElement {
    let name = match tool {
        Tool::Select => IconName::MousePointer2,
        Tool::Rect => IconName::RectangleHorizontal,
        Tool::Ellipse => IconName::Circle,
        Tool::Line => IconName::Slash,
        Tool::Arrow => IconName::ArrowUpRight,
        Tool::Highlight => IconName::Highlighter,
        Tool::Blur => IconName::Droplet,
        Tool::Pen => IconName::PenTool,
        Tool::Text => IconName::Type,
        Tool::Censor => IconName::Square,
        // SF Symbols' "1.circle"; lucide has no numbered circle.
        Tool::Counter | Tool::Image => {
            return div()
                .size(px(15.))
                .rounded_full()
                .border(px(1.5))
                .border_color(gpui::white())
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::BOLD)
                .child("1")
                .into_any_element();
        }
    };
    icon(name, 14., gpui::white()).into_any_element()
}

pub fn tools_pill(board: &Entity<Board>, cx: &App) -> Div {
    let current = board.read(cx).tool;
    let mut pill = pill();
    for (g, group) in Tool::GROUPS.iter().enumerate() {
        if g > 0 {
            pill = pill.child(divider());
        }
        for &tool in group.iter() {
            let selected = tool == current;
            let board = board.clone();
            pill = pill.child(
                tinted_button(tool.id(), tool.tooltip(), if selected { accent() } else { white(0.08) }, move |_, window, cx| {
                    board.update(cx, |b, cx| {
                        b.pick(tool, cx);
                        b.end_editing(window, cx);
                    })
                })
                .w(px(30.))
                .h(px(28.))
                .text_color(if selected { gpui::white() } else { gpui::hsla(0., 0., 0.8, 1.) })
                .when(selected, |d| d.bg(accent()))
                .child(tool_icon(tool)),
            );
        }
    }
    pill
}

/// Anchors a popover panel under its button, above everything else.
fn with_popover(button: impl IntoElement, open: bool, offset_x: f32, panel: impl Fn() -> AnyElement) -> Div {
    div().relative().child(button).when(open, |d| {
        d.child(
            div()
                .absolute()
                .top(px(38.))
                .left(px(offset_x))
                .child(deferred(gpui::anchored().snap_to_window_with_margin(px(8.)).child(panel())).with_priority(1)),
        )
    })
}

fn popover_panel(board: &Entity<Board>) -> Div {
    let board = board.clone();
    preview::hud(10.)
        .occlude()
        .on_mouse_down_out(move |_, _, cx| board.update(cx, |b, cx| b.close_popover(cx)))
}

pub fn style_pill(board: &Entity<Board>, cx: &App) -> Div {
    let b = board.read(cx);
    let (color, width, open) = (b.color, b.width, b.popover);

    let color_button = {
        let board = board.clone();
        hud_button("color", "Color", move |_, _, cx| board.update(cx, |b, cx| b.toggle_popover(Pop::Color, cx)))
            .p(px(8.))
            .child(div().size(px(18.)).rounded_full().bg(preview::hsla(color)).border(px(1.5)).border_color(white(0.55)))
    };
    let color_panel = {
        let board = board.clone();
        move || {
            let swatches = PALETTE.into_iter().map(|(name, c)| {
                let chosen = c == color;
                let board = board.clone();
                hud_button(name, name, move |_, _, cx| {
                    board.update(cx, |b, cx| {
                        b.apply_color(c, cx);
                        b.popover = None;
                    })
                })
                .size(px(26.))
                .rounded_full()
                .child(
                    div()
                        .size(px(24.))
                        .rounded_full()
                        .bg(preview::hsla(c))
                        .border(px(if chosen { 2.5 } else { 1. }))
                        .border_color(white(if chosen { 0.95 } else { 0.25 })),
                )
            });
            popover_panel(&board)
                .p(px(14.))
                .w(px(26. * 5. + 10. * 4. + 28.))
                .flex()
                .flex_wrap()
                .gap(px(10.))
                .children(swatches)
                .into_any_element()
        }
    };

    let width_button = {
        let board = board.clone();
        hud_button("width", "Stroke width · blur strength", move |_, _, cx| {
            board.update(cx, |b, cx| b.toggle_popover(Pop::Width, cx))
        })
        .size(px(30.))
        .child(icon(IconName::Baseline, 14., gpui::hsla(0., 0., 0.85, 1.)))
    };
    let width_panel = {
        let board = board.clone();
        move || {
            let rows = WIDTHS.into_iter().map(|(name, w)| {
                let chosen = (w - width).abs() < f32::EPSILON;
                let board = board.clone();
                hud_button(name, name, move |_, _, cx| {
                    board.update(cx, |b, cx| {
                        b.apply_width(w, cx);
                        b.popover = None;
                    })
                })
                .w(px(200.))
                .px(px(12.))
                .py(px(8.))
                .justify_start()
                .gap(px(12.))
                .child(
                    div()
                        .w(px(64.))
                        .h(px((w * 600.).max(1.5)))
                        .rounded(px(3.))
                        .bg(if chosen { accent() } else { gpui::hsla(0., 0., 0.8, 1.) }),
                )
                .child(div().flex_1().text_size(px(13.)).child(name))
                .when(chosen, |d| d.child(icon(IconName::Check, 11., gpui::white())))
            });
            popover_panel(&board).p(px(8.)).flex().flex_col().gap(px(2.)).children(rows).into_any_element()
        }
    };

    let sticker_button = {
        let board = board.clone();
        hud_button("stickers", "Stickers", move |_, _, cx| board.update(cx, |b, cx| b.toggle_popover(Pop::Sticker, cx)))
            .size(px(30.))
            .child(icon(IconName::Sticker, 15., gpui::hsla(0., 0., 0.85, 1.)))
    };
    let sticker_panel = {
        let (board, picker) = (board.clone(), b.stickers.clone());
        move || popover_panel(&board).children(picker.clone()).into_any_element()
    };

    pill()
        .child(with_popover(color_button, open == Some(Pop::Color), -60., color_panel))
        .child(with_popover(width_button, open == Some(Pop::Width), -85., width_panel))
        .child(divider())
        .child(with_popover(sticker_button, open == Some(Pop::Sticker), -220., sticker_panel))
}

/// Orange with a count when the scan on open already found something.
pub fn redact_button(board: &Entity<Board>, cx: &App) -> Stateful<Div> {
    let b = board.read(cx);
    let (hint, scanning) = (b.redact_hint(), b.is_scanning());
    let orange = preview::hsla(PALETTE[1].1);
    let tip = if hint > 0 {
        format!("Redact — {hint} sensitive item{} found", plural(hint))
    } else {
        "Redact — find & black out emails, phone numbers, card numbers and tokens".into()
    };
    let board = board.clone();
    hud_button("redact", tip, move |_, _, cx| board.update(cx, |b, cx| b.redact(cx)))
        .relative()
        .w(px(32.))
        .h(px(30.))
        .child(icon(
            if scanning { IconName::Hourglass } else { IconName::EyeOff },
            14.,
            if hint > 0 { orange } else { gpui::hsla(0., 0., 0.85, 1.) },
        ))
        .when(hint > 0, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .right(px(-4.))
                    .px(px(4.))
                    .py(px(1.))
                    .rounded_full()
                    .bg(orange)
                    .text_size(px(9.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(gpui::white())
                    .child(hint.to_string()),
            )
        })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub fn undo_button(board: &Entity<Board>, cx: &App) -> Stateful<Div> {
    let empty = board.read(cx).annotations.is_empty();
    let board = board.clone();
    hud_button("undo", "Undo", move |_, _, cx| board.update(cx, |b, cx| b.undo(cx)))
        .w(px(32.))
        .h(px(30.))
        .when(empty, |d| d.opacity(0.4))
        .child(icon(IconName::Undo2, 14., gpui::hsla(0., 0., 0.85, 1.)))
}

pub fn clear_button(board: &Entity<Board>, cx: &App) -> Stateful<Div> {
    let empty = board.read(cx).annotations.is_empty();
    let board = board.clone();
    hud_button("clear", "Clear all annotations", move |_, _, cx| board.update(cx, |b, cx| b.clear_all(cx)))
        .w(px(32.))
        .h(px(30.))
        .when(empty, |d| d.opacity(0.4))
        .child(icon(IconName::Trash, 14., gpui::hsla(0., 0., 0.85, 1.)))
}

pub fn transform_menu(board: &Entity<Board>, cx: &App) -> Div {
    let open = board.read(cx).popover == Some(Pop::Transform);
    let button = {
        let board = board.clone();
        hud_button("transform", "Flip & rotate", move |_, _, cx| {
            board.update(cx, |b, cx| b.toggle_popover(Pop::Transform, cx))
        })
        .w(px(30.))
        .h(px(28.))
        .child(icon(IconName::RotateCwSquare, 14., gpui::hsla(0., 0., 0.85, 1.)))
    };
    let panel = {
        let board = board.clone();
        move || {
            let item = |id: &'static str, label: &'static str, name: IconName, act: fn(&mut Board, &mut Context<Board>)| {
                let board = board.clone();
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(5.))
                    .w(px(180.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(|s| s.bg(accent()))
                    .text_size(px(13.))
                    .child(icon(name, 13., gpui::white()))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        board.update(cx, |b, cx| {
                            b.popover = None;
                            act(b, cx);
                        })
                    })
            };
            popover_panel(&board)
                .p(px(5.))
                .flex()
                .flex_col()
                .child(item("flip-h", "Flip horizontal", IconName::FlipHorizontal2, |b, cx| b.flip(true, cx)))
                .child(item("flip-v", "Flip vertical", IconName::FlipVertical2, |b, cx| b.flip(false, cx)))
                .child(div().h(px(1.)).my(px(4.)).bg(white(0.12)))
                .child(item("rot-l", "Rotate left", IconName::RotateCcw, |b, cx| b.rotate(false, cx)))
                .child(item("rot-r", "Rotate right", IconName::RotateCw, |b, cx| b.rotate(true, cx)))
                .into_any_element()
        }
    };
    with_popover(button, open, -75., panel)
}
