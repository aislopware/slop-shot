//! CPU rendering of annotations onto the full-resolution image for export.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use ab_glyph::{Font as _, FontArc, PxScale, ScaleFont as _};
use image::RgbaImage;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::annotate::{Annotation, Metrics, Prim, Pt, Rgba, prims, rounded_rect};

pub struct UiFont {
    /// Family name GPUI is asked for, so preview and export use the same face.
    pub family: String,
    pub font: FontArc,
}

/// Precomposed Vietnamese letters. A face without them gets them decomposed by
/// shaping, and a loose dot under "ư" lands beside it; Ubuntu and Ubuntu Sans lack them.
const VIETNAMESE: &str = "ạảấầẩẫậắằẳẵặẹẻẽếềểễệỉịọỏốồổỗộớờởỡợụủứừửữựỳỵỷỹ";

/// The system sans-serif face in bold, resolved once: the first that draws Vietnamese,
/// as GNOME's own text falls back to one.
pub fn ui_font() -> &'static UiFont {
    static FONT: OnceLock<UiFont> = OnceLock::new();
    FONT.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let families = [
            fontdb::Family::Name("Ubuntu"),
            fontdb::Family::Name("Ubuntu Sans"),
            fontdb::Family::Name("Noto Sans"),
            fontdb::Family::Name("DejaVu Sans"),
            fontdb::Family::SansSerif,
        ];
        let load = |family: fontdb::Family| {
            let query = fontdb::Query { families: &[family], weight: fontdb::Weight::BOLD, ..Default::default() };
            let id = db.query(&query)?;
            let font = db
                .with_face_data(id, |data, index| {
                    ab_glyph::FontVec::try_from_vec_and_index(data.to_vec(), index).ok().map(FontArc::new)
                })
                .flatten()?;
            Some(UiFont { family: db.face(id)?.families[0].0.clone(), font })
        };
        let mut installed: Vec<UiFont> = families.into_iter().filter_map(load).collect();
        let pick = installed.iter().position(|f| covers(&f.font, VIETNAMESE)).unwrap_or(0);
        assert!(!installed.is_empty(), "no sans-serif font installed");
        installed.swap_remove(pick)
    })
}

fn covers(font: &FontArc, text: &str) -> bool {
    text.chars().all(|c| font.glyph_id(c).0 != 0)
}

/// ab_glyph scales by ascent-to-descent height; GPUI and CSS size text by the em.
pub fn px_scale(font: &FontArc, em: f32) -> PxScale {
    let upem = font.units_per_em().unwrap_or(1000.);
    PxScale::from(em * font.height_unscaled() / upem)
}

pub fn text_width(text: &str, em: f32) -> f32 {
    let font = &ui_font().font;
    let scaled = font.as_scaled(px_scale(font, em));
    let mut width = 0.;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            width += scaled.kern(p, id);
        }
        width += scaled.h_advance(id);
        prev = Some(id);
    }
    width
}

pub const LINE_HEIGHT: f32 = 1.25;

/// Box blur run three times approximates a Gaussian; running sums keep it O(pixels)
/// whatever the radius.
pub fn blur(image: &RgbaImage, radius: f32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let (w, h) = (w as usize, h as usize);
    let r = ((radius / 3f32.sqrt()).round() as usize).max(1);
    let mut a: Vec<u8> = image.as_raw().clone();
    let mut b = vec![0u8; a.len()];
    for _ in 0..3 {
        box_pass(&a, &mut b, w, h, r, 4, w * 4);
        box_pass(&b, &mut a, h, w, r, w * 4, 4);
    }
    RgbaImage::from_raw(w as u32, h as u32, a).unwrap()
}

/// One blur pass along lines of `len` samples, `step` bytes apart, lines `stride` apart.
fn box_pass(src: &[u8], dst: &mut [u8], len: usize, lines: usize, r: usize, step: usize, stride: usize) {
    let window = (2 * r + 1) as u32;
    for line in 0..lines {
        let base = line * stride;
        let at = |i: isize| base + (i.clamp(0, len as isize - 1) as usize) * step;
        for c in 0..4 {
            let mut sum: u32 = (-(r as isize)..=r as isize).map(|i| src[at(i) + c] as u32).sum();
            for i in 0..len as isize {
                dst[base + i as usize * step + c] = ((sum + window / 2) / window) as u8;
                sum += src[at(i + r as isize + 1) + c] as u32;
                sum -= src[at(i - r as isize) + c] as u32;
            }
        }
    }
}

pub type BlurCache = HashMap<u32, Arc<RgbaImage>>;

pub fn blurred<'a>(cache: &'a mut BlurCache, base: &RgbaImage, radius: f32) -> &'a Arc<RgbaImage> {
    cache
        .entry(radius.round() as u32)
        .or_insert_with(|| Arc::new(blur(base, radius)))
}

/// `unit` is the inline editor's size scale (1 in the editor window).
pub fn flatten(base: &RgbaImage, annotations: &[Annotation], unit: f32, blur_cache: &mut BlurCache) -> RgbaImage {
    let (w, h) = base.dimensions();
    let mut pixmap = Pixmap::new(w, h).unwrap();
    // Screenshots are opaque, so straight and premultiplied RGBA are the same bytes.
    pixmap.data_mut().copy_from_slice(base.as_raw());
    let metrics = Metrics { image_w: w as f32, unit, pt: 1. };
    for a in annotations {
        for prim in prims(a, &metrics) {
            draw(&mut pixmap, &prim, base, blur_cache);
        }
    }
    RgbaImage::from_raw(w, h, pixmap.take()).unwrap()
}

fn paint(color: Rgba) -> Paint<'static> {
    let [r, g, b, a] = color.0;
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    paint
}

fn path(points: &[Pt], closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

fn draw(pixmap: &mut Pixmap, prim: &Prim, base: &RgbaImage, blur_cache: &mut BlurCache) {
    match prim {
        Prim::Stroke { points, width, color, closed } => {
            if let Some(p) = path(points, *closed) {
                let stroke = Stroke {
                    width: *width,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&p, &paint(*color), &stroke, Transform::identity(), None);
            }
        }
        Prim::Fill { points, color } => {
            if let Some(p) = path(points, true) {
                pixmap.fill_path(&p, &paint(*color), FillRule::Winding, Transform::identity(), None);
            }
        }
        Prim::Blur { rect, corner, radius } => {
            let blurred = blurred(blur_cache, base, *radius);
            fill_with_image(pixmap, &rounded_rect(*rect, *corner), blurred, Transform::identity());
        }
        Prim::Text { origin, size, text, color, centered } => draw_text(pixmap, *origin, *size, text, *color, *centered),
        Prim::Image { rect, layer } => {
            let img = crate::annotate::oriented(layer);
            let (iw, ih) = (img.width().max(1) as f32, img.height().max(1) as f32);
            let transform = Transform::from_row(rect.w / iw, 0., 0., rect.h / ih, rect.x, rect.y);
            let corners = [(rect.x, rect.y), (rect.x + rect.w, rect.y), (rect.x + rect.w, rect.y + rect.h), (rect.x, rect.y + rect.h)];
            fill_with_image(pixmap, &corners, &img, transform);
        }
    }
}

/// Fills the polygon with `image` mapped through `transform` (image px to canvas).
fn fill_with_image(pixmap: &mut Pixmap, polygon: &[Pt], image: &RgbaImage, transform: Transform) {
    let Some(p) = path(polygon, true) else { return };
    let Some(src) = premultiplied(image) else { return };
    let paint = Paint {
        shader: tiny_skia::Pattern::new(
            src.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bicubic,
            1.,
            transform,
        ),
        anti_alias: true,
        ..Default::default()
    };
    pixmap.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
}

fn premultiplied(image: &RgbaImage) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(image.width(), image.height())?;
    for (dst, src) in pixmap.data_mut().chunks_exact_mut(4).zip(image.as_raw().chunks_exact(4)) {
        let a = src[3] as u16;
        for k in 0..3 {
            dst[k] = ((src[k] as u16 * a + 127) / 255) as u8;
        }
        dst[3] = src[3];
    }
    Some(pixmap)
}

/// Top of the text box to baseline, matching how GPUI centres a line in its line height.
pub fn baseline_offset(em: f32) -> f32 {
    let font = &ui_font().font;
    let scaled = font.as_scaled(px_scale(font, em));
    let (ascent, descent) = (scaled.ascent(), scaled.descent());
    (em * LINE_HEIGHT - (ascent - descent)) / 2. + ascent
}

fn draw_text(pixmap: &mut Pixmap, origin: Pt, em: f32, text: &str, color: Rgba, centered: bool) {
    let font = &ui_font().font;
    let scaled = font.as_scaled(px_scale(font, em));
    let (mut x, top) = if centered {
        (origin.0 - text_width(text, em) / 2., origin.1 - em * LINE_HEIGHT / 2.)
    } else {
        origin
    };
    let baseline = top + baseline_offset(em);
    let [cr, cg, cb, ca] = color.0;
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let data = pixmap.data_mut();
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        let glyph = id.with_scale_and_position(scaled.scale(), ab_glyph::point(x, baseline));
        x += scaled.h_advance(id);
        prev = Some(id);
        let Some(outlined) = font.outline_glyph(glyph) else { continue };
        let bb = outlined.px_bounds();
        outlined.draw(|gx, gy, coverage| {
            let px = bb.min.x as i32 + gx as i32;
            let py = bb.min.y as i32 + gy as i32;
            if px < 0 || py < 0 || px >= pw || py >= ph {
                return;
            }
            let a = coverage.min(1.) * ca as f32 / 255.;
            let i = (py as usize * pw as usize + px as usize) * 4;
            for (k, c) in [cr, cg, cb].into_iter().enumerate() {
                data[i + k] = (c as f32 * a + data[i + k] as f32 * (1. - a)).round() as u8;
            }
            data[i + 3] = (255. * a + data[i + 3] as f32 * (1. - a)).round() as u8;
        });
    }
}

pub fn encode(image: &RgbaImage, format: crate::settings::ImageFormat) -> anyhow::Result<Vec<u8>> {
    use crate::settings::ImageFormat;
    let mut out = std::io::Cursor::new(Vec::new());
    let dynamic = image::DynamicImage::ImageRgba8(image.clone());
    match format {
        ImageFormat::Png => dynamic.write_to(&mut out, image::ImageFormat::Png)?,
        ImageFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgb8(dynamic.to_rgb8());
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
            rgb.write_with_encoder(encoder)?
        }
        ImageFormat::Tiff => dynamic.write_to(&mut out, image::ImageFormat::Tiff)?,
        ImageFormat::Gif => dynamic.write_to(&mut out, image::ImageFormat::Gif)?,
        ImageFormat::Bmp => dynamic.write_to(&mut out, image::ImageFormat::Bmp)?,
    }
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ui_font_draws_vietnamese() {
        let font = ui_font();
        assert!(covers(&font.font, VIETNAMESE), "{} can't draw Vietnamese", font.family);
    }
}
