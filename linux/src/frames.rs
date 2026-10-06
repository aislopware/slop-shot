//! An image as a sequence of frames (FrameSequence in AnimatedImage.swift): a still
//! image is one frame, so the editor treats stills and animations alike.
//!
//! Frames come from animated GIF / APNG / WebP, or from a horizontal sprite sheet (how
//! Zalo packs its animated stickers: 3250×130 is 25 frames of 130×130).

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, ImageFormat, RgbaImage};

/// Sprite sheets carry no timing; 12 fps is what matches Zalo's playback.
const SPRITE_FPS: f64 = 12.;
/// Pickers only need a small first frame; full-size frames of a 110-sticker pack
/// would hold tens of megabytes.
pub const THUMB_EDGE: u32 = 160;

#[derive(Debug)]
pub struct Frames {
    pub frames: Vec<Arc<RgbaImage>>,
    /// Seconds, one per frame.
    pub delays: Vec<f64>,
}

impl Frames {
    pub fn new(frames: Vec<RgbaImage>, delays: Vec<f64>) -> Self {
        let delays = (0..frames.len()).map(|i| delays.get(i).copied().filter(|d| *d > 0.).unwrap_or(0.1)).collect();
        Self { frames: frames.into_iter().map(Arc::new).collect(), delays }
    }

    pub fn still(image: RgbaImage) -> Self {
        Self { frames: vec![Arc::new(image)], delays: vec![0.] }
    }

    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    pub fn duration(&self) -> f64 {
        self.delays.iter().sum()
    }

    pub fn first(&self) -> &Arc<RgbaImage> {
        &self.frames[0]
    }

    /// The frame showing `t` seconds in, looping forever.
    pub fn at(&self, t: f64) -> &Arc<RgbaImage> {
        let duration = self.duration();
        if !self.is_animated() || duration <= 0. {
            return self.first();
        }
        let mut x = t.rem_euclid(duration);
        for (frame, d) in self.frames.iter().zip(&self.delays) {
            if x < *d {
                return frame;
            }
            x -= d;
        }
        self.frames.last().unwrap()
    }

    /// Every frame at full size: only for placing a sticker, never for a picker grid.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
        Self::decode(&bytes)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        if let Some(frames) = animation(bytes)? {
            return Ok(frames);
        }
        let image = image::load_from_memory(bytes)?.to_rgba8();
        Ok(slice(&image).unwrap_or_else(|| Self::still(image)))
    }
}

/// None for a format or file without animation.
fn animation(bytes: &[u8]) -> anyhow::Result<Option<Frames>> {
    let decoded = match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => GifDecoder::new(Cursor::new(bytes))?.into_frames().collect_frames()?,
        Ok(ImageFormat::Png) => {
            let png = PngDecoder::new(Cursor::new(bytes))?;
            if !png.is_apng()? {
                return Ok(None);
            }
            png.apng()?.into_frames().collect_frames()?
        }
        Ok(ImageFormat::WebP) => {
            let webp = WebPDecoder::new(Cursor::new(bytes))?;
            if !webp.has_animation() {
                return Ok(None);
            }
            webp.into_frames().collect_frames()?
        }
        _ => return Ok(None),
    };
    if decoded.len() < 2 {
        return Ok(None);
    }
    let delays = decoded
        .iter()
        .map(|f| {
            let (num, den) = f.delay().numer_denom_ms();
            num as f64 / den.max(1) as f64 / 1000.
        })
        .collect();
    Ok(Some(Frames::new(decoded.into_iter().map(|f| f.into_buffer()).collect(), delays)))
}

/// Only an exact multiple of at least three squares counts, so a panorama or banner
/// isn't chopped up.
fn is_sprite_sheet(w: u32, h: u32) -> bool {
    h > 0 && w >= h * 3 && w % h == 0
}

fn slice(image: &RgbaImage) -> Option<Frames> {
    let (w, side) = image.dimensions();
    if !is_sprite_sheet(w, side) {
        return None;
    }
    let frames: Vec<RgbaImage> =
        (0..w / side).map(|i| image::imageops::crop_imm(image, i * side, 0, side, side).to_image()).collect();
    let delays = vec![1. / SPRITE_FPS; frames.len()];
    Some(Frames::new(frames, delays))
}

/// A sticker's first frame shrunk for a picker grid, and whether it moves.
pub fn thumbnail(path: &Path) -> anyhow::Result<(RgbaImage, bool)> {
    let bytes = std::fs::read(path)?;
    let animated = is_animated(&bytes);
    let mut first = image::load_from_memory(&bytes)?.to_rgba8();
    let (w, h) = first.dimensions();
    if is_sprite_sheet(w, h) {
        first = image::imageops::crop_imm(&first, 0, 0, h, h).to_image();
    }
    let (w, h) = first.dimensions();
    let long = w.max(h);
    if long > THUMB_EDGE {
        let k = THUMB_EDGE as f32 / long as f32;
        let (tw, th) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
        first = image::imageops::resize(&first, tw, th, image::imageops::FilterType::Triangle);
    }
    Ok((first, animated))
}

/// More than one frame, decoding at most two of them.
fn is_animated(bytes: &[u8]) -> bool {
    let two = |frames: image::Frames| frames.take(2).filter(Result::is_ok).count() == 2;
    match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => GifDecoder::new(Cursor::new(bytes)).is_ok_and(|d| two(d.into_frames())),
        Ok(ImageFormat::Png) => PngDecoder::new(Cursor::new(bytes))
            .is_ok_and(|d| d.is_apng().unwrap_or(false) || sprite_dimensions(bytes)),
        Ok(ImageFormat::WebP) => WebPDecoder::new(Cursor::new(bytes)).is_ok_and(|d| d.has_animation()),
        _ => sprite_dimensions(bytes),
    }
}

fn sprite_dimensions(bytes: &[u8]) -> bool {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .is_some_and(|(w, h)| is_sprite_sheet(w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn gif_bytes(colors: &[[u8; 4]], delay_cs: u16) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = gif::Encoder::new(&mut out, 8, 6, &[]).unwrap();
            for c in colors {
                let mut raw = RgbaImage::from_pixel(8, 6, Rgba(*c)).into_raw();
                let mut frame = gif::Frame::from_rgba_speed(8, 6, &mut raw, 10);
                frame.delay = delay_cs;
                enc.write_frame(&frame).unwrap();
            }
        }
        out
    }

    #[test]
    fn a_gif_keeps_its_frames_and_timing() {
        let frames = Frames::decode(&gif_bytes(&[[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]], 5)).unwrap();
        assert_eq!(frames.frames.len(), 3);
        assert!(frames.delays.iter().all(|d| (d - 0.05).abs() < 1e-9), "{:?}", frames.delays);
        assert_eq!(frames.at(0.).get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(frames.at(0.07).get_pixel(0, 0), &Rgba([0, 0, 255, 255]));
        assert_eq!(frames.at(0.15 + 0.12).get_pixel(0, 0), &Rgba([0, 255, 0, 255]), "loops");
        assert!(is_animated(&gif_bytes(&[[1, 2, 3, 255], [4, 5, 6, 255]], 5)));
        assert!(!is_animated(&gif_bytes(&[[1, 2, 3, 255]], 5)));
    }

    #[test]
    fn a_gif_without_delays_plays_at_ten_fps() {
        let frames = Frames::decode(&gif_bytes(&[[255, 0, 0, 255], [0, 0, 255, 255]], 0)).unwrap();
        assert_eq!(frames.delays, vec![0.1, 0.1]);
    }

    #[test]
    fn a_strip_of_squares_is_a_sprite_sheet_and_a_banner_is_not() {
        let mut sheet = RgbaImage::new(40, 10);
        for (x, _, p) in sheet.enumerate_pixels_mut() {
            *p = Rgba([(x / 10 * 60) as u8, 0, 0, 255]);
        }
        let mut png = Vec::new();
        sheet.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let frames = Frames::decode(&png).unwrap();
        assert_eq!(frames.frames.len(), 4);
        assert_eq!(frames.first().dimensions(), (10, 10));
        assert_eq!(frames.frames[2].get_pixel(5, 5), &Rgba([120, 0, 0, 255]));
        assert!((frames.duration() - 4. / 12.).abs() < 1e-9);
        assert!(is_animated(&png));

        for (w, h) in [(25, 10), (20, 10)] {
            let mut png = Vec::new();
            RgbaImage::new(w, h).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
            assert!(!Frames::decode(&png).unwrap().is_animated(), "{w}×{h}");
            assert!(!is_animated(&png));
        }
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(Frames::decode(b"GIF89a not really").is_err());
        assert!(Frames::decode(b"").is_err());
    }
}
