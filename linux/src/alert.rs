//! Modal-style alert window, the counterpart of the NSAlerts the macOS app shows for
//! capture errors and missing permissions.

use gpui::{
    App, AppContext as _, Bounds, Context, FocusHandle, KeyDownEvent, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, prelude::*, px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Root};

pub struct Alert {
    pub title: String,
    pub message: String,
    /// The first button is the default one (Return); the last one answers Escape.
    pub buttons: Vec<&'static str>,
}

type OnChoice = Box<dyn FnOnce(usize, &mut App)>;

pub fn show(alert: Alert, on_choice: impl FnOnce(usize, &mut App) + 'static, cx: &mut App) {
    // Text wraps at ~52 characters per line in the 300 pt text column.
    let lines: usize = alert.message.split('\n').map(|l| l.chars().count() / 52 + 1).sum();
    let height = 120. + lines as f32 * 17.;
    let bounds = Bounds::centered(None, size(px(440.), px(height)), cx);
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
    let result = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            AlertView { alert, on_choice: Some(Box::new(on_choice)), focus }
        });
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Err(err) = result {
        log::error!("opening an alert: {err:#}");
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
}

impl AlertView {
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(on_choice) = self.on_choice.take() {
            window.remove_window();
            cx.defer(move |cx| on_choice(index, cx));
        }
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
            let button = Button::new(("alert-button", i)).label(*label).min_w(px(88.));
            let button = if i == 0 { button.primary() } else { button };
            button.on_click(cx.listener(move |this, _, window, cx| this.choose(i, window, cx)))
        });
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .p_5()
            .flex()
            .gap_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(img("app-icon.png").size(px(64.)).flex_none())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().font_weight(gpui::FontWeight::BOLD).child(self.alert.title.clone()))
                    .child(div().text_sm().whitespace_normal().child(self.alert.message.clone()))
                    .child(div().flex_1())
                    .child(div().flex().justify_end().gap_2().children(buttons)),
            )
    }
}
