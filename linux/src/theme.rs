// Derived from zeron (MIT, github.com/zeronsh/zeron @ 053ce2e), see THIRD_PARTY_NOTICES.md
//! zeron's dark theme as Slopty ships it: a near-black neutral ladder, Geist, and the
//! Slopty indigo accent. SlopShot is dark-only, so the tokens are one static [`Theme`].
//!
//! Frosted floats need gpui's backdrop blur, which this gpui paints only on Metal, so
//! Linux takes zeron's opaque treatment: popovers are solid cards with a shadow.

use std::borrow::Cow;
use std::sync::LazyLock;

use gpui::{App, Hsla, SharedString, hsla};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../data/fonts/Geist-Regular.ttf"),
    include_bytes!("../data/fonts/Geist-Medium.ttf"),
    include_bytes!("../data/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../data/fonts/Geist-Bold.ttf"),
];

#[derive(Debug, Clone)]
pub struct Theme {
    /// Main content panel, the deepest plane (#060606).
    pub bg: Hsla,
    /// Shell / sidebar surface, one step up from `bg` (#0d0d0d).
    pub surface: Hsla,
    /// Hover wash for interactive rows and buttons.
    pub element_hover: Hsla,
    /// Hairline border.
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    /// The primary button plate: near-white on dark.
    pub solid: Hsla,
    /// Label on [`Self::solid`].
    pub on_solid: Hsla,
    pub accent: Hsla,
    /// Stronger accent for fills that carry [`Self::on_accent`] text.
    pub accent_strong: Hsla,
    pub on_accent: Hsla,
    pub danger: Hsla,
    pub danger_muted: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    /// Input plates.
    pub input_bg: Hsla,
    pub font_sans: SharedString,
}

impl Theme {
    /// The unified window titlebar strip.
    pub const TITLEBAR_HEIGHT: f32 = 38.0;
    pub const SPACE_SM: f32 = 8.0;
    pub const SPACE_MD: f32 = 12.0;
    pub const SPACE_LG: f32 = 16.0;
    /// Optical separation for a tightly coupled title/description stack.
    pub const TEXT_STACK_GAP: f32 = 1.0;

    /// The dark theme. Surface tones are sampled from zeron's reference screenshots.
    pub fn dark() -> Self {
        // The Slopty indigo, authored for dark.
        let accent = oklch(0.673, 0.182, 276.935);
        let accent_strong = oklch(0.585, 0.233, 277.117);
        Self {
            bg: grey(6),
            surface: grey(13),
            element_hover: hsla(0.0, 0.0, 0.92, 0.11),
            border: hsla(0.0, 0.0, 1.0, 0.08),
            text: neutral(0.922),
            text_muted: neutral(0.708),
            text_faint: neutral(0.556),
            solid: neutral(0.922),
            on_solid: grey(0x0e),
            accent,
            accent_strong,
            on_accent: neutral(0.985),
            danger: oklch(0.704, 0.191, 22.216),
            danger_muted: oklch(0.808, 0.114, 19.571),
            warning: oklch(0.828, 0.189, 84.429),
            success: oklch(0.765, 0.177, 163.223),
            input_bg: hsla(0.0, 0.0, 1.0, 0.03),
            font_sans: "Geist".into(),
        }
    }

    /// The theme. A context-free read: SlopShot never swaps it.
    pub fn of(_: &App) -> &'static Theme {
        &THEME
    }

    pub fn get() -> &'static Theme {
        &THEME
    }

    /// Overlay ink at `alpha` — see [`ink`].
    pub fn ink(&self, alpha: f32) -> Hsla {
        ink(alpha)
    }

    /// State wash at `alpha` — see [`wash`].
    pub fn wash(&self, alpha: f32) -> Hsla {
        wash(alpha)
    }

    /// Whether floating surfaces paint a backdrop blur under a translucent tint.
    /// Off here: see the module notes.
    pub fn is_frost(&self) -> bool {
        false
    }

    /// The hover wash for chrome rows, tabs and titlebar buttons.
    pub fn glass_hover(&self) -> Hsla {
        self.element_hover
    }

    /// Fill for input panels: the input tint composited onto the page, so it hides
    /// whatever overlaps underneath.
    pub fn input_glass_bg(&self) -> Hsla {
        flatten(self.input_bg, self.bg)
    }
}

static THEME: LazyLock<Theme> = LazyLock::new(Theme::dark);

/// Load Geist and point gpui-base's unstyled controls (inputs, scrollbars) at the theme.
pub fn install(cx: &mut App) {
    if let Err(err) = cx.text_system().add_fonts(FONTS.map(Cow::Borrowed).to_vec()) {
        log::warn!("loading Geist: {err:#}");
    }
    let theme = Theme::get();
    let base = gpui_base::Theme::global_mut(cx);
    let colors = &mut base.tokens.colors;
    colors.foreground = theme.text;
    colors.muted_foreground = theme.text_faint;
    colors.surface = theme.input_glass_bg();
    colors.border = theme.border;
    colors.accent = theme.accent;
    let scrollbar = &mut base.scrollbar;
    *scrollbar = std::mem::take(scrollbar).with_styles(
        gpui_base::ScrollbarStyles::default()
            .thumb(|style| style.bg(theme.text.opacity(0.30)))
            .thumb_hover(|style| style.bg(theme.text.opacity(0.42)))
            .thumb_active(|style| style.bg(theme.text.opacity(0.55))),
    );
}

/// A neutral (chroma 0) oklch tone. Chroma 0 means r == g == b exactly, so this
/// goes straight to an achromatic Hsla.
pub fn neutral(lightness: f32) -> Hsla {
    let [v, _, _] = oklch_to_srgb(lightness, 0.0, 0.0);
    hsla(0.0, 0.0, v, 1.0)
}

/// Translucent fill ink for interactive states and chip plates: white on dark.
pub fn ink(alpha: f32) -> Hsla {
    hsla(0.0, 0.0, 1.0, alpha)
}

/// Translucent hairline ink for borders, dividers and rings.
pub fn hairline(alpha: f32) -> Hsla {
    hsla(0.0, 0.0, 1.0, alpha)
}

/// Interactive-state wash: a softened [`ink`] that stops short of pure white, so
/// hover plates read as tinted glass rather than paint.
pub fn wash(alpha: f32) -> Hsla {
    hsla(0.0, 0.0, 0.92, alpha)
}

/// Selected-state treatment for tabs and rows: a translucent wash.
pub fn glass_selected_bg() -> Hsla {
    wash(0.11)
}

/// Selected/keyboard-active treatment for rows inside a floating card.
pub fn card_selected_bg() -> Hsla {
    wash(0.11)
}

/// An exact achromatic tone from an 8-bit channel value (`grey(13)` is `#0d0d0d`).
pub fn grey(value: u8) -> Hsla {
    hsla(0.0, 0.0, value as f32 / 255.0, 1.0)
}

/// An oklch color (CSS notation: L 0..1, C, H in degrees).
pub fn oklch(l: f32, c: f32, h_deg: f32) -> Hsla {
    let [r, g, b] = oklch_to_srgb(l, c, h_deg);
    let (h, s, l) = rgb_to_hsl(r, g, b);
    hsla(h, s, l, 1.0)
}

/// oklch to sRGB (each 0..1, gamut-clipped per channel), with Björn Ottosson's OKLab
/// matrices (the ones CSS Color 4 uses).
fn oklch_to_srgb(l: f32, c: f32, h_deg: f32) -> [f32; 3] {
    let h = h_deg.to_radians();
    let a = c * h.cos();
    let b = c * h.sin();

    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);

    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_93 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3;
    let b = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;

    [gamma_encode(r), gamma_encode(g), gamma_encode(b)]
}

fn gamma_encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB (0..1 components) to HSL, all 0..1 (gpui's Hsla convention).
fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { delta / (2.0 - max - min) } else { delta / (max + min) };
    let h = if (max - r).abs() < f32::EPSILON {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    (h, s, l)
}

/// HSL (gpui convention, all 0..1) to sRGB components 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= f32::EPSILON {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0)]
}

/// Composite `fg` (which may be translucent) over an opaque `bg`: the color the eye
/// actually receives.
pub fn flatten(fg: Hsla, bg: Hsla) -> Hsla {
    let a = fg.a.clamp(0.0, 1.0);
    let [fr, fg_, fb] = hsl_to_rgb(fg.h, fg.s, fg.l);
    let [br, bg_, bb] = hsl_to_rgb(bg.h, bg.s, bg.l);
    let (h, s, l) = rgb_to_hsl(fr * a + br * (1.0 - a), fg_ * a + bg_ * (1.0 - a), fb * a + bb * (1.0 - a));
    hsla(h, s, l, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srgb_u8(c: [f32; 3]) -> [u8; 3] {
        c.map(|v| (v * 255.0).round() as u8)
    }

    #[test]
    fn neutral_950_is_0a0a0a() {
        assert_eq!(srgb_u8(oklch_to_srgb(0.145, 0.0, 0.0)), [10, 10, 10]);
    }

    #[test]
    fn oklch_accents_match_reference() {
        assert_eq!(srgb_u8(oklch_to_srgb(0.673, 0.182, 276.935)), [124, 134, 255]);
    }

    #[test]
    fn flatten_of_an_opaque_color_is_itself() {
        let c = grey(0x42);
        assert_eq!(flatten(c, grey(0)), c);
    }
}
