//! Sticker packs (StickerLibrary.swift), the Sticker Store that downloads them
//! (StickerStore.swift) and the picker the editor's sticker button opens.
//!
//! No config file: every folder in `~/.local/share/slopshot/Stickers` is a pack and
//! every image in it a sticker. The folder name is the pack's name, the file name the
//! sticker's tooltip.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use gpui::{
    App, AppContext as _, Context, ExternalPaths, ObjectFit, PathPromptOptions, RenderImage, SharedString, Window,
    div, img, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::progress::Progress;
use gpui_component::tooltip::Tooltip;
use gpui_component::{Icon, Sizable as _};
use gpui_kit_assets::IconName;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use unicode_normalization::UnicodeNormalization as _;

use crate::frames::{self, Frames};
use crate::preview::{accent, to_render_image, white};
use crate::settings::{Settings, data_dir};

pub const DEFAULT_STORE: &str = "https://pub-def5854c65c3430f9a38ca0585e79c1e.r2.dev";

#[derive(Clone, Debug)]
pub struct Sticker {
    pub path: PathBuf,
}

impl Sticker {
    pub fn name(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub struct Pack {
    pub dir: PathBuf,
    /// Composed: names from a zip or a Mac are often decomposed, and a detached accent
    /// renders apart from its letter.
    pub name: String,
    pub stickers: Vec<Sticker>,
}

pub fn root() -> PathBuf {
    data_dir().join("Stickers")
}

/// Packs with at least one image, in Finder order.
pub fn scan() -> Vec<Pack> {
    let root = root();
    let _ = std::fs::create_dir_all(&root);
    let mut packs: Vec<Pack> = visible_entries(&root)
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|dir| Pack {
            name: dir.file_name().unwrap_or_default().to_string_lossy().nfc().collect(),
            stickers: images_in(&dir).into_iter().map(|path| Sticker { path }).collect(),
            dir,
        })
        .filter(|p| !p.stickers.is_empty())
        .collect();
    packs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    packs
}

fn visible_entries(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

fn images_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = visible_entries(dir).into_iter().filter(|p| p.is_file() && is_image(p)).collect();
    files.sort_by(|a, b| natural_cmp(&a.file_name().unwrap().to_string_lossy(), &b.file_name().unwrap().to_string_lossy()));
    files
}

pub fn is_image(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "apng" | "gif" | "webp" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff")
}

/// Finder's order: case-insensitive, and "10.png" after "9.png".
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        digits.push(c);
                    }
                    digits
                };
                let (da, db) = (take(&mut a), take(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let order = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

// ── Adding packs ──────────────────────────────────────────────────────────────

/// Copies a folder of images in as a new pack; a taken name gets " 2", " 3"…
fn import_pack(folder: &Path) -> Option<String> {
    let files = images_in(folder);
    if files.is_empty() {
        return None;
    }
    let raw: String = folder.file_name()?.to_string_lossy().replace('/', "-").nfc().collect();
    let mut name = raw.clone();
    let mut n = 2;
    while root().join(&name).exists() {
        name = format!("{raw} {n}");
        n += 1;
    }
    copy_into(&files, &root().join(&name));
    Some(name)
}

fn add_images(files: &[PathBuf], dir: &Path) -> Option<String> {
    let files: Vec<PathBuf> = files.iter().filter(|f| is_image(f)).cloned().collect();
    if files.is_empty() {
        return None;
    }
    copy_into(&files, dir);
    Some(dir.file_name()?.to_string_lossy().nfc().collect())
}

/// Keeps file names, numbering a clash.
fn copy_into(files: &[PathBuf], dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    for src in files {
        let (Some(stem), ext) = (src.file_stem(), src.extension()) else { continue };
        let mut dest = dir.join(src.file_name().unwrap());
        let mut n = 2;
        while dest.exists() {
            let mut file = format!("{} {n}", stem.to_string_lossy());
            if let Some(ext) = ext {
                file = format!("{file}.{}", ext.to_string_lossy());
            }
            dest = dir.join(file);
            n += 1;
        }
        if let Err(err) = std::fs::copy(src, &dest) {
            log::error!("copying sticker {}: {err}", src.display());
        }
    }
}

/// Dropped or picked folders become packs of their own; loose images join the pack
/// on show. Returns the pack to show next.
fn absorb(paths: &[PathBuf], current: Option<&Path>) -> Option<String> {
    let mut last = None;
    let mut loose = Vec::new();
    for path in paths {
        if path.is_dir() {
            if let Some(name) = import_pack(path) {
                last = Some(name);
            }
        } else if is_image(path) {
            loose.push(path.clone());
        }
    }
    if !loose.is_empty()
        && let Some(name) = add_images(&loose, &current.map(Path::to_owned).unwrap_or_else(|| root().join("My stickers")))
    {
        last = Some(name);
    }
    last
}

/// Installs downloaded images as the pack `raw_name`. Names inside a downloaded zip
/// are foreign data: only the last component is kept, so a "../../" can't write
/// outside the pack.
fn install(files: &[PathBuf], raw_name: &str) -> anyhow::Result<usize> {
    let name: String = raw_name.replace(['/', ':', '\\'], "-").replace("..", ".").trim().nfc().collect();
    if name.is_empty() || files.is_empty() {
        return Ok(0);
    }
    let dir = root().join(&name);
    std::fs::create_dir_all(&dir)?;
    // An update may ship 01.webp where 01.png was; without this the pack holds both.
    let mut existing: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for old in visible_entries(&dir) {
        if let Some(stem) = old.file_stem() {
            existing.entry(stem.to_string_lossy().into_owned()).or_default().push(old);
        }
    }
    let mut n = 0;
    for src in files.iter().filter(|f| is_image(f)) {
        let Some(file) = src.file_name().map(|f| f.to_string_lossy().into_owned()) else { continue };
        if file.starts_with('.') || file.contains(['/', ':']) {
            continue;
        }
        let stem = Path::new(&file).file_stem().unwrap().to_string_lossy().into_owned();
        for old in existing.remove(&stem).unwrap_or_default() {
            let _ = std::fs::remove_file(old);
        }
        std::fs::copy(src, dir.join(&file))?;
        n += 1;
    }
    Ok(n)
}

// ── Sticker Store ─────────────────────────────────────────────────────────────
//
// Static files on Cloudflare R2, no server:
//   <base>/packs.json        the manifest
//   <base>/packs/<id>.zip    a whole pack
//   <base>/covers/<id>.png   its picture in the list

#[derive(Clone, Debug, Deserialize)]
pub struct RemotePack {
    pub id: String,
    pub name: String,
    pub stickers: usize,
    pub animated: usize,
    pub bytes: u64,
    pub sha256: String,
    pub zip: String,
    pub cover: String,
}

impl RemotePack {
    fn subtitle(&self) -> String {
        let n = format!("{} sticker{}", self.stickers, if self.stickers == 1 { "" } else { "s" });
        let size = crate::output::byte_size(self.bytes);
        if self.animated > 0 { format!("{n} · {} animated · {size}", self.animated) } else { format!("{n} · {size}") }
    }
}

#[derive(Deserialize)]
struct Manifest {
    packs: Vec<RemotePack>,
}

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Downloading(f64),
    Installing,
    Failed(String),
}

fn store_base(cx: &App) -> String {
    let base = Settings::get(cx).sticker_store_url.clone().unwrap_or_else(|| DEFAULT_STORE.to_owned());
    base.trim_end_matches('/').to_owned()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(20))).build().into()
}

fn get(url: &str) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
    agent().get(url).call().map_err(|err| anyhow::anyhow!(message(&err)))
}

fn message(err: &ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(404) => "Not found on the server".into(),
        ureq::Error::StatusCode(code) => format!("Server error {code}"),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => "No internet connection".into(),
        other => other.to_string(),
    }
}

fn fetch_manifest(base: &str) -> anyhow::Result<Vec<RemotePack>> {
    let bytes = get(&format!("{base}/packs.json"))?.into_body().read_to_vec()?;
    let mut packs = serde_json::from_slice::<Manifest>(&bytes)?.packs;
    for pack in &mut packs {
        pack.name = pack.name.nfc().collect();
    }
    Ok(packs)
}

fn fetch_cover(base: &str, pack: &RemotePack) -> anyhow::Result<image::RgbaImage> {
    let bytes = get(&format!("{base}/{}", pack.cover))?.into_body().read_to_vec()?;
    Ok(image::load_from_memory(&bytes)?.to_rgba8())
}

/// Downloads, checks and installs a pack; `progress` gets 0…1 while bytes arrive.
fn download(base: &str, pack: &RemotePack, progress: impl Fn(f64)) -> anyhow::Result<usize> {
    let response = get(&format!("{base}/{}", pack.zip))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
        .unwrap_or(pack.bytes);
    let mut reader = response.into_body().into_reader();
    let mut data = Vec::with_capacity(total as usize);
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n]);
        if total > 0 {
            progress((data.len() as f64 / total as f64).min(1.));
        }
    }
    // A pack broken on the way stops here instead of littering the library.
    let sum: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
    anyhow::ensure!(sum.eq_ignore_ascii_case(&pack.sha256), "Download was corrupted");
    let n = unpack(&data, &pack.name)?;
    anyhow::ensure!(n > 0, "The pack had no images in it");
    Ok(n)
}

fn unpack(zip: &[u8], name: &str) -> anyhow::Result<usize> {
    let tmp = std::env::temp_dir().join(format!("slopshot-pack-{}-{}", std::process::id(), chrono::Local::now().timestamp_nanos_opt().unwrap_or(0)));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).context("Could not unpack the archive")?;
        let mut files = Vec::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).context("Could not unpack the archive")?;
            let Some(path) = entry.enclosed_name() else { continue };
            let hidden = path.components().any(|c| {
                let c = c.as_os_str().to_string_lossy();
                c.starts_with('.') || c == "__MACOSX"
            });
            if entry.is_dir() || hidden || !is_image(&path) {
                continue;
            }
            let dest = tmp.join(path.file_name().unwrap());
            std::io::copy(&mut entry, &mut std::fs::File::create(&dest)?)?;
            files.push(dest);
        }
        install(&files, name)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[derive(Default)]
struct Store {
    packs: Vec<RemotePack>,
    loading: bool,
    loaded: bool,
    error: Option<String>,
    status: HashMap<String, Status>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_asked: HashSet<String>,
}

// ── Picker ────────────────────────────────────────────────────────────────────

pub type OnPick = Rc<dyn Fn(Frames, String, &mut Window, &mut App)>;

/// Pack list on the left, grid of stickers on the right; drop folders or images on it.
pub struct StickerPicker {
    packs: Vec<Pack>,
    /// None once a thumbnail failed to load.
    thumbs: HashMap<PathBuf, Option<(Arc<RenderImage>, bool)>>,
    thumbs_asked: HashSet<PathBuf>,
    show_store: bool,
    store: Store,
    on_pick: OnPick,
}

impl StickerPicker {
    pub fn new(on_pick: OnPick) -> Self {
        Self {
            packs: scan(),
            thumbs: HashMap::new(),
            thumbs_asked: HashSet::new(),
            show_store: false,
            store: Store::default(),
            on_pick,
        }
    }

    /// Picks up packs added since the picker was last open.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.packs = scan();
        cx.notify();
    }

    /// The rescan button doubles as "I just changed files in Files", so caches go too.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.packs = scan();
        self.thumbs.clear();
        self.thumbs_asked.clear();
        cx.notify();
    }

    fn pack<'a>(&'a self, cx: &App) -> Option<&'a Pack> {
        let last = Settings::get(cx).last_sticker_pack.as_deref();
        self.packs.iter().find(|p| Some(p.name.as_str()) == last).or(self.packs.first())
    }

    fn show_pack(name: String, cx: &mut Context<Self>) {
        Settings::update(cx, |s| s.last_sticker_pack = Some(name));
    }

    fn absorb(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let current = self.pack(cx).map(|p| p.dir.clone());
        let shown = absorb(&paths, current.as_deref());
        self.reload(cx);
        if let Some(name) = shown {
            Self::show_pack(name, cx);
        }
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                let _ = this.update(cx, |this, cx| this.absorb(paths, cx));
            }
        })
        .detach();
    }

    fn open_folder(&self, cx: &mut Context<Self>) {
        let dir = root();
        let _ = std::fs::create_dir_all(&dir);
        cx.spawn(async move |_, _| {
            let result = async {
                let dir = std::fs::File::open(&dir)?;
                ashpd::desktop::open_uri::OpenFileRequest::default().send_file(&dir).await?;
                anyhow::Ok(())
            }
            .await;
            if let Err(err) = result {
                log::error!("opening the stickers folder: {err:#}");
            }
        })
        .detach();
    }

    fn pick(&self, sticker: &Sticker, window: &mut Window, cx: &mut Context<Self>) {
        match Frames::load(&sticker.path) {
            Ok(frames) => (self.on_pick)(frames, sticker.name(), window, cx),
            Err(err) => log::error!("loading sticker {}: {err:#}", sticker.path.display()),
        }
    }

    /// Thumbnails load off the main thread, a pack at a time.
    fn request_thumbs(&mut self, pack: &Pack, cx: &mut Context<Self>) {
        let missing: Vec<PathBuf> =
            pack.stickers.iter().map(|s| s.path.clone()).filter(|p| self.thumbs_asked.insert(p.clone())).collect();
        if missing.is_empty() {
            return;
        }
        let (tx, rx) = async_channel::unbounded();
        cx.background_spawn(async move {
            for path in missing {
                let thumb = frames::thumbnail(&path);
                if let Err(err) = &thumb {
                    log::warn!("sticker thumbnail {}: {err:#}", path.display());
                }
                if tx.send((path, thumb.ok())).await.is_err() {
                    break;
                }
            }
        })
        .detach();
        // Each one shows as soon as it is ready rather than the pack all at once.
        cx.spawn(async move |this, cx| {
            while let Ok((path, thumb)) = rx.recv().await {
                let updated = this.update(cx, |this, cx| {
                    this.thumbs.insert(path, thumb.map(|(image, animated)| (to_render_image(&image), animated)));
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn refresh_store(&mut self, cx: &mut Context<Self>) {
        if self.store.loading {
            return;
        }
        self.store.loading = true;
        self.store.error = None;
        let base = store_base(cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { fetch_manifest(&base) }).await;
            let _ = this.update(cx, |this, cx| {
                let store = &mut this.store;
                store.loading = false;
                store.loaded = true;
                match result {
                    Ok(packs) => store.packs = packs,
                    Err(err) => {
                        store.packs.clear();
                        store.error = Some(format!("{err:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn request_cover(&mut self, pack: &RemotePack, cx: &mut Context<Self>) {
        if !self.store.covers_asked.insert(pack.id.clone()) {
            return;
        }
        let (base, pack) = (store_base(cx), pack.clone());
        cx.spawn(async move |this, cx| {
            let cover = cx.background_spawn({
                let pack = pack.clone();
                async move { fetch_cover(&base, &pack) }
            });
            if let Ok(cover) = cover.await {
                let _ = this.update(cx, |this, cx| {
                    this.store.covers.insert(pack.id, to_render_image(&cover));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn get_pack(&mut self, pack: &RemotePack, cx: &mut Context<Self>) {
        if matches!(self.store.status.get(&pack.id), Some(Status::Downloading(_) | Status::Installing)) {
            return;
        }
        self.store.status.insert(pack.id.clone(), Status::Downloading(0.));
        let (base, pack) = (store_base(cx), pack.clone());
        let (tx, rx) = async_channel::unbounded::<f64>();
        let job = cx.background_spawn({
            let pack = pack.clone();
            async move {
                download(&base, &pack, |p| {
                    let _ = tx.try_send(p);
                })
            }
        });
        cx.spawn(async move |this, cx| {
            while let Ok(p) = rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    // Hashing and unpacking follow the last byte.
                    let status = if p >= 1. { Status::Installing } else { Status::Downloading(p) };
                    this.store.status.insert(pack.id.clone(), status);
                    cx.notify();
                });
            }
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.store.status.remove(&pack.id);
                        this.reload(cx);
                    }
                    Err(err) => {
                        this.store.status.insert(pack.id.clone(), Status::Failed(format!("{err:#}")));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn browser(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(pack) = self.pack(cx).cloned() else { return self.empty_state(cx) };
        self.request_thumbs(&pack, cx);
        let rows = self.packs.iter().map(|p| {
            let active = p.name == pack.name;
            let name = p.name.clone();
            div()
                .id(SharedString::from(format!("pack-{}", p.name)))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(active, |d| d.bg(accent()))
                .when(!active, |d| d.hover(|s| s.bg(white(0.06))))
                .on_click(cx.listener(move |_, _, _, cx| Self::show_pack(name.clone(), cx)))
                .child(
                    div()
                        .text_size(px(12.))
                        .truncate()
                        .when(active, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                        .child(p.name.clone()),
                )
                .child(div().text_size(px(10.)).opacity(0.6).child(p.stickers.len().to_string()))
        });
        let cells = pack.stickers.iter().enumerate().map(|(i, sticker)| {
            let thumb = self.thumbs.get(&sticker.path);
            let animated = matches!(thumb, Some(Some((_, true))));
            let tip: SharedString = if animated { format!("{} · animated", sticker.name()) } else { sticker.name() }.into();
            let s = sticker.clone();
            div()
                .id(("sticker", i))
                .relative()
                .size(px(62.))
                .p(px(3.))
                .rounded(px(8.))
                .bg(white(0.05))
                .cursor_pointer()
                .hover(|d| d.bg(white(0.12)))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(&s, window, cx)))
                .flex()
                .items_center()
                .justify_center()
                .child(match thumb {
                    Some(Some((image, _))) => img(image.clone()).size(px(56.)).object_fit(ObjectFit::Contain).into_any_element(),
                    Some(None) => Icon::new(IconName::TriangleAlert).size(px(16.)).text_color(white(0.5)).into_any_element(),
                    None => div().into_any_element(),
                })
                // The grid shows only first frames, so moving ones need a mark.
                .when(animated, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(4.))
                            .bottom(px(4.))
                            .px(px(3.))
                            .rounded(px(3.))
                            .bg(gpui::hsla(0., 0., 0., 0.65))
                            .text_size(px(7.))
                            .font_weight(gpui::FontWeight::EXTRA_BOLD)
                            .text_color(gpui::white())
                            .child("GIF"),
                    )
                })
        });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .id("packs")
                    .w(px(132.))
                    .h_full()
                    .flex_none()
                    .overflow_y_scroll()
                    .p(px(6.))
                    .bg(white(0.04))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(rows),
            )
            .child(div().w(px(1.)).h_full().bg(white(0.1)))
            .child(
                div()
                    .id("grid")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .child(div().p(px(10.)).flex().flex_wrap().gap(px(8.)).children(cells)),
            )
            .into_any_element()
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .child(Icon::new(IconName::Sticker).size(px(30.)).text_color(white(0.5)))
            .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::SEMIBOLD).child("No sticker packs yet"))
            .child(
                div()
                    .max_w(px(300.))
                    .text_size(px(11.))
                    .text_center()
                    .text_color(white(0.55))
                    .child("Drop a folder of images here, or use Import — each folder becomes one pack."),
            )
            .child(
                Button::new("browse")
                    .small()
                    .label("Browse packs…")
                    .on_click(cx.listener(|this, _, _, cx| this.show_store(cx))),
            )
            .into_any_element()
    }

    fn show_store(&mut self, cx: &mut Context<Self>) {
        self.show_store = true;
        if !self.store.loaded {
            self.refresh_store(cx);
        }
        cx.notify();
    }

    fn store_pane(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let packs = self.store.packs.clone();
        for pack in &packs {
            self.request_cover(pack, cx);
        }
        let rows = packs.iter().map(|p| {
            let installed = self.packs.iter().any(|l| l.name == p.name);
            let cover = self.store.covers.get(&p.id).cloned();
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(8.))
                .border_b_1()
                .border_color(white(0.08))
                .child(
                    div()
                        .size(px(40.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(7.))
                        .bg(white(0.05))
                        .child(match cover {
                            Some(c) => img(c).size(px(40.)).object_fit(ObjectFit::Contain).into_any_element(),
                            None => Icon::new(IconName::Package).size(px(16.)).text_color(white(0.35)).into_any_element(),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(12.)).font_weight(gpui::FontWeight::MEDIUM).truncate().child(p.name.clone()))
                        .child(div().text_size(px(10.)).text_color(white(0.55)).truncate().child(p.subtitle())),
                )
                .child(self.store_action(p, installed, cx))
        }).collect::<Vec<_>>();
        let overlay = if self.store.loading && packs.is_empty() {
            Some(Icon::new(IconName::LoaderCircle).size(px(16.)).text_color(white(0.6)).into_any_element())
        } else if let Some(err) = self.store.error.clone() {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(Icon::new(IconName::WifiOff).size(px(26.)).text_color(white(0.5)))
                    .child(div().text_size(px(11.)).text_color(white(0.55)).child(err))
                    .child(
                        Button::new("retry-store")
                            .small()
                            .label("Try again")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_store(cx))),
                    )
                    .into_any_element(),
            )
        } else if packs.is_empty() && !self.store.loading {
            Some(div().text_size(px(11.)).text_color(white(0.55)).child("No packs published yet").into_any_element())
        } else {
            None
        };
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(div().id("store").size_full().overflow_y_scroll().children(rows))
            .children(overlay.map(|o| div().absolute().inset_0().flex().items_center().justify_center().child(o)))
            .into_any_element()
    }

    fn store_action(&self, pack: &RemotePack, installed: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id = SharedString::from(format!("get-{}", pack.id));
        match self.store.status.get(&pack.id) {
            // The manifest gives the size, so this is a real bar, not a spinner.
            Some(Status::Downloading(p)) => div().w(px(64.)).child(Progress::new(id).value((*p * 100.) as f32)).into_any_element(),
            Some(Status::Installing) => Icon::new(IconName::LoaderCircle).size(px(14.)).text_color(white(0.6)).into_any_element(),
            Some(Status::Failed(msg)) => {
                let (pack, msg) = (pack.clone(), SharedString::from(msg.clone()));
                Button::new(id)
                    .small()
                    .label("Retry")
                    .tooltip(msg)
                    .on_click(cx.listener(move |this, _, _, cx| this.get_pack(&pack, cx)))
                    .into_any_element()
            }
            None if installed => div()
                .flex()
                .items_center()
                .gap(px(3.))
                .text_size(px(10.))
                .text_color(white(0.55))
                .child(Icon::new(IconName::Check).size(px(10.)))
                .child("Installed")
                .into_any_element(),
            None => {
                let pack = pack.clone();
                Button::new(id)
                    .small()
                    .label("Get")
                    .on_click(cx.listener(move |this, _, _, cx| this.get_pack(&pack, cx)))
                    .into_any_element()
            }
        }
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = div().flex().items_center().gap(px(8.)).px(px(10.)).py(px(8.)).border_t_1().border_color(white(0.1));
        if self.show_store {
            return bar
                .child(
                    Button::new("my-packs")
                        .small()
                        .ghost()
                        .icon(IconName::ChevronLeft)
                        .label("My packs")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_store = false;
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    Button::new("refresh-store")
                        .small()
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip("Check for new packs")
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_store(cx))),
                );
        }
        bar.child(Button::new("import").small().label("Import pack…").on_click(cx.listener(|this, _, _, cx| this.import(cx))))
            .child(Button::new("open-folder").small().label("Open folder").on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))))
            .when(!self.packs.is_empty(), |d| {
                d.child(
                    Button::new("get-packs")
                        .small()
                        .icon(IconName::CircleArrowDown)
                        .label("Get packs")
                        .on_click(cx.listener(|this, _, _, cx| this.show_store(cx))),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("rescan")
                    .small()
                    .ghost()
                    .icon(IconName::RefreshCw)
                    .tooltip("Rescan the stickers folder")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
    }
}

impl Render for StickerPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.show_store {
            self.store_pane(cx)
        } else if self.packs.is_empty() {
            self.empty_state(cx)
        } else {
            self.browser(cx)
        };
        div()
            .id("sticker-picker")
            .w(px(470.))
            .h(px(330.))
            .flex()
            .flex_col()
            .rounded(px(10.))
            .text_color(gpui::white())
            .drag_over::<ExternalPaths>(|style, _, _, _| style.border_2().border_color(accent()))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.absorb(paths.paths().to_vec(), cx)))
            .child(body)
            .child(self.footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_sort_like_finder() {
        let mut names = vec!["10.png", "9.png", "b.png", "A.png", "01.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["01.png", "9.png", "10.png", "A.png", "b.png"]);
    }
}
