//! SlopShot's own controls, drawn in the zeron vocabulary of [`crate::widgets`]: icon
//! buttons, the window close button, checkboxes, a progress bar and text fields.

use gpui::{AnyElement, Div, ElementId, Entity, Hsla, SharedString, Stateful, div, prelude::*, px};
use gpui_base::input::{Input, InputState};

use crate::icons::{self, icon};
use crate::motion;
use crate::theme::{Theme, hairline, ink};
use crate::widgets::text_tooltip;

/// A 24px square glyph button whose wash fades in on hover, like Slopty's titlebar
/// controls. Kept out of drag strips: it occludes and swallows its own press.
pub fn icon_button(id: impl Into<SharedString>, path: &'static str, tip: impl Into<SharedString>) -> Stateful<Div> {
    let id: SharedString = id.into();
    let theme = Theme::get();
    let fade_key = format!("icon-button-{id}");
    div()
        .id(ElementId::Name(id))
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .bg(motion::hover_blend(&fade_key, theme.glass_hover().opacity(0.0), theme.glass_hover()))
        .on_hover(motion::hover_listener(fade_key))
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .tooltip(text_tooltip(tip))
        .child(icon(path).size(px(16.0)).text_color(theme.text_muted))
}

/// The window's close button, in Slopty's Linux caption style: 24px, red on hover.
pub fn close_button() -> Stateful<Div> {
    let theme = Theme::get();
    let red: Hsla = gpui::rgb(0xe81123).into();
    div()
        .id("close")
        .group("close-button")
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .hover(move |style| style.bg(red))
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(|_, window, cx| {
            cx.stop_propagation();
            window.remove_window();
        })
        .child(
            icon(icons::CLOSE)
                .size(px(16.0))
                .text_color(theme.text_muted)
                .group_hover("close-button", |style| style.text_color(gpui::white())),
        )
}

/// A labeled checkbox: a 16px box on the accent when checked. The caller owns the click.
pub fn checkbox(id: impl Into<ElementId>, checked: bool, label: impl Into<SharedString>) -> Stateful<Div> {
    let theme = Theme::get();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .text_size(px(12.5))
        .text_color(theme.text)
        .child(
            div()
                .size(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .map(|el| {
                    if checked {
                        el.bg(theme.accent_strong).child(icon(icons::CHECK).size(px(12.0)).text_color(theme.on_accent))
                    } else {
                        el.border_1().border_color(hairline(0.24)).bg(ink(0.04))
                    }
                }),
        )
        .child(label.into())
}

/// A thin determinate progress bar, `value` in 0..=1.
pub fn progress(value: f32) -> Div {
    let theme = Theme::get();
    div()
        .h(px(4.0))
        .w_full()
        .rounded_full()
        .bg(theme.border)
        .child(div().h_full().w(gpui::relative(value.clamp(0.0, 1.0))).rounded_full().bg(theme.accent))
}

/// A single-line text field in the dialog-field frame.
pub fn text_field(state: &Entity<InputState>) -> Div {
    field(Input::new(state).into_any_element()).text_size(px(13.0))
}

/// The dialog-field frame around an input, at control height.
fn field(input: AnyElement) -> Div {
    div()
        .w_full()
        .min_h(px(32.0))
        .flex()
        .items_center()
        .px(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(hairline(0.08))
        .bg(ink(0.04))
        .text_color(Theme::get().text)
        .child(div().flex_1().min_w_0().child(input))
}
