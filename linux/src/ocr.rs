//! Text and QR recognition (TextRecognizer.swift) through the tesseract CLI, and the
//! result window (OCRWindowController.swift). Like Vision it runs on-device.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Global, KeyDownEvent, ObjectFit, Window,
    div, img, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;
use image::RgbaImage;

use crate::{alert, card, history, output, preview};

/// One recognised word, `rect` in image pixels.
#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    pub rect: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub words: Vec<Word>,
}

impl Line {
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// Union of the words overlapping the byte range `start..end` of `text()`.
    pub fn rect_of(&self, start: usize, end: usize) -> Option<[f32; 4]> {
        let mut at = 0;
        let mut out: Option<[f32; 4]> = None;
        for w in &self.words {
            let (ws, we) = (at, at + w.text.len());
            at = we + 1;
            if we <= start || ws >= end {
                continue;
            }
            let [x, y, wd, ht] = w.rect;
            out = Some(match out {
                None => w.rect,
                Some([ox, oy, ow, oh]) => {
                    let (x0, y0) = (ox.min(x), oy.min(y));
                    [x0, y0, (ox + ow).max(x + wd) - x0, (oy + oh).max(y + ht) - y0]
                }
            });
        }
        out
    }
}

pub struct Scan {
    pub text: String,
    pub qr_codes: Vec<String>,
}

impl Scan {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.qr_codes.is_empty()
    }

    /// Text first; a capture of only QR codes copies their payloads.
    pub fn copy_text(&self) -> String {
        if self.text.trim().is_empty() { self.qr_codes.join("\n") } else { self.text.clone() }
    }
}

/// Every installed language except the script detector, so Vietnamese captures read
/// as Vietnamese without the user picking a language, as Vision's auto-detection does.
fn languages() -> &'static str {
    static LANGS: OnceLock<String> = OnceLock::new();
    LANGS.get_or_init(|| {
        let listed = Command::new("tesseract").arg("--list-langs").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let mut langs: Vec<&str> = listed.lines().skip(1).map(str::trim).filter(|l| !l.is_empty() && *l != "osd").collect();
        // eng first: tesseract leans on the first language when words are ambiguous.
        langs.sort_by_key(|l| *l != "eng");
        if langs.is_empty() { "eng".into() } else { langs.join("+") }
    })
}

/// Lines of words, top to bottom. Blocking; run it off the main thread.
pub fn lines(image: &RgbaImage) -> anyhow::Result<Vec<Line>> {
    // Screen text is ~12 px tall; tesseract is tuned for ~30 px, so small captures are
    // read at twice the size.
    let scale = if image.width().max(image.height()) < 2500 { 2. } else { 1. };
    let gray = image::DynamicImage::ImageRgba8(image.clone()).to_luma8();
    let input = if scale > 1. {
        image::imageops::resize(&gray, image.width() * 2, image.height() * 2, image::imageops::FilterType::CatmullRom)
    } else {
        gray
    };
    let mut png = Vec::new();
    input.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;

    let mut child = Command::new("tesseract")
        .args(["stdin", "stdout", "-l", languages(), "tsv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("running tesseract: {e} (install the tesseract-ocr package)"))?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&png));
    let out = child.wait_with_output()?;
    let _ = writer.join();
    anyhow::ensure!(out.status.success(), "tesseract failed");
    Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout), scale))
}

fn parse_tsv(tsv: &str, scale: f32) -> Vec<Line> {
    let mut lines: Vec<((u32, u32, u32), Line)> = Vec::new();
    for row in tsv.lines().skip(1) {
        let cols: Vec<&str> = row.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let text = cols[11].trim();
        if text.is_empty() {
            continue;
        }
        let n = |i: usize| cols[i].parse::<f32>().unwrap_or(0.);
        let key = (n(2) as u32, n(3) as u32, n(4) as u32);
        let word = Word { text: text.to_owned(), rect: [n(6) / scale, n(7) / scale, n(8) / scale, n(9) / scale] };
        match lines.last_mut() {
            Some((k, line)) if *k == key => line.words.push(word),
            _ => lines.push((key, Line { words: vec![word] })),
        }
    }
    lines.into_iter().map(|(_, l)| l).collect()
}

pub fn recognize(image: &RgbaImage) -> String {
    match lines(image) {
        Ok(lines) => lines.iter().map(Line::text).collect::<Vec<_>>().join("\n"),
        Err(err) => {
            log::warn!("reading text: {err:#}");
            String::new()
        }
    }
}

/// QR payloads in reading order, duplicates dropped.
pub fn qr_codes(image: &RgbaImage) -> Vec<String> {
    let gray = image::DynamicImage::ImageRgba8(image.clone()).to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(gray);
    let mut seen = std::collections::HashSet::new();
    prepared
        .detect_grids()
        .into_iter()
        .filter_map(|g| g.decode().ok().map(|(_, s)| s))
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect()
}

pub fn scan(image: &RgbaImage) -> Scan {
    Scan { text: recognize(image), qr_codes: qr_codes(image) }
}

/// Capture Text: reads the crop, copies the text and shows the result window.
pub fn capture_text(crop: RgbaImage, cx: &mut App) {
    let crop = Arc::new(crop);
    let task = cx.background_spawn({
        let crop = crop.clone();
        async move { scan(&crop) }
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| {
            if result.is_empty() {
                alert::error("No text found", "SlopShot couldn't find any text or QR code in the selected area.", cx);
                return;
            }
            let text = result.copy_text();
            history::add_text(&text, cx);
            open(result, Some(crop), true, cx);
        });
    })
    .detach();
}

struct OcrWindow(AnyWindowHandle);
impl Global for OcrWindow {}

/// Each result replaces the previous window. `copy` puts the text on the clipboard once
/// the window has focus, since Wayland only lets the focused client set it.
pub fn open(result: Scan, image: Option<Arc<RgbaImage>>, copy: bool, cx: &mut App) {
    if let Some(OcrWindow(handle)) = cx.try_global::<OcrWindow>() {
        let handle = *handle;
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
    let title = if result.qr_codes.is_empty() { "Text Recognition" } else { "Text & QR Recognition" };
    let options = crate::chrome::window_options(title, (960., 620.), Some((760., 440.)), cx);
    let opened = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| OcrView::new(title, result, image, copy, window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    });
    match opened {
        Ok(handle) => cx.set_global(OcrWindow(handle.into())),
        Err(err) => log::error!("opening the text window: {err:#}"),
    }
}

struct OcrView {
    title: &'static str,
    qr_codes: Vec<String>,
    image: Option<(Arc<gpui::RenderImage>, u32, u32)>,
    text: Entity<TextareaState>,
    copied: bool,
    focus: FocusHandle,
    pending_copy: Option<String>,
    _sub: gpui::Subscription,
}

impl OcrView {
    fn new(title: &'static str, result: Scan, image: Option<Arc<RgbaImage>>, copy: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A window GNOME opened without focus can't set the clipboard; the X11 helper can.
        let pending_copy = copy.then(|| result.copy_text()).filter(|text| !card::relay_copy(&card::Request::CopyText { text: text.clone() }, cx));
        let text = cx.new(|cx| TextareaState::new(window, cx).default_value(result.text.clone()));
        let sub = cx.subscribe(&text, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            title,
            qr_codes: result.qr_codes,
            image: image.map(|i| (preview::to_render_image(&i), i.width(), i.height())),
            text,
            copied: false,
            focus,
            pending_copy,
            _sub: sub,
        }
    }

    fn copy(&mut self, s: String, cx: &mut Context<Self>) {
        output::copy_text(s, cx);
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if k.modifiers.control && k.modifiers.shift && k.key == "c" {
            let s = self.text.read(cx).value().to_string();
            self.copy(s, cx);
        }
    }

    fn toolbar(&self, cx: &App) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .h(px(48.))
            .flex_none()
            .bg(cx.theme().title_bar)
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(IconName::ScanText).size(px(15.)))
            .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::MEDIUM).child("Recognized text"))
    }

    fn preview(&self, cx: &App) -> Option<impl IntoElement> {
        let (image, w, h) = self.image.clone()?;
        Some(
            div()
                .w(px(268.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p_4()
                .bg(cx.theme().secondary.opacity(0.4))
                .border_r_1()
                .border_color(cx.theme().border)
                .child(
                    div().flex_1().min_h_0().flex().items_center().justify_center().child(
                        img(image)
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain)
                            .rounded(px(8.))
                            .border_1()
                            .border_color(cx.theme().border),
                    ),
                )
                .child(div().text_xs().text_center().text_color(cx.theme().muted_foreground).child(format!("{w} × {h} px"))),
        )
    }

    fn footer(&self, shown: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let words = shown.split_whitespace().count();
        let chars = shown.chars().count();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .py(px(10.))
            .flex_none()
            .bg(cx.theme().title_bar)
            .border_t_1()
            .border_color(cx.theme().border)
            .children(self.qr_codes.iter().enumerate().map(|(i, code)| {
                let link = code.trim().to_ascii_lowercase();
                let is_link = link.starts_with("http://") || link.starts_with("https://");
                let open = code.trim().to_owned();
                let copy = code.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(Icon::new(IconName::QrCode).size(px(14.)).text_color(cx.theme().muted_foreground))
                    .child(
                        div()
                            .id(("qr", i))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(is_link, |d| d.text_color(cx.theme().link).cursor_pointer().on_click(move |_, _, cx| cx.open_url(&open)))
                            .child(code.clone()),
                    )
                    .child(Button::new(("qr-copy", i)).label("Copy").ghost().small().on_click(cx.listener(move |this, _, _, cx| this.copy(copy.clone(), cx))))
            }))
            .when(!self.qr_codes.is_empty(), |d| d.child(div().h(px(1.)).bg(cx.theme().border)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("{chars} characters · {words} words")))
                    .child(
                        Button::new("copy-text")
                            .icon(Icon::new(if self.copied { IconName::Check } else { IconName::Copy }))
                            .label(if self.copied { "Copied" } else { "Copy text" })
                            .w(px(120.))
                            .disabled(shown.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let s = this.text.read(cx).value().to_string();
                                this.copy(s, cx);
                            })),
                    ),
            )
    }
}

impl Render for OcrView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(text) = self.pending_copy.take_if(|_| window.is_window_active()) {
            output::copy_text(text, cx);
        }
        let shown = self.text.read(cx).value().to_string();
        let blank = shown.trim().is_empty();
        let preview = self.preview(cx);
        gpui_component::window_border().child(
            div()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key_down))
                .size_full()
                .flex()
                .flex_col()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(crate::chrome::title_bar(self.title, cx))
                .child(self.toolbar(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .children(preview)
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .p_2()
                                .child(Textarea::new(&self.text).h_full().appearance(false).text_size(px(14.)))
                                .when(blank, |d| {
                                    d.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(6.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(Icon::new(IconName::FileX).size(px(30.)))
                                            .child("No text found in this capture"),
                                    )
                                }),
                        ),
                )
                .child(self.footer(&shown, cx)),
        )
    }
}
