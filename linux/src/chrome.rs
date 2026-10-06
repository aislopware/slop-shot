//! The frame shared by SlopShot's ordinary windows. GNOME draws no title bar for
//! Wayland clients, so each window draws its own inside a client-side shadow.

use gpui::{App, Bounds, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon};
use gpui_kit_assets::IconName;

use crate::preview::csd_size;

/// Centered window of `size` content; resizable down to `min` when given.
pub fn window_options(title: &str, size: (f32, f32), min: Option<(f32, f32)>, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, csd_size(size.0, size.1), cx))),
        titlebar: Some(gpui::TitlebarOptions { title: Some(title.to_owned().into()), appears_transparent: true, ..Default::default() }),
        window_decorations: Some(WindowDecorations::Client),
        // The client-side shadow around the frame needs an alpha channel.
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(crate::APP_ID.into()),
        focus: true,
        is_resizable: min.is_some(),
        is_minimizable: min.is_some(),
        window_min_size: min.map(|(w, h)| csd_size(w, h)),
        ..Default::default()
    }
}

pub fn title_bar(title: impl Into<gpui::SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    div()
        .id("title-bar")
        .h(px(32.))
        .flex_none()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| window.start_window_move())
        .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::SEMIBOLD).child(title.into()))
        .child(
            div()
                .id("close")
                .absolute()
                .right(px(8.))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .hover(|d| d.bg(cx.theme().secondary_hover))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, window, _| window.remove_window())
                .child(Icon::new(IconName::X).size(px(14.)).text_color(cx.theme().muted_foreground)),
        )
}

/// A borderless icon button that tints on hover, like SwiftUI's `.borderless` style.
pub fn icon_button(id: impl Into<gpui::ElementId>, icon: IconName, tip: &'static str, cx: &App) -> gpui::Stateful<gpui::Div> {
    let muted = cx.theme().muted_foreground;
    let hover = cx.theme().secondary_hover;
    div()
        .id(id)
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .hover(move |d| d.bg(hover))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
        .child(Icon::new(icon).size(px(14.)).text_color(muted))
}
