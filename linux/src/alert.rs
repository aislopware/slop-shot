//! Modal-style alert window, the counterpart of the NSAlerts the macOS app shows for
//! capture errors and missing permissions.

use gpui::{App, AppContext as _, Context, FocusHandle, KeyDownEvent, Task, Window, WindowKind, div, img, prelude::*, px, size};

use crate::theme::Theme;
use crate::{chrome, motion, popover};

pub struct Alert {
    pub title: String,
    pub message: String,
    /// The first button is the default one (Return); the last one answers Escape.
    pub buttons: Vec<&'static str>,
}

/// Runs once the alert window is gone.
pub type Then = Box<dyn FnOnce(&mut App)>;

type OnChoice = Box<dyn FnOnce(usize, &mut App) -> Option<Task<Then>>>;

pub fn show(alert: Alert, on_choice: impl FnOnce(usize, &mut App) + 'static, cx: &mut App) {
    show_holding(
        alert,
        move |choice, _| {
            let then: Then = Box::new(move |cx| on_choice(choice, cx));
            Some(Task::ready(then))
        },
        cx,
    );
}

/// Like `show`, but the alert stays up, its buttons disabled, until the task `on_choice`
/// returns finishes, for work that needs a SlopShot window to have focus meanwhile.
pub fn show_holding(alert: Alert, on_choice: impl FnOnce(usize, &mut App) -> Option<Task<Then>> + 'static, cx: &mut App) {
    // AlertView fits the height to the wrapped text once it is laid out.
    let options = gpui::WindowOptions { kind: WindowKind::Dialog, ..chrome::window_options(&alert.title, (440., 160.), None, cx) };
    let title = alert.title.clone();
    let result = cx.open_window(options, |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            AlertView { alert, on_choice: Some(Box::new(on_choice)), focus, holding: false }
        })
    });
    match result {
        Ok(_) => log::debug!("alert opened ({title})"),
        Err(err) => log::error!("opening an alert: {err:#}"),
    }
}

pub fn error(title: &str, message: &str, cx: &mut App) {
    log::error!("{title}: {message}");
    show(
        Alert { title: title.into(), message: message.into(), buttons: vec!["OK"] },
        |_, _| {},
        cx,
    );
}

struct AlertView {
    alert: Alert,
    on_choice: Option<OnChoice>,
    focus: FocusHandle,
    holding: bool,
}

impl AlertView {
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(on_choice) = self.on_choice.take() else {
            return;
        };
        let Some(task) = on_choice(index, cx) else {
            window.remove_window();
            return;
        };
        self.holding = true;
        cx.notify();
        cx.spawn_in(window, async move |_, cx| {
            let then = task.await;
            cx.update(|window, cx| {
                window.remove_window();
                cx.defer(then);
            })
            .ok();
        })
        .detach();
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e.keystroke.key.as_str() {
            "enter" => self.choose(0, window, cx),
            "escape" => self.choose(self.alert.buttons.len().saturating_sub(1), window, cx),
            _ => {}
        }
    }
}

impl Render for AlertView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        motion::window_frame(window);
        let t = Theme::get();
        let holding = self.holding;
        let buttons = self.alert.buttons.iter().enumerate().rev().map(|(i, label)| {
            let button = if i == 0 {
                popover::btn_primary(t, *label)
            } else {
                popover::btn_ghost(t, *label, format!("alert-button-{i}"))
            };
            button
                .id(("alert-button", i))
                .min_w(px(88.))
                .flex()
                .justify_center()
                .when(holding, |b| b.opacity(0.5).cursor_default())
                .on_click(cx.listener(move |this, _, window, cx| this.choose(i, window, cx)))
        });
        let content = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .on_children_prepainted(|content, window, _| {
                let Some(content) = content.first() else { return };
                let viewport = window.viewport_size();
                if (content.size.height - viewport.height).abs() > px(1.) {
                    window.resize(size(viewport.width, content.size.height));
                }
            })
            .child(
                div()
                    .w_full()
                    .p(px(20.))
                    .flex()
                    .gap(px(16.))
                    .child(img("app-icon.png").size(px(64.)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(popover::dialog_title(t, self.alert.title.clone()))
                            .child(popover::dialog_body(t, self.alert.message.clone()).whitespace_normal())
                            .child(div().pt(px(8.)).flex().justify_end().gap(px(8.)).children(buttons)),
                    ),
            );
        chrome::frame(window, false, t.surface, content)
    }
}
