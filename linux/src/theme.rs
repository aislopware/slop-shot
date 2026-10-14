//! zeron's interface as Slopty ships it: Geist, a near-black neutral palette and one
//! violet accent. gpui-component's own controls take the same palette from
//! `zeron-dark.json`, so popovers, inputs and tooltips match the hand-drawn surfaces.

use std::borrow::Cow;

use gpui::{App, Hsla, SharedString, rgb};
use gpui_component::{Theme, ThemeMode, ThemeRegistry};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../data/fonts/Geist-Regular.ttf"),
    include_bytes!("../data/fonts/Geist-Medium.ttf"),
    include_bytes!("../data/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../data/fonts/Geist-Bold.ttf"),
];

pub fn install(cx: &mut App) {
    if let Err(err) = cx.text_system().add_fonts(FONTS.map(Cow::Borrowed).to_vec()) {
        log::warn!("loading Geist: {err:#}");
    }
    let registry = ThemeRegistry::global_mut(cx);
    registry.load_themes_from_str(include_str!("zeron-dark.json")).expect("zeron-dark.json is a theme set");
    let config = registry.themes()[&SharedString::from("Zeron Dark")].clone();
    Theme::global_mut(cx).dark_theme = config;
    Theme::change(ThemeMode::Dark, None, cx);
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// The content plane.
pub fn bg() -> Hsla {
    hex(0x0d0d0d)
}

/// Sidebars and tool bars, one step up from the content plane.
pub fn sidebar() -> Hsla {
    hex(0x181818)
}

/// A block of rows resting on the content plane.
pub fn block() -> Hsla {
    hex(0x171717)
}

/// Controls on a block (select triggers, filled buttons), at rest and lifted.
pub fn fill() -> Hsla {
    hex(0x242424)
}

pub fn fill_hover() -> Hsla {
    hex(0x2c2c2c)
}

/// The selected row of a sidebar.
pub fn selected() -> Hsla {
    hex(0x2f2f2f)
}

/// Menus and popovers, the highest plane.
pub fn popover() -> Hsla {
    hex(0x1c1c1c)
}

pub fn text() -> Hsla {
    hex(0xe8e8ea)
}

pub fn muted() -> Hsla {
    hex(0xa9a9ae)
}

pub fn faint() -> Hsla {
    hex(0x85858a)
}

pub fn accent() -> Hsla {
    hex(0x8b7cf6)
}

/// The accent under a black 14% veil: a switch that is on.
pub fn accent_strong() -> Hsla {
    hex(0x776ad3)
}

pub fn success() -> Hsla {
    hex(0x34d399)
}

pub fn warning() -> Hsla {
    hex(0xfacc15)
}

pub fn danger() -> Hsla {
    hex(0xf87171)
}

/// White at `alpha`: hairlines and hover washes on the dark planes.
pub fn ink(alpha: f32) -> Hsla {
    gpui::hsla(0., 0., 1., alpha)
}

/// The hairline between rows of a block.
pub fn divider() -> Hsla {
    hex(0x252525)
}
