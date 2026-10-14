//! zeron's widget vocabulary, with the metrics of Slopty's settings widgets (MIT): a
//! 20 px page title, small muted section labels over filled blocks of hairline-split
//! rows, pill switches, 32 px buttons and a quiet sidebar.

use gpui::{App, Div, ElementId, FontWeight, SharedString, Stateful, Window, div, prelude::*, px};
use gpui_component::Icon;
use gpui_component::tooltip::Tooltip;
use gpui_kit_assets::IconName;

use crate::theme::{self, ink};

pub const ROW_TITLE: f32 = 13.;
pub const ROW_DESCRIPTION: f32 = 12.;
pub const CONTROL_HEIGHT: f32 = 32.;
/// The clear strip at the top of every window, where the frame is dragged.
pub const TITLEBAR_HEIGHT: f32 = 38.;
/// Section labels and page titles sit a little inside the blocks' edge.
const LABEL_INSET: f32 = 8.;

pub fn icon(name: IconName, size: f32, color: gpui::Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

pub fn tooltip(text: SharedString) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView {
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

/// The page title, optionally followed by a muted count on the same baseline.
pub fn page_header(title: impl Into<SharedString>, count: Option<usize>) -> Div {
    div()
        .px(px(LABEL_INSET))
        .flex()
        .items_baseline()
        .gap(px(10.))
        .child(
            div()
                .text_size(px(20.))
                .line_height(px(26.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text())
                .child(title.into()),
        )
        .children(count.map(|n| div().text_size(px(13.)).text_color(theme::muted()).child(n.to_string())))
}

pub fn page_subtitle(copy: impl Into<SharedString>) -> Div {
    div()
        .mt(px(4.))
        .px(px(LABEL_INSET))
        .text_size(px(13.))
        .line_height(px(17.))
        .text_color(theme::muted())
        .child(copy.into())
}

/// A labeled section: the plain label (sentence case, never an eyebrow), then its block.
pub fn section(label: impl Into<SharedString>, block: impl IntoElement) -> Div {
    div()
        .mt(px(28.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().px(px(LABEL_INSET)).text_size(px(13.)).line_height(px(17.)).text_color(theme::muted()).child(label.into()))
        .child(block)
}

/// A filled, rounded block of related rows: a faint tint, no outline.
pub fn block() -> Div {
    div().rounded(px(12.)).bg(theme::block()).overflow_hidden().flex().flex_col()
}

/// One row of a [`block`]: text on the left, the control on the right, split from the
/// previous row by a hairline inset to the text edge.
pub fn row(first: bool) -> Div {
    div()
        .mx(px(16.))
        .py(px(12.))
        .min_h(px(56.))
        .when(!first, |d| d.border_t_1().border_color(theme::divider()))
        .flex()
        .items_center()
        .gap(px(16.))
}

/// A row's title and its quiet description.
pub fn row_text(title: impl Into<SharedString>, description: Option<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(
            div()
                .text_size(px(ROW_TITLE))
                .line_height(px(17.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::text())
                .child(title.into()),
        )
        .children(description.map(|d| {
            div().text_size(px(ROW_DESCRIPTION)).line_height(px(16.)).text_color(theme::muted()).child(d)
        }))
}

/// A muted line under a block.
pub fn caption(copy: impl Into<SharedString>) -> Div {
    div().px(px(LABEL_INSET)).text_size(px(ROW_DESCRIPTION)).line_height(px(16.)).text_color(theme::faint()).child(copy.into())
}

/// A bare leading glyph for rows whose icon carries identity.
pub fn row_tile(name: IconName) -> Div {
    div().flex_none().w(px(24.)).h(px(32.)).flex().items_center().justify_center().child(icon(name, 16., theme::muted()))
}

const SWITCH_WIDTH: f32 = 44.8;
const SWITCH_TRACK_HEIGHT: f32 = 20.8;
const SWITCH_INSET: f32 = 1.6;
const SWITCH_THUMB_WIDTH: f32 = 24.;
const SWITCH_MARK: f32 = 7.2;

/// A pill switch with an on or off mark in the room the thumb leaves. The caller owns
/// the click.
pub fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    let free = SWITCH_WIDTH - SWITCH_THUMB_WIDTH - 2. * SWITCH_INSET;
    let mark_center = SWITCH_INSET + free / 2.;
    let mark_left = if on { mark_center } else { SWITCH_WIDTH - mark_center } - SWITCH_MARK / 2.;
    let mark = if on {
        div().ml(px((SWITCH_MARK - 1.6) / 2.)).w(px(1.6)).h_full().rounded_full().bg(ink(0.9))
    } else {
        div().size_full().rounded_full().border(px(1.2)).border_color(theme::muted())
    };
    div()
        .id(id)
        .flex_none()
        .w(px(SWITCH_WIDTH))
        .h(px(28.8))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(SWITCH_TRACK_HEIGHT))
                .rounded_full()
                .bg(if on { theme::accent_strong() } else { ink(0.16) })
                .child(
                    div()
                        .absolute()
                        .top(px((SWITCH_TRACK_HEIGHT - SWITCH_MARK) / 2.))
                        .left(px(mark_left))
                        .size(px(SWITCH_MARK))
                        .child(mark),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(SWITCH_INSET))
                        .left(px(if on { SWITCH_WIDTH - SWITCH_INSET - SWITCH_THUMB_WIDTH } else { SWITCH_INSET }))
                        .w(px(SWITCH_THUMB_WIDTH))
                        .h(px(SWITCH_TRACK_HEIGHT - 2. * SWITCH_INSET))
                        .rounded_full()
                        .bg(gpui::rgb(0xf2f2f2)),
                ),
        )
}

/// One size, corner and hover language for every action: filled for row actions,
/// solid for commits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Filled,
    Solid,
}

pub fn button(id: impl Into<ElementId>, tone: Tone) -> Stateful<Div> {
    let b = div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(CONTROL_HEIGHT))
        .px(px(10.))
        .rounded(px(8.))
        .text_size(px(12.5))
        .cursor_pointer();
    match tone {
        Tone::Filled => b.bg(theme::fill()).text_color(theme::text()).hover(|s| s.bg(theme::fill_hover())),
        Tone::Solid => b
            .bg(theme::text())
            .text_color(theme::bg())
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.opacity(0.9)),
    }
}

pub fn text_button(id: impl Into<ElementId>, tone: Tone, label: impl Into<SharedString>) -> Stateful<Div> {
    button(id, tone).child(label.into())
}

/// A square glyph button that washes on hover.
pub fn icon_button(id: impl Into<ElementId>, name: IconName, tip: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(ink(0.06)))
        .tooltip(tooltip(tip.into()))
        .child(icon(name, 16., theme::muted()))
}

/// A sidebar destination: the selected one sits on a lifted plate.
pub fn nav_item(id: impl Into<ElementId>, name: IconName, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    let color = if selected { theme::text() } else { theme::muted() };
    div()
        .id(id)
        .h(px(34.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(13.))
        .text_color(color)
        .map(|d| if selected { d.bg(theme::selected()) } else { d.hover(|s| s.bg(ink(0.05)).text_color(theme::text())) })
        .child(icon(name, 16., color))
        .child(label.into())
}

/// A small pill with a filled wash.
pub fn badge(label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(ink(0.08))
        .text_size(px(10.5))
        .text_color(theme::muted())
        .child(label.into())
}

/// Close button for a window's top-right corner.
pub fn close_button() -> Stateful<Div> {
    icon_button("close", IconName::X, "Close")
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, window, _| window.remove_window())
}
