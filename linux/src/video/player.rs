//! Plays a clip through a `TimeMap` (the AVPlayer + AVComposition of the macOS app).
//!
//! Each normal or speed piece is one segment seek at the piece's rate, chained on
//! SEGMENT_DONE so pieces follow without a flush; a freeze pauses on its frame for
//! as long as it holds. Frames land in an appsink the window polls.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_pbutils as pbutils;
use image::RgbaImage;

use super::model::TimeMap;
use crate::media;

/// What the editor needs to know about the clip before showing it.
#[derive(Clone, Debug)]
pub struct Info {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub audio_tracks: usize,
}

pub fn probe(file: &Path) -> anyhow::Result<Info> {
    media::init()?;
    let discoverer = pbutils::Discoverer::new(gst::ClockTime::from_seconds(10))?;
    let info = discoverer.discover_uri(&media::file_uri(file))?;
    let video = info.video_streams().into_iter().next().context("This file has no video track.")?;
    let fps = video.framerate();
    Ok(Info {
        duration: info.duration().map_or(0., |d| d.nseconds() as f64 / 1e9).max(0.05),
        width: video.width(),
        height: video.height(),
        fps: if fps.denom() > 0 && fps.numer() > 0 { fps.numer() as f64 / fps.denom() as f64 } else { 30. },
        audio_tracks: info.audio_streams().len(),
    })
}

/// The largest even size inside `max` with the clip's aspect.
pub fn fit(info: &Info, max: (u32, u32)) -> (u32, u32) {
    let scale = (max.0 as f64 / info.width as f64).min(max.1 as f64 / info.height as f64).min(1.);
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    (even(info.width as f64 * scale), even(info.height as f64 * scale))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Paused,
    /// The pipeline plays piece `piece`; `seqnum` is its seek's.
    Playing { piece: usize, seqnum: gst::Seqnum },
    /// A freeze holds its frame until `until`.
    Holding { piece: usize, until: Instant },
}

/// The newest frame and its source time.
#[derive(Default)]
struct Latest {
    frame: Option<(RgbaImage, f64)>,
}

pub struct Player {
    pipeline: gst::Pipeline,
    latest: Arc<Mutex<Latest>>,
    fresh: Arc<AtomicBool>,
    volumes: Arc<Mutex<Vec<gst::Element>>>,
    levels: Arc<Mutex<(Vec<f64>, bool)>>,
    map: TimeMap,
    mode: Mode,
    /// Edited-clip time of the playhead.
    pub comp: f64,
}

impl Player {
    /// Frames come scaled to `size`.
    pub fn open(file: &Path, size: (u32, u32)) -> anyhow::Result<Self> {
        media::init()?;
        let pipeline = gst::Pipeline::new();
        let decode = gst::ElementFactory::make("uridecodebin").property("uri", media::file_uri(file)).build()?;
        pipeline.add(&decode)?;

        let latest = Arc::new(Mutex::new(Latest::default()));
        let fresh = Arc::new(AtomicBool::new(false));
        let volumes = Arc::new(Mutex::new(Vec::<gst::Element>::new()));
        let levels = Arc::new(Mutex::new((Vec::<f64>::new(), false)));

        let weak = pipeline.downgrade();
        let (latest_in, fresh_in, volumes_in, levels_in) = (latest.clone(), fresh.clone(), volumes.clone(), levels.clone());
        decode.connect_pad_added(move |_, pad| {
            let Some(pipeline) = weak.upgrade() else { return };
            let caps = pad.current_caps().or_else(|| Some(pad.query_caps(None)));
            let Some(name) = caps.as_ref().and_then(|c| c.structure(0)).map(|s| s.name().to_string()) else { return };
            let linked = if name.starts_with("video/") {
                link_video(&pipeline, pad, size, latest_in.clone(), fresh_in.clone())
            } else if name.starts_with("audio/") {
                link_audio(&pipeline, pad).map(|volume| {
                    let mut vols = volumes_in.lock().unwrap();
                    let (levels, muted) = &*levels_in.lock().unwrap();
                    apply_level(&volume, levels.get(vols.len()).copied().unwrap_or(1.), *muted);
                    vols.push(volume);
                })
            } else {
                Ok(())
            };
            if let Err(err) = linked {
                log::warn!("video player: linking {name}: {err:#}");
            }
        });

        media::wait_state(&pipeline, gst::State::Paused, Duration::from_secs(10))?;
        Ok(Self { pipeline, latest, fresh, volumes, levels, map: TimeMap::default(), mode: Mode::Paused, comp: 0. })
    }

    pub fn map(&self) -> &TimeMap {
        &self.map
    }

    pub fn is_playing(&self) -> bool {
        self.mode != Mode::Paused
    }

    pub fn source(&self) -> f64 {
        self.map.source_time(self.comp)
    }

    /// Swaps in an edited map and keeps the playhead on the same source frame.
    pub fn set_map(&mut self, map: TimeMap) {
        let source = self.source();
        let playing = self.is_playing();
        self.map = map;
        self.comp = self.map.comp_time(source).min(self.map.duration());
        if playing { self.start_at(self.comp) } else { self.show(self.comp) }
    }

    /// Track volumes 0…1 in order; muted silences them all.
    pub fn set_levels(&self, levels: &[f64], muted: bool) {
        *self.levels.lock().unwrap() = (levels.to_vec(), muted);
        for (i, volume) in self.volumes.lock().unwrap().iter().enumerate() {
            apply_level(volume, levels.get(i).copied().unwrap_or(1.), muted);
        }
    }

    pub fn play(&mut self) {
        if self.is_playing() || self.map.pieces.is_empty() {
            return;
        }
        if self.comp >= self.map.duration() - 0.05 {
            self.comp = 0.;
        }
        self.start_at(self.comp);
    }

    pub fn pause(&mut self) {
        if self.is_playing() {
            self.comp = self.current_comp();
            let _ = self.pipeline.set_state(gst::State::Paused);
            self.mode = Mode::Paused;
        }
    }

    pub fn toggle(&mut self) {
        if self.is_playing() { self.pause() } else { self.play() }
    }

    /// Keeps playing if it was.
    pub fn seek_comp(&mut self, t: f64) {
        self.comp = t.clamp(0., self.map.duration());
        if self.is_playing() { self.start_at(self.comp) } else { self.show(self.comp) }
    }

    pub fn seek_source(&mut self, t: f64) {
        self.seek_comp(self.map.comp_time(t));
    }

    /// Advances the playhead; returns the newest frame and its source time, if one came.
    pub fn tick(&mut self) -> Option<(RgbaImage, f64)> {
        self.drain_bus();
        if let Mode::Holding { piece, until } = self.mode
            && Instant::now() >= until
        {
            self.next(piece);
        }
        if self.is_playing() {
            self.comp = self.current_comp();
        }
        if self.fresh.swap(false, Ordering::AcqRel) { self.latest.lock().unwrap().frame.take() } else { None }
    }

    fn current_comp(&self) -> f64 {
        match self.mode {
            Mode::Paused => self.comp,
            Mode::Holding { piece, until } => {
                let p = self.map.pieces[piece];
                p.comp_end() - until.saturating_duration_since(Instant::now()).as_secs_f64()
            }
            Mode::Playing { piece, .. } => {
                let p = self.map.pieces[piece];
                let Some(pos) = self.pipeline.query_position::<gst::ClockTime>() else { return self.comp };
                let src = pos.nseconds() as f64 / 1e9;
                (p.comp_start + (src - p.src_start) / p.factor()).clamp(p.comp_start, p.comp_end())
            }
        }
    }

    fn drain_bus(&mut self) {
        let Some(bus) = self.pipeline.bus() else { return };
        while let Some(msg) = bus.pop() {
            match msg.view() {
                gst::MessageView::SegmentDone(_) | gst::MessageView::Eos(_) => {
                    if let Mode::Playing { piece, seqnum } = self.mode
                        && msg.seqnum() == seqnum
                    {
                        self.next(piece);
                    }
                }
                gst::MessageView::Error(err) => log::warn!("video player: {} ({:?})", err.error(), err.debug()),
                _ => {}
            }
        }
    }

    /// Piece `done` finished: the next one follows, or playback stops at the end.
    fn next(&mut self, done: usize) {
        let i = done + 1;
        let Some(p) = self.map.pieces.get(i).copied() else {
            self.comp = self.map.duration();
            let _ = self.pipeline.set_state(gst::State::Paused);
            self.mode = Mode::Paused;
            return;
        };
        self.comp = p.comp_start;
        // From one played piece straight into another, a non-flushing seek keeps the flow seamless.
        if matches!(self.mode, Mode::Playing { .. }) && !p.is_freeze() {
            if let Some(seqnum) = self.seek(p.src_start, p.src_end(), p.factor(), false) {
                self.mode = Mode::Playing { piece: i, seqnum };
                return;
            }
        }
        self.start_at(p.comp_start);
    }

    fn start_at(&mut self, comp: f64) {
        let Some(i) = self.map.piece_at(comp) else { return };
        let p = self.map.pieces[i];
        if p.is_freeze() {
            let _ = self.pipeline.set_state(gst::State::Paused);
            self.seek(p.src_start, p.src_end(), 1., true);
            let left = (p.comp_end() - comp).max(0.);
            self.mode = Mode::Holding { piece: i, until: Instant::now() + Duration::from_secs_f64(left) };
            return;
        }
        let src = p.src_start + (comp - p.comp_start).max(0.) * p.factor();
        match self.seek(src, p.src_end(), p.factor(), true) {
            Some(seqnum) => {
                self.mode = Mode::Playing { piece: i, seqnum };
                let _ = self.pipeline.set_state(gst::State::Playing);
            }
            None => self.mode = Mode::Paused,
        }
    }

    /// Shows the frame at `comp` without playing.
    fn show(&mut self, comp: f64) {
        self.mode = Mode::Paused;
        let src = self.map.source_time(comp);
        self.seek(src, -1., 1., true);
    }

    fn seek(&self, from: f64, to: f64, rate: f64, flush: bool) -> Option<gst::Seqnum> {
        let at = |s: f64| gst::ClockTime::from_nseconds((s.max(0.) * 1e9) as u64);
        let mut flags = gst::SeekFlags::ACCURATE | gst::SeekFlags::SEGMENT;
        if flush {
            flags |= gst::SeekFlags::FLUSH;
        }
        let (stop_type, stop) = if to > from { (gst::SeekType::Set, Some(at(to))) } else { (gst::SeekType::None, None) };
        let event = gst::event::Seek::new(rate, flags, gst::SeekType::Set, at(from), stop_type, stop);
        let seqnum = event.seqnum();
        if self.pipeline.send_event(event) {
            Some(seqnum)
        } else {
            log::warn!("video player: seek to {from:.2}s refused");
            None
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn apply_level(volume: &gst::Element, level: f64, muted: bool) {
    volume.set_property("volume", level.clamp(0., 1.));
    volume.set_property("mute", muted);
}

fn add_chain(pipeline: &gst::Pipeline, pad: &gst::Pad, elements: &[gst::Element]) -> anyhow::Result<()> {
    pipeline.add_many(elements)?;
    gst::Element::link_many(elements)?;
    for e in elements {
        e.sync_state_with_parent()?;
    }
    pad.link(&elements[0].static_pad("sink").context("no sink pad")?)?;
    Ok(())
}

fn link_video(pipeline: &gst::Pipeline, pad: &gst::Pad, size: (u32, u32), latest: Arc<Mutex<Latest>>, fresh: Arc<AtomicBool>) -> anyhow::Result<()> {
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGBA")
        .field("width", size.0 as i32)
        .field("height", size.1 as i32)
        .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
        .build();
    let sink = gst_app::AppSink::builder().caps(&caps).max_buffers(2).drop(true).sync(true).build();
    let store = move |sample: gst::Sample| {
        let source = sample
            .buffer()
            .and_then(|b| b.pts())
            .and_then(|pts| sample.segment()?.downcast_ref::<gst::ClockTime>()?.to_stream_time(pts))
            .map_or(0., |t| t.nseconds() as f64 / 1e9);
        if let Some(image) = media::sample_image(&sample) {
            latest.lock().unwrap().frame = Some((image, source));
            fresh.store(true, Ordering::Release);
        }
    };
    let store = Arc::new(store);
    let on_preroll = store.clone();
    sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                store(sink.pull_sample().map_err(|_| gst::FlowError::Eos)?);
                Ok(gst::FlowSuccess::Ok)
            })
            .new_preroll(move |sink| {
                on_preroll(sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?);
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    let elements = [
        gst::ElementFactory::make("queue").build()?,
        gst::ElementFactory::make("videoconvert").build()?,
        gst::ElementFactory::make("videoscale").build()?,
        sink.upcast(),
    ];
    add_chain(pipeline, pad, &elements)
}

/// Returns the track's volume element.
fn link_audio(pipeline: &gst::Pipeline, pad: &gst::Pad) -> anyhow::Result<gst::Element> {
    let volume = gst::ElementFactory::make("volume").build()?;
    let elements = [
        gst::ElementFactory::make("queue").build()?,
        gst::ElementFactory::make("audioconvert").build()?,
        // Speed pieces play at another rate; this keeps the pitch.
        gst::ElementFactory::make("scaletempo").build()?,
        gst::ElementFactory::make("audioconvert").build()?,
        gst::ElementFactory::make("audioresample").build()?,
        volume.clone(),
        gst::ElementFactory::make("autoaudiosink").build()?,
    ];
    add_chain(pipeline, pad, &elements)?;
    Ok(volume)
}
