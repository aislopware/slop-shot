use gpui::{App, Global};
use ksni::blocking::{Handle, TrayMethods as _};
use ksni::menu::{MenuItem, StandardItem};

use crate::ipc::Command;
use crate::settings::Settings;
use crate::shortcuts;

/// Menu order and titles follow ContentView.swift.
pub struct Tray {
    tx: async_channel::Sender<Command>,
    icons: Vec<ksni::Icon>,
    has_last: bool,
    /// The last capture is an image, so Edit Last Screenshot applies.
    can_edit_last: bool,
    recording: bool,
    /// dbusmenu shortcut hints, refreshed when the menu is rebuilt.
    bindings: Vec<(&'static str, Vec<Vec<String>>)>,
}

impl Tray {
    fn item(&self, label: &str, command: Command) -> MenuItem<Self> {
        let shortcut = match &command {
            Command::CaptureArea => self.binding("area"),
            Command::CaptureFullscreen => self.binding("full"),
            Command::CaptureScrolling => self.binding("scroll"),
            Command::CaptureText => self.binding("text"),
            Command::PickColor => self.binding("color"),
            Command::RecordArea if !self.recording => self.binding("record"),
            _ => Vec::new(),
        };
        StandardItem {
            label: label.into(),
            shortcut,
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.tx.send_blocking(command.clone());
            }),
            ..Default::default()
        }
        .into()
    }

    fn binding(&self, id: &str) -> Vec<Vec<String>> {
        self.bindings.iter().find(|(i, _)| *i == id).map(|(_, b)| b.clone()).unwrap_or_default()
    }
}

impl ksni::Tray for Tray {
    /// Left click opens the menu, as a macOS menu-bar extra does.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        crate::APP_ID.into()
    }

    fn title(&self) -> String {
        "SlopShot".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = vec![
            self.item("Capture Area", Command::CaptureArea),
            self.item("Capture Fullscreen", Command::CaptureFullscreen),
            self.item("Capture Scrolling Area", Command::CaptureScrolling),
            self.item("Capture Text (OCR)", Command::CaptureText),
            self.item("Pick Color", Command::PickColor),
            self.item(if self.recording { "Stop Recording" } else { "Record Area" }, Command::RecordArea),
            MenuItem::Separator,
        ];
        if self.has_last {
            if self.can_edit_last {
                items.push(self.item("Edit Last Screenshot", Command::EditLast));
            }
            items.push(self.item("Open Last File", Command::OpenLast));
            items.push(MenuItem::Separator);
        }
        items.push(self.item("Capture History…", Command::History));
        items.push(self.item("Settings…", Command::Settings));
        items.push(MenuItem::Separator);
        items.push(self.item("About SlopShot", Command::About));
        items.push(self.item("Quit SlopShot", Command::Quit));
        items
    }
}

/// dbusmenu spells modifiers "Control", "Alt", "Shift", "Super".
fn dbusmenu_binding(accel: &str) -> Vec<Vec<String>> {
    let mut keys = Vec::new();
    let mut rest = accel;
    while let Some(start) = rest.strip_prefix('<') {
        let Some(end) = start.find('>') else { break };
        keys.push(match &start[..end] {
            "Primary" | "Ctrl" => "Control".to_owned(),
            other => other.to_owned(),
        });
        rest = &start[end + 1..];
    }
    keys.push(rest.to_owned());
    vec![keys]
}

fn current_bindings() -> Vec<(&'static str, Vec<Vec<String>>)> {
    shortcuts::SHORTCUTS
        .iter()
        .filter_map(|s| Some((s.id, dbusmenu_binding(&shortcuts::current_binding(s.id)?))))
        .collect()
}

/// The macOS menu-bar template icon is black on transparent; panels on Linux are
/// dark, so keep the alpha and paint it white.
fn icons() -> Vec<ksni::Icon> {
    [
        &include_bytes!("../data/tray/bar_16.png")[..],
        &include_bytes!("../data/tray/bar_32.png")[..],
        &include_bytes!("../data/tray/bar_48.png")[..],
    ]
    .into_iter()
    .filter_map(|png| image::load_from_memory(png).ok())
    .map(|img| {
        let img = img.to_rgba8();
        let data = img.pixels().flat_map(|p| [p.0[3], 255, 255, 255]).collect();
        ksni::Icon { width: img.width() as i32, height: img.height() as i32, data }
    })
    .collect()
}

struct TrayState {
    tx: async_channel::Sender<Command>,
    handle: Option<Handle<Tray>>,
}

impl Global for TrayState {}

pub fn init(tx: async_channel::Sender<Command>, cx: &mut App) {
    cx.set_global(TrayState { tx, handle: None });
    sync(cx);
}

/// Shows or hides the icon to match the "Show icon in the tray" setting, and
/// refreshes the menu.
pub fn sync(cx: &mut App) {
    let show = Settings::get(cx).show_tray_icon;
    let has_last = crate::flow::last(cx).is_some();
    let can_edit_last = crate::flow::last(cx).is_some_and(|l| l.image.is_some());
    let recording = crate::record::is_recording(cx);
    let state = cx.global_mut::<TrayState>();
    match (&state.handle, show) {
        (Some(handle), true) => {
            handle.update(|tray| {
                tray.has_last = has_last;
                tray.can_edit_last = can_edit_last;
                tray.recording = recording;
                tray.bindings = current_bindings();
            });
        }
        (Some(handle), false) => {
            handle.shutdown();
            state.handle = None;
        }
        (None, true) => {
            let tray = Tray { tx: state.tx.clone(), icons: icons(), has_last, can_edit_last, recording, bindings: current_bindings() };
            // No StatusNotifier host means stock GNOME without the AppIndicator extension.
            state.handle = tray.spawn().inspect_err(|err| log::warn!("tray icon unavailable: {err}")).ok();
        }
        (None, false) => {}
    }
}
