mod alert;
mod annotate;
mod board;
mod capture;
mod chrome;
mod card;
mod editor;
mod flow;
mod frames;
mod gifwriter;
mod history;
mod hud;
mod ipc;
mod media;
mod ocr;
mod output;
mod overlay;
mod prefs;
mod preview;
mod raster;
mod record;
mod screencast;
mod scroll;
mod sensitive;
mod settings;
mod shortcuts;
mod snap;
mod stickers;
mod tray;
mod video;

use std::borrow::Cow;

use gpui::{App, AssetSource, QuitMode, SharedString};
use gpui_component::{Theme, ThemeMode};

use crate::ipc::{Command, Instance};
use crate::overlay::{Mode, Purpose};
use crate::settings::Settings;

pub const APP_ID: &str = "com.thanglb.slopshot";

const USAGE: &str = "\
Usage: slopshot [COMMAND]

Without a command, starts SlopShot in the background with its tray icon.
A command is handed to the running instance, starting one if needed.

Commands:
  area          Capture an area
  full          Capture the full screen
  scroll        Capture a scrolling area
  text          Read the text in an area (OCR)
  color         Pick a colour from the screen
  record        Record an area, or stop the running recording
  history       Open the capture history
  edit <FILE>   Open an image or a video in its editor
  edit-last     Open the last capture in the editor
  open-last     Open the last capture's file
  settings      Open Settings
  about         Show the version
  quit          Quit the running instance

Starting SlopShot again while it runs opens Settings.";

gpui_kit_assets::icon_assets!(
    ExtraIcons,
    [
        MousePointer2, RectangleHorizontal, Circle, Slash, ArrowUpRight, Highlighter, Droplet, PenTool, Type, Baseline,
        Check, Undo2, Trash, RotateCwSquare, FlipHorizontal2, FlipVertical2, RotateCcw, RotateCw, Minus, Plus, Share,
        Copy, X, Download, Camera, Scan, Pipette, Pin, PinOff, SquarePen, Share2, Settings, Hand, Keyboard, Info,
        Eye, Scissors, Square, Pause, Mic, MicOff, GripVertical, LoaderCircle, Timer, Sticker,
        Folder, CircleCheck, TriangleAlert, CircleQuestionMark, Lock, ScanText, ChevronsUpDown, CircleDot, QrCode,
        FileX, TextAlignStart, Image, CirclePlay, Play, Search, RotateCcwClock, TextSearch, Gauge, Snowflake, ZoomIn,
        EyeOff, WandSparkles, SkipBack, ChevronDown, Volume2, VolumeX, Hourglass, ChevronLeft, CircleArrowDown, Package,
        RefreshCw, WifiOff
    ]
);

/// Component icons plus the editor's tool icons.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == "app-icon.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../data/icons/hicolor/128x128/apps/com.thanglb.slopshot.png"))));
        }
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("slopshot=info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return;
        }
        Some("-V" | "--version") => {
            println!("slopshot {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some(card::HOST_ARG) => {
            card::host_main();
            return;
        }
        _ => {}
    }
    let command = match Command::parse(&args) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("slopshot: {err}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    // A second launch with no command is the macOS "reopen": it opens Settings.
    let forwarded = if command == Command::Run { Command::Settings } else { command.clone() };
    let listener = match ipc::claim(&forwarded) {
        Ok(Instance::Forwarded) => return,
        Ok(Instance::Primary(listener)) => listener,
        Err(err) => {
            eprintln!("slopshot: cannot open the control socket: {err}");
            std::process::exit(1);
        }
    };
    if command == Command::Quit {
        ipc::cleanup();
        return;
    }

    let (tx, rx) = async_channel::unbounded();
    ipc::serve(listener, tx.clone());

    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            preview::apply_ui_font(cx);
            cx.set_global(Settings::load());
            card::init(flow::on_card_event, cx);
            tray::init(tx, cx);
            first_run(cx);
            capture::request_permission_early(cx);
            cx.spawn(async move |cx| {
                while let Ok(command) = rx.recv().await {
                    cx.update(|cx| run(command, cx));
                }
            })
            .detach();
            run(command, cx);
        });
    ipc::cleanup();
}

/// Registers each GNOME shortcut once, including ones a later version adds. Like the
/// macOS app, no welcome or notification.
fn first_run(cx: &mut App) {
    if !shortcuts::is_gnome() {
        return;
    }
    let s = Settings::get(cx);
    let done = s.installed_shortcuts.clone().unwrap_or_else(|| {
        if s.shortcuts_installed { ["area", "full", "color"].map(String::from).to_vec() } else { Vec::new() }
    });
    let missing: Vec<&str> = shortcuts::SHORTCUTS.iter().map(|s| s.id).filter(|id| !done.iter().any(|d| d == id)).collect();
    if missing.is_empty() {
        return;
    }
    match shortcuts::install(&missing) {
        Ok(()) => Settings::update(cx, |s| {
            s.shortcuts_installed = true;
            s.installed_shortcuts = Some(shortcuts::SHORTCUTS.iter().map(|s| s.id.to_owned()).collect());
        }),
        Err(err) => log::warn!("registering GNOME shortcuts: {err:#}"),
    }
}

pub fn run(command: Command, cx: &mut App) {
    match command {
        Command::Run => {}
        Command::CaptureArea => flow::begin_session(|cx| capture(Target::Area(Purpose::Capture), cx), cx),
        Command::CaptureFullscreen => flow::begin_session(|cx| capture(Target::Fullscreen, cx), cx),
        // Without the X11 helper there is no Done button: the command itself finishes.
        Command::CaptureScrolling if scroll::is_active(cx) => scroll::done(cx),
        Command::CaptureScrolling => flow::begin_session(|cx| capture(Target::Area(Purpose::Scroll), cx), cx),
        Command::CaptureText => flow::begin_session(|cx| capture(Target::Area(Purpose::Text), cx), cx),
        Command::PickColor => flow::begin_session(|cx| capture(Target::Color, cx), cx),
        Command::RecordArea => {
            // The same command stops a running recording, like the macOS hotkey.
            if record::is_recording(cx) {
                record::stop(cx);
            } else {
                flow::begin_session(|cx| capture(Target::Area(Purpose::Record), cx), cx);
            }
        }
        Command::History => history::open_window(cx),
        Command::Edit(path) if video::is_video(&path) => video::edit(path, cx),
        Command::Edit(path) => editor::open_file(&path, cx),
        Command::EditLast => flow::edit_last(cx),
        Command::OpenLast => flow::open_last_file(cx),
        Command::Settings => prefs::open(cx),
        Command::About => prefs::open_tab(prefs::Tab::About, cx),
        Command::Quit => {
            record::shutdown(cx);
            scroll::cancel(cx);
            card::shutdown(cx);
            cx.quit()
        }
    }
}

#[derive(Clone, Copy)]
enum Target {
    Area(Purpose),
    Fullscreen,
    Color,
}

impl Target {
    fn command(self) -> Command {
        match self {
            Target::Area(Purpose::Capture) => Command::CaptureArea,
            Target::Area(Purpose::Text) => Command::CaptureText,
            Target::Area(Purpose::Scroll) => Command::CaptureScrolling,
            Target::Area(Purpose::Record) => Command::RecordArea,
            Target::Fullscreen => Command::CaptureFullscreen,
            Target::Color => Command::PickColor,
        }
    }
}

/// Freezes the screen, then shows the overlay for `target` (Fullscreen shows none).
fn capture(target: Target, cx: &mut App) {
    cx.set_global(flow::Busy);
    cx.spawn(async move |cx| {
        let result = capture::screenshot_if_asked().await;
        cx.update(|cx| {
            // The overlay keeps the session busy until it closes.
            if !(result.is_ok() && !matches!(target, Target::Fullscreen)) {
                cx.remove_global::<flow::Busy>();
            }
            match (result, target) {
                (Ok(image), Target::Area(purpose)) => overlay::open_for(Mode::Area, purpose, image, cx),
                (Ok(image), Target::Color) => overlay::open(Mode::Color, image, cx),
                (Ok(image), Target::Fullscreen) => overlay::capture_fullscreen(image, cx),
                (Err(capture::Failure::Refused), _) => capture::ask_permission(true, Some(target.command()), cx),
                (Err(capture::Failure::Unanswered), _) => capture::ask_permission(false, Some(target.command()), cx),
                (Err(capture::Failure::Other(err)), _) => alert::error("Capture failed", &format!("{err:#}"), cx),
            }
        });
    })
    .detach();
}
