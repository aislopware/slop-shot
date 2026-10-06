//! Capture history (CaptureHistory.swift) and its window (HistoryWindowController.swift).
//! Stored as `history.json` plus 240 px thumbnails in `~/.local/share/slopshot`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Global, KeyDownEvent, ObjectFit, RenderImage, Window,
    div, img, prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization as _;

use crate::settings::{Settings, data_dir};
use crate::{chrome, editor, output, preview};

const MAX_ITEMS: usize = 100;
const THUMB_SIDE: u32 = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Video,
    Text,
    Color,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    /// Unix seconds.
    pub date: i64,
    #[serde(default, rename = "fileURL")]
    pub file: Option<PathBuf>,
    #[serde(default)]
    pub text: Option<String>,
    pub subtitle: String,
    /// Text read inside a screenshot, only for search.
    #[serde(default)]
    pub ocr_text: Option<String>,
}

impl Item {
    pub fn title(&self) -> &'static str {
        match self.kind {
            Kind::Image => "Screenshot",
            Kind::Video => "Recording",
            Kind::Text => "Text (OCR)",
            Kind::Color => "Color",
        }
    }

    /// Captures live in the temp folder, which the system may clear; the row then stays
    /// searchable but can no longer be edited or saved.
    pub fn file_exists(&self) -> bool {
        self.file.as_deref().is_some_and(Path::exists)
    }

    fn matches(&self, q: &str) -> bool {
        [Some(self.title()), Some(self.subtitle.as_str()), self.text.as_deref(), self.ocr_text.as_deref()]
            .into_iter()
            .flatten()
            .any(|s| fold(s).contains(q))
    }

    /// The text around the match, shown under the row.
    fn snippet(&self, q: &str) -> Option<String> {
        const RADIUS: usize = 34;
        let hay: Vec<char> = [self.text.as_deref(), self.ocr_text.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ").chars().collect();
        // Folding maps each char to one char, so indices line up with `hay`.
        let folded: Vec<char> = hay.iter().map(|c| fold(&c.to_string()).chars().next().unwrap_or(*c)).collect();
        let needle: Vec<char> = q.chars().collect();
        let at = folded.windows(needle.len().max(1)).position(|w| w == needle.as_slice())?;
        let lo = at.saturating_sub(RADIUS);
        let hi = (at + needle.len() + RADIUS).min(hay.len());
        let core: String = hay[lo..hi].iter().map(|c| if *c == '\n' { ' ' } else { *c }).collect();
        Some(format!("{}{core}{}", if lo > 0 { "…" } else { "" }, if hi < hay.len() { "…" } else { "" }))
    }
}

/// Lowercased with diacritics stripped, so "loi" finds "lỗi".
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'đ' { 'd' } else { c })
        .collect()
}

fn thumbs_dir() -> PathBuf {
    data_dir().join("Thumbnails")
}

fn thumb_path(id: &str) -> PathBuf {
    thumbs_dir().join(format!("{id}.png"))
}

pub struct History {
    items: Vec<Item>,
    thumbs: HashMap<String, Arc<RenderImage>>,
}

impl Global for History {}

impl History {
    fn load() -> Self {
        let items = std::fs::read(data_dir().join("history.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { items, thumbs: HashMap::new() }
    }

    fn save(&self) {
        let dir = data_dir();
        let result = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("history.json"), serde_json::to_vec(&self.items).unwrap_or_default()));
        if let Err(err) = result {
            log::warn!("saving the history: {err}");
        }
    }

    fn thumbnail(&mut self, id: &str) -> Option<Arc<RenderImage>> {
        if let Some(t) = self.thumbs.get(id) {
            return Some(t.clone());
        }
        let image = image::open(thumb_path(id)).ok()?.to_rgba8();
        let t = preview::to_render_image(&image);
        self.thumbs.insert(id.to_owned(), t.clone());
        Some(t)
    }

    fn drop_thumb(&mut self, id: &str) {
        let _ = std::fs::remove_file(thumb_path(id));
        self.thumbs.remove(id);
    }
}

fn store(cx: &mut App) -> &mut History {
    if !cx.has_global::<History>() {
        cx.set_global(History::load());
    }
    cx.global_mut::<History>()
}

pub fn items(cx: &mut App) -> Vec<Item> {
    store(cx).items.clone()
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{nanos:x}-{:x}-{:x}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Adds a row, newest first. `image` is full size: the thumbnail is made from it, and
/// screenshots are read for search at full resolution, since OCR on 240 px is noise.
pub fn add(kind: Kind, file: Option<PathBuf>, text: Option<String>, subtitle: String, image: Option<Arc<RgbaImage>>, cx: &mut App) {
    let item = Item { id: new_id(), kind, date: chrono::Local::now().timestamp(), file, text, subtitle, ocr_text: None };
    let id = item.id.clone();
    let index = kind == Kind::Image && Settings::get(cx).index_capture_text;
    let history = store(cx);
    history.items.insert(0, item);
    for old in history.items.split_off(history.items.len().min(MAX_ITEMS)) {
        history.drop_thumb(&old.id);
    }
    history.save();
    cx.refresh_windows();

    let Some(image) = image else { return };
    let task = cx.background_spawn(async move {
        let thumb = write_thumb(&image, &id);
        let text = if index { crate::ocr::recognize(&image) } else { String::new() };
        (id, thumb, text)
    });
    cx.spawn(async move |cx| {
        let (id, thumb, text) = task.await;
        cx.update(|cx| {
            let history = store(cx);
            if let Some(thumb) = thumb {
                history.thumbs.insert(id.clone(), thumb);
            }
            if !text.is_empty()
                && let Some(item) = history.items.iter_mut().find(|i| i.id == id)
            {
                item.ocr_text = Some(text);
                history.save();
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

fn write_thumb(image: &RgbaImage, id: &str) -> Option<Arc<RenderImage>> {
    let (w, h) = image.dimensions();
    let scale = (THUMB_SIDE as f32 / w.max(h) as f32).min(1.);
    let small = image::imageops::resize(image, ((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1), image::imageops::FilterType::Triangle);
    let path = thumb_path(id);
    let written = std::fs::create_dir_all(thumbs_dir()).map_err(anyhow::Error::from).and_then(|_| {
        let png = output::encode_png(&small)?;
        // Written aside then renamed: the history window may read it while it's being written.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, png)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    });
    if let Err(err) = written {
        log::warn!("writing a history thumbnail: {err:#}");
    }
    Some(preview::to_render_image(&small))
}

pub fn add_image(file: Option<PathBuf>, image: Arc<RgbaImage>, subtitle: String, cx: &mut App) {
    add(Kind::Image, file, None, subtitle, Some(image), cx);
}

pub fn add_text(text: &str, cx: &mut App) {
    add(Kind::Text, None, Some(text.to_owned()), format!("{} chars", text.chars().count()), None, cx);
}

pub fn add_color(text: String, rgb: [u8; 3], cx: &mut App) {
    let swatch = RgbaImage::from_pixel(56, 40, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    add(Kind::Color, None, Some(text), hex, Some(Arc::new(swatch)), cx);
}

fn remove(id: &str, cx: &mut App) {
    let history = store(cx);
    history.items.retain(|i| i.id != id);
    history.drop_thumb(id);
    history.save();
}

fn clear(cx: &mut App) {
    let history = store(cx);
    for item in std::mem::take(&mut history.items) {
        history.drop_thumb(&item.id);
    }
    history.save();
}

fn copy(item: &Item, cx: &mut App) {
    match item.kind {
        Kind::Text | Kind::Color => {
            if let Some(text) = &item.text {
                output::copy_text(text.clone(), cx);
            }
        }
        // The file is gone but its text is indexed: copying that beats copying nothing.
        _ if !item.file_exists() => {
            if let Some(text) = &item.ocr_text {
                output::copy_text(text.clone(), cx);
            }
        }
        Kind::Image => {
            let file = item.file.clone().unwrap();
            match image::open(&file) {
                Ok(img) => output::copy_image(&img.to_rgba8(), Some(&file), cx),
                Err(_) => output::copy_file(&file, cx),
            }
        }
        Kind::Video => output::copy_file(item.file.as_deref().unwrap(), cx),
    }
}

/// Copies the file into the save folder without asking.
fn save(item: &Item, cx: &mut App) {
    let Some(src) = item.file.clone() else { return };
    let folder = Settings::get(cx).save_folder.clone();
    let result = (|| {
        std::fs::create_dir_all(&folder)?;
        let stem = src.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let ext = src.extension().unwrap_or_default().to_string_lossy().into_owned();
        std::fs::copy(&src, output::unique_path(&folder, &stem, &ext))?;
        anyhow::Ok(())
    })();
    if let Err(err) = result {
        crate::alert::error("Couldn't save", &format!("{err:#}"), cx);
    }
}

/// Images reopen in the editor, text in the recognition window.
fn edit(item: &Item, cx: &mut App) {
    match item.kind {
        Kind::Text => {
            let scan = crate::ocr::Scan { text: item.text.clone().unwrap_or_default(), qr_codes: vec![] };
            crate::ocr::open(scan, None, false, cx);
        }
        Kind::Image => {
            if let Some(file) = &item.file {
                editor::open_file(file, cx);
            }
        }
        _ => {}
    }
}

/// "now", "5 min. ago", "2 hr. ago", "3 days ago": RelativeDateTimeFormatter's abbreviated style.
fn relative(date: i64) -> String {
    let secs = (chrono::Local::now().timestamp() - date).max(0);
    let (n, unit) = match secs {
        0..=4 => return "now".into(),
        5..=59 => (secs, "sec."),
        60..=3599 => (secs / 60, "min."),
        3600..=86_399 => (secs / 3600, "hr."),
        86_400..=604_799 => (secs / 86_400, if secs / 86_400 == 1 { "day" } else { "days" }),
        604_800..=2_629_799 => (secs / 604_800, "wk."),
        2_629_800..=31_557_599 => (secs / 2_629_800, "mo."),
        _ => (secs / 31_557_600, "yr."),
    };
    format!("{n} {unit} ago")
}

struct HistoryWindow(AnyWindowHandle);
impl Global for HistoryWindow {}

/// Re-opening focuses the open window.
pub fn open_window(cx: &mut App) {
    if let Some(HistoryWindow(handle)) = cx.try_global::<HistoryWindow>()
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let options = chrome::window_options("Capture History", (420., 520.), Some((360., 320.)), cx);
    match cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| HistoryView::new(window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    }) {
        Ok(handle) => cx.set_global(HistoryWindow(handle.into())),
        Err(err) => log::error!("opening the history: {err:#}"),
    }
}

struct HistoryView {
    query: Entity<InputState>,
    hovered: Option<String>,
    _sub: gpui::Subscription,
}

impl HistoryView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search text inside captures…"));
        let sub = cx.subscribe(&query, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        });
        Self { query, hovered: None, _sub: sub }
    }

    /// Escape clears the search instead of closing the window.
    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key == "escape" {
            self.query.update(cx, |q, cx| q.set_value("", window, cx));
            cx.stop_propagation();
        }
    }

    fn empty_state(icon: IconName, title: String, caption: &'static str, cx: &App) -> gpui::Div {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(Icon::new(icon).size(px(34.)).text_color(cx.theme().muted_foreground.opacity(0.6)))
            .child(div().text_color(cx.theme().muted_foreground).child(title))
            .child(div().max_w(px(260.)).text_center().text_xs().text_color(cx.theme().muted_foreground.opacity(0.7)).child(caption))
    }

    fn row(&self, item: Item, thumb: Option<Arc<RenderImage>>, q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let hovering = self.hovered.as_deref() == Some(item.id.as_str());
        let snippet = (!q.is_empty()).then(|| item.snippet(q)).flatten();
        let fallback = match item.kind {
            Kind::Text => IconName::TextAlignStart,
            Kind::Color => IconName::Pipette,
            _ => IconName::Image,
        };
        let exists = item.file_exists();
        let id = item.id.clone();
        let action = |name: &'static str, icon: IconName, tip: &'static str, f: fn(&Item, &mut App), item: &Item, cx: &mut Context<Self>| {
            let item = item.clone();
            chrome::icon_button(gpui::SharedString::from(format!("{name}-{}", item.id)), icon, tip, cx).on_click(cx.listener(move |_, _, _, cx| {
                f(&item, cx);
                cx.notify();
            }))
        };
        div()
            .id(gpui::SharedString::from(item.id.clone()))
            .flex()
            .items_center()
            .gap_3()
            .px(px(14.))
            .py(px(10.))
            .when(hovering, |d| d.bg(cx.theme().foreground.opacity(0.06)))
            .on_hover(cx.listener(move |this, hover: &bool, _, cx| {
                this.hovered = hover.then(|| id.clone());
                cx.notify();
            }))
            .child(
                div()
                    .relative()
                    .size_full()
                    .w(px(56.))
                    .h(px(40.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(cx.theme().secondary)
                    .child(match thumb {
                        Some(t) => img(t).size_full().object_fit(ObjectFit::Cover).into_any_element(),
                        None => Icon::new(fallback).text_color(cx.theme().muted_foreground).into_any_element(),
                    })
                    .when(item.kind == Kind::Video, |d| {
                        d.child(div().absolute().inset_0().flex().items_center().justify_center().child(Icon::new(IconName::CirclePlay).size(px(18.)).text_color(gpui::white())))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::SEMIBOLD).child(item.title()))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("{} · {}", item.subtitle, relative(item.date))))
                    .children(snippet.map(|s| div().text_size(px(11.)).truncate().text_color(cx.theme().muted_foreground.opacity(0.7)).child(s))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .opacity(if hovering { 1. } else { 0.85 })
                    .child(action("copy", IconName::Copy, "Copy", copy, &item, cx))
                    .when(matches!(item.kind, Kind::Image | Kind::Video) && exists, |d| d.child(action("save", IconName::Download, "Save", save, &item, cx)))
                    .map(|d| match item.kind {
                        Kind::Image if exists => d.child(action("edit", IconName::SquarePen, "Edit", edit, &item, cx)),
                        Kind::Video if exists => d.child(action("play", IconName::Play, "Play", |i, cx| crate::video::play(i.file.clone().unwrap(), cx), &item, cx)),
                        Kind::Text => d.child(action("open", IconName::ScanText, "Open", edit, &item, cx)),
                        _ => d,
                    })
                    .child(action("delete", IconName::Trash, "Delete", |i, cx| remove(&i.id, cx), &item, cx)),
            )
    }
}

impl Render for HistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let q = fold(self.query.read(cx).value().trim());
        let all = items(cx);
        let total = all.len();
        let shown: Vec<(Item, Option<Arc<RenderImage>>)> = all
            .into_iter()
            .filter(|i| q.is_empty() || i.matches(&q))
            .map(|i| {
                let t = store(cx).thumbnail(&i.id);
                (i, t)
            })
            .collect();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .pt(px(4.))
            .pb_2()
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Recent captures"))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(chrome::icon_button("settings", IconName::Settings, "Settings", cx).on_click(|_, _, cx| crate::prefs::open(cx)))
                    .child(
                        chrome::icon_button("clear", IconName::Trash, "Clear all", cx)
                            .when(total == 0, |d| d.opacity(0.4))
                            .on_click(cx.listener(|_, _, _, cx| {
                                clear(cx);
                                cx.notify();
                            })),
                    ),
            );
        let search = div().px_4().pb(px(10.)).child(
            Input::new(&self.query)
                .small()
                .cleanable(true)
                .prefix(Icon::new(IconName::Search).size(px(12.)).text_color(cx.theme().muted_foreground)),
        );
        let body = if total == 0 {
            Self::empty_state(IconName::RotateCcwClock, "No captures yet".into(), "Your screenshots, recordings and OCR text will show up here.", cx).into_any_element()
        } else if shown.is_empty() {
            let raw = self.query.read(cx).value();
            Self::empty_state(IconName::TextSearch, format!("No captures match “{raw}”"), "Search looks at the text SlopShot read inside each screenshot.", cx).into_any_element()
        } else {
            let border = cx.theme().border;
            div()
                .id("history-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(shown.into_iter().map(|(item, thumb)| {
                    div().child(self.row(item, thumb, &q, cx)).child(div().ml(px(84.)).h(px(1.)).bg(border))
                }))
                .into_any_element()
        };
        gpui_component::window_border().child(
            div()
                .on_key_down(cx.listener(Self::key_down))
                .size_full()
                .flex()
                .flex_col()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .text_sm()
                .child(chrome::title_bar("Capture History", cx))
                .child(header)
                .child(search)
                .child(div().h(px(1.)).bg(cx.theme().border))
                .child(body),
        )
    }
}
