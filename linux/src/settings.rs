use std::path::PathBuf;

use gpui::{App, Global};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Tiff,
    Gif,
    Bmp,
}

impl ImageFormat {
    /// macOS also offers HEIC; no HEIF encoder ships with Ubuntu's image stack.
    pub const ALL: [ImageFormat; 5] =
        [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Tiff, ImageFormat::Gif, ImageFormat::Bmp];

    pub fn label(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::Tiff => "TIFF",
            ImageFormat::Gif => "GIF",
            ImageFormat::Bmp => "BMP",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
        }
    }

    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(ImageFormat::Png),
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "tif" | "tiff" => Some(ImageFormat::Tiff),
            "gif" => Some(ImageFormat::Gif),
            "bmp" => Some(ImageFormat::Bmp),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorFormat {
    Hex,
    HexLower,
    Rgb,
    Hsl,
    #[serde(rename = "swiftUI")]
    SwiftUi,
    NsColor,
}

impl ColorFormat {
    pub const ALL: [ColorFormat; 6] = [
        ColorFormat::Hex,
        ColorFormat::HexLower,
        ColorFormat::Rgb,
        ColorFormat::Hsl,
        ColorFormat::SwiftUi,
        ColorFormat::NsColor,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColorFormat::Hex => "HEX  #A4773F",
            ColorFormat::HexLower => "hex  #a4773f",
            ColorFormat::Rgb => "CSS  rgb(…)",
            ColorFormat::Hsl => "CSS  hsl(…)",
            ColorFormat::SwiftUi => "SwiftUI Color",
            ColorFormat::NsColor => "AppKit NSColor",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            ColorFormat::Hex => "HEX",
            ColorFormat::HexLower => "hex",
            ColorFormat::Rgb => "RGB",
            ColorFormat::Hsl => "HSL",
            ColorFormat::SwiftUi => "SwiftUI",
            ColorFormat::NsColor => "NSColor",
        }
    }

    pub fn step(self, by: isize) -> Self {
        let n = Self::ALL.len() as isize;
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(0) as isize;
        Self::ALL[(i + by).rem_euclid(n) as usize]
    }

    pub fn format(self, [r, g, b]: [u8; 3]) -> String {
        let (rf, gf, bf) = (r as f32 / 255., g as f32 / 255., b as f32 / 255.);
        match self {
            ColorFormat::Hex => format!("#{r:02X}{g:02X}{b:02X}"),
            ColorFormat::HexLower => format!("#{r:02x}{g:02x}{b:02x}"),
            ColorFormat::Rgb => format!("rgb({r}, {g}, {b})"),
            ColorFormat::Hsl => {
                let (h, s, l) = hsl(rf, gf, bf);
                format!(
                    "hsl({}, {}%, {}%)",
                    h.round() as i32,
                    (s * 100.).round() as i32,
                    (l * 100.).round() as i32
                )
            }
            ColorFormat::SwiftUi => format!("Color(red: {rf:.3}, green: {gf:.3}, blue: {bf:.3})"),
            ColorFormat::NsColor => format!("NSColor(srgbRed: {rf:.3}, green: {gf:.3}, blue: {bf:.3}, alpha: 1)"),
        }
    }
}

fn hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.;
    if mx == mn {
        return (0., 0., l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2. - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if mx == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60., s, l)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewSide {
    Left,
    Right,
}

impl PreviewSide {
    pub fn label(self) -> &'static str {
        match self {
            PreviewSide::Left => "Bottom left",
            PreviewSide::Right => "Bottom right",
        }
    }
}

/// Keys and defaults mirror the macOS app's UserDefaults (AppSettings.swift).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub save_folder: PathBuf,
    pub image_format: ImageFormat,
    pub copy_to_clipboard: bool,
    pub show_thumbnail: bool,
    pub preview_side: PreviewSide,
    pub snap: bool,
    pub color_format: ColorFormat,
    pub show_tray_icon: bool,
    pub play_sound: bool,
    pub edit_after_select: bool,
    pub editor_tool: Option<String>,
    /// Looks for emails, keys, card numbers… when the editor opens, to offer redaction.
    pub redact_scan_on_open: bool,
    /// Reads the text inside each screenshot so the history can search it.
    pub index_capture_text: bool,
    pub record_system_audio: bool,
    pub record_microphone: bool,
    /// Set once the GNOME custom shortcuts were written, so a user who later deletes
    /// them in GNOME Settings does not get them back on the next launch.
    pub shortcuts_installed: bool,
    /// Which shortcuts were registered, so ones added by an update get registered
    /// once too. `None` on settings from before it existed.
    pub installed_shortcuts: Option<Vec<String>>,
    /// The ScreenCast portal's restore tokens, so later recordings start without asking.
    pub screencast_token: Option<String>,
    pub remote_desktop_token: Option<String>,
    /// The pack the sticker picker showed last.
    pub last_sticker_pack: Option<String>,
    /// Points the Sticker Store at another bucket without a rebuild.
    pub sticker_store_url: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_folder: xdg_user_dir("XDG_DESKTOP_DIR", "Desktop"),
            image_format: ImageFormat::Png,
            copy_to_clipboard: true,
            show_thumbnail: true,
            preview_side: PreviewSide::Left,
            snap: true,
            color_format: ColorFormat::Hex,
            show_tray_icon: true,
            play_sound: true,
            edit_after_select: true,
            editor_tool: None,
            redact_scan_on_open: true,
            index_capture_text: true,
            record_system_audio: false,
            record_microphone: false,
            shortcuts_installed: false,
            installed_shortcuts: None,
            screencast_token: None,
            remote_desktop_token: None,
            last_sticker_pack: None,
            sticker_store_url: None,
        }
    }
}

impl Global for Settings {}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("slopshot").join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        let result = std::fs::create_dir_all(path.parent().unwrap())
            .and_then(|_| std::fs::write(&path, serde_json::to_vec_pretty(self).unwrap()));
        if let Err(err) = result {
            log::error!("saving {}: {err}", path.display());
        }
    }

    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn update(cx: &mut App, f: impl FnOnce(&mut Self)) {
        let settings = cx.global_mut::<Self>();
        f(settings);
        settings.save();
        cx.refresh_windows();
    }
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"))
}

/// `~/.local/share/slopshot`: history, thumbnails and stickers.
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("slopshot")
}

/// A folder from user-dirs.dirs, which is localized (e.g. "~/Màn hình nền").
pub fn xdg_user_dir(key: &str, fallback: &str) -> PathBuf {
    let home = home_dir();
    std::fs::read_to_string(config_dir().join("user-dirs.dirs"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let value = line.strip_prefix(key)?.strip_prefix('=')?.trim().trim_matches('"');
                Some(PathBuf::from(value.replace("$HOME", &home.to_string_lossy())))
            })
        })
        .filter(|p| p.is_absolute() && *p != home)
        .unwrap_or_else(|| home.join(fallback))
}

/// Shows `~/…` for paths inside the home directory.
pub fn display_path(path: &std::path::Path) -> String {
    match path.strip_prefix(home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
