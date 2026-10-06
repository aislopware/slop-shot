//! Draws the region effects onto a frame (VideoCompositionBuilder.swift's Core Image
//! pass). Preview and export both call `compose`, so the file matches what was seen.

use ab_glyph::{Font as _, ScaleFont as _};
use image::{Rgba as Px, RgbaImage, imageops};

use super::model::{CensorStyle, Kind, NRect, Segment, zoom_level};
use crate::raster;

/// The effects frozen for one frame size: captions are rasterized once, not per frame.
pub struct Plan {
    width: u32,
    height: u32,
    zooms: Vec<Segment>,
    censors: Vec<Segment>,
    texts: Vec<(Segment, Option<RgbaImage>)>,
}

impl Plan {
    pub fn new(segments: &[Segment], width: u32, height: u32) -> Self {
        let of = |kind| segments.iter().filter(|s| s.kind == kind).cloned().collect::<Vec<_>>();
        let texts = of(Kind::Text)
            .into_iter()
            .map(|s| {
                let (w, h) = ((s.rect.w * width as f64).max(4.), (s.rect.h * height as f64).max(4.));
                let image = caption(&s.text, w.round() as u32, h.round() as u32, (s.font_scale * height as f64) as f32, s.text_color.0, s.shadow);
                (s, image)
            })
            .collect();
        Self { width, height, zooms: of(Kind::Zoom), censors: of(Kind::Censor), texts }
    }

    pub fn is_empty(&self) -> bool {
        self.zooms.is_empty() && self.censors.is_empty() && self.texts.is_empty()
    }

    pub fn fits(&self, frame: &RgbaImage) -> bool {
        frame.dimensions() == (self.width, self.height)
    }
}

/// The frame at source time `t` with censors, then the zoom, then captions on top.
pub fn compose(frame: RgbaImage, plan: &Plan, t: f64) -> RgbaImage {
    let mut img = frame;
    for c in plan.censors.iter().filter(|c| t >= c.start && t <= c.end) {
        censor(&mut img, c);
    }
    let (level, center) = plan
        .zooms
        .iter()
        .map(|z| (zoom_level(z, t), z.center))
        .fold((1., (0.5, 0.5)), |best, z| if z.0 > best.0 { z } else { best });
    if level > 1.001 {
        img = zoomed(&img, level, center);
    }
    for (s, caption) in &plan.texts {
        if let Some(caption) = caption
            && t >= s.start
            && t <= s.end
        {
            let r = pixels(s.rect, img.width(), img.height());
            imageops::overlay(&mut img, caption, r.0 as i64, r.1 as i64);
        }
    }
    img
}

/// A normalized rect in pixels, clipped to the frame: (x, y, w, h).
fn pixels(r: NRect, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let x = (r.x * w as f64).round().clamp(0., w as f64) as u32;
    let y = (r.y * h as f64).round().clamp(0., h as f64) as u32;
    let rw = ((r.w * w as f64).round() as u32).max(1).min(w - x);
    let rh = ((r.h * h as f64).round() as u32).max(1).min(h - y);
    (x, y, rw, rh)
}

fn censor(img: &mut RgbaImage, spec: &Segment) {
    let (x, y, w, h) = pixels(spec.rect, img.width(), img.height());
    if w <= 1 || h <= 1 {
        return;
    }
    let side = w.min(h) as f64;
    let patch = imageops::crop_imm(img, x, y, w, h).to_image();
    let patch = match spec.censor_style {
        CensorStyle::Blur => raster::blur(&patch, (side * (0.02 + spec.strength * 0.12)).max(4.) as f32),
        CensorStyle::Pixelate => pixelate(&patch, (side * (0.03 + spec.strength * 0.15)).max(4.) as u32),
    };
    imageops::replace(img, &patch, x as i64, y as i64);
}

/// Blocks of `cell` pixels, laid out from the patch's centre like CIPixellate.
fn pixelate(patch: &RgbaImage, cell: u32) -> RgbaImage {
    let (w, h) = patch.dimensions();
    let offset = |len: u32| (cell - (len / 2) % cell) % cell;
    let (ox, oy) = (offset(w), offset(h));
    let mut out = RgbaImage::new(w, h);
    let mut by = 0;
    while by < h + oy {
        let (y0, y1) = (by.saturating_sub(oy), (by + cell).saturating_sub(oy).min(h));
        let mut bx = 0;
        while bx < w + ox {
            let (x0, x1) = (bx.saturating_sub(ox), (bx + cell).saturating_sub(ox).min(w));
            if x1 > x0 && y1 > y0 {
                let mut sum = [0u64; 4];
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        for (s, v) in sum.iter_mut().zip(patch.get_pixel(xx, yy).0) {
                            *s += v as u64;
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)) as u64;
                let avg = Px(sum.map(|s| (s / n) as u8));
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        out.put_pixel(xx, yy, avg);
                    }
                }
            }
            bx += cell;
        }
        by += cell;
    }
    out
}

/// Magnifies `level` times around `center`, kept inside the frame so no edge shows.
fn zoomed(img: &RgbaImage, level: f64, center: (f64, f64)) -> RgbaImage {
    let (w, h) = img.dimensions();
    let half = 1. / (2. * level);
    let cx = center.0.clamp(half, 1. - half);
    let cy = center.1.clamp(half, 1. - half);
    let (cw, ch) = (((w as f64 / level).round() as u32).max(1), ((h as f64 / level).round() as u32).max(1));
    let x = ((cx * w as f64 - cw as f64 / 2.).round().max(0.) as u32).min(w - cw);
    let y = ((cy * h as f64 - ch as f64 / 2.).round().max(0.) as u32).min(h - ch);
    let crop = imageops::crop_imm(img, x, y, cw, ch).to_image();
    imageops::resize(&crop, w, h, imageops::FilterType::Triangle)
}

/// `text` word-wrapped and centred in a `w`×`h` box, with an optional soft shadow.
fn caption(text: &str, w: u32, h: u32, em: f32, color: [u8; 4], shadow: bool) -> Option<RgbaImage> {
    if text.is_empty() {
        return None;
    }
    let em = em.max(6.);
    let lines = wrap(text, w as f32, em);
    let line_h = em * raster::LINE_HEIGHT;
    let top = ((h as f32 - line_h * lines.len() as f32) / 2.).max(0.);
    let mut mask = vec![0f32; (w * h) as usize];
    let font = &raster::ui_font().font;
    let scaled = font.as_scaled(raster::px_scale(font, em));
    for (i, line) in lines.iter().enumerate() {
        let mut x = (w as f32 - raster::text_width(line, em)) / 2.;
        let baseline = top + i as f32 * line_h + raster::baseline_offset(em);
        let mut prev = None;
        for c in line.chars() {
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
                let (px, py) = (bb.min.x as i32 + gx as i32, bb.min.y as i32 + gy as i32);
                if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                    let m = &mut mask[(py as u32 * w + px as u32) as usize];
                    *m = (*m + coverage).min(1.);
                }
            });
        }
    }
    let mut out = RgbaImage::new(w, h);
    if shadow {
        let alpha = RgbaImage::from_fn(w, h, |x, y| Px([0, 0, 0, (mask[(y * w + x) as usize] * 255. * 0.75) as u8]));
        let soft = raster::blur(&alpha, (em * 0.12).max(2.));
        imageops::overlay(&mut out, &soft, 0, (em * 0.05).max(1.).round() as i64);
    }
    let [r, g, b, a] = color;
    let ink = RgbaImage::from_fn(w, h, |x, y| Px([r, g, b, (mask[(y * w + x) as usize] * a as f32) as u8]));
    imageops::overlay(&mut out, &ink, 0, 0);
    Some(out)
}

/// Greedy word wrap at `width`; a word wider than the box gets a line of its own.
fn wrap(text: &str, width: f32, em: f32) -> Vec<String> {
    let mut lines = vec![];
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if !line.is_empty() && raster::text_width(&candidate, em) > width {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| if (x / 2 + y / 2) % 2 == 0 { Px([255, 255, 255, 255]) } else { Px([0, 0, 0, 255]) })
    }

    #[test]
    fn censor_only_touches_its_box_and_only_while_active() {
        let mut s = Segment::new(Kind::Censor, 1., 2.);
        s.rect = NRect { x: 0.5, y: 0., w: 0.5, h: 1. };
        let plan = Plan::new(&[s], 40, 20);
        let frame = checker(40, 20);
        let out = compose(frame.clone(), &plan, 1.5);
        assert_eq!(out.get_pixel(0, 0), frame.get_pixel(0, 0));
        let grey = out.get_pixel(30, 10).0[0];
        assert!((60..200).contains(&grey), "blurred checker should be grey, got {grey}");
        assert_eq!(compose(frame.clone(), &plan, 3.), frame);
    }

    #[test]
    fn pixelate_makes_flat_cells() {
        let out = pixelate(&checker(16, 16), 8);
        assert_eq!(out.get_pixel(0, 0), out.get_pixel(7, 7));
    }

    #[test]
    fn zoom_magnifies_around_the_center() {
        let frame = RgbaImage::from_fn(100, 100, |x, _| if x < 50 { Px([255, 0, 0, 255]) } else { Px([0, 0, 255, 255]) });
        let mut z = Segment::new(Kind::Zoom, 0., 10.);
        z.zoom = 4.;
        z.fade = 0.;
        z.center = (0.2, 0.5);
        let out = compose(frame, &Plan::new(&[z], 100, 100), 5.);
        // A quarter-width window around x = 20 is all red.
        assert_eq!(out.get_pixel(99, 50).0, [255, 0, 0, 255]);
    }
}
