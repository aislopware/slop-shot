//! GPU preview of the same `Prim`s the exporter rasterizes.

use std::sync::Arc;

use gpui::{
    App, Bounds, Font, FontWeight, Hsla, PathBuilder, Pixels, Point, RenderImage,
    SharedString, StrokeOptions, TextAlign, TextRun, Window, point, px, size,
};
use image::RgbaImage;
use lyon::tessellation::{LineCap, LineJoin};

use crate::annotate::{Layer, Prim, Pt, Rgba};
use crate::raster::{self, LINE_HEIGHT};

/// GPUI textures are BGRA.
pub fn to_render_image(image: &RgbaImage) -> Arc<RenderImage> {
    let mut bgra = image.clone();
    for px in bgra.pixels_mut() {
        px.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(bgra)]))
}

pub fn hsla(c: Rgba) -> Hsla {
    let [r, g, b, a] = c.0;
    gpui::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: a as f32 / 255. }.into()
}

/// Maps image pixels to window points.
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub origin: Point<Pixels>,
    /// Window points per image pixel.
    pub scale: f32,
}

impl View {
    pub fn to_window(self, (x, y): Pt) -> Point<Pixels> {
        self.origin + point(px(x * self.scale), px(y * self.scale))
    }

    pub fn to_image(self, p: Point<Pixels>) -> Pt {
        let d = p - self.origin;
        (f32::from(d.x) / self.scale, f32::from(d.y) / self.scale)
    }

    pub fn rect(self, r: crate::annotate::Rect) -> Bounds<Pixels> {
        Bounds::new(self.to_window((r.x, r.y)), size(px(r.w * self.scale), px(r.h * self.scale)))
    }
}

/// GPUI's system font on Ubuntu is Ubuntu, which can't draw Vietnamese; the UI uses
/// the face the editor draws text with instead.
pub fn apply_ui_font(cx: &mut gpui::App) {
    gpui_component::Theme::global_mut(cx).font_family = raster::ui_font().family.clone().into();
}

pub fn ui_font() -> Font {
    Font { weight: FontWeight::BOLD, ..gpui::font(raster::ui_font().family.clone()) }
}

/// A texture a primitive needs: the base image blurred by a radius, or a pasted layer.
pub enum Texture<'a> {
    Blur(f32),
    Layer(&'a Layer),
}

/// `texture` uploads (or returns the cached) image a primitive draws.
pub fn paint(
    prim: &Prim,
    view: View,
    image_bounds: Bounds<Pixels>,
    texture: &mut dyn FnMut(Texture) -> Arc<RenderImage>,
    window: &mut Window,
    cx: &mut App,
) {
    let s = view.scale;
    match prim {
        Prim::Stroke { points, width, color, closed } => {
            let options = StrokeOptions::default()
                .with_line_width(width * s)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round);
            let mut pb = PathBuilder::stroke(px(width * s)).with_style(gpui::PathStyle::Stroke(options));
            let Some((first, rest)) = points.split_first() else { return };
            pb.move_to(view.to_window(*first));
            for p in rest {
                pb.line_to(view.to_window(*p));
            }
            if *closed {
                pb.close();
            }
            if let Ok(path) = pb.build() {
                window.paint_path(path, hsla(*color));
            }
        }
        Prim::Fill { points, color } => {
            let mut pb = PathBuilder::fill();
            let pts: Vec<_> = points.iter().map(|p| view.to_window(*p)).collect();
            pb.add_polygon(&pts, true);
            if let Ok(path) = pb.build() {
                window.paint_path(path, hsla(*color));
            }
        }
        Prim::Blur { rect, corner, radius } => {
            let texture = texture(Texture::Blur(*radius));
            let radii = gpui::Corners::all(px(corner * s));
            let _ = window.paint_image(view.rect(*rect), image_bounds, radii, texture, 0, false);
        }
        Prim::Image { rect, layer } => {
            let texture = texture(Texture::Layer(layer));
            let bounds = view.rect(*rect);
            let _ = window.paint_image(bounds, bounds, Default::default(), texture, 0, false);
        }
        Prim::Text { origin, size: em, text, color, centered } => {
            if text.is_empty() {
                return;
            }
            let font_size = px(em * s);
            let run = TextRun {
                len: text.len(),
                font: ui_font(),
                color: hsla(*color),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().shape_line(SharedString::from(text.clone()), font_size, &[run], None);
            let line_height = px(em * LINE_HEIGHT * s);
            let mut at = view.to_window(*origin);
            if *centered {
                at -= point(line.width / 2., line_height / 2.);
            }
            let _ = line.paint(at, line_height, TextAlign::Left, None, window, cx);
        }
    }
}

/// macOS's default accent (dark appearance).
pub fn accent() -> Hsla {
    gpui::rgb(0x0A84FF).into()
}

pub fn white(alpha: f32) -> Hsla {
    gpui::hsla(0., 0., 1., alpha)
}

pub fn black(alpha: f32) -> Hsla {
    gpui::hsla(0., 0., 0., alpha)
}

/// The dark translucent panel macOS draws with `.hudBackground`.
pub fn hud(radius: f32) -> gpui::Div {
    use gpui::{Styled as _, div};
    div()
        .rounded(px(radius))
        .bg(gpui::hsla(0., 0., 0.11, 0.92))
        .border_1()
        .border_color(white(0.1))
        .shadow(vec![gpui::BoxShadow {
            color: black(0.4),
            offset: point(px(0.), px(4.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
                inset: false,
        }])
}

/// Dashed box and white handles around the selected layer (EditorView.drawSelection).
pub fn paint_selection(frame: Bounds<Pixels>, grips: &[Point<Pixels>], window: &mut Window) {
    if grips.len() != 2 {
        let outer = Bounds::new(frame.origin - point(px(3.), px(3.)), frame.size + size(px(6.), px(6.)));
        window.paint_quad(gpui::outline(outer, white(0.9), gpui::BorderStyle::Dashed));
    }
    let hs = px(9.);
    for c in grips {
        let sq = Bounds::new(*c - point(hs / 2., hs / 2.), size(hs, hs));
        window.paint_quad(gpui::quad(sq, px(2.), gpui::white(), px(1.5), accent(), gpui::BorderStyle::Solid));
    }
}

/// `window_border()` draws the client-side shadow inside the window, this far from
/// each edge, so windows are this much bigger than their macOS content size.
pub const CSD_SHADOW: f32 = 20.;

pub fn csd_size(w: f32, h: f32) -> gpui::Size<gpui::Pixels> {
    gpui::size(px(w + 2. * CSD_SHADOW), px(h + 2. * CSD_SHADOW))
}
