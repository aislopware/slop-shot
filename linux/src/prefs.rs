//! Settings window: General, Capture, Privacy, Shortcuts and About pages beside a
//! sidebar, in zeron's layout; every change applies and persists immediately.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, KeyDownEvent,
    PathPromptOptions, SharedString, Subscription, Window, div, img, prelude::*, px,
};
use gpui_component::IndexPath;
use gpui_component::searchable_list::SearchableVec;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_kit_assets::IconName;

use crate::settings::{ColorFormat, ImageFormat, PreviewSide, Settings, config_dir, display_path};
use crate::ui::{self, Tone};
use crate::{capture, shortcuts, theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    General,
    Capture,
    Privacy,
    Shortcuts,
    About,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Capture => "Capture",
            Tab::Privacy => "Privacy",
            Tab::Shortcuts => "Shortcuts",
            Tab::About => "About",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Tab::General => IconName::Settings,
            Tab::Capture => IconName::Scan,
            Tab::Privacy => IconName::Hand,
            Tab::Shortcuts => IconName::Keyboard,
            Tab::About => IconName::Info,
        }
    }
}

struct PrefsWindow(AnyWindowHandle, Entity<Prefs>);
impl gpui::Global for PrefsWindow {}

pub fn open(cx: &mut App) {
    open_tab(Tab::General, cx);
}

/// Re-opening focuses the existing window, like macOS.
pub fn open_tab(tab: Tab, cx: &mut App) {
    if let Some(PrefsWindow(handle, prefs)) = cx.try_global::<PrefsWindow>().map(|w| PrefsWindow(w.0, w.1.clone()))
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        prefs.update(cx, |p, cx| {
            p.tab = tab;
            cx.notify();
        });
        return;
    }
    let options = crate::chrome::window_options("Settings", (820., 560.), Some((700., 440.)), cx);
    let mut prefs = None;
    let result = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| Prefs::new(tab, window, cx));
        prefs = Some(view.clone());
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    });
    match (result, prefs) {
        (Ok(handle), Some(prefs)) => cx.set_global(PrefsWindow(handle.into(), prefs)),
        (Err(err), _) => log::error!("opening settings: {err:#}"),
        _ => {}
    }
}

fn autostart_path() -> PathBuf {
    config_dir().join("autostart").join(format!("{}.desktop", crate::APP_ID))
}

fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let path = autostart_path();
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=SlopShot\nExec={}\nIcon={}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
            exe.display(),
            crate::APP_ID
        ),
    )
}

type Choice = SelectState<SearchableVec<SharedString>>;

pub(crate) struct Prefs {
    tab: Tab,
    focus: FocusHandle,
    image_format: Entity<Choice>,
    preview_side: Entity<Choice>,
    color_format: Entity<Choice>,
    /// The shortcut whose recorder is listening.
    recording: Option<&'static str>,
    shortcut_error: Option<SharedString>,
    permission: Option<bool>,
    grant_copied: bool,
    _subscriptions: Vec<Subscription>,
}

fn choice<T: Copy + PartialEq + 'static>(
    all: &'static [T],
    label: fn(T) -> &'static str,
    current: T,
    set: fn(&mut Settings, T),
    window: &mut Window,
    cx: &mut Context<Prefs>,
) -> (Entity<Choice>, Subscription) {
    let items: Vec<SharedString> = all.iter().map(|v| label(*v).into()).collect();
    let selected = all.iter().position(|v| *v == current).map(IndexPath::new);
    let state = cx.new(|cx| SelectState::new(SearchableVec::new(items), selected, window, cx));
    let sub = cx.subscribe(&state, move |_, _, event: &SelectEvent<SearchableVec<SharedString>>, cx| {
        let SelectEvent::Confirm(Some(value)) = event else { return };
        if let Some(v) = all.iter().find(|v| label(**v) == value.as_ref()) {
            let v = *v;
            Settings::update(cx, |s| set(s, v));
        }
    });
    (state, sub)
}

const SIDES: [PreviewSide; 2] = [PreviewSide::Left, PreviewSide::Right];

impl Prefs {
    pub(crate) fn new(tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = Settings::get(cx).clone();
        let (image_format, a) = choice(&ImageFormat::ALL, ImageFormat::label, s.image_format, |s, v| s.image_format = v, window, cx);
        let (preview_side, b) = choice(&SIDES, PreviewSide::label, s.preview_side, |s, v| s.preview_side = v, window, cx);
        let (color_format, c) = choice(&ColorFormat::ALL, ColorFormat::label, s.color_format, |s, v| s.color_format = v, window, cx);
        // The Privacy tab refreshes every 2 s, as on macOS.
        cx.spawn(async move |this, cx| {
            loop {
                let granted = capture::permission_granted().await;
                if this.update(cx, |p, cx| {
                    if p.permission != granted {
                        p.permission = granted;
                        cx.notify();
                    }
                })
                .is_err()
                {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            tab,
            focus,
            image_format,
            preview_side,
            color_format,
            recording: None,
            shortcut_error: None,
            permission: None,
            grant_copied: false,
            _subscriptions: vec![a, b, c],
        }
    }

    /// While a recorder listens every key goes to it.
    fn key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.recording else { return };
        cx.stop_propagation();
        let k = &e.keystroke;
        if k.key == "escape" && !(k.modifiers.control || k.modifiers.alt || k.modifiers.shift || k.modifiers.platform) {
            self.recording = None;
            cx.notify();
            return;
        }
        if matches!(k.key.as_str(), "control" | "alt" | "shift" | "super" | "platform" | "") {
            return;
        }
        match shortcuts::accelerator(k) {
            Some(binding) => {
                self.shortcut_error = shortcuts::set_binding(id, &binding).err().map(|e| e.to_string().into());
                self.recording = None;
            }
            None => beep(),
        }
        cx.notify();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let group = |tabs: &[Tab], cx: &mut Context<Self>| {
            div().flex().flex_col().gap(px(1.)).children(tabs.iter().map(|&tab| {
                ui::nav_item(tab.title(), tab.icon(), tab.title(), self.tab == tab).on_click(cx.listener(move |p, _, _, cx| {
                    p.tab = tab;
                    p.recording = None;
                    cx.notify();
                }))
            }))
        };
        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(8.))
            .bg(theme::sidebar())
            .border_r_1()
            .border_color(theme::ink(0.06))
            .child(drag_strip())
            .child(group(&[Tab::General, Tab::Capture], cx))
            .child(group(&[Tab::Privacy, Tab::Shortcuts], cx))
            .child(group(&[Tab::About], cx))
    }

    fn general(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = Settings::get(cx).clone();
        page("General", "How SlopShot starts, and where captures go.")
            .child(ui::section(
                "Startup",
                ui::block()
                    .child(
                        ui::row(true).child(ui::row_text("Launch SlopShot at login", None)).child(
                            ui::switch("autostart", autostart_path().exists()).on_click(|_, _, cx| {
                                if let Err(err) = set_autostart(!autostart_path().exists()) {
                                    log::error!("autostart: {err}");
                                }
                                cx.refresh_windows();
                            }),
                        ),
                    )
                    .child(
                        ui::row(false)
                            .child(ui::row_text(
                                "Show icon in the top bar",
                                (!s.show_tray_icon).then_some("Hotkeys keep working. To get back here, open SlopShot again from the app grid.".into()),
                            ))
                            .child(ui::switch("tray", s.show_tray_icon).on_click(move |_, _, cx| {
                                Settings::update(cx, |s| s.show_tray_icon = !s.show_tray_icon);
                                crate::tray::sync(cx);
                            })),
                    ),
            ))
            .child(ui::section(
                "Save location",
                ui::block().child(
                    ui::row(true)
                        .child(ui::row_tile(IconName::Folder))
                        .child(ui::row_text(
                            display_path(&s.save_folder),
                            Some("Captures stay temporary until you press Save on the preview, then land here.".into()),
                        ))
                        .child(ui::text_button("folder", Tone::Filled, "Choose…").on_click(|_, _, cx| {
                            let receiver = cx.prompt_for_paths(PathPromptOptions {
                                files: false,
                                directories: true,
                                multiple: false,
                                prompt: Some("Choose".into()),
                            });
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(paths))) = receiver.await
                                    && let Some(dir) = paths.into_iter().next()
                                {
                                    cx.update(|cx| Settings::update(cx, |s| s.save_folder = dir));
                                }
                            })
                            .detach();
                        })),
                ),
            ))
            .child(ui::section(
                "Image format",
                ui::block().child(
                    ui::row(true)
                        .child(ui::row_text("Format", Some("Used when a capture is saved; the editor can save as any of them.".into())))
                        .child(select(&self.image_format, 140., false)),
                ),
            ))
            .into_any_element()
    }

    fn capture(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = Settings::get(cx).clone();
        page("Capture", "What happens while you select, and right after.")
            .child(ui::section(
                "Selection",
                ui::block().child(toggle_row(
                    true,
                    "snap",
                    "Snap to window & item edges",
                    Some("Hover to outline the item under the cursor, then click to grab it. Hold Alt for a free selection."),
                    s.snap,
                    |s, v| s.snap = v,
                )),
            ))
            .child(ui::section(
                "After capture",
                ui::block()
                    .child(toggle_row(
                        true,
                        "inline",
                        "Annotate right after selecting an area",
                        Some("Drawing tools appear on the selection; Ctrl+C copies, Ctrl+S saves, Esc cancels."),
                        s.edit_after_select,
                        |s, v| s.edit_after_select = v,
                    ))
                    .child(toggle_row(false, "copy", "Also copy to clipboard", None, s.copy_to_clipboard, |s, v| s.copy_to_clipboard = v))
                    .child(toggle_row(false, "sound", "Play a sound", None, s.play_sound, |s, v| s.play_sound = v))
                    .child(toggle_row(false, "thumb", "Show preview thumbnail", None, s.show_thumbnail, |s, v| s.show_thumbnail = v))
                    .child(
                        ui::row(false)
                            .when(!s.show_thumbnail, |d| d.opacity(0.5))
                            .child(ui::row_text("Preview position", Some("Right-handed? Put it on the right, closer to your mouse.".into())))
                            .child(select(&self.preview_side, 160., !s.show_thumbnail)),
                    ),
            ))
            .child(ui::section(
                "Recording",
                ui::block()
                    .child(toggle_row(
                        true,
                        "sys-audio",
                        "Record system audio",
                        Some("What your computer plays: apps, videos, calls."),
                        s.record_system_audio,
                        |s, v| s.record_system_audio = v,
                    ))
                    .child(toggle_row(
                        false,
                        "mic",
                        "Record microphone",
                        Some("The default input in Sound settings."),
                        s.record_microphone,
                        |s, v| s.record_microphone = v,
                    )),
            ))
            .child(ui::section(
                "Color picker",
                ui::block().child(
                    ui::row(true)
                        .child(ui::row_text("Copy color as", Some("Press ← → while picking to switch without leaving the screen.".into())))
                        .child(select(&self.color_format, 180., false)),
                ),
            ))
            .into_any_element()
    }

    fn privacy(&self, cx: &mut Context<Self>) -> AnyElement {
        let (icon, status, color) = match self.permission {
            Some(true) => (IconName::CircleCheck, "Granted", theme::success()),
            Some(false) => (IconName::TriangleAlert, "Not granted", theme::warning()),
            None => (IconName::CircleQuestionMark, "Not asked yet", theme::muted()),
        };
        let s = Settings::get(cx).clone();
        let refused = self.permission == Some(false);
        let permissions = ui::block()
            .child(
                ui::row(true)
                    .child(ui::row_text(
                        "Screenshots",
                        Some("Every capture. Without it GNOME blocks SlopShot from reading the screen.".into()),
                    ))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.5))
                            .text_color(color)
                            .child(ui::icon(icon, 14., color))
                            .child(status),
                    )
                    .when(self.permission != Some(true), |d| {
                        d.child(
                            ui::text_button("ask", Tone::Filled, if refused { "Ask Again" } else { "Ask Now" })
                                .on_click(move |_, _, cx| capture::ask_permission(refused, None, cx)),
                        )
                    }),
            )
            .when(self.permission != Some(true), |d| {
                d.child(
                    ui::row(false)
                        .child(ui::row_text(
                            "GNOME's prompt never appears?",
                            Some("Copy the command, paste it into Terminal and press Enter.".into()),
                        ))
                        .child(
                            ui::button("copy-grant", Tone::Filled)
                                .child(ui::icon(if self.grant_copied { IconName::Check } else { IconName::Copy }, 14., theme::muted()))
                                .child(if self.grant_copied { "Copied" } else { "Copy Command" })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    capture::copy_grant_command(cx);
                                    this.grant_copied = true;
                                    cx.notify();
                                })),
                        ),
                )
            });
        page("Privacy", "Both checks run on-device with Tesseract: no image, and no text read out of one, leaves this computer.")
            .child(ui::section("Permissions", permissions))
            .child(ui::section(
                "Sensitive data",
                ui::block().child(toggle_row(
                    true,
                    "redact",
                    "Scan captures for sensitive data",
                    Some("Emails, phone and card numbers, tokens, passwords and IDs are counted and offered for covering. Nothing is covered until you press Redact."),
                    s.redact_scan_on_open,
                    |s, v| s.redact_scan_on_open = v,
                )),
            ))
            .child(ui::section(
                "Search",
                ui::block().child(toggle_row(
                    true,
                    "index",
                    "Index text inside screenshots",
                    Some("Reads each screenshot in the background so History finds it by what's written in it."),
                    s.index_capture_text,
                    |s, v| s.index_capture_text = v,
                )),
            ))
            .into_any_element()
    }

    fn shortcuts(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = shortcuts::SHORTCUTS.iter().enumerate().map(|(i, sc)| {
            let listening = self.recording == Some(sc.id);
            let binding = if listening {
                "Type shortcut…".to_owned()
            } else {
                shortcuts::current_binding(sc.id).map(|b| shortcuts::display_binding(&b)).unwrap_or_else(|| "—".into())
            };
            let id = sc.id;
            ui::row(i == 0)
                .child(ui::row_text(sc.name.trim_start_matches("SlopShot: ").to_owned(), None))
                .child(
                    div()
                        .id(SharedString::from(format!("rec-{id}")))
                        .flex_none()
                        .min_w(px(120.))
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .border_1()
                        .text_size(px(12.))
                        .cursor_pointer()
                        .map(|d| {
                            if listening {
                                d.bg(theme::accent().opacity(0.14)).border_color(theme::accent()).text_color(theme::accent())
                            } else {
                                d.bg(theme::fill()).border_color(gpui::transparent_black()).text_color(theme::text()).hover(|s| s.bg(theme::fill_hover()))
                            }
                        })
                        .on_click(cx.listener(move |p, _, window, cx| {
                            p.recording = Some(id);
                            window.focus(&p.focus, cx);
                            cx.notify();
                        }))
                        .child(binding),
                )
                .child(ui::icon_button(SharedString::from(format!("reset-{id}")), IconName::Undo2, "Reset to default").on_click(cx.listener(
                    move |p, _, _, cx| {
                        p.shortcut_error = shortcuts::reset(id).err().map(|e| e.to_string().into());
                        p.recording = None;
                        cx.notify();
                    },
                )))
        });
        let subtitle = if shortcuts::is_gnome() {
            "Click a shortcut, then press the new combination (with Ctrl, Alt, Shift or Super). Esc cancels."
        } else {
            "Bind these commands in your desktop's keyboard settings: slopshot area, slopshot full, slopshot color."
        };
        page("Shortcuts", subtitle)
            .child(ui::section("Global shortcuts", ui::block().children(rows)))
            .children(self.shortcut_error.clone().map(|e| div().mt(px(8.)).child(ui::caption(e).text_color(theme::danger()))))
            .into_any_element()
    }

    fn about(&self, _: &mut Context<Self>) -> AnyElement {
        page("About", "A CleanShot-style capture tool, built native on Linux.")
            .child(
                ui::block().mt(px(28.)).child(
                    ui::row(true)
                        .child(img("app-icon.png").flex_none().size(px(40.)))
                        .child(ui::row_text("SlopShot", Some(format!("Version {}", env!("CARGO_PKG_VERSION")).into())))
                        .child(ui::badge("Linux")),
                ),
            )
            .into_any_element()
    }
}

const SIDEBAR_WIDTH: f32 = 208.;

/// The clear strip at the top: dragging it moves the window.
fn drag_strip() -> gpui::Div {
    div()
        .h(px(ui::TITLEBAR_HEIGHT))
        .flex_none()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, _| window.start_window_move())
}

/// The centered page column: title, subtitle, then sections.
fn page(title: &'static str, subtitle: &'static str) -> gpui::Div {
    div()
        .w_full()
        .max_w(px(600.))
        .mx_auto()
        .px(px(32.))
        .pb(px(40.))
        .flex()
        .flex_col()
        .child(ui::page_header(title, None))
        .child(ui::page_subtitle(subtitle))
}

/// Select stretches to its container, so the container holds the width.
fn select(state: &Entity<Choice>, width: f32, disabled: bool) -> gpui::Div {
    div().flex_none().w(px(width)).child(
        Select::new(state)
            .disabled(disabled)
            .h(px(ui::CONTROL_HEIGHT))
            .px(px(10.))
            .rounded(px(8.))
            .bg(theme::fill())
            .border_color(gpui::transparent_black())
            .text_size(px(12.5))
            .text_color(theme::text()),
    )
}

fn toggle_row(
    first: bool,
    id: &'static str,
    title: &'static str,
    description: Option<&'static str>,
    on: bool,
    set: fn(&mut Settings, bool),
) -> gpui::Div {
    ui::row(first)
        .child(ui::row_text(title, description.map(Into::into)))
        .child(ui::switch(id, on).on_click(move |_, _, cx| Settings::update(cx, |s| set(s, !on))))
}

fn beep() {
    let _ = std::process::Command::new("canberra-gtk-play").args(["-i", "bell"]).spawn();
}

impl Render for Prefs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.tab {
            Tab::General => self.general(cx),
            Tab::Capture => self.capture(cx),
            Tab::Privacy => self.privacy(cx),
            Tab::Shortcuts => self.shortcuts(cx),
            Tab::About => self.about(cx),
        };
        gpui_component::window_border().child(
            div()
                .track_focus(&self.focus)
                .on_key_down(cx.listener(Self::key_down))
                .size_full()
                .flex()
                .bg(theme::bg())
                .text_color(theme::text())
                .text_size(px(13.))
                .child(self.sidebar(cx))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(drag_strip().w_full().flex().justify_end().items_center().px(px(8.)).child(ui::close_button()))
                        .child(div().id("prefs-body").flex_1().min_h_0().overflow_y_scroll().child(body)),
                ),
        )
    }
}
