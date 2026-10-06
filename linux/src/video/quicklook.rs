//! Quick Look for a recording (VideoPlayerWindowController in VideoWindows.swift):
//! plays as soon as it opens, with play/pause, a scrubber and mute.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Global, KeyDownEvent, ObjectFit, RenderImage, Task, Window, div, img, prelude::*, px};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::window_border;
use gpui_kit_assets::IconName;

use super::editor::timecode;
use super::model::TimeMap;
use super::player::{self, Player};
use crate::preview::white;
use crate::{alert, board, chrome};

struct QuickLookWindow {
    handle: AnyWindowHandle,
}

impl Global for QuickLookWindow {}

pub fn open(file: PathBuf, cx: &mut App) {
    close(cx);
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let title = format!("Quick Look — {name}");
    let options = chrome::window_options(&title, (720., 460.), Some((420., 280.)), cx);
    let result = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| QuickLook::new(file, title, window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    });
    match result {
        Ok(handle) => cx.set_global(QuickLookWindow { handle: handle.into() }),
        Err(err) => alert::error("Couldn't open the clip", &format!("{err:#}"), cx),
    }
}

pub fn close(cx: &mut App) {
    if let Some(w) = cx.try_global::<QuickLookWindow>() {
        let handle = w.handle;
        cx.remove_global::<QuickLookWindow>();
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

struct QuickLook {
    title: String,
    player: Option<Player>,
    error: Option<String>,
    frame: Option<Arc<RenderImage>>,
    stale: Vec<Arc<RenderImage>>,
    scrubber: Entity<SliderState>,
    scrubbing: bool,
    muted: bool,
    focus: FocusHandle,
    _subs: Vec<gpui::Subscription>,
    _loop: Task<()>,
}

impl QuickLook {
    fn new(file: PathBuf, title: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let scrubber = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.001).default_value(0.));
        let sub = cx.subscribe(&scrubber, |this: &mut Self, _, event: &SliderEvent, cx| {
            let Some(player) = &mut this.player else { return };
            match event {
                SliderEvent::Change(v) => {
                    this.scrubbing = true;
                    player.seek_comp(v.start() as f64 * player.map().duration());
                }
                SliderEvent::Release(_) => this.scrubbing = false,
            }
            cx.notify();
        });
        let ticker = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(15)).await;
                if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                    break;
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_spawn(async move {
                    let info = player::probe(&file)?;
                    let size = player::fit(&info, (1280, 720));
                    let mut player = Player::open(&file, size)?;
                    player.set_map(TimeMap::build(&[(0., info.duration)], &[]));
                    player.play();
                    anyhow::Ok(player)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match opened {
                    Ok(player) => this.player = Some(player),
                    Err(err) => this.error = Some(format!("Couldn't play the clip — {err:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        Self { title, player: None, error: None, frame: None, stale: vec![], scrubber, scrubbing: false, muted: false, focus, _subs: vec![sub], _loop: ticker }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for image in self.stale.drain(..) {
            window.drop_image(image).ok();
        }
        let Some(player) = &mut self.player else { return };
        let before = (player.comp, player.is_playing());
        if let Some((frame, _)) = player.tick() {
            if let Some(old) = self.frame.replace(crate::preview::to_render_image(&frame)) {
                self.stale.push(old);
            }
            cx.notify();
        }
        let (comp, total) = (player.comp, player.map().duration());
        if (comp, player.is_playing()) != before {
            if !self.scrubbing && total > 0. {
                self.scrubber.update(cx, |s, cx| s.set_value((comp / total) as f32, window, cx));
            }
            cx.notify();
        }
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &mut self.player {
            player.toggle();
            cx.notify();
        }
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if let Some(player) = &self.player {
            player.set_levels(&[], self.muted);
        }
        cx.notify();
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e.keystroke.key.as_str() {
            "space" => self.toggle(cx),
            "escape" => window.remove_window(),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for QuickLook {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (comp, total, playing) = self.player.as_ref().map_or((0., 0., false), |p| (p.comp, p.map().duration(), p.is_playing()));
        let gray = gpui::hsla(0., 0., 0.85, 1.);
        let button = |id: &'static str, icon: IconName, tip: &'static str, f: fn(&mut Self, &mut Context<Self>)| {
            board::hud_button(id, tip, cx.listener(move |this, _, _, cx| f(this, cx))).size(px(28.)).child(board::icon(icon, 13., gray))
        };
        let picture = match (&self.frame, &self.error) {
            (_, Some(error)) => div().p(px(24.)).text_size(px(13.)).text_color(gpui::hsla(0., 0., 0.75, 1.)).child(error.clone()).into_any_element(),
            (Some(frame), _) => img(frame.clone()).size_full().object_fit(ObjectFit::Contain).into_any_element(),
            _ => div().into_any_element(),
        };
        window_border().child(
            div()
                .id("quick-look")
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key_down))
                .size_full()
                .flex()
                .flex_col()
                .bg(gpui::black())
                .text_color(gpui::white())
                .child(chrome::title_bar(self.title.clone(), cx).bg(gpui::hsla(0., 0., 0.13, 1.)))
                .child(div().flex_1().min_h_0().flex().items_center().justify_center().child(picture))
                .child(
                    div()
                        .h(px(40.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .bg(gpui::hsla(0., 0., 0.13, 1.))
                        .border_t_1()
                        .border_color(white(0.08))
                        .child(button("play", if playing { IconName::Pause } else { IconName::Play }, "Play / Pause (Space)", Self::toggle))
                        .child(div().text_size(px(11.)).text_color(gpui::hsla(0., 0., 0.75, 1.)).child(timecode(comp, false)))
                        .child(div().flex_1().child(Slider::new(&self.scrubber)))
                        .child(div().text_size(px(11.)).text_color(gpui::hsla(0., 0., 0.75, 1.)).child(timecode(total, false)))
                        .child(button("mute", if self.muted { IconName::VolumeX } else { IconName::Volume2 }, "Mute", Self::toggle_mute)),
                ),
        )
    }
}
