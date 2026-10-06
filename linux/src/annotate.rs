//! Annotation model and the geometry both renderers draw from. The editor preview
//! (GPUI) and the exported file (tiny-skia) consume the same `Prim` list, so what is
//! on screen is what lands in the file. Mirrors EditorView.swift.

use std::f32::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use image::RgbaImage;

use crate::frames::Frames;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rect,
    Ellipse,
    Line,
    Arrow,
    Highlight,
    Blur,
    Pen,
    Text,
    Counter,
    /// A pasted image; never picked from the toolbar.
    Image,
    /// A solid box Redact lays over sensitive text; never picked from the toolbar.
    Censor,
}

impl Tool {
    /// Toolbar groups, in the macOS order.
    pub const GROUPS: [&'static [Tool]; 4] = [
        &[Tool::Select],
        &[Tool::Rect, Tool::Ellipse, Tool::Line, Tool::Arrow],
        &[Tool::Highlight, Tool::Blur, Tool::Pen],
        &[Tool::Text, Tool::Counter],
    ];

    pub fn pickable() -> impl Iterator<Item = Tool> {
        Self::GROUPS.into_iter().flatten().copied()
    }

    pub fn id(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Line => "line",
            Tool::Arrow => "arrow",
            Tool::Highlight => "highlight",
            Tool::Blur => "blur",
            Tool::Pen => "pen",
            Tool::Text => "text",
            Tool::Counter => "counter",
            Tool::Image => "image",
            Tool::Censor => "censor",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::pickable().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Highlight => "Highlight",
            Tool::Blur => "Blur",
            Tool::Pen => "Pen",
            Tool::Text => "Text",
            Tool::Counter => "Counter",
            Tool::Image => "Image",
            Tool::Censor => "Redaction",
        }
    }

    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            Tool::Select => "v",
            Tool::Rect => "r",
            Tool::Ellipse => "o",
            Tool::Line => "l",
            Tool::Arrow => "a",
            Tool::Highlight => "h",
            Tool::Blur => "b",
            Tool::Pen => "p",
            Tool::Text => "t",
            Tool::Counter => "n",
            Tool::Image | Tool::Censor => return None,
        })
    }

    pub fn tooltip(self) -> String {
        match self.key() {
            Some(k) => format!("{} ({})", self.label(), k.to_uppercase()),
            None => self.label().into(),
        }
    }

    pub fn draws_on_drag(self) -> bool {
        !matches!(self, Tool::Select | Tool::Text | Tool::Counter | Tool::Image | Tool::Censor)
    }

    pub fn recolorable(self) -> bool {
        matches!(
            self,
            Tool::Rect
                | Tool::Ellipse
                | Tool::Line
                | Tool::Arrow
                | Tool::Highlight
                | Tool::Pen
                | Tool::Text
                | Tool::Counter
                | Tool::Censor
        )
    }

    /// Only the tools whose look the width setting changes.
    pub fn resizable_stroke(self) -> bool {
        matches!(self, Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Pen | Tool::Blur)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);

    pub fn with_alpha(self, a: f32) -> Rgba {
        let [r, g, b, _] = self.0;
        Rgba([r, g, b, (a * 255.).round() as u8])
    }
}

/// Same order as the macOS editor; values are the system colours AppKit uses there.
pub const PALETTE: [(&str, Rgba); 9] = [
    ("Red", Rgba([255, 59, 48, 255])),
    ("Orange", Rgba([255, 149, 0, 255])),
    ("Yellow", Rgba([255, 204, 0, 255])),
    ("Green", Rgba([52, 199, 89, 255])),
    ("Blue", Rgba([0, 122, 255, 255])),
    ("Purple", Rgba([175, 82, 222, 255])),
    ("Pink", Rgba([255, 45, 85, 255])),
    ("White", Rgba::WHITE),
    ("Black", Rgba::BLACK),
];

/// Stroke widths as a fraction of the image width, so a stroke looks the same on a
/// small crop and on a 5K screenshot.
pub const WIDTHS: [(&str, f32); 4] = [("Thin", 0.003), ("Normal", 0.005), ("Bold", 0.008), ("Heavy", 0.012)];

pub type Pt = (f32, f32);

/// A pasted image or sticker layer's pixels and orientation.
#[derive(Clone, Debug)]
pub struct Layer {
    /// The frame on show.
    pub image: Arc<RgbaImage>,
    /// Every frame, when the layer moves.
    pub frames: Option<Arc<Frames>>,
    pub flip_h: bool,
    pub flip_v: bool,
    /// Quarter turns clockwise.
    pub rotation: u8,
}

impl Layer {
    pub fn new(frames: Frames) -> Self {
        let image = frames.first().clone();
        let frames = frames.is_animated().then(|| Arc::new(frames));
        Self { image, frames, flip_h: false, flip_v: false, rotation: 0 }
    }

    /// This layer showing the frame `t` seconds into its loop.
    pub fn at(&self, t: f64) -> Self {
        match &self.frames {
            Some(frames) => Self { image: frames.at(t).clone(), ..self.clone() },
            None => self.clone(),
        }
    }
}

/// The frames of the annotation's layer, when it moves.
pub fn animation(a: &Annotation) -> Option<&Arc<Frames>> {
    a.layer.as_ref()?.frames.as_ref()
}

/// The annotations as they look `t` seconds in.
pub fn at_time(annotations: &[Annotation], t: f64) -> Vec<Annotation> {
    annotations
        .iter()
        .map(|a| match &a.layer {
            Some(layer) if layer.frames.is_some() => Annotation { layer: Some(layer.at(t)), ..a.clone() },
            _ => a.clone(),
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct Annotation {
    /// Stable identity for selection and undo while indices shift.
    pub id: u64,
    pub tool: Tool,
    pub color: Rgba,
    pub width: f32,
    /// Image pixel coordinates.
    pub points: Vec<Pt>,
    pub text: String,
    pub number: u32,
    pub layer: Option<Layer>,
}

impl Annotation {
    pub fn new(tool: Tool, color: Rgba, width: f32, points: Vec<Pt>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Self { id, tool, color, width, points, text: String::new(), number: 0, layer: None }
    }

    pub fn start(&self) -> Pt {
        self.points[0]
    }

    pub fn end(&self) -> Pt {
        *self.points.last().unwrap()
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        for p in &mut self.points {
            p.0 += dx;
            p.1 += dy;
        }
    }

    pub fn bounding_rect(&self) -> Rect {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in &self.points {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    /// Shapes under 0.4 % of the image in both axes are slips of the mouse.
    pub fn is_big_enough(&self, image_w: f32, image_h: f32) -> bool {
        let (s, e) = (self.start(), self.end());
        (e.0 - s.0).abs() > 0.004 * image_w || (e.1 - s.1).abs() > 0.004 * image_h
    }
}

/// How sizes on the canvas relate to image pixels.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub image_w: f32,
    /// The inline editor sizes strokes and text as if the canvas were ~900 pt wide.
    pub unit: f32,
    /// Image pixels per screen point, for tolerances given in points.
    pub pt: f32,
}

impl Metrics {
    /// The nominal width stroke, text and counter sizes are fractions of.
    pub fn w(&self) -> f32 {
        self.image_w * self.unit
    }

    fn line_width(&self, a: &Annotation) -> f32 {
        (a.width * self.w()).max(self.pt)
    }

    fn stroke_extent(&self, a: &Annotation) -> f32 {
        if a.tool == Tool::Highlight { 0.03 * self.w() } else { self.line_width(a) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn from_points(a: Pt, b: Pt) -> Self {
        Rect { x: a.0.min(b.0), y: a.1.min(b.1), w: (b.0 - a.0).abs(), h: (b.1 - a.1).abs() }
    }

    pub fn contains(&self, (x, y): Pt) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn inflate(&self, d: f32) -> Rect {
        Rect { x: self.x - d, y: self.y - d, w: self.w + 2. * d, h: self.h + 2. * d }
    }

    pub fn center(&self) -> Pt {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
}

pub enum Prim {
    Stroke { points: Vec<Pt>, width: f32, color: Rgba, closed: bool },
    Fill { points: Vec<Pt>, color: Rgba },
    /// Redraw the pre-blurred base image inside `rect`.
    Blur { rect: Rect, corner: f32, radius: f32 },
    /// `origin` is the top-left corner, or the centre when `centered`.
    Text { origin: Pt, size: f32, text: String, color: Rgba, centered: bool },
    Image { rect: Rect, layer: Layer },
}

pub fn text_size(m: &Metrics) -> f32 {
    0.022 * m.w()
}

pub fn counter_radius(m: &Metrics) -> f32 {
    0.013 * m.w()
}

/// Blur strength is a fraction of the image itself, not of the nominal width, so the
/// preview at any zoom and the export blur alike.
pub fn blur_radius(width: f32, image_w: f32) -> f32 {
    (width * 4.).max(0.004) * image_w
}

pub fn prims(a: &Annotation, m: &Metrics) -> Vec<Prim> {
    let w = m.w();
    let lw = m.line_width(a);
    let (s, e) = (a.start(), a.end());
    let stroke = |points: Vec<Pt>, width: f32, color: Rgba, closed: bool| Prim::Stroke { points, width, color, closed };
    match a.tool {
        Tool::Select => vec![],
        Tool::Rect => vec![stroke(rounded_rect(Rect::from_points(s, e), lw), lw, a.color, true)],
        Tool::Ellipse => vec![stroke(ellipse(Rect::from_points(s, e)), lw, a.color, true)],
        Tool::Line => vec![stroke(vec![s, e], lw, a.color, false)],
        Tool::Highlight => vec![stroke(vec![s, e], 0.03 * w, a.color.with_alpha(0.35), false)],
        Tool::Pen => vec![stroke(a.points.clone(), lw, a.color, false)],
        Tool::Arrow => {
            let head = 0.02 * w + lw * 2.;
            let angle = (e.1 - s.1).atan2(e.0 - s.0);
            let spread = PI / 7.;
            let wing = |sign: f32| {
                let t = angle + PI + sign * spread;
                (e.0 + t.cos() * head, e.1 + t.sin() * head)
            };
            vec![stroke(vec![s, e], lw, a.color, false), stroke(vec![wing(-1.), e, wing(1.)], lw, a.color, false)]
        }
        Tool::Blur => {
            let rect = Rect::from_points(s, e);
            if rect.w <= m.pt || rect.h <= m.pt {
                return vec![];
            }
            let corner = (6. * m.pt).min(rect.w.min(rect.h) / 4.);
            vec![Prim::Blur { rect, corner, radius: blur_radius(a.width, m.image_w) }]
        }
        Tool::Text => vec![Prim::Text { origin: s, size: text_size(m), text: a.text.clone(), color: a.color, centered: false }],
        Tool::Counter => {
            let r = counter_radius(m);
            vec![
                Prim::Fill { points: ellipse(Rect { x: s.0 - r, y: s.1 - r, w: 2. * r, h: 2. * r }), color: a.color },
                Prim::Text { origin: s, size: r * 1.2, text: a.number.to_string(), color: Rgba::WHITE, centered: true },
            ]
        }
        Tool::Image => match &a.layer {
            Some(layer) => vec![Prim::Image { rect: Rect::from_points(s, e), layer: layer.clone() }],
            None => vec![],
        },
        // Solid, not blurred: a blur is still made of the pixels underneath, a flat fill
        // leaves nothing of them in the exported file.
        Tool::Censor => {
            let rect = Rect::from_points(s, e);
            if rect.w <= 0. || rect.h <= 0. {
                return vec![];
            }
            vec![Prim::Fill { points: rounded_rect(rect, (3. * m.pt).min(rect.w.min(rect.h) / 6.)), color: a.color }]
        }
    }
}

/// Where the layer sits on the canvas, stroke included; matches `prims`.
pub fn frame(a: &Annotation, m: &Metrics, text_width: impl Fn(&str, f32) -> f32) -> Rect {
    let p0 = a.start();
    match a.tool {
        Tool::Text => {
            let size = text_size(m);
            let text = if a.text.is_empty() { " " } else { a.text.as_str() };
            Rect { x: p0.0, y: p0.1, w: text_width(text, size), h: size * crate::raster::LINE_HEIGHT }
        }
        Tool::Counter => {
            let r = counter_radius(m);
            Rect { x: p0.0 - r, y: p0.1 - r, w: 2. * r, h: 2. * r }
        }
        Tool::Blur | Tool::Image | Tool::Censor => a.bounding_rect(),
        _ => a.bounding_rect().inflate(m.stroke_extent(a) / 2.),
    }
}

/// Hollow shapes and lines are hit near their stroke; filled ones anywhere inside.
pub fn hits(a: &Annotation, p: Pt, m: &Metrics, text_width: impl Fn(&str, f32) -> f32) -> bool {
    let tol = m.stroke_extent(a) / 2. + 6. * m.pt;
    let near = |u: Pt, v: Pt| dist_to_segment(p, u, v) <= tol;
    let (first, last) = (a.start(), a.end());
    match a.tool {
        Tool::Line | Tool::Arrow | Tool::Highlight => near(first, last),
        Tool::Pen => {
            if a.points.len() == 1 {
                near(first, first)
            } else {
                a.points.windows(2).any(|s| near(s[0], s[1]))
            }
        }
        Tool::Rect => {
            let r = Rect::from_points(first, last);
            let inner = r.inflate(-tol);
            r.inflate(tol).contains(p) && !(inner.w >= 0. && inner.h >= 0. && inner.contains(p))
        }
        Tool::Ellipse => {
            let r = Rect::from_points(first, last);
            let (rx, ry) = (r.w / 2., r.h / 2.);
            if rx <= tol || ry <= tol {
                return r.inflate(tol).contains(p);
            }
            let (cx, cy) = r.center();
            let d = ((p.0 - cx) / rx).hypot((p.1 - cy) / ry);
            (d - 1.).abs() * rx.min(ry) <= tol
        }
        Tool::Select => false,
        Tool::Blur | Tool::Image | Tool::Censor | Tool::Text | Tool::Counter => {
            frame(a, m, text_width).inflate(4. * m.pt).contains(p)
        }
    }
}

/// Handles: TL, TR, BR, BL for box shapes (to resize); both ends for lines; none for
/// pen strokes, text and counters (they only move).
pub fn grips(a: &Annotation) -> Vec<Pt> {
    match a.tool {
        Tool::Rect | Tool::Ellipse | Tool::Blur | Tool::Image | Tool::Censor => {
            let r = a.bounding_rect();
            vec![(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)]
        }
        Tool::Line | Tool::Arrow | Tool::Highlight => vec![a.start(), a.end()],
        _ => vec![],
    }
}

pub fn grip_at(a: &Annotation, p: Pt, m: &Metrics) -> Option<usize> {
    grips(a).iter().position(|g| (p.0 - g.0).hypot(p.1 - g.1) <= 9. * m.pt)
}

/// Drags corner `corner` (TL, TR, BR, BL) to `p`; `points` must be [top-left, bottom-right].
pub fn resize(a: &mut Annotation, corner: usize, p: Pt) {
    if a.points.len() < 2 {
        return;
    }
    match corner {
        0 => a.points[0] = p,
        1 => {
            a.points[1].0 = p.0;
            a.points[0].1 = p.1;
        }
        2 => a.points[1] = p,
        3 => {
            a.points[0].0 = p.0;
            a.points[1].1 = p.1;
        }
        _ => {}
    }
}

fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0. { 0. } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0., 1.) };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    (p.0 - cx).hypot(p.1 - cy)
}

pub fn ellipse(r: Rect) -> Vec<Pt> {
    let (cx, cy, rx, ry) = (r.x + r.w / 2., r.y + r.h / 2., r.w / 2., r.h / 2.);
    let n = 72;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * 2. * PI;
            (cx + rx * t.cos(), cy + ry * t.sin())
        })
        .collect()
}

pub fn rounded_rect(r: Rect, radius: f32) -> Vec<Pt> {
    let radius = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
    if radius < 0.5 {
        return vec![(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)];
    }
    let corners = [
        (r.x + r.w - radius, r.y + radius, -PI / 2.),
        (r.x + r.w - radius, r.y + r.h - radius, 0.),
        (r.x + radius, r.y + r.h - radius, PI / 2.),
        (r.x + radius, r.y + radius, PI),
    ];
    let steps = 6;
    corners
        .iter()
        .flat_map(|&(cx, cy, start)| {
            (0..=steps).map(move |i| {
                let t = start + i as f32 / steps as f32 * PI / 2.;
                (cx + radius * t.cos(), cy + radius * t.sin())
            })
        })
        .collect()
}

/// The layer's pixels with its flips and rotation applied.
pub fn oriented(layer: &Layer) -> RgbaImage {
    let mut img = (*layer.image).clone();
    if layer.flip_h {
        img = image::imageops::flip_horizontal(&img);
    }
    if layer.flip_v {
        img = image::imageops::flip_vertical(&img);
    }
    match layer.rotation % 4 {
        1 => image::imageops::rotate90(&img),
        2 => image::imageops::rotate180(&img),
        3 => image::imageops::rotate270(&img),
        _ => img,
    }
}
