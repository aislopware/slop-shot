//! The frame shared by SlopShot's ordinary windows. GNOME draws no title bar for
//! Wayland clients, so each window draws its own inside a client-side shadow.

use gpui::{App, Bounds, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions, div, prelude::*, px};

use crate::preview::csd_size;
use crate::{theme, ui};

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

pub fn title_bar(title: impl Into<gpui::SharedString>, _: &App) -> gpui::Stateful<gpui::Div> {
    div()
        .id("title-bar")
        .h(px(ui::TITLEBAR_HEIGHT))
        .flex_none()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| window.start_window_move())
        .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::MEDIUM).text_color(theme::text()).child(title.into()))
        .child(div().absolute().right(px(6.)).child(ui::close_button()))
}
