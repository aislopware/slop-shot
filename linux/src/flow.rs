//! What happens around a capture: the session gate before it, and the macOS
//! `finishImage` flow after it (ScreenCapturer.swift).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext as _, Bounds, Global, Pixels};
use image::RgbaImage;

use crate::alert::{self, Alert};
use crate::card::{self, Request};
use crate::settings::Settings;
use crate::{editor, output};

/// The last capture, for the tray's Edit Last Screenshot / Open Last File and the
/// preview card's buttons.
pub struct Last {
    /// None for a recording.
    pub image: Option<Arc<RgbaImage>>,
    pub base: String,
    /// The temp PNG, or the saved file once the user pressed Save.
    pub file: PathBuf,
}

impl Global for Last {}

/// Set while a capture is in progress, so a second hotkey press is ignored.
pub struct Busy;
impl Global for Busy {}

pub fn last(cx: &App) -> Option<&Last> {
    cx.try_global::<Last>()
}

/// Every capture action starts here. Calls `start` unless the action is ignored or the
/// user keeps editing.
pub fn begin_session(start: impl FnOnce(&mut App) + 'static, cx: &mut App) {
    if cx.has_global::<Busy>() {
        return;
    }
    let go = |start: Box<dyn FnOnce(&mut App)>, cx: &mut App| {
        editor::close(cx);
        crate::video::close(cx);
        card::send(&Request::Hide, cx);
        start(cx);
    };
    if editor::has_unsaved_annotations(cx) {
        let start: Box<dyn FnOnce(&mut App)> = Box::new(start);
        alert::show(
            Alert {
                title: "Discard your annotations?".into(),
                message: "The editor is still open with drawings you haven't copied or saved. Starting a new capture closes it.".into(),
                buttons: vec!["Discard & Capture", "Keep Editing"],
            },
            move |choice, cx| {
                if choice == 0 {
                    go(start, cx);
                }
            },
            cx,
        );
    } else {
        go(Box::new(start), cx);
    }
}

/// `display` is the logical bounds of the display the image came from; the preview
/// card shows on it.
pub fn finish_image(image: RgbaImage, display: Bounds<Pixels>, force_copy: bool, cx: &mut App) {
    finish_image_as(image, display, force_copy, "", cx);
}

/// `note` follows the size in the history subtitle, e.g. " (scrolling)".
pub fn finish_image_as(image: RgbaImage, display: Bounds<Pixels>, force_copy: bool, note: &str, cx: &mut App) {
    let subtitle = format!("{}×{}px{note}", image.width(), image.height());
    let settings = Settings::get(cx).clone();
    let base = output::base_name();
    let file = output::reserve_temp_png(&base);
    if settings.play_sound {
        output::play_shutter();
    }
    let image = Arc::new(image);
    cx.set_global(Last { image: Some(image.clone()), base, file: file.clone() });
    crate::tray::sync(cx);

    if settings.show_thumbnail {
        match card::write_thumbnail(&image) {
            Ok(thumb) => {
                let b = display;
                let display = [b.origin.x, b.origin.y, b.size.width, b.size.height].map(f32::from);
                card::send(&Request::Show { thumb, file: file.clone(), display, side: settings.preview_side, video: false }, cx);
            }
            Err(err) => log::error!("making the card thumbnail: {err:#}"),
        }
    }

    let copy = settings.copy_to_clipboard || force_copy;
    // Without the X11 helper only a focused window of ours can set the clipboard, and
    // the caller's overlay is still focused right now.
    let copy_via_host = copy && card::has_host(cx);
    if copy && !copy_via_host {
        output::copy_image(&image, Some(&file), cx);
    }
    let write = cx.background_spawn({
        let image = image.clone();
        let file = file.clone();
        async move { output::encode_png(&image).and_then(|png| Ok(output::write_file(&png, &file)?)) }
    });
    cx.spawn(async move |cx| {
        let result = write.await;
        cx.update(|cx| {
            let written = match result {
                Ok(()) => {
                    if copy_via_host {
                        card::send(&Request::Copy { file: file.clone() }, cx);
                    }
                    true
                }
                Err(err) => {
                    log::error!("writing {}: {err:#}", file.display());
                    let _ = std::fs::remove_file(&file);
                    if copy_via_host {
                        output::copy_image(&image, None, cx);
                    }
                    false
                }
            };
            crate::history::add_image(written.then_some(file), image, subtitle, cx);
        });
    })
    .detach();
}

/// White flash over `rect` (local to `display`); needs the X11 helper, since a
/// Wayland client can't place a window.
pub fn flash(display: Bounds<Pixels>, rect: Bounds<Pixels>, cx: &mut App) {
    let arr = |b: Bounds<Pixels>| [b.origin.x, b.origin.y, b.size.width, b.size.height].map(f32::from);
    if card::has_host(cx) {
        card::send(&Request::Flash { display: arr(display), rect: arr(rect) }, cx);
    }
}

/// Writes the last capture into the save folder; the card's Save button.
pub fn save_last(cx: &mut App) {
    let Some(last) = last(cx) else { return };
    match last.image.clone() {
        Some(image) => save_image(image, last.base.clone(), cx),
        None => {
            let file = last.file.clone();
            crate::record::save_video(file, cx);
        }
    }
}

pub fn save_image(image: Arc<RgbaImage>, base: String, cx: &mut App) {
    let settings = Settings::get(cx).clone();
    let task = cx.background_spawn(async move { output::save_to_folder(&image, &base, &settings) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(path) => {
                if cx.has_global::<Last>() {
                    cx.global_mut::<Last>().file = path;
                }
            }
            Err(err) => alert::error("Capture failed", &format!("{err:#}"), cx),
        });
    })
    .detach();
}

pub fn edit_last(cx: &mut App) {
    if let Some(Last { image: Some(image), file, .. }) = last(cx) {
        let (image, file) = (image.clone(), file.clone());
        editor::open((*image).clone(), Some(file), cx);
    }
}

pub fn open_last_file(cx: &mut App) {
    if let Some(last) = last(cx) {
        cx.open_with_system(&last.file.clone());
    }
}

/// Linux has no share sheet; the portal's app chooser is the closest thing: it offers
/// every app that can take the file.
pub fn share_last(cx: &mut App) {
    if let Some(file) = last(cx).map(|l| l.file.clone()) {
        share_file(file, cx);
    }
}

pub fn share_file(file: PathBuf, cx: &mut App) {
    cx.spawn(async move |_| {
        let result = async {
            let file = std::fs::File::open(&file)?;
            ashpd::desktop::open_uri::OpenFileRequest::default()
                .ask(true)
                .send_file(&file)
                .await?;
            anyhow::Ok(())
        }
        .await;
        if let Err(err) = result {
            log::error!("sharing the capture: {err:#}");
        }
    })
    .detach();
}

pub fn on_card_event(event: card::Event, cx: &mut App) {
    log::debug!("card event {event:?}");
    let video = last(cx).is_some_and(|l| l.image.is_none());
    match event {
        card::Event::Edit if video => crate::video::play(last(cx).unwrap().file.clone(), cx),
        card::Event::Trim => {
            if let Some(last) = last(cx) {
                crate::video::edit(last.file.clone(), cx);
            }
        }
        card::Event::Edit => edit_last(cx),
        card::Event::Save => save_last(cx),
        card::Event::Share => share_last(cx),
        card::Event::RecordStop => crate::record::stop(cx),
        card::Event::RecordPause(paused) => crate::record::set_paused(paused, cx),
        card::Event::RecordRestart => crate::record::restart(cx),
        card::Event::RecordDiscard => crate::record::discard(cx),
        card::Event::RecordMute(muted) => crate::record::set_mic_muted(muted, cx),
        card::Event::ScrollAuto(on) => crate::scroll::set_auto(on, cx),
        card::Event::ScrollDone => crate::scroll::done(cx),
        card::Event::ScrollCancel => crate::scroll::cancel(cx),
    }
}
