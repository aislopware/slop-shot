//! GStreamer helpers shared by recording, scrolling capture and the video windows.

use std::path::Path;
use std::sync::Once;
use std::time::Duration;

use anyhow::{Context as _, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use image::RgbaImage;

pub fn init() -> anyhow::Result<()> {
    static INIT: Once = Once::new();
    let mut result = Ok(());
    INIT.call_once(|| result = gst::init());
    result.context("starting GStreamer")
}

pub fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path).map(|u| u.to_string()).unwrap_or_default()
}

/// An RGBA frame from an appsink sample.
pub fn sample_image(sample: &gst::Sample) -> Option<RgbaImage> {
    let caps = sample.caps()?;
    let s = caps.structure(0)?;
    let (w, h) = (s.get::<i32>("width").ok()? as u32, s.get::<i32>("height").ok()? as u32);
    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    let stride = (map.len() / h.max(1) as usize).max(w as usize * 4);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in 0..h as usize {
        out.extend_from_slice(map.get(row * stride..row * stride + w as usize * 4)?);
    }
    RgbaImage::from_raw(w, h, out)
}

/// Waits for the pipeline to reach `state`, or fails with the bus's error.
pub fn wait_state(pipeline: &gst::Pipeline, state: gst::State, timeout: Duration) -> anyhow::Result<()> {
    pipeline.set_state(state)?;
    let (result, current, _) = pipeline.state(gst::ClockTime::from_nseconds(timeout.as_nanos() as u64));
    if result.is_err() || current != state {
        if let Some(bus) = pipeline.bus()
            && let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error])
            && let gst::MessageView::Error(err) = msg.view()
        {
            bail!("{}", err.error());
        }
        bail!("the media pipeline didn't start");
    }
    Ok(())
}

/// A frame ~0.1 s in, skipping the black frame clips often start with, scaled to
/// fit `max_side`; and the clip's length.
pub fn poster(file: &Path, max_side: u32) -> anyhow::Result<(RgbaImage, Duration)> {
    init()?;
    let pipeline = gst::parse::launch(&format!(
        "uridecodebin uri=\"{}\" ! videoconvert ! videoscale ! video/x-raw,format=RGBA,pixel-aspect-ratio=1/1 ! appsink name=sink",
        file_uri(file)
    ))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| anyhow::anyhow!("not a pipeline"))?;
    let sink = pipeline.by_name("sink").context("no sink")?.downcast::<gst_app::AppSink>().map_err(|_| anyhow::anyhow!("not an appsink"))?;
    let result = (|| {
        wait_state(&pipeline, gst::State::Paused, Duration::from_secs(5))?;
        let length = pipeline.query_duration::<gst::ClockTime>().map(|d| Duration::from_nanos(d.nseconds())).unwrap_or_default();
        if length > Duration::from_millis(200) {
            let _ = pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, gst::ClockTime::from_mseconds(100));
            let _ = pipeline.state(gst::ClockTime::from_seconds(5));
        }
        let sample = sink.pull_preroll().context("no frame")?;
        let image = sample_image(&sample).context("unreadable frame")?;
        let (w, h) = image.dimensions();
        let scale = (max_side as f32 / w.max(h) as f32).min(1.);
        let image = if scale < 1. {
            image::imageops::resize(&image, ((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1), image::imageops::FilterType::Triangle)
        } else {
            image
        };
        anyhow::Ok((image, length))
    })();
    let _ = pipeline.set_state(gst::State::Null);
    result
}

/// `m:ss`, the history subtitle and timeline labels.
pub fn timecode(d: Duration) -> String {
    let s = d.as_secs_f64().round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Elements this machine lacks among `names`, for a clear error instead of a parse failure.
pub fn missing(names: &[&str]) -> Vec<String> {
    names.iter().filter(|n| gst::ElementFactory::find(n).is_none()).map(|n| n.to_string()).collect()
}
