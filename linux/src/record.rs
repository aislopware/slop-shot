//! Record Area (ScreenRecorder.swift and ScreenCapturer.recordRegion): a ScreenCast
//! feed cropped to the rect, encoded to H.264/AAC in an MP4 by GStreamer.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use futures_util::StreamExt as _;
use gpui::{App, AppContext as _, AsyncApp, Bounds, Global, Pixels};
use gstreamer as gst;
use gstreamer::glib::object::Cast as _;
use gstreamer::prelude::*;

use crate::card::{self, Request};
use crate::screencast::{self, Cast};
use crate::settings::Settings;
use crate::{alert, flow, history, media, output};

struct Recording {
    cast: Option<Arc<Cast>>,
    pipeline: Option<gst::Pipeline>,
    file: PathBuf,
    base: String,
    display: Bounds<Pixels>,
    rect: Bounds<Pixels>,
    scale: f32,
    system_audio: bool,
    mic: bool,
    paused_at: Option<gst::ClockTime>,
    /// Time spent paused so far, cut out of the clip.
    paused_total: gst::ClockTime,
    /// Bumped per pipeline, so a stale bus watcher can tell it's stale.
    generation: u64,
    stopping: bool,
}

impl Global for Recording {}

pub fn is_recording(cx: &App) -> bool {
    cx.has_global::<Recording>()
}

fn arr(b: Bounds<Pixels>) -> [f32; 4] {
    [b.origin.x, b.origin.y, b.size.width, b.size.height].map(f32::from)
}

/// `display` and `rect` (display-local) are logical; `scale` is pixels per logical pixel.
pub fn start(display: Bounds<Pixels>, rect: Bounds<Pixels>, scale: f32, cx: &mut App) {
    if let Err(err) = media::init() {
        alert::error("Couldn't start recording", &format!("{err:#}"), cx);
        return;
    }
    let s = Settings::get(cx);
    let (system_audio, mic) = (s.record_system_audio, s.record_microphone);
    cx.set_global(flow::Busy);
    cx.spawn(async move |cx| {
        let opened = screencast::open(display, true, false, cx).await;
        cx.update(|cx| {
            cx.remove_global::<flow::Busy>();
            let cast = match opened {
                Ok(cast) => Arc::new(cast),
                Err(err) if err.is::<screencast::Cancelled>() => return,
                Err(err) => return alert::error("Couldn't start recording", &format!("{err:#}"), cx),
            };
            let base = output::base_name();
            cx.set_global(Recording {
                cast: Some(cast),
                pipeline: None,
                file: output::unique_path(&output::temp_dir(), &base, "mp4"),
                base,
                display,
                rect,
                scale,
                system_audio,
                mic,
                paused_at: None,
                paused_total: gst::ClockTime::ZERO,
                generation: 0,
                stopping: false,
            });
            if let Err(err) = run_pipeline(cx) {
                abort(cx);
                return alert::error("Couldn't start recording", &format!("{err:#}"), cx);
            }
            card::send(&Request::Focus { display: arr(display), rect: arr(rect) }, cx);
            card::send(&Request::RecordBar { display: arr(display), rect: arr(rect), mic }, cx);
            crate::tray::sync(cx);
        });
    })
    .detach();
}

fn describe(rec: &Recording, cast: &Cast) -> anyhow::Result<String> {
    let (crop, w, h) = cast.crop(rec.display, rec.rect, rec.scale);
    let fps = 60.;
    let kbps = ((w * h) as f64 * fps * 0.12).clamp(4e6, 60e6) / 1000.;
    let mut desc = format!(
        "{src} ! queue ! videoconvert ! {crop} ! videoscale ! \
         video/x-raw,width={w},height={h} ! videoconvert ! video/x-raw,format=I420,colorimetry=bt709 ! valve name=vvalve ! \
         x264enc bitrate={kbps} speed-preset=veryfast tune=zerolatency key-int-max=120 ! queue ! \
         mp4mux name=mux ! filesink location=\"{file}\"",
        src = cast.source("")?,
        kbps = kbps as u32,
        file = rec.file.display(),
    );
    // Each source its own track, as on macOS, so the video editor can set their volumes.
    let audio = |device: &str, tail: &str| {
        format!(
            " pulsesrc {device} ! queue ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! {tail} \
             avenc_aac bitrate=160000 ! queue ! mux."
        )
    };
    if rec.system_audio {
        desc += &audio("device=@DEFAULT_MONITOR@", "valve name=svalve !");
    }
    if rec.mic {
        desc += &audio("", "volume name=micvol ! valve name=mvalve !");
    }
    Ok(desc)
}

fn run_pipeline(cx: &mut App) -> anyhow::Result<()> {
    let rec = cx.global::<Recording>();
    let cast = rec.cast.clone().context("no screen feed")?;
    let mut needed = vec!["pipewiresrc", "videocrop", "x264enc", "mp4mux"];
    if rec.system_audio || rec.mic {
        needed.extend(["pulsesrc", "avenc_aac"]);
    }
    let missing = media::missing(&needed);
    anyhow::ensure!(missing.is_empty(), "GStreamer is missing {}; install gstreamer1.0-pipewire, gstreamer1.0-plugins-ugly and gstreamer1.0-libav.", missing.join(", "));
    let pipeline = gst::parse::launch(&describe(rec, &cast)?)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
    let bus = pipeline.bus().context("no bus")?;
    pipeline.set_state(gst::State::Playing).context("the recording pipeline didn't start")?;
    let rec = cx.global_mut::<Recording>();
    rec.generation += 1;
    rec.pipeline = Some(pipeline);
    rec.paused_at = None;
    rec.paused_total = gst::ClockTime::ZERO;
    let generation = rec.generation;
    log::info!("recording started: {}", rec.file.display());
    cx.spawn(async move |cx| watch(bus, generation, cx).await).detach();
    Ok(())
}

async fn watch(bus: gst::Bus, generation: u64, cx: &mut AsyncApp) {
    let mut messages = bus.stream();
    while let Some(msg) = messages.next().await {
        let current = cx.update(|cx| cx.try_global::<Recording>().is_some_and(|r| r.generation == generation));
        if !current {
            return;
        }
        match msg.view() {
            gst::MessageView::Eos(_) => {
                cx.update(finish);
                return;
            }
            gst::MessageView::Error(err) => {
                let text = format!("{}", err.error());
                log::error!("recording: {text} ({:?})", err.debug());
                cx.update(|cx| {
                    abort(cx);
                    alert::error("Recording stopped", &text, cx);
                });
                return;
            }
            _ => {}
        }
    }
}

fn valves(pipeline: &gst::Pipeline) -> Vec<gst::Element> {
    ["vvalve", "svalve", "mvalve"].iter().filter_map(|n| pipeline.by_name(n)).collect()
}

/// AVAssetWriter has no pause either: drop buffers while paused, then shift later ones
/// back by the paused time so the clip has no frozen gap.
pub fn set_paused(paused: bool, cx: &mut App) {
    let Some(rec) = cx.try_global::<Recording>() else { return };
    let Some(pipeline) = rec.pipeline.clone() else { return };
    let now = pipeline.current_running_time().unwrap_or_default();
    let rec = cx.global_mut::<Recording>();
    if paused {
        rec.paused_at = Some(now);
    } else if let Some(at) = rec.paused_at.take() {
        rec.paused_total += now.saturating_sub(at);
    }
    let offset = -(rec.paused_total.nseconds() as i64);
    for valve in valves(&pipeline) {
        if !paused && let Some(pad) = valve.static_pad("src") {
            pad.set_offset(offset);
        }
        valve.set_property("drop", paused);
    }
}

pub fn set_mic_muted(muted: bool, cx: &mut App) {
    if let Some(vol) = cx.try_global::<Recording>().and_then(|r| r.pipeline.as_ref()?.by_name("micvol")) {
        vol.set_property("mute", muted);
    }
}

fn teardown(cx: &mut App) -> Option<Recording> {
    let rec = cx.has_global::<Recording>().then(|| cx.remove_global::<Recording>())?;
    if let Some(pipeline) = &rec.pipeline {
        let _ = pipeline.set_state(gst::State::Null);
    }
    card::send(&Request::HideChrome, cx);
    crate::tray::sync(cx);
    if let Some(cast) = rec.cast.clone() {
        cx.background_spawn(async move {
            if let Ok(cast) = Arc::try_unwrap(cast) {
                cast.close().await;
            }
        })
        .detach();
    }
    Some(rec)
}

fn abort(cx: &mut App) {
    if let Some(rec) = teardown(cx) {
        let _ = std::fs::remove_file(&rec.file);
    }
}

/// Stop: the muxer needs end-of-stream to write a playable file; `finish` runs on EOS.
pub fn stop(cx: &mut App) {
    let Some(rec) = cx.try_global::<Recording>() else { return };
    if rec.stopping {
        return;
    }
    let Some(pipeline) = rec.pipeline.clone() else { return abort(cx) };
    let generation = rec.generation;
    cx.global_mut::<Recording>().stopping = true;
    card::send(&Request::HideChrome, cx);
    // A paused pipeline drops everything, so let EOS through.
    for valve in valves(&pipeline) {
        valve.set_property("drop", false);
    }
    pipeline.send_event(gst::event::Eos::new());
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(5)).await;
        cx.update(|cx| {
            if cx.try_global::<Recording>().is_some_and(|r| r.generation == generation) {
                log::warn!("recording: no end-of-stream after 5 s");
                finish(cx);
            }
        });
    })
    .detach();
}

/// Quitting mid-recording: the clip is finalised, synchronously, and kept in the temp folder.
pub fn shutdown(cx: &mut App) {
    let Some(pipeline) = cx.try_global::<Recording>().and_then(|r| r.pipeline.clone()) else { return abort(cx) };
    for valve in valves(&pipeline) {
        valve.set_property("drop", false);
    }
    pipeline.send_event(gst::event::Eos::new());
    if let Some(bus) = pipeline.bus() {
        bus.timed_pop_filtered(gst::ClockTime::from_seconds(3), &[gst::MessageType::Eos, gst::MessageType::Error]);
    }
    teardown(cx);
}

pub fn discard(cx: &mut App) {
    abort(cx);
}

/// Throws the clip away and records the same rect again.
pub fn restart(cx: &mut App) {
    let Some(rec) = cx.try_global::<Recording>() else { return };
    if let Some(pipeline) = &rec.pipeline {
        let _ = pipeline.set_state(gst::State::Null);
    }
    let _ = std::fs::remove_file(&rec.file);
    let base = output::base_name();
    let rec = cx.global_mut::<Recording>();
    rec.file = output::unique_path(&output::temp_dir(), &base, "mp4");
    rec.base = base;
    rec.pipeline = None;
    if let Err(err) = run_pipeline(cx) {
        abort(cx);
        alert::error("Couldn't restart recording", &format!("{err:#}"), cx);
    }
}

fn finish(cx: &mut App) {
    let Some(rec) = teardown(cx) else { return };
    let file = rec.file.clone();
    if !std::fs::metadata(&file).is_ok_and(|m| m.len() > 0) {
        let _ = std::fs::remove_file(&file);
        return alert::error("Recording failed", "The recording couldn't be saved — no frames were captured.", cx);
    }
    let settings = Settings::get(cx).clone();
    if settings.play_sound {
        output::play_shutter();
    }
    if settings.copy_to_clipboard {
        output::copy_file(&file, cx);
    }
    cx.set_global(flow::Last { image: None, base: rec.base.clone(), file: file.clone() });
    crate::tray::sync(cx);

    let task = cx.background_spawn({
        let file = file.clone();
        async move { media::poster(&file, 1280) }
    });
    cx.spawn(async move |cx| {
        let poster = task.await;
        cx.update(|cx| {
            let (image, length) = match poster {
                Ok((image, length)) => (image, length),
                Err(err) => {
                    log::warn!("reading the recording back: {err:#}");
                    (image::RgbaImage::from_pixel(16, 9, image::Rgba([0, 0, 0, 255])), Duration::ZERO)
                }
            };
            let subtitle = if length.is_zero() { "video".to_owned() } else { media::timecode(length) };
            if settings.show_thumbnail
                && let Ok(thumb) = card::write_thumbnail(&image)
            {
                card::send(&Request::Show { thumb, file: file.clone(), display: arr(rec.display), side: settings.preview_side, video: true }, cx);
            }
            history::add(history::Kind::Video, Some(file), None, subtitle, Some(Arc::new(image)), cx);
        });
    })
    .detach();
}

/// The card's Save: copies the clip into the save folder.
pub fn save_video(file: PathBuf, cx: &mut App) {
    let folder = Settings::get(cx).save_folder.clone();
    let result = (|| {
        std::fs::create_dir_all(&folder)?;
        let stem = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let dest = output::unique_path(&folder, &stem, "mp4");
        std::fs::copy(&file, &dest)?;
        anyhow::Ok(dest)
    })();
    match result {
        Ok(dest) => {
            if cx.has_global::<flow::Last>() {
                cx.global_mut::<flow::Last>().file = dest;
            }
        }
        Err(err) => alert::error("Couldn't save", &format!("{err:#}"), cx),
    }
}
