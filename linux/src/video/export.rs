//! Writes the edited clip (VideoExportService.swift): every output frame is decoded,
//! composed with the same `render::compose` the preview uses, scaled and encoded.

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use image::RgbaImage;

use super::model::{Edit, TimeMap};
use super::player::Info;
use super::render::{self, Plan};
use crate::gifwriter::GifWriter;
use crate::media;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mov,
    Mp4,
    Gif,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Mov, Format::Mp4, Format::Gif];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Mov => "mov",
            Format::Mp4 => "mp4",
            Format::Gif => "gif",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Mov => "MOV",
            Format::Mp4 => "MP4",
            Format::Gif => "GIF",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    High,
    Medium,
    Low,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::High, Quality::Medium, Quality::Low];

    pub fn label(self) -> &'static str {
        match self {
            Quality::High => "High",
            Quality::Medium => "Medium",
            Quality::Low => "Low",
        }
    }

    fn bits_per_pixel(self) -> f64 {
        match self {
            Quality::High => 0.20,
            Quality::Medium => 0.10,
            Quality::Low => 0.05,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Original,
    P1080,
    P720,
    P480,
}

impl Resolution {
    pub const ALL: [Resolution; 4] = [Resolution::Original, Resolution::P1080, Resolution::P720, Resolution::P480];

    pub fn label(self) -> &'static str {
        match self {
            Resolution::Original => "Original",
            Resolution::P1080 => "1080p",
            Resolution::P720 => "720p",
            Resolution::P480 => "480p",
        }
    }

    fn scale(self, height: u32) -> f64 {
        let target = match self {
            Resolution::Original => return 1.,
            Resolution::P1080 => 1080.,
            Resolution::P720 => 720.,
            Resolution::P480 => 480.,
        };
        (target / height.max(1) as f64).min(1.)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRate {
    Source,
    F60,
    F30,
    F24,
    F15,
    F10,
    F5,
}

impl FrameRate {
    pub fn label(self) -> &'static str {
        match self {
            FrameRate::Source => "Source",
            FrameRate::F60 => "60 fps",
            FrameRate::F30 => "30 fps",
            FrameRate::F24 => "24 fps",
            FrameRate::F15 => "15 fps",
            FrameRate::F10 => "10 fps",
            FrameRate::F5 => "5 fps",
        }
    }

    fn value(self) -> Option<f64> {
        match self {
            FrameRate::Source => None,
            FrameRate::F60 => Some(60.),
            FrameRate::F30 => Some(30.),
            FrameRate::F24 => Some(24.),
            FrameRate::F15 => Some(15.),
            FrameRate::F10 => Some(10.),
            FrameRate::F5 => Some(5.),
        }
    }

    /// GIF only takes low rates.
    pub fn choices(format: Format) -> &'static [FrameRate] {
        if format == Format::Gif {
            &[FrameRate::F15, FrameRate::F10, FrameRate::F5]
        } else {
            &[FrameRate::Source, FrameRate::F60, FrameRate::F30, FrameRate::F24, FrameRate::F15]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub format: Format,
    pub quality: Quality,
    pub resolution: Resolution,
    pub frame_rate: FrameRate,
}

impl Default for Options {
    fn default() -> Self {
        Self { format: Format::Mov, quality: Quality::High, resolution: Resolution::Original, frame_rate: FrameRate::Source }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub muted: bool,
    /// Per source track, 0…1; missing means 1.
    pub volumes: Vec<f64>,
}

impl Audio {
    fn volume(&self, track: usize) -> f32 {
        if self.muted { 0. } else { self.volumes.get(track).copied().unwrap_or(1.) as f32 }
    }
}

const RATE: usize = 48_000;
const MAX_GIF_FRAMES: usize = 3000;

/// Blocking; `progress` gets 0…1.
pub fn export(file: &Path, info: &Info, edit: &Edit, audio: &Audio, options: &Options, out: &Path, progress: &dyn Fn(f64)) -> anyhow::Result<()> {
    media::init()?;
    let map = edit.time_map();
    if map.duration() <= 0.05 {
        bail!("Nothing left to export — the trim range is empty.");
    }
    let scale = options.resolution.scale(info.height);
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    let size = (even(info.width as f64 * scale), even(info.height as f64 * scale));
    let fps = options.frame_rate.value().unwrap_or(if info.fps > 0. { info.fps } else { 60. });
    let _ = std::fs::remove_file(out);
    let result = match options.format {
        Format::Gif => gif(file, info, edit, &map, size, fps, out, progress),
        Format::Mov | Format::Mp4 => movie(file, info, edit, &map, audio, options, size, fps, out, progress),
    };
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

/// Composed output frames `0..count` at `fps`, each handed to `sink` with its index.
fn frames(file: &Path, info: &Info, edit: &Edit, map: &TimeMap, size: (u32, u32), fps: f64, count: usize, mut sink: impl FnMut(usize, RgbaImage) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let plan = Plan::new(&edit.segments, info.width, info.height);
    let mut decoder = Decoder::open(file, map.pieces.first().map_or(0., |p| p.src_start))?;
    for n in 0..count {
        let src = map.source_time(n as f64 / fps);
        let frame = decoder.frame_at(src)?;
        let frame = if plan.fits(&frame) && !plan.is_empty() { render::compose(frame, &plan, src) } else { frame };
        let frame = if frame.dimensions() == size { frame } else { image::imageops::resize(&frame, size.0, size.1, image::imageops::FilterType::Triangle) };
        sink(n, frame)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn movie(file: &Path, info: &Info, edit: &Edit, map: &TimeMap, audio: &Audio, options: &Options, size: (u32, u32), fps: f64, out: &Path, progress: &dyn Fn(f64)) -> anyhow::Result<()> {
    let missing = media::missing(&["x264enc", "avenc_aac", "qtmux", "mp4mux"]);
    if !missing.is_empty() {
        bail!("GStreamer is missing {} (install gstreamer1.0-plugins-ugly and gstreamer1.0-libav)", missing.join(", "));
    }
    let (w, h) = size;
    let rate = fps.round().max(1.) as i32;
    let bitrate = (w as f64 * h as f64 * fps * options.quality.bits_per_pixel()).clamp(200_000., 80_000_000.);
    let with_audio = info.audio_tracks > 0;
    let mux = if options.format == Format::Mp4 { "mp4mux" } else { "qtmux" };
    let audio_branch = if with_audio {
        format!(
            "appsrc name=a format=time caps=audio/x-raw,format=F32LE,rate={RATE},channels=2,layout=interleaved \
             ! audioconvert ! avenc_aac bitrate=128000 ! mux. "
        )
    } else {
        String::new()
    };
    // Only the video source blocks: an audio push that waited on the muxer, while the
    // muxer waits on video frames still inside x264's lookahead, would never return.
    let description = format!(
        "appsrc name=v format=time block=true max-bytes={} caps=video/x-raw,format=RGBA,width={w},height={h},framerate={rate}/1 \
         ! videoconvert ! video/x-raw,format=I420 \
         ! x264enc bitrate={} speed-preset=medium key-int-max={} ! video/x-h264,profile=high ! mux. \
         {audio_branch}{mux} name=mux ! filesink location=\"{}\"",
        (w * h * 4 * 3) as u64,
        (bitrate / 1000.) as u32,
        rate * 2,
        out.display().to_string().replace('"', "\\\""),
    );
    let pipeline = gst::parse::launch(&description)?.downcast::<gst::Pipeline>().map_err(|_| anyhow::anyhow!("not a pipeline"))?;
    let appsrc = |name: &str| pipeline.by_name(name).and_then(|e| e.downcast::<gst_app::AppSrc>().ok());
    let video = appsrc("v").context("no video source")?;
    let audio_src = appsrc("a");
    let mix = if with_audio { Some(mix_audio(file, audio, map)?) } else { None };

    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline.bus().context("no bus")?;
    let total = map.duration();
    let count = ((total * fps).ceil() as usize).max(1);
    let samples = |n: usize| ((n as f64 / fps * RATE as f64).round() as usize).min((total * RATE as f64) as usize);
    let at = |s: f64| gst::ClockTime::from_nseconds((s * 1e9).round() as u64);
    let result = frames(file, info, edit, map, size, fps, count, |n, frame| {
        if let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error])
            && let gst::MessageView::Error(err) = msg.view()
        {
            bail!("{}", err.error());
        }
        let mut buffer = gst::Buffer::from_mut_slice(frame.into_raw());
        {
            let b = buffer.get_mut().unwrap();
            b.set_pts(at(n as f64 / fps));
            b.set_duration(at(1. / fps));
        }
        video.push_buffer(buffer).map_err(|e| anyhow::anyhow!("encoding video: {e:?}"))?;
        if let (Some(src), Some(mix)) = (&audio_src, &mix) {
            let (a, b) = (samples(n), samples(n + 1));
            if b > a {
                let chunk: Vec<u8> = mix[a * 2..b * 2].iter().flat_map(|s| s.to_le_bytes()).collect();
                let mut buffer = gst::Buffer::from_mut_slice(chunk);
                {
                    let buf = buffer.get_mut().unwrap();
                    buf.set_pts(at(a as f64 / RATE as f64));
                    buf.set_duration(at((b - a) as f64 / RATE as f64));
                }
                src.push_buffer(buffer).map_err(|e| anyhow::anyhow!("encoding audio: {e:?}"))?;
            }
        }
        progress((n + 1) as f64 / count as f64 * 0.99);
        Ok(())
    });
    let finished = result.and_then(|()| {
        video.end_of_stream().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        if let Some(src) = &audio_src {
            src.end_of_stream().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        match bus.timed_pop_filtered(gst::ClockTime::from_seconds(120), &[gst::MessageType::Eos, gst::MessageType::Error]) {
            Some(msg) => match msg.view() {
                gst::MessageView::Error(err) => bail!("{}", err.error()),
                _ => Ok(()),
            },
            None => bail!("the encoder didn't finish"),
        }
    });
    let _ = pipeline.set_state(gst::State::Null);
    finished?;
    progress(1.);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn gif(file: &Path, info: &Info, edit: &Edit, map: &TimeMap, size: (u32, u32), fps: f64, out: &Path, progress: &dyn Fn(f64)) -> anyhow::Result<()> {
    let count = ((map.duration() * fps) as usize).max(1);
    if count > MAX_GIF_FRAMES {
        bail!("That clip is too long for a GIF — trim it or drop the frame rate.");
    }
    let writer = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut gif = GifWriter::new(writer, size.0, size.1)?;
    let centis = |n: usize| (n as f64 * 100. / fps).round() as u32;
    frames(file, info, edit, map, size, fps, count, |n, frame| {
        gif.add(frame, centis(n))?;
        progress((n + 1) as f64 / count as f64 * 0.99);
        Ok(())
    })?;
    gif.finish(centis(count))?.flush()?;
    progress(1.);
    Ok(())
}

/// Decodes the clip's frames in order at full size.
struct Decoder {
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    current: Option<(RgbaImage, f64)>,
    next: Option<(RgbaImage, f64)>,
    ended: bool,
}

impl Decoder {
    fn open(file: &Path, start: f64) -> anyhow::Result<Self> {
        let pipeline = gst::Pipeline::new();
        let decode = gst::ElementFactory::make("uridecodebin").property("uri", media::file_uri(file)).build()?;
        pipeline.add(&decode)?;
        let caps = gst::Caps::builder("video/x-raw").field("format", "RGBA").field("pixel-aspect-ratio", gst::Fraction::new(1, 1)).build();
        let sink = gst_app::AppSink::builder().caps(&caps).sync(false).max_buffers(4).build();
        let video = [gst::ElementFactory::make("queue").build()?, gst::ElementFactory::make("videoconvert").build()?, sink.clone().upcast()];
        pipeline.add_many(&video)?;
        gst::Element::link_many(&video)?;
        let weak = pipeline.downgrade();
        let video_in = video[0].static_pad("sink").context("no pad")?;
        decode.connect_pad_added(move |_, pad| {
            let Some(pipeline) = weak.upgrade() else { return };
            let is_video = pad.current_caps().and_then(|c| c.structure(0).map(|s| s.name().starts_with("video/"))).unwrap_or(false);
            let linked = if is_video && !video_in.is_linked() {
                pad.link(&video_in).map(|_| ()).map_err(anyhow::Error::from)
            } else {
                // Audio is read separately; here it only needs somewhere to go.
                discard(&pipeline, pad)
            };
            if let Err(err) = linked {
                log::warn!("export video: {err:#}");
            }
        });
        media::wait_state(&pipeline, gst::State::Paused, Duration::from_secs(10))?;
        if start > 0.05 {
            let _ = pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, gst::ClockTime::from_nseconds((start * 1e9) as u64));
            let _ = pipeline.state(gst::ClockTime::from_seconds(10));
        }
        pipeline.set_state(gst::State::Playing)?;
        Ok(Self { pipeline, sink, current: None, next: None, ended: false })
    }

    fn pull(&mut self) -> anyhow::Result<Option<(RgbaImage, f64)>> {
        if self.ended {
            return Ok(None);
        }
        let Some(sample) = self.sink.try_pull_sample(gst::ClockTime::from_seconds(10)) else {
            if self.sink.is_eos() {
                self.ended = true;
                return Ok(None);
            }
            if let Some(bus) = self.pipeline.bus()
                && let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error])
                && let gst::MessageView::Error(err) = msg.view()
            {
                bail!("{}", err.error());
            }
            bail!("decoding stalled");
        };
        let pts = sample.buffer().and_then(|b| b.pts()).map_or(0., |t| t.nseconds() as f64 / 1e9);
        Ok(media::sample_image(&sample).map(|image| (image, pts)))
    }

    /// The frame showing at `t`; times only move forward.
    fn frame_at(&mut self, t: f64) -> anyhow::Result<RgbaImage> {
        loop {
            if self.next.is_none() {
                self.next = self.pull()?;
            }
            match &self.next {
                Some((_, pts)) if *pts <= t + 0.0005 || self.current.is_none() => self.current = self.next.take(),
                _ => break,
            }
        }
        self.current.as_ref().map(|(image, _)| image.clone()).context("the clip has no frames")
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn discard(pipeline: &gst::Pipeline, pad: &gst::Pad) -> anyhow::Result<()> {
    let fake = gst::ElementFactory::make("fakesink").property("sync", false).property("async", false).build()?;
    pipeline.add(&fake)?;
    fake.sync_state_with_parent()?;
    pad.link(&fake.static_pad("sink").context("no pad")?)?;
    Ok(())
}

/// The edited clip's sound, stereo f32 at `RATE`: the tracks mixed with their
/// volumes, laid out on the edited clock. Speed pieces resample by nearest
/// sample; a freeze is silent.
fn mix_audio(file: &Path, audio: &Audio, map: &TimeMap) -> anyhow::Result<Vec<f32>> {
    let total = (map.duration() * RATE as f64).ceil() as usize + 1;
    if audio.muted {
        return Ok(vec![0.; total * 2]);
    }
    let source = decode_audio(file, audio)?;
    let mut out = vec![0f32; total * 2];
    let mut piece = 0;
    for k in 0..total {
        let t = k as f64 / RATE as f64;
        while piece + 1 < map.pieces.len() && t >= map.pieces[piece].comp_end() {
            piece += 1;
        }
        let Some(p) = map.pieces.get(piece) else { break };
        if p.is_freeze() {
            continue;
        }
        let src = p.src_start + (t - p.comp_start).max(0.) * p.factor();
        let i = (src * RATE as f64).round() as usize * 2;
        if i + 1 < source.len() {
            out[k * 2] = source[i];
            out[k * 2 + 1] = source[i + 1];
        }
    }
    Ok(out)
}

/// All audio tracks of the clip mixed down on the source clock.
fn decode_audio(file: &Path, audio: &Audio) -> anyhow::Result<Vec<f32>> {
    let pipeline = gst::Pipeline::new();
    let decode = gst::ElementFactory::make("uridecodebin")
        .property("uri", media::file_uri(file))
        .property("caps", gst::Caps::builder("audio/x-raw").build())
        .property("expose-all-streams", false)
        .build()?;
    pipeline.add(&decode)?;
    let mix = Arc::new(Mutex::new(Vec::<f32>::new()));
    let tracks = Arc::new(AtomicUsize::new(0));
    let weak = pipeline.downgrade();
    let (mix_in, audio) = (mix.clone(), audio.clone());
    decode.connect_pad_added(move |_, pad| {
        let Some(pipeline) = weak.upgrade() else { return };
        let is_audio = pad.current_caps().and_then(|c| c.structure(0).map(|s| s.name().starts_with("audio/"))).unwrap_or(false);
        let linked = (|| {
            if !is_audio {
                return discard(&pipeline, pad);
            }
            let volume = audio.volume(tracks.fetch_add(1, Ordering::Relaxed));
            let caps = gst::Caps::builder("audio/x-raw").field("format", "F32LE").field("rate", RATE as i32).field("channels", 2).field("layout", "interleaved").build();
            let sink = gst_app::AppSink::builder().caps(&caps).sync(false).build();
            let mix = mix_in.clone();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let (Some(buffer), Some(segment)) = (sample.buffer(), sample.segment()) else { return Ok(gst::FlowSuccess::Ok) };
                        let at = buffer
                            .pts()
                            .and_then(|pts| segment.downcast_ref::<gst::ClockTime>()?.to_stream_time(pts))
                            .map_or(0, |t| (t.nseconds() as f64 / 1e9 * RATE as f64).round() as usize * 2);
                        let Ok(map) = buffer.map_readable() else { return Ok(gst::FlowSuccess::Ok) };
                        let samples = map.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                        let mut mix = mix.lock().unwrap();
                        let n = map.len() / 4;
                        if mix.len() < at + n {
                            mix.resize(at + n, 0.);
                        }
                        for (dst, s) in mix[at..at + n].iter_mut().zip(samples) {
                            *dst += s * volume;
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
            let elements = [
                gst::ElementFactory::make("queue").build()?,
                gst::ElementFactory::make("audioconvert").build()?,
                gst::ElementFactory::make("audioresample").build()?,
                sink.upcast(),
            ];
            pipeline.add_many(&elements)?;
            gst::Element::link_many(&elements)?;
            for e in &elements {
                e.sync_state_with_parent()?;
            }
            pad.link(&elements[0].static_pad("sink").context("no pad")?)?;
            Ok(())
        })();
        if let Err(err) = linked {
            log::warn!("export audio: {err:#}");
        }
    });
    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline.bus().context("no bus")?;
    let result = match bus.timed_pop_filtered(gst::ClockTime::from_seconds(600), &[gst::MessageType::Eos, gst::MessageType::Error]) {
        Some(msg) => match msg.view() {
            gst::MessageView::Error(err) => Err(anyhow::anyhow!("reading the audio: {}", err.error())),
            _ => Ok(()),
        },
        None => Err(anyhow::anyhow!("reading the audio timed out")),
    };
    let _ = pipeline.set_state(gst::State::Null);
    result?;
    let mix = std::mem::take(&mut *mix.lock().unwrap());
    Ok(mix)
}

#[cfg(test)]
mod tests {
    use super::super::model::{Kind, Segment};
    use super::super::player::probe;
    use super::*;

    /// A real 3 s clip with sound, 30 fps, the left half black and the right half white.
    fn clip(dir: &Path) -> std::path::PathBuf {
        media::init().unwrap();
        let path = dir.join("clip.mp4");
        let pipeline = gst::parse::launch(&format!(
            "videotestsrc num-buffers=90 pattern=white ! video/x-raw,width=320,height=240,framerate=30/1 \
             ! videobox right=160 fill=black ! videobox left=-160 fill=black ! videoconvert ! x264enc ! mp4mux name=m ! filesink location={} \
             audiotestsrc num-buffers=141 samplesperbuffer=1024 ! audio/x-raw,rate=48000 ! audioconvert ! avenc_aac ! m.",
            path.display()
        ))
        .unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();
        let bus = pipeline.bus().unwrap();
        let msg = bus.timed_pop_filtered(gst::ClockTime::from_seconds(30), &[gst::MessageType::Eos, gst::MessageType::Error]).unwrap();
        assert!(matches!(msg.view(), gst::MessageView::Eos(_)), "{msg:?}");
        pipeline.set_state(gst::State::Null).unwrap();
        path
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("slopshot-export-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cut_and_speed_shorten_the_movie_and_keep_its_sound() {
        let dir = scratch("movie");
        let src = clip(&dir);
        let info = probe(&src).unwrap();
        assert!((info.duration - 3.).abs() < 0.1, "{info:?}");
        let mut edit = Edit::new(info.duration);
        edit.trim_end = 3.;
        edit.segments.push(Segment::new(Kind::Cut, 1., 1.5));
        let mut fast = Segment::new(Kind::Speed, 2., 3.);
        fast.speed = 2.;
        edit.segments.push(fast);
        let mut censor = Segment::new(Kind::Censor, 0., 3.);
        censor.rect = super::super::model::NRect { x: 0., y: 0., w: 0.5, h: 1. };
        edit.segments.push(censor);
        for (format, ext) in [(Format::Mov, "mov"), (Format::Mp4, "mp4")] {
            let out = dir.join(format!("out.{ext}"));
            let options = Options { format, ..Default::default() };
            let last = Mutex::new(0.);
            export(&src, &info, &edit, &Audio::default(), &options, &out, &|p| *last.lock().unwrap() = p).unwrap();
            assert_eq!(*last.lock().unwrap(), 1.);
            let got = probe(&out).unwrap();
            // 3 s − 0.5 s cut − half of the 1 s sped up.
            assert!((got.duration - 2.).abs() < 0.1, "{format:?}: {got:?}");
            assert_eq!(got.audio_tracks, 1);
            assert_eq!((got.width, got.height), (320, 240));
        }
    }

    #[test]
    fn freeze_lengthens_and_resolution_scales() {
        let dir = scratch("freeze");
        let src = clip(&dir);
        let info = probe(&src).unwrap();
        let mut edit = Edit::new(info.duration);
        let mut hold = Segment::new(Kind::Freeze, 1., 2.);
        hold.end = hold.start + 1.5;
        edit.segments.push(hold);
        let out = dir.join("out.mp4");
        let options = Options { format: Format::Mp4, resolution: Resolution::P480, frame_rate: FrameRate::F15, ..Default::default() };
        export(&src, &info, &edit, &Audio { muted: true, volumes: vec![] }, &options, &out, &|_| {}).unwrap();
        let got = probe(&out).unwrap();
        assert!((got.duration - (info.duration + 1.5)).abs() < 0.15, "{got:?}");
        assert!((got.fps - 15.).abs() < 0.5, "{got:?}");
        // 240 p is already under 480 p: never upscaled.
        assert_eq!(got.height, 240);
    }

    #[test]
    fn gif_lasts_the_kept_time_and_the_censor_is_burnt_in() {
        use image::AnimationDecoder as _;
        let dir = scratch("gif");
        let src = clip(&dir);
        let info = probe(&src).unwrap();
        let mut edit = Edit::new(info.duration);
        edit.trim_end = 2.;
        // The caption comes in halfway, so the GIF's second frame holds just its box.
        let mut text = Segment::new(Kind::Text, 1., 2.);
        text.text = "Hello".into();
        edit.segments.push(text);
        let mut censor = Segment::new(Kind::Censor, 0., 2.);
        censor.censor_style = super::super::model::CensorStyle::Pixelate;
        // Cells are laid out from the box centre (x = 144), so one straddles the edge.
        censor.rect = super::super::model::NRect { x: 0.2, y: 0., w: 0.5, h: 0.5 };
        edit.segments.push(censor);
        let out = dir.join("out.gif");
        let options = Options { format: Format::Gif, frame_rate: FrameRate::F10, ..Default::default() };
        export(&src, &info, &edit, &Audio::default(), &options, &out, &|_| {}).unwrap();
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap())).unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        let centis: u32 = frames.iter().map(|f| f.delay().numer_denom_ms()).map(|(n, d)| n / d / 10).sum();
        assert_eq!(centis, 200, "the GIF lasts the 2 s kept");
        // The fixture is a still picture: 20 ticks collapse into the first frame and the caption's.
        assert_eq!(frames.len(), 2);
        for frame in &frames {
            let image = frame.buffer();
            // Below the censor and the caption the black/white edge at x = 160 stays sharp…
            assert!(image.get_pixel(150, 230).0[0] < 50 && image.get_pixel(170, 230).0[0] > 200, "{:?} {:?}", image.get_pixel(150, 230), image.get_pixel(170, 230));
            // …and inside it the edge is averaged away into one grey cell.
            let cell = image.get_pixel(160, 60).0[0];
            assert!((60..200).contains(&cell), "censored edge should be grey, got {cell}");
        }
        assert_ne!(frames[0].buffer(), frames[1].buffer(), "the caption shows in the second frame");
    }

    #[test]
    fn nothing_to_export_and_too_long_a_gif_fail_clearly() {
        let dir = scratch("fail");
        let src = clip(&dir);
        let info = probe(&src).unwrap();
        let mut edit = Edit::new(info.duration);
        edit.segments.push(Segment::new(Kind::Cut, 0., 3.5));
        let err = export(&src, &info, &edit, &Audio::default(), &Options::default(), &dir.join("x.mov"), &|_| {}).unwrap_err();
        assert!(err.to_string().contains("Nothing left to export"), "{err}");
        assert!(!dir.join("x.mov").exists());

        let long = Info { duration: 400., ..info.clone() };
        let edit = Edit::new(400.);
        let options = Options { format: Format::Gif, frame_rate: FrameRate::F10, ..Default::default() };
        let err = export(&src, &long, &edit, &Audio::default(), &options, &dir.join("x.gif"), &|_| {}).unwrap_err();
        assert!(err.to_string().contains("too long for a GIF"), "{err}");
        assert!(!dir.join("x.gif").exists());

        let err = export(&dir.join("missing.mp4"), &info, &Edit::new(3.), &Audio::default(), &Options::default(), &dir.join("y.mov"), &|_| {}).unwrap_err();
        assert!(!dir.join("y.mov").exists(), "{err}");
    }
}
