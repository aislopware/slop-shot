//! A live feed of one monitor through the ScreenCast portal, the Wayland counterpart of
//! ScreenCaptureKit's SCStream. With `pointer` it is a RemoteDesktop session too, so
//! the scrolling capture can scroll the page itself.
//!
//! The portal asks the user to pick a screen the first time; the restore token it
//! returns makes later sessions start without asking.

use std::os::fd::OwnedFd;

use anyhow::{Context as _, bail};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream};
use ashpd::enumflags2::BitFlags;
use gpui::{AsyncApp, Bounds, Pixels};

use crate::settings::Settings;

enum Kind {
    Cast(Session<Screencast>),
    Remote(RemoteDesktop, Session<RemoteDesktop>),
}

pub struct Cast {
    kind: Kind,
    /// The user may share the screen yet leave "Allow Remote Interaction" off.
    pointer: bool,
    pub fd: OwnedFd,
    pub node: u32,
    /// The shared monitor's logical bounds in the compositor layout.
    pub area: Bounds<Pixels>,
}

fn token(remote: bool, cx: &mut AsyncApp) -> Option<String> {
    cx.update(|cx| {
        let s = Settings::get(cx);
        if remote { s.remote_desktop_token.clone() } else { s.screencast_token.clone() }
    })
}

fn set_token(remote: bool, token: Option<String>, cx: &mut AsyncApp) {
    cx.update(|cx| {
        Settings::update(cx, |s| {
            if remote {
                s.remote_desktop_token = token;
            } else {
                s.screencast_token = token;
            }
        })
    });
}

/// Opens a feed of the monitor with logical bounds `display`. A remembered choice of
/// another monitor is dropped and the portal asked again, once.
pub async fn open(display: Bounds<Pixels>, cursor: bool, pointer: bool, cx: &mut AsyncApp) -> anyhow::Result<Cast> {
    for attempt in 0..2 {
        let restore = if attempt == 0 { token(pointer, cx) } else { None };
        let (cast, stream) = start(restore.as_deref(), cursor, pointer, cx).await?;
        let area = match (stream.position(), stream.size()) {
            (Some((x, y)), Some((w, h))) => Bounds::new(gpui::point(gpui::px(x as f32), gpui::px(y as f32)), gpui::size(gpui::px(w as f32), gpui::px(h as f32))),
            _ => display,
        };
        if area.origin == display.origin && area.size == display.size {
            return Ok(Cast { area, ..cast });
        }
        cast.close().await;
        if restore.is_none() {
            bail!("Share the screen you selected the area on.");
        }
        set_token(pointer, None, cx);
    }
    unreachable!()
}

async fn start(restore: Option<&str>, cursor: bool, pointer: bool, cx: &mut AsyncApp) -> anyhow::Result<(Cast, Stream)> {
    let screencast = Screencast::new().await.context("the ScreenCast portal is unavailable")?;
    let sources = SelectSourcesOptions::default()
        .set_cursor_mode(if cursor { CursorMode::Embedded } else { CursorMode::Hidden })
        .set_sources(BitFlags::from(SourceType::Monitor))
        .set_multiple(false);
    let mut pointer_granted = false;
    let (kind, streams, new_token) = if pointer {
        let remote = RemoteDesktop::new().await.context("the RemoteDesktop portal is unavailable")?;
        let session = remote.create_session(Default::default()).await?;
        remote
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(BitFlags::from(DeviceType::Pointer))
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(restore),
            )
            .await?
            .response()?;
        screencast.select_sources(&session, sources).await?.response()?;
        let started = remote.start(&session, None, Default::default()).await?.response().map_err(refused)?;
        let streams = started.streams().to_vec();
        let token = started.restore_token().map(str::to_owned);
        pointer_granted = started.devices().contains(DeviceType::Pointer);
        (Kind::Remote(remote, session), streams, token)
    } else {
        let session = screencast.create_session(Default::default()).await?;
        screencast
            .select_sources(&session, sources.set_persist_mode(PersistMode::ExplicitlyRevoked).set_restore_token(restore))
            .await?
            .response()?;
        let started = screencast.start(&session, None, Default::default()).await?.response().map_err(refused)?;
        let streams = started.streams().to_vec();
        let token = started.restore_token().map(str::to_owned);
        (Kind::Cast(session), streams, token)
    };
    if new_token.is_some() {
        set_token(pointer, new_token, cx);
    }
    let stream = streams.into_iter().next().context("the portal shared no screen")?;
    let fd = match &kind {
        Kind::Cast(session) => screencast.open_pipe_wire_remote(session, Default::default()).await?,
        Kind::Remote(_, session) => screencast.open_pipe_wire_remote(session, Default::default()).await?,
    };
    let node = stream.pipe_wire_node_id();
    Ok((Cast { kind, pointer: pointer_granted, fd, node, area: Bounds::default() }, stream))
}

fn refused(err: ashpd::Error) -> anyhow::Error {
    match err {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => anyhow::anyhow!(Cancelled),
        other => other.into(),
    }
}

/// The user closed the portal's screen picker.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("screen sharing was cancelled")
    }
}

impl std::error::Error for Cancelled {}

impl Cast {
    /// A `pipewiresrc` description reading this feed; each call hands it its own fd.
    pub fn source(&self, extra: &str) -> anyhow::Result<String> {
        use std::os::fd::IntoRawFd as _;
        let fd = self.fd.try_clone()?.into_raw_fd();
        Ok(format!("pipewiresrc fd={fd} path={} do-timestamp=true keepalive-time=500 {extra}", self.node))
    }

    /// A `videocrop` cutting the feed (the whole monitor) to `rect` on `display`, both
    /// logical, and the cropped size; `scale` is pixels per logical pixel. Even sizes,
    /// as H.264 needs.
    pub fn crop(&self, display: Bounds<Pixels>, rect: Bounds<Pixels>, scale: f32) -> (String, i32, i32) {
        let px = |v: Pixels| (f32::from(v) * scale).round() as i32;
        let (frame_w, frame_h) = (px(self.area.size.width), px(self.area.size.height));
        let at = rect.origin + display.origin - self.area.origin;
        let (left, top) = (px(at.x).clamp(0, frame_w - 2), px(at.y).clamp(0, frame_h - 2));
        let even = |v: i32| (v & !1).max(2);
        let w = even(px(rect.size.width).min(frame_w - left));
        let h = even(px(rect.size.height).min(frame_h - top));
        let (right, bottom) = (frame_w - left - w, frame_h - top - h);
        (format!("videocrop left={left} top={top} right={right} bottom={bottom}"), w, h)
    }

    pub fn can_scroll(&self) -> bool {
        self.pointer
    }

    /// Turns the wheel down `clicks` notches with the pointer at `at` (monitor-local).
    /// Notches, not pixels: apps scale smooth-scroll deltas each their own way.
    pub async fn scroll(&self, at: (f64, f64), clicks: i32) -> anyhow::Result<()> {
        let Kind::Remote(remote, session) = &self.kind else { return Ok(()) };
        if !self.pointer {
            return Ok(());
        }
        remote.notify_pointer_motion_absolute(session, self.node, at.0, at.1, Default::default()).await?;
        remote
            .notify_pointer_axis_discrete(session, ashpd::desktop::remote_desktop::Axis::Vertical, clicks, Default::default())
            .await?;
        Ok(())
    }

    pub async fn close(self) {
        let _ = match self.kind {
            Kind::Cast(session) => session.close().await,
            Kind::Remote(_, session) => session.close().await,
        };
    }
}
