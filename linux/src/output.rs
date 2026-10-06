use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;

use gpui::{App, ClipboardEntry, ClipboardItem};
use image::RgbaImage;

use crate::raster;
use crate::settings::{ImageFormat, Settings};

pub fn encode_png(image: &RgbaImage) -> anyhow::Result<Vec<u8>> {
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        ),
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(png)
}

/// Image plus, when given, the file it came from: like macOS writing an NSImage and an
/// NSURL, image editors paste the pixels and file managers paste the file.
pub fn clipboard_item(png: Vec<u8>, file: Option<&Path>) -> ClipboardItem {
    encoded_item(gpui::ImageFormat::Png, png, file)
}

fn encoded_item(format: gpui::ImageFormat, bytes: Vec<u8>, file: Option<&Path>) -> ClipboardItem {
    let mut entries = vec![ClipboardEntry::Image(gpui::Image::from_bytes(format, bytes))];
    if let Some(uri) = file.and_then(|p| url::Url::from_file_path(p).ok()) {
        entries.push(ClipboardEntry::String(gpui::ClipboardString::new(uri.to_string())));
    }
    ClipboardItem { entries }
}

/// Wayland only lets the client with keyboard focus set the clipboard, so call this
/// while one of our windows is focused (the overlay, or the editor).
pub fn copy_image(image: &RgbaImage, file: Option<&Path>, cx: &mut App) {
    match encode_png(image) {
        Ok(png) => cx.write_to_clipboard(clipboard_item(png, file)),
        Err(err) => log::error!("encoding clipboard image: {err}"),
    }
}

/// An animated GIF as data and as its file, with no still fallback to lose it to.
pub fn copy_gif(gif: Vec<u8>, file: &Path, cx: &mut App) {
    cx.write_to_clipboard(encoded_item(gpui::ImageFormat::Gif, gif, Some(file)));
}

pub fn copy_text(text: String, cx: &mut App) {
    if !crate::card::relay_copy(&crate::card::Request::CopyText { text: text.clone() }, cx) {
        set_clipboard_text(text, cx);
    }
}

pub fn set_clipboard_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// `SlopShot 2026-10-06 at 14.03.27`, the macOS naming.
pub fn base_name() -> String {
    format!("SlopShot {}", chrono::Local::now().format("%Y-%m-%d at %H.%M.%S"))
}

/// `name.ext` in `dir`, or `name (2).ext`, `name (3).ext`… if taken.
pub fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Per-user and private, since captures can hold anything that was on screen.
pub fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slopshot-{}", unsafe { libc::getuid() }));
    if std::fs::create_dir_all(&dir).is_ok() {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir
}

/// Creates the file empty, so a second capture within the same second can't take
/// the name before the background write fills it.
pub fn reserve_temp_png(base: &str) -> PathBuf {
    let path = unique_path(&temp_dir(), base, "png");
    if let Err(err) = std::fs::File::create(&path) {
        log::warn!("reserving {}: {err}", path.display());
    }
    path
}

pub fn write_file(bytes: &[u8], path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// Writes into the save folder in the configured format, never overwriting.
pub fn save_to_folder(image: &RgbaImage, base: &str, settings: &Settings) -> anyhow::Result<PathBuf> {
    let format = settings.image_format;
    let bytes = raster::encode(image, format)?;
    std::fs::create_dir_all(&settings.save_folder)?;
    let path = unique_path(&settings.save_folder, base, format.ext());
    std::fs::write(&path, bytes)?;
    Ok(path)
}

pub fn format_for(path: &Path) -> ImageFormat {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(ImageFormat::from_ext)
        .unwrap_or(ImageFormat::Png)
}

static SHUTTER: Mutex<Option<Child>> = Mutex::new(None);

/// GNOME's own screenshot sound; a capture right after another restarts it.
pub fn play_shutter() {
    const SOUND: &str = "/usr/share/sounds/freedesktop/stereo/screen-capture.oga";
    let mut current = SHUTTER.lock().unwrap();
    if let Some(mut child) = current.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    for player in ["paplay", "pw-play"] {
        match std::process::Command::new(player).arg(SOUND).spawn() {
            Ok(child) => {
                *current = Some(child);
                return;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                log::warn!("playing the capture sound: {err}");
                return;
            }
        }
    }
}

/// Size label as macOS formats it: `1.2 MB` or `340 KB`.
pub fn byte_size(len: u64) -> String {
    if len >= 1_000_000 {
        format!("{:.1} MB", len as f64 / 1_000_000.)
    } else {
        format!("{} KB", (len / 1000).max(1))
    }
}

/// The file alone, as Files' Copy puts it on the clipboard (a recording).
pub fn copy_file(file: &Path, cx: &mut App) {
    if !crate::card::relay_copy(&crate::card::Request::CopyFile { file: file.to_owned() }, cx) {
        set_clipboard_file(file, cx);
    }
}

pub fn set_clipboard_file(file: &Path, cx: &mut App) {
    let Ok(uri) = url::Url::from_file_path(file) else { return };
    let entry = gpui::ClipboardString::new(uri.to_string()).with_json_metadata("text/uri-list");
    cx.write_to_clipboard(ClipboardItem { entries: vec![ClipboardEntry::String(entry)] });
}
