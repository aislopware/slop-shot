//! Modal-style alert window, the counterpart of the NSAlerts the macOS app shows for
//! capture errors and missing permissions.

use gpui::{
    App, AppContext as _, Bounds, Context, FocusHandle, KeyDownEvent, Task, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, prelude::*, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Disableable as _, Root};

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
    let bounds = Bounds::centered(None, size(px(440.), px(160.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions { title: Some(alert.title.clone().into()), ..Default::default() }),
        kind: WindowKind::Dialog,
        is_resizable: false,
        is_minimizable: false,
        focus: true,
        app_id: Some(crate::APP_ID.into()),
        ..Default::default()
    };
    let title = alert.title.clone();
    let result = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            AlertView { alert, on_choice: Some(Box::new(on_choice)), focus, holding: false }
        });
        cx.new(|cx| Root::new(view, window, cx))
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.alert.buttons.iter().enumerate().rev().map(|(i, label)| {
            let button = Button::new(("alert-button", i)).label(*label).min_w(px(88.)).disabled(self.holding);
            let button = if i == 0 { button.primary() } else { button };
            button.on_click(cx.listener(move |this, _, window, cx| this.choose(i, window, cx)))
        });
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_children_prepainted(|content, window, _| {
                let Some(content) = content.first() else { return };
                let viewport = window.viewport_size();
                // The client-side frame insets every side alike; the width shows by how much.
                let height = content.size.height + (viewport.width - content.size.width);
                if (height - viewport.height).abs() > px(1.) {
                    window.resize(size(viewport.width, height));
                }
            })
            .child(
                div()
                    .w_full()
                    .p_5()
                    .flex()
                    .gap_4()
                    .child(img("app-icon.png").size(px(64.)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().font_weight(gpui::FontWeight::BOLD).child(self.alert.title.clone()))
                            .child(div().text_sm().whitespace_normal().child(self.alert.message.clone()))
                            .child(div().pt_2().flex().justify_end().gap_2().children(buttons)),
                    ),
            )
    }
}
