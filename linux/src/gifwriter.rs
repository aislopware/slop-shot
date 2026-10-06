//! Streams frames into a looping GIF (GIFWriter in AnimatedImage.swift). A frame only
//! stores the box that changed, and an unchanged frame lengthens the one before:
//! screenshots and screen recordings are mostly still.

use std::io::Write;

use image::RgbaImage;

use crate::annotate::{self, Annotation};
use crate::raster::{self, BlurCache};

/// A screenshot's long edge in the GIF: a 50-frame GIF of a 3000 px Retina capture
/// would be hundreds of megabytes, which no chat app accepts.
const MAX_EDGE: u32 = 1600;
/// About 10 s at 20 fps; past that a GIF is too heavy to be useful.
const MAX_FRAMES: usize = 200;
/// Shorter delays read as "unset" in many viewers, which then slow down to 0.1 s.
const MIN_DELAY: f64 = 0.02;

pub struct GifWriter<W: Write> {
    encoder: gif::Encoder<W>,
    canvas: Option<RgbaImage>,
    /// The last changed box and the centisecond it appeared at, written once the next
    /// change says how long it stays.
    pending: Option<(RgbaImage, u32, u32, u32)>,
}

impl<W: Write> GifWriter<W> {
    pub fn new(writer: W, width: u32, height: u32) -> anyhow::Result<Self> {
        let mut encoder = gif::Encoder::new(writer, width as u16, height as u16, &[])?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        Ok(Self { encoder, canvas: None, pending: None })
    }

    /// Shows `frame` from `start` centiseconds in. Timestamps rather than delays keep
    /// the total exact although each delay rounds to whole centiseconds.
    pub fn add(&mut self, frame: RgbaImage, start: u32) -> anyhow::Result<()> {
        let rect = match &self.canvas {
            None => Some((0, 0, frame.width(), frame.height())),
            Some(canvas) => changed_rect(canvas, &frame),
        };
        let Some((x, y, w, h)) = rect else { return Ok(()) };
        self.flush(start)?;
        let patch = image::imageops::crop_imm(&frame, x, y, w, h).to_image();
        match &mut self.canvas {
            Some(canvas) => image::imageops::replace(canvas, &patch, x.into(), y.into()),
            None => self.canvas = Some(frame),
        }
        self.pending = Some((patch, x, y, start));
        Ok(())
    }

    /// Ends the animation at `end` centiseconds.
    pub fn finish(mut self, end: u32) -> anyhow::Result<W> {
        self.flush(end)?;
        Ok(self.encoder.into_inner()?)
    }

    fn flush(&mut self, end: u32) -> anyhow::Result<()> {
        if let Some((patch, x, y, start)) = self.pending.take() {
            let (w, h) = patch.dimensions();
            let mut frame = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut patch.into_raw(), 10);
            (frame.left, frame.top) = (x as u16, y as u16);
            frame.delay = end.saturating_sub(start).max(1) as u16;
            // Later frames only cover what changed, so each one stays on screen under them.
            frame.dispose = gif::DisposalMethod::Keep;
            self.encoder.write_frame(&frame)?;
        }
        Ok(())
    }
}

/// When to sample the annotations and how long each frame stays, in seconds. One
/// moving layer (nearly always the case) keeps its own timing, so the GIF plays just
/// like the sticker; several have no common beat and are sampled at 20 fps over the
/// longest.
pub fn timeline(annotations: &[Annotation]) -> Vec<(f64, f64)> {
    let moving: Vec<_> = annotations.iter().filter_map(annotate::animation).collect();
    if let [frames] = moving.as_slice() {
        let mut t = 0.;
        return frames
            .delays
            .iter()
            .take(MAX_FRAMES)
            .map(|&d| {
                let step = (t, d);
                t += d;
                step
            })
            .collect();
    }
    let longest = moving.iter().map(|f| f.duration()).fold(0., f64::max);
    let longest = if longest > 0. { longest } else { 1. };
    let step = 1. / 20.;
    let n = ((longest.min(10.) / step).round() as usize).clamp(1, MAX_FRAMES);
    (0..n).map(|i| (i as f64 * step, step)).collect()
}

/// The annotated image as a GIF following `plan`; `progress` gets each frame number
/// as it is written.
pub fn animate(base: &RgbaImage, annotations: &[Annotation], unit: f32, plan: &[(f64, f64)], progress: impl Fn(usize)) -> anyhow::Result<Vec<u8>> {
    let (w, h) = base.dimensions();
    let k = (MAX_EDGE as f32 / w.max(h) as f32).min(1.);
    let size = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
    let mut gif = GifWriter::new(Vec::new(), size.0, size.1)?;
    let mut blur = BlurCache::new();
    let mut elapsed: f64 = 0.;
    for (i, &(t, delay)) in plan.iter().enumerate() {
        let mut frame = raster::flatten(base, &annotate::at_time(annotations, t), unit, &mut blur);
        if k < 1. {
            frame = image::imageops::resize(&frame, size.0, size.1, image::imageops::FilterType::Triangle);
        }
        gif.add(frame, (elapsed * 100.).round() as u32)?;
        elapsed += delay.max(MIN_DELAY);
        progress(i + 1);
    }
    gif.finish((elapsed * 100.).round() as u32)
}

/// The box around the pixels of `frame` that differ from `canvas` by more than a
/// video decoder's noise; None when nothing visibly changed.
fn changed_rect(canvas: &RgbaImage, frame: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    const NOISE: u8 = 3;
    let (w, h) = frame.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in frame.enumerate_pixels() {
        let q = canvas.get_pixel(x, y);
        if p.0.iter().zip(q.0).any(|(a, b)| a.abs_diff(b) > NOISE) {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    (x0 <= x1).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{AnimationDecoder as _, Rgba};

    use super::*;
    use crate::annotate::{Layer, Rgba as Color, Tool};
    use crate::frames::Frames;

    fn sticker(colors: &[[u8; 4]], delay: f64, at: (f32, f32)) -> Annotation {
        let frames = colors.iter().map(|c| RgbaImage::from_pixel(10, 10, Rgba(*c))).collect();
        let mut a = Annotation::new(Tool::Image, Color::BLACK, 0., vec![at, (at.0 + 10., at.1 + 10.)]);
        a.layer = Some(Layer::new(Frames::new(frames, vec![delay; colors.len()])));
        a
    }

    fn decode(gif: &[u8]) -> Vec<(RgbaImage, u32)> {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(gif)).unwrap();
        decoder
            .into_frames()
            .collect_frames()
            .unwrap()
            .into_iter()
            .map(|f| {
                let (n, d) = f.delay().numer_denom_ms();
                (f.into_buffer(), n / d)
            })
            .collect()
    }

    #[test]
    fn one_sticker_keeps_its_own_beat() {
        let base = RgbaImage::from_pixel(60, 40, Rgba([255, 255, 255, 255]));
        let layers = vec![sticker(&[[255, 0, 0, 255], [0, 0, 255, 255], [0, 160, 0, 255]], 0.07, (20., 10.))];
        let plan = timeline(&layers);
        assert_eq!(plan, vec![(0., 0.07), (0.07, 0.07), (0.14, 0.07)]);
        let frames = decode(&animate(&base, &layers, 1., &plan, |_| {}).unwrap());
        assert_eq!(frames.len(), 3);
        assert_eq!(frames.iter().map(|f| f.1).collect::<Vec<_>>(), vec![70, 70, 70]);
        for ((frame, _), want) in frames.iter().zip([[255, 0, 0], [0, 0, 255], [0, 160, 0]]) {
            let p = frame.get_pixel(25, 15);
            assert!(p.0[..3].iter().zip(want).all(|(a, b)| a.abs_diff(b) < 12), "{p:?} vs {want:?}");
            assert_eq!(frame.get_pixel(2, 2).0[..3], [255, 255, 255], "the screenshot stays under it");
        }
    }

    #[test]
    fn two_stickers_sample_twenty_fps_over_the_longest() {
        let layers = vec![sticker(&[[255, 0, 0, 255], [0, 0, 255, 255]], 0.5, (0., 0.)), sticker(&[[0, 0, 0, 255], [9, 9, 9, 255]], 0.1, (20., 0.))];
        let plan = timeline(&layers);
        assert_eq!(plan.len(), 20);
        assert!(plan.iter().all(|&(_, d)| d == 0.05));
        let long = vec![sticker(&[[1, 1, 1, 255], [2, 2, 2, 255]], 30., (0., 0.)), layers[1].clone()];
        assert_eq!(timeline(&long).len(), 200, "capped at 10 s");
    }

    #[test]
    fn a_huge_screenshot_shrinks_and_tiny_delays_are_raised() {
        let base = RgbaImage::from_pixel(3200, 400, Rgba([255, 255, 255, 255]));
        let layers = vec![sticker(&[[255, 0, 0, 255], [0, 0, 255, 255]], 0.01, (100., 100.))];
        let gif = animate(&base, &layers, 1., &timeline(&layers), |_| {}).unwrap();
        let frames = decode(&gif);
        assert_eq!(frames[0].0.dimensions(), (1600, 200));
        assert_eq!(frames.iter().map(|f| f.1).collect::<Vec<_>>(), vec![20, 20]);
    }

    #[test]
    fn a_gif_frame_keeps_only_the_box_that_changed() {
        let canvas = RgbaImage::from_pixel(40, 30, image::Rgba([100, 100, 100, 255]));
        let mut frame = canvas.clone();
        frame.put_pixel(3, 3, image::Rgba([102, 99, 100, 255]));
        assert_eq!(changed_rect(&canvas, &frame), None, "decoder noise is not a change");
        frame.put_pixel(10, 5, image::Rgba([200, 100, 100, 255]));
        frame.put_pixel(20, 12, image::Rgba([100, 100, 0, 255]));
        assert_eq!(changed_rect(&canvas, &frame), Some((10, 5, 11, 8)));
    }
}
