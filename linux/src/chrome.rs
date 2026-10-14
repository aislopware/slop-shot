// Derived from zeron (MIT, github.com/zeronsh/zeron @ 75b7260), see THIRD_PARTY_NOTICES.md
//! The frame shared by SlopShot's ordinary windows. GNOME draws no title bar for
//! Wayland clients, so each window draws its own: Slopty's client-side frame, with
//! rounded corners while the window floats and invisible resize strips on free edges.

use gpui::{
    AnyElement, App, Bounds, Decorations, Div, Hsla, ResizeEdge, Stateful, Window, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowOptions, div, prelude::*, px, size,
};

use crate::theme::Theme;
use crate::ui;

/// Corner radius of a floating window; tiled or maximized windows go square.
pub const WINDOW_CORNER_RADIUS: f32 = 10.0;

/// Centered window of `size`; resizable down to `min` when given.
pub fn window_options(title: &str, (width, height): (f32, f32), min: Option<(f32, f32)>, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(title.to_owned().into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_decorations: Some(WindowDecorations::Client),
        // The rounded corners need an alpha channel.
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some(crate::APP_ID.into()),
        focus: true,
        is_resizable: min.is_some(),
        is_minimizable: min.is_some(),
        window_min_size: min.map(|(w, h)| size(px(w), px(h))),
        ..Default::default()
    }
}

/// Which edges are free: not taken by the compositor (tiled or maximized). Server
/// decorations hand the frame back to the compositor.
fn free_edges(window: &Window) -> (bool, bool, bool, bool) {
    match window.window_decorations() {
        Decorations::Client { tiling } => (!tiling.top, !tiling.bottom, !tiling.left, !tiling.right),
        Decorations::Server => (false, false, false, false),
    }
}

/// The corner radius full-bleed layers at a window corner must paint with: gpui's
/// content masks are rectangles, so `.rounded()` never clips children.
pub fn corner_radius(window: &Window) -> f32 {
    let (top, bottom, left, right) = free_edges(window);
    if top && bottom && left && right { WINDOW_CORNER_RADIUS } else { 0.0 }
}

/// The window root: `content` on `bg`, rounded while floating, with resize strips on
/// the free edges when `resizable`. Call [`crate::motion::window_frame`] first in the
/// same render.
pub fn frame(window: &Window, resizable: bool, bg: Hsla, content: impl IntoElement) -> Div {
    let radius = corner_radius(window);
    div()
        .relative()
        .size_full()
        .bg(bg)
        .when(radius > 0.0, |el| el.rounded(px(radius)).overflow_hidden())
        .text_color(Theme::get().text)
        .font_family(Theme::get().font_sans.clone())
        .child(content)
        .when(resizable, |el| el.children(resize_strips(window)))
}

/// Invisible edge strips that turn presses into compositor resizes, the CSD contract
/// on X11 and Wayland. They paint last so they sit above the content.
fn resize_strips(window: &Window) -> Vec<AnyElement> {
    const EDGE: f32 = 6.0;
    const CORNER: f32 = 14.0;
    let (top, bottom, left, right) = free_edges(window);
    let strip = |el: Div, edge: ResizeEdge| {
        el.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            window.start_window_resize(edge);
        })
        .into_any_element()
    };
    let mut out = Vec::new();
    if top {
        out.push(strip(div().absolute().top_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE)).cursor_ns_resize(), ResizeEdge::Top));
    }
    if bottom {
        out.push(strip(
            div().absolute().bottom_0().left(px(CORNER)).right(px(CORNER)).h(px(EDGE)).cursor_ns_resize(),
            ResizeEdge::Bottom,
        ));
    }
    if left {
        out.push(strip(div().absolute().left_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE)).cursor_ew_resize(), ResizeEdge::Left));
    }
    if right {
        out.push(strip(
            div().absolute().right_0().top(px(CORNER)).bottom(px(CORNER)).w(px(EDGE)).cursor_ew_resize(),
            ResizeEdge::Right,
        ));
    }
    let corner = || div().absolute().size(px(CORNER));
    if top && left {
        out.push(strip(corner().top_0().left_0().cursor_nwse_resize(), ResizeEdge::TopLeft));
    }
    if top && right {
        out.push(strip(corner().top_0().right_0().cursor_nesw_resize(), ResizeEdge::TopRight));
    }
    if bottom && left {
        out.push(strip(corner().bottom_0().left_0().cursor_nesw_resize(), ResizeEdge::BottomLeft));
    }
    if bottom && right {
        out.push(strip(corner().bottom_0().right_0().cursor_nwse_resize(), ResizeEdge::BottomRight));
    }
    out
}

/// A titlebar strip that moves the window, with a centered title and the close button.
pub fn title_bar(title: impl Into<gpui::SharedString>) -> Stateful<Div> {
    drag_strip("title-bar")
        .h(px(Theme::TITLEBAR_HEIGHT))
        .flex_none()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div().text_size(px(13.0)).font_weight(gpui::FontWeight::MEDIUM).text_color(Theme::get().text).child(title.into()),
        )
        .child(div().absolute().right(px(8.0)).child(ui::close_button()))
}

/// An area that moves the window when dragged.
pub fn drag_strip(id: &'static str) -> Stateful<Div> {
    div().id(id).on_mouse_down(gpui::MouseButton::Left, |_, window, _| window.start_window_move())
}
