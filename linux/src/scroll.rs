//! Scrolling capture (ScrollCaptureController.swift, ScrollStitcher.swift): the rect is
//! sampled from a ScreenCast feed while the page scrolls, and each frame's new rows are
//! appended to one tall image.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context as _;
use gpui::{App, AppContext as _, AsyncApp, Bounds, Global, Pixels};
use gstreamer as gst;
use gstreamer::glib::object::Cast as _;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use image::RgbaImage;

use crate::card::{self, Request};
use crate::screencast::{self, Cast};
use crate::{alert, flow, media};

const TICK: Duration = Duration::from_millis(120);
const PREVIEW_W: u32 = 150;
/// Ticks without new rows before auto-scroll decides the page has ended.
const IDLE_END: u32 = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Advanced,
    Idle,
    /// No shift matched: scrolled too far between two frames.
    TooFast,
}

/// Consecutive frames of the same rect are the same picture moved up by the distance
/// scrolled. That shift is found by matching per-row signatures, then only the rows it
/// uncovered at the bottom are appended.
pub struct Stitcher {
    width: usize,
    pixels: Vec<u8>,
    rows: usize,
    last_sig: Vec<[f32; SAMPLES]>,
    preview_step: usize,
    preview_w: usize,
    preview: Vec<u8>,
    preview_rows: usize,
}

const SAMPLES: usize = 16;

impl Stitcher {
    pub fn new(width: u32, preview_width: u32) -> Self {
        let width = width.max(1) as usize;
        let preview_step = (width / preview_width.max(1) as usize).max(1);
        Self {
            width,
            pixels: Vec::new(),
            rows: 0,
            last_sig: Vec::new(),
            preview_step,
            preview_w: (width / preview_step).max(1),
            preview: Vec::new(),
            preview_rows: 0,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn add(&mut self, frame: &RgbaImage) -> Outcome {
        if frame.width() as usize != self.width || frame.height() == 0 {
            return Outcome::Idle;
        }
        let h = frame.height() as usize;
        let pixels = frame.as_raw();
        let sig = self.signatures(pixels, h);
        if self.rows == 0 {
            self.pixels = pixels.clone();
            self.rows = h;
            self.last_sig = sig;
            self.append_preview(pixels, 0, h);
            return Outcome::Advanced;
        }
        let Some(d) = best_shift(&self.last_sig, &sig) else { return Outcome::TooFast };
        self.last_sig = sig;

        let stride = self.width * 4;
        let overlap = h - d;
        let (band_top, band_h) = band(h);
        // Rows below the band are certainly scrolled content, not a sticky header:
        // refreshing them with this frame hides a slightly-off previous seam.
        let safe_top = (band_top + band_h).min(overlap);
        if overlap > safe_top {
            let dst = (self.rows - overlap + safe_top) * stride;
            self.pixels.truncate(dst);
            self.pixels.extend_from_slice(&pixels[safe_top * stride..overlap * stride]);
        }
        if d == 0 {
            return Outcome::Idle;
        }
        self.pixels.extend_from_slice(&pixels[overlap * stride..]);
        self.rows += d;
        self.append_preview(pixels, overlap, h);
        Outcome::Advanced
    }

    /// Each row's average brightness in 16 buckets: cheap and robust to noise.
    fn signatures(&self, pixels: &[u8], h: usize) -> Vec<[f32; SAMPLES]> {
        let stride = self.width * 4;
        let bucket = (self.width / SAMPLES).max(1);
        (0..h)
            .map(|y| {
                let row = &pixels[y * stride..(y + 1) * stride];
                let mut sig = [0.; SAMPLES];
                for (s, out) in sig.iter_mut().enumerate() {
                    let (x0, x1) = (s * bucket, ((s + 1) * bucket).min(self.width));
                    if x1 <= x0 {
                        continue;
                    }
                    let sum: u32 = row[x0 * 4..x1 * 4].chunks_exact(4).map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32).sum();
                    *out = sum as f32 / ((x1 - x0) * 3) as f32;
                }
                sig
            })
            .collect()
    }

    /// The live preview grows with sparse samples of the new rows, never rescaling the whole.
    fn append_preview(&mut self, pixels: &[u8], y0: usize, y1: usize) {
        let stride = self.width * 4;
        let step = self.preview_step;
        for y in (y0..y1).step_by(step) {
            for sx in 0..self.preview_w {
                let x = (sx * step + step / 2).min(self.width - 1);
                let p = y * stride + x * 4;
                self.preview.extend_from_slice(&[pixels[p], pixels[p + 1], pixels[p + 2], 255]);
            }
            self.preview_rows += 1;
        }
    }

    pub fn preview(&self) -> Option<RgbaImage> {
        (self.preview_rows > 0).then(|| RgbaImage::from_raw(self.preview_w as u32, self.preview_rows as u32, self.preview.clone()))?
    }

    pub fn finish(self) -> Option<RgbaImage> {
        (self.rows > 0).then(|| RgbaImage::from_raw(self.width as u32, self.rows as u32, self.pixels))?
    }
}

/// The matched band starts ~1/6 down, past a sticky header, and is ~14% tall.
fn band(h: usize) -> (usize, usize) {
    ((h / 6).max(4), (h * 14 / 100).max(8))
}

/// How far `next` moved up from `prev`: `next[y] ≈ prev[y + d]`, searched with a band of
/// `next` only, which is fast and skips fixed headers. None when nothing matches well.
fn best_shift(prev: &[[f32; SAMPLES]], next: &[[f32; SAMPLES]]) -> Option<usize> {
    let h = next.len();
    if prev.len() != h {
        return None;
    }
    let (top, band_h) = band(h);
    let max_d = h.checked_sub(top + band_h).filter(|d| *d >= 1)?;
    let distance = |a: &[f32; SAMPLES], b: &[f32; SAMPLES]| a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum::<f32>() / SAMPLES as f32;
    let (d, cost) = (0..=max_d)
        .map(|d| (d, (0..band_h).map(|i| distance(&next[top + i], &prev[top + d + i])).sum::<f32>() / band_h as f32))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    (cost <= 12.).then_some(d)
}

// ── Session ──────────────────────────────────────────────────────────────────

struct Scroll {
    cast: Arc<Cast>,
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    stitcher: Arc<Mutex<Stitcher>>,
    display: Bounds<Pixels>,
    /// The rect's centre on the shared monitor, where auto-scroll points.
    center: (f64, f64),
    /// Rows each auto-scroll tick should advance: 38% of the rect, so ~62% overlaps.
    target: f64,
    /// Rows one wheel notch moves this page, learned from the frames.
    per_click: Option<f64>,
    /// Notches sent since the last frame that advanced, and the rows that frame had.
    sent: i32,
    rows_at_send: usize,
    auto: bool,
    idle: u32,
    finished: bool,
}

impl Global for Scroll {}

pub fn is_active(cx: &App) -> bool {
    cx.has_global::<Scroll>()
}

fn arr(b: Bounds<Pixels>) -> [f32; 4] {
    [b.origin.x, b.origin.y, b.size.width, b.size.height].map(f32::from)
}

pub fn start(display: Bounds<Pixels>, rect: Bounds<Pixels>, scale: f32, cx: &mut App) {
    if let Err(err) = media::init() {
        return alert::error("Couldn't start the scrolling capture", &format!("{err:#}"), cx);
    }
    cx.set_global(flow::Busy);
    cx.spawn(async move |cx| {
        // RemoteDesktop too, so auto-scroll can scroll; the cursor would confuse matching.
        let opened = screencast::open(display, false, true, cx).await;
        cx.update(|cx| {
            cx.remove_global::<flow::Busy>();
            let cast = match opened {
                Ok(cast) => Arc::new(cast),
                Err(err) if err.is::<screencast::Cancelled>() => return,
                Err(err) => return alert::error("Couldn't start the scrolling capture", &format!("{err:#}"), cx),
            };
            let (pipeline, sink, width) = match feed(&cast, display, rect, scale) {
                Ok(feed) => feed,
                Err(err) => {
                    close_cast(cast, cx);
                    return alert::error("Couldn't start the scrolling capture", &format!("{err:#}"), cx);
                }
            };
            let at = rect.center() + display.origin - cast.area.origin;
            cx.set_global(Scroll {
                cast,
                pipeline,
                sink,
                stitcher: Arc::new(Mutex::new(Stitcher::new(width, (PREVIEW_W as f32 * scale) as u32))),
                display,
                center: (f32::from(at.x) as f64, f32::from(at.y) as f64),
                target: (f32::from(rect.size.height) * scale) as f64 * 0.38,
                per_click: None,
                sent: 0,
                rows_at_send: 0,
                auto: false,
                idle: 0,
                finished: false,
            });
            log::info!("scrolling capture started");
            card::send(&Request::Focus { display: arr(display), rect: arr(rect) }, cx);
            card::send(&Request::ScrollBar { display: arr(display), rect: arr(rect) }, cx);
            cx.spawn(async move |cx| run(cx).await).detach();
        });
    })
    .detach();
}

fn feed(cast: &Cast, display: Bounds<Pixels>, rect: Bounds<Pixels>, scale: f32) -> anyhow::Result<(gst::Pipeline, gst_app::AppSink, u32)> {
    let missing = media::missing(&["pipewiresrc", "videocrop", "appsink"]);
    anyhow::ensure!(missing.is_empty(), "GStreamer is missing {}; install gstreamer1.0-pipewire and gstreamer1.0-plugins-good.", missing.join(", "));
    let (crop, w, h) = cast.crop(display, rect, scale);
    let pipeline = gst::parse::launch(&format!(
        "{src} ! queue leaky=downstream max-size-buffers=2 ! videoconvert ! {crop} ! videoscale ! \
         video/x-raw,format=RGBA,width={w},height={h} ! appsink name=sink max-buffers=1 drop=true sync=false",
        src = cast.source("")?
    ))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
    let sink = pipeline.by_name("sink").context("no sink")?.downcast::<gst_app::AppSink>().map_err(|_| anyhow::anyhow!("not an appsink"))?;
    pipeline.set_state(gst::State::Playing).context("the screen feed didn't start")?;
    Ok((pipeline, sink, w as u32))
}

/// About eight frames a second, each stitched off the main thread before the next.
async fn run(cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(TICK).await;
        let Some((sink, stitcher)) = cx.update(|cx| cx.try_global::<Scroll>().filter(|s| !s.finished).map(|s| (s.sink.clone(), s.stitcher.clone()))) else {
            return;
        };
        // PipeWire sends no frame while nothing on screen changes: that tick is Idle too,
        // or auto-scroll would never notice the page has ended.
        let (outcome, rows, preview, fresh) = cx
            .background_spawn(async move {
                let frame = sink.try_pull_sample(gst::ClockTime::ZERO).and_then(|s| media::sample_image(&s));
                let mut stitcher = stitcher.lock().unwrap();
                let Some(frame) = frame else { return (Outcome::Idle, stitcher.rows(), None, false) };
                let outcome = stitcher.add(&frame);
                let preview = (outcome == Outcome::Advanced).then(|| stitcher.preview()).flatten();
                (outcome, stitcher.rows(), preview, true)
            })
            .await;
        let scroll = cx.update(|cx| {
            if !cx.try_global::<Scroll>().is_some_and(|s| !s.finished) {
                return None;
            }
            let s = cx.global_mut::<Scroll>();
            let was_auto = s.auto;
            let mut scroll = None;
            if s.auto {
                s.idle = if outcome == Outcome::Idle { s.idle + 1 } else { 0 };
                if outcome == Outcome::Advanced && s.sent > 0 {
                    let moved = (rows - s.rows_at_send) as f64 / s.sent as f64;
                    s.per_click = Some(s.per_click.map_or(moved, |p| (p + moved) / 2.));
                    s.sent = 0;
                }
                if s.idle >= IDLE_END {
                    s.auto = false;
                } else if outcome != Outcome::TooFast && s.sent == 0 {
                    // One notch until its size is known, then as many as fit the target.
                    let clicks = s.per_click.map_or(1, |p| (s.target / p.max(1.)).floor().clamp(1., 20.) as i32);
                    s.sent = clicks;
                    s.rows_at_send = rows;
                    scroll = Some((s.cast.clone(), s.center, clicks));
                } else if outcome == Outcome::Idle && s.sent > 0 && s.idle >= 2 {
                    // The notches went nowhere (page end, or a slow app): send again.
                    s.sent = 0;
                }
            }
            let auto = s.auto;
            if fresh || auto != was_auto {
                let preview = preview.and_then(|p| write_preview(&p));
                card::send(&Request::ScrollStatus { text: format!("Captured {rows} px"), slow: outcome == Outcome::TooFast, auto, preview }, cx);
            }
            Some(scroll)
        });
        match scroll {
            None => return,
            Some(Some((cast, at, clicks))) => {
                if let Err(err) = cast.scroll(at, clicks).await {
                    log::warn!("auto-scroll: {err:#}");
                }
            }
            Some(None) => {}
        }
    }
}

fn write_preview(image: &RgbaImage) -> Option<std::path::PathBuf> {
    let path = crate::output::unique_path(&crate::output::temp_dir(), ".scroll", "png");
    std::fs::write(&path, crate::output::encode_png(image).ok()?).ok()?;
    Some(path)
}

pub fn set_auto(on: bool, cx: &mut App) {
    let Some(s) = cx.try_global::<Scroll>() else { return };
    if !s.cast.can_scroll() && on {
        log::warn!("auto-scroll needs \"Allow Remote Interaction\" in the screen sharing dialog");
        return;
    }
    let s = cx.global_mut::<Scroll>();
    s.auto = on;
    s.idle = 0;
    s.sent = 0;
}

fn close_cast(cast: Arc<Cast>, cx: &mut App) {
    cx.background_spawn(async move {
        if let Ok(cast) = Arc::try_unwrap(cast) {
            cast.close().await;
        }
    })
    .detach();
}

fn teardown(cx: &mut App) -> Option<Scroll> {
    let s = cx.has_global::<Scroll>().then(|| cx.remove_global::<Scroll>())?;
    let _ = s.pipeline.set_state(gst::State::Null);
    close_cast(s.cast.clone(), cx);
    Some(s)
}

pub fn cancel(cx: &mut App) {
    if teardown(cx).is_some() {
        card::send(&Request::HideChrome, cx);
    }
}

/// Done: stitches the tall image off the main thread behind "Building image…".
pub fn done(cx: &mut App) {
    let Some(display) = cx.try_global::<Scroll>().filter(|s| !s.finished).map(|s| s.display) else { return };
    cx.global_mut::<Scroll>().finished = true;
    card::send(&Request::Processing { display: arr(display) }, cx);
    let Some(s) = teardown(cx) else { return };
    let stitcher = s.stitcher;
    // The run loop may still hold the stitcher for a frame; the lock waits that out.
    let image = cx.background_spawn(async move { std::mem::replace(&mut *stitcher.lock().ok()?, Stitcher::new(1, 1)).finish() });
    cx.spawn(async move |cx| {
        let image = image.await;
        cx.update(|cx| {
            card::send(&Request::HideChrome, cx);
            match image {
                Some(image) => flow::finish_image_as(image, display, false, " (scrolling)", cx),
                None => alert::error("Nothing was captured", "No frames of the area arrived before Done.", cx),
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of distinct horizontal stripes, so every row signature differs.
    fn page(h: u32, w: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let mut z = y.wrapping_mul(0x9E37_79B9);
            z ^= z >> 16;
            z = z.wrapping_mul(0x85EB_CA6B);
            let v = ((z ^ (z >> 13)) >> 24) as u8;
            image::Rgba([v, v.wrapping_add((x / 8) as u8 * 3), 255 - v, 255])
        })
    }

    fn view(page: &RgbaImage, top: u32, h: u32) -> RgbaImage {
        image::imageops::crop_imm(page, 0, top, page.width(), h).to_image()
    }

    #[test]
    fn stitches_a_scrolled_page_back_together() {
        let full = page(900, 64);
        let mut s = Stitcher::new(64, 16);
        for top in [0, 40, 40, 120, 200, 260, 300] {
            s.add(&view(&full, top, 300));
        }
        let out = s.finish().unwrap();
        assert_eq!(out.height(), 600);
        assert_eq!(out, view(&full, 0, 600));
    }

    #[test]
    fn a_jump_past_the_band_is_too_fast_and_changes_nothing() {
        let full = page(900, 64);
        let mut s = Stitcher::new(64, 16);
        s.add(&view(&full, 0, 300));
        assert_eq!(s.add(&view(&full, 280, 300)), Outcome::TooFast);
        assert_eq!(s.rows(), 300);
        assert_eq!(s.add(&view(&full, 0, 300)), Outcome::Idle);
    }

    #[test]
    fn frames_of_another_width_are_ignored() {
        let mut s = Stitcher::new(64, 16);
        assert_eq!(s.add(&page(300, 32)), Outcome::Idle);
        assert!(s.finish().is_none());
    }
}
