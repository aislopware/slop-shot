// Derived from zeron (MIT, github.com/zeronsh/zeron @ 053ce2e), see THIRD_PARTY_NOTICES.md
//! Embedded icon assets and the gpui [`AssetSource`] that serves them.
//!
//! Glyphs come from the **Solar Icons** set (Linear weight) by 480 Design, licensed
//! under CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/), as Slopty ships them;
//! `close`, `check`, `plus`, `stop`, `drag-handle`, `sort-vertical`, `magic-stick-3`,
//! `microphone-off`, `wifi-off` and `alt-arrow-down` are Slopty's own drawings. The
//! drawing tools' `line`, `rectangle`, `loader` and `pin-off` are drawn for SlopShot.
//!
//! Icons render via [`icon`]: `icon(icons::COPY).size(px(16.)).text_color(…)`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, svg};

macro_rules! icon_assets {
    ($(($const_name:ident, $path:literal)),+ $(,)?) => {
        $(pub const $const_name: &str = concat!("icons/", $path, ".svg");)+

        struct IconAssets;

        impl AssetSource for IconAssets {
            fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
                Ok(match path {
                    $(concat!("icons/", $path, ".svg") => Some(Cow::Borrowed(
                        include_bytes!(concat!("../assets/icons/", $path, ".svg")).as_slice(),
                    )),)+
                    _ => None,
                })
            }

            fn list(&self, path: &str) -> Result<Vec<SharedString>> {
                let all = [$(concat!("icons/", $path, ".svg")),+];
                Ok(all.iter().filter(|p| p.starts_with(path)).map(|p| SharedString::from(*p)).collect())
            }
        }
    };
}

icon_assets![
    (ALT_ARROW_DOWN, "alt-arrow-down"),
    (ALT_ARROW_LEFT, "alt-arrow-left"),
    (ARROW_UP_RIGHT, "arrow-up-right"),
    (BOX, "box"),
    (CAMERA, "camera"),
    (CHECK_CIRCLE, "check-circle"),
    (CHECK, "check"),
    (CIRCLE, "circle"),
    (CLOSE, "close"),
    (COPY, "copy"),
    (CURSOR, "cursor"),
    (DANGER_TRIANGLE, "danger-triangle"),
    (DOCUMENT_TEXT, "document-text"),
    (DOWNLOAD_MINIMALISTIC, "download-minimalistic"),
    (DRAG_HANDLE, "drag-handle"),
    (EYE_CLOSED, "eye-closed"),
    (EYE, "eye"),
    (FILE_REMOVE, "file-remove"),
    (FLIP_HORIZONTAL, "flip-horizontal"),
    (FLIP_VERTICAL, "flip-vertical"),
    (FOLDER, "folder"),
    (FULL_SCREEN, "full-screen"),
    (GALLERY, "gallery"),
    (HISTORY, "history"),
    (HOURGLASS, "hourglass"),
    (INFO_CIRCLE, "info-circle"),
    (KEYBOARD, "keyboard"),
    (LINE, "line"),
    (LOADER, "loader"),
    (MAGIC_STICK_3, "magic-stick-3"),
    (MAGNIFER_ZOOM_IN, "magnifer-zoom-in"),
    (MAGNIFER, "magnifer"),
    (MICROPHONE_OFF, "microphone-off"),
    (MICROPHONE, "microphone"),
    (MINUS, "minus"),
    (PAUSE, "pause"),
    (PEN_2, "pen-2"),
    (PEN_NEW_SQUARE, "pen-new-square"),
    (PEN, "pen"),
    (PIN_OFF, "pin-off"),
    (PIN, "pin"),
    (PIPETTE, "pipette"),
    (PLAY_CIRCLE, "play-circle"),
    (PLAY, "play"),
    (PLUS, "plus"),
    (QR_CODE, "qr-code"),
    (QUESTION_CIRCLE, "question-circle"),
    (RADIAL_BLUR, "radial-blur"),
    (RECTANGLE, "rectangle"),
    (REFRESH, "refresh"),
    (RESTART_SQUARE, "restart-square"),
    (RESTART, "restart"),
    (ROUND_ARROW_DOWN, "round-arrow-down"),
    (SCISSORS, "scissors"),
    (SETTINGS, "settings"),
    (SHARE, "share"),
    (SHIELD_KEYHOLE_MINIMALISTIC, "shield-keyhole-minimalistic"),
    (SKIP_PREVIOUS, "skip-previous"),
    (SNOWFLAKE, "snowflake"),
    (SORT_VERTICAL, "sort-vertical"),
    (SPEEDOMETER_MIDDLE, "speedometer-middle"),
    (SQUARE, "square"),
    (STICKER_SMILE_SQUARE, "sticker-smile-square"),
    (STOP, "stop"),
    (TEXT_FIELD, "text-field"),
    (TEXT_SELECTION, "text-selection"),
    (TEXT_UNDERLINE, "text-underline"),
    (TRASH_BIN_MINIMALISTIC, "trash-bin-minimalistic"),
    (UNDO_LEFT_ROUND, "undo-left-round"),
    (UNDO_LEFT, "undo-left"),
    (UNDO_RIGHT_ROUND, "undo-right-round"),
    (VIDEOCAMERA_RECORD, "videocamera-record"),
    (VOLUME_CROSS, "volume-cross"),
    (VOLUME_LOUD, "volume-loud"),
    (WIFI_OFF, "wifi-off"),
];

/// The icons, plus the app icon the About page shows.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "app-icon.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../data/icons/hicolor/128x128/apps/com.thanglb.slopshot.png"))));
        }
        IconAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        IconAssets.list(path)
    }
}

/// An icon element for an embedded asset path. gpui's SVG painter reads the SVG's
/// own text color, so always set `.text_color(..)` on the icon itself.
pub fn icon(path: &'static str) -> Svg {
    svg().path(path).flex_none()
}
