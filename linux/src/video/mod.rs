//! Video player and editor.

mod editor;
mod export;
mod model;
mod player;
mod quicklook;
mod render;

use std::path::PathBuf;

use gpui::App;

/// Quick Look: plays the clip as recorded.
pub fn play(file: PathBuf, cx: &mut App) {
    quicklook::open(file, cx);
}

/// The editor: trim, cut, speed, freeze, zoom, censor, captions and export.
pub fn edit(file: PathBuf, cx: &mut App) {
    editor::open(file, cx);
}

pub fn is_video(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "mov" | "m4v" | "mkv" | "webm"))
}

/// A new capture closes the video windows, as it does the image editor.
pub fn close(cx: &mut App) {
    quicklook::close(cx);
    editor::close(cx);
}
