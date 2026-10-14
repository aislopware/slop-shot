//! Settings window: General, Capture, Privacy, Shortcuts and About pages beside a
//! section list, in Slopty's settings layout; every change applies and persists
//! immediately.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, KeyDownEvent,
    PathPromptOptions, SharedString, Window, div, img, prelude::*, px,
};

use crate::icons::{self, icon};
use crate::settings::{ColorFormat, ImageFormat, PreviewSide, Settings, config_dir, display_path};
use crate::theme::{self, Theme};
use crate::widgets::{self, ActionTone, SelectOption, SelectState};
use crate::{capture, chrome, motion, shortcuts, ui};

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

    fn icon(self) -> &'static str {
        match self {
            Tab::General => icons::SETTINGS,
            Tab::Capture => icons::FULL_SCREEN,
            Tab::Privacy => icons::SHIELD_KEYHOLE_MINIMALISTIC,
            Tab::Shortcuts => icons::KEYBOARD,
            Tab::About => icons::INFO_CIRCLE,
        }
    }

    /// Groups are set apart by spacing alone.
    fn starts_group(self) -> bool {
        matches!(self, Tab::Privacy | Tab::About)
    }

    const ALL: [Tab; 5] = [Tab::General, Tab::Capture, Tab::Privacy, Tab::Shortcuts, Tab::About];
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
    let options = chrome::window_options("Settings", (900., 620.), Some((720., 460.)), cx);
    let mut prefs = None;
    let result = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| Prefs::new(tab, window, cx));
        prefs = Some(view.clone());
        view
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

pub(crate) struct Prefs {
    tab: Tab,
    focus: FocusHandle,
    image_format: SelectState,
    preview_side: SelectState,
    color_format: SelectState,
    /// The shortcut whose recorder is listening.
    recording: Option<&'static str>,
    shortcut_error: Option<SharedString>,
    permission: Option<bool>,
    grant_copied: bool,
}

const SIDES: [PreviewSide; 2] = [PreviewSide::Left, PreviewSide::Right];

impl Prefs {
    pub(crate) fn new(tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            image_format: SelectState::default(),
            preview_side: SelectState::default(),
            color_format: SelectState::default(),
            recording: None,
            shortcut_error: None,
            permission: None,
            grant_copied: false,
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

    fn sidebar(&self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = Tab::ALL.map(|tab| {
            let selected = self.tab == tab;
            let key = format!("prefs-nav-{}", tab.title());
            let hover_key = format!("{key}-hover");
            let selection_t =
                widgets::tab_selection_t(window, format!("{key}-selection"), selected, motion::reduced_motion(cx));
            let glyph =
                motion::hover_blend(&hover_key, motion::mix(theme.text_muted, theme.text, selection_t), theme.text);
            widgets::section_tab(theme, selected, selection_t, SharedString::from(key), hover_key)
                .when(tab.starts_group(), |el| el.mt(px(Theme::SPACE_LG)))
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.tab = tab;
                    p.recording = None;
                    cx.notify();
                }))
                .child(icon(tab.icon()).size(px(16.0)).text_color(glyph))
                .child(tab.title())
        });
        let radius = chrome::corner_radius(window);
        div()
            .relative()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .when(radius > 0., |el| el.rounded_tl(px(radius)).rounded_bl(px(radius)))
                    .bg(theme::wash(0.05))
                    .border_r_1()
                    .border_color(theme.border),
            )
            .child(chrome::drag_strip("prefs-sidebar-drag").absolute().inset_0())
            .child(
                div()
                    .relative()
                    .pt(px(Theme::TITLEBAR_HEIGHT + Theme::SPACE_MD))
                    .px(px(Theme::SPACE_SM))
                    .pb(px(Theme::SPACE_SM))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(tabs),
            )
    }

    fn general(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::get();
        let s = Settings::get(cx).clone();
        page("General", "How SlopShot starts, and where captures go.")
            .child(widgets::section(t,
                "Startup",
                widgets::section_card(t)
                    .child(
                        widgets::card_row(t, true).child(widgets::row_text(t, "Launch SlopShot at login", None)).child(
                            switch("autostart", autostart_path().exists()).on_click(|_, _, cx| {
                                if let Err(err) = set_autostart(!autostart_path().exists()) {
                                    log::error!("autostart: {err}");
                                }
                                cx.refresh_windows();
                            }),
                        ),
                    )
                    .child(
                        widgets::card_row(t, false)
                            .child(widgets::row_text(t,
                                "Show icon in the top bar",
                                (!s.show_tray_icon).then_some("Hotkeys keep working. To get back here, open SlopShot again from the app grid.".into()),
                            ))
                            .child(switch("tray", s.show_tray_icon).on_click(move |_, _, cx| {
                                Settings::update(cx, |s| s.show_tray_icon = !s.show_tray_icon);
                                crate::tray::sync(cx);
                            })),
                    ),
            ))
            .child(widgets::section(t,
                "Save location",
                widgets::section_card(t).child(
                    widgets::card_row(t, true)
                        .child(widgets::row_tile(t, icons::FOLDER))
                        .child(widgets::row_text(t,
                            display_path(&s.save_folder),
                            Some("Captures stay temporary until you press Save on the preview, then land here.".into()),
                        ))
                        .child(text_action("folder", "Choose…").on_click(|_, _, cx| {
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
            .child(widgets::section(t,
                "Image format",
                widgets::section_card(t).child(
                    widgets::card_row(t, true)
                        .child(widgets::row_text(t, "Format", Some("Used when a capture is saved; the editor can save as any of them.".into())))
                        .child(choice("image-format", &ImageFormat::ALL, ImageFormat::label, s.image_format, |s, v| s.image_format = v, &self.image_format, |p| &mut p.image_format, 140., false, window, cx)),
                ),
            ))
            .into_any_element()
    }

    fn capture(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::get();
        let s = Settings::get(cx).clone();
        page("Capture", "What happens while you select, and right after.")
            .child(widgets::section(t,
                "Selection",
                widgets::section_card(t).child(toggle_row(
                    true,
                    "snap",
                    "Snap to window & item edges",
                    Some("Hover to outline the item under the cursor, then click to grab it. Hold Alt for a free selection."),
                    s.snap,
                    |s, v| s.snap = v,
                )),
            ))
            .child(widgets::section(t,
                "After capture",
                widgets::section_card(t)
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
                        widgets::card_row(t, false)
                            .when(!s.show_thumbnail, |d| d.opacity(0.5))
                            .child(widgets::row_text(t, "Preview position", Some("Right-handed? Put it on the right, closer to your mouse.".into())))
                            .child(choice("preview-side", &SIDES, PreviewSide::label, s.preview_side, |s, v| s.preview_side = v, &self.preview_side, |p| &mut p.preview_side, 160., !s.show_thumbnail, window, cx)),
                    ),
            ))
            .child(widgets::section(t,
                "Recording",
                widgets::section_card(t)
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
            .child(widgets::section(t,
                "Color picker",
                widgets::section_card(t).child(
                    widgets::card_row(t, true)
                        .child(widgets::row_text(t, "Copy color as", Some("Press ← → while picking to switch without leaving the screen.".into())))
                        .child(choice("color-format", &ColorFormat::ALL, ColorFormat::label, s.color_format, |s, v| s.color_format = v, &self.color_format, |p| &mut p.color_format, 180., false, window, cx)),
                ),
            ))
            .into_any_element()
    }

    fn privacy(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = Theme::get();
        let (glyph, status, color) = match self.permission {
            Some(true) => (icons::CHECK_CIRCLE, "Granted", t.success),
            Some(false) => (icons::DANGER_TRIANGLE, "Not granted", t.warning),
            None => (icons::QUESTION_CIRCLE, "Not asked yet", t.text_muted),
        };
        let s = Settings::get(cx).clone();
        let refused = self.permission == Some(false);
        let permissions = widgets::section_card(t)
            .child(
                widgets::card_row(t, true)
                    .child(widgets::row_text(t,
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
                            .child(icon(glyph).size(px(14.)).text_color(color))
                            .child(status),
                    )
                    .when(self.permission != Some(true), |d| {
                        d.child(
                            text_action("ask", if refused { "Ask Again" } else { "Ask Now" })
                                .on_click(move |_, _, cx| capture::ask_permission(refused, None, cx)),
                        )
                    }),
            )
            .when(self.permission != Some(true), |d| {
                d.child(
                    widgets::card_row(t, false)
                        .child(widgets::row_text(t,
                            "GNOME's prompt never appears?",
                            Some("Copy the command, paste it into Terminal and press Enter.".into()),
                        ))
                        .child(
                            widgets::action_button(t, ActionTone::Filled)
                                .id("copy-grant")
                                .child(icon(if self.grant_copied { icons::CHECK } else { icons::COPY }).size(px(14.)).text_color(t.text_muted))
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
            .child(widgets::section(t, "Permissions", permissions))
            .child(widgets::section(t,
                "Sensitive data",
                widgets::section_card(t).child(toggle_row(
                    true,
                    "redact",
                    "Scan captures for sensitive data",
                    Some("Emails, phone and card numbers, tokens, passwords and IDs are counted and offered for covering. Nothing is covered until you press Redact."),
                    s.redact_scan_on_open,
                    |s, v| s.redact_scan_on_open = v,
                )),
            ))
            .child(widgets::section(t,
                "Search",
                widgets::section_card(t).child(toggle_row(
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
        let t = Theme::get();
        let rows = shortcuts::SHORTCUTS.iter().enumerate().map(|(i, sc)| {
            let listening = self.recording == Some(sc.id);
            let binding = if listening {
                "Type shortcut…".to_owned()
            } else {
                shortcuts::current_binding(sc.id).map(|b| shortcuts::display_binding(&b)).unwrap_or_else(|| "—".into())
            };
            let id = sc.id;
            widgets::card_row(t, i == 0)
                .child(widgets::row_text(t, sc.name.trim_start_matches("SlopShot: ").to_owned(), None))
                .child(
                    div()
                        .id(SharedString::from(format!("rec-{id}")))
                        .flex_none()
                        .min_w(px(120.))
                        .h(px(widgets::SELECT_HEIGHT))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .border_1()
                        .text_size(px(12.5))
                        .cursor_pointer()
                        .map(|d| {
                            if listening {
                                d.bg(t.accent.opacity(0.14)).border_color(t.accent).text_color(t.accent)
                            } else {
                                d.bg(widgets::select_fill(t, false))
                                    .border_color(gpui::transparent_black())
                                    .text_color(t.text)
                                    .hover(|s| s.bg(widgets::select_fill(t, true)))
                            }
                        })
                        .on_click(cx.listener(move |p, _, window, cx| {
                            p.recording = Some(id);
                            window.focus(&p.focus, cx);
                            cx.notify();
                        }))
                        .child(binding),
                )
                .child(ui::icon_button(format!("reset-{id}"), icons::UNDO_LEFT, "Reset to default").on_click(cx.listener(
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
            .child(widgets::section(t, "Global shortcuts", widgets::section_card(t).children(rows)))
            .children(self.shortcut_error.clone().map(|e| widgets::error_strip(t, e)))
            .into_any_element()
    }

    fn about(&self) -> AnyElement {
        let t = Theme::get();
        page("About", "A CleanShot-style capture tool, built native on Linux.")
            .child(
                widgets::section_card(t).mt(px(32.)).child(
                    widgets::card_row(t, true)
                        .child(img("app-icon.png").flex_none().size(px(40.)))
                        .child(widgets::row_text(t, "SlopShot", Some(format!("Version {}", env!("CARGO_PKG_VERSION")).into())))
                        .child(widgets::badge(t, "Linux")),
                ),
            )
            .into_any_element()
    }
}

const SIDEBAR_WIDTH: f32 = 256.;

/// The centered page column: title, subtitle, then sections.
fn page(title: &'static str, subtitle: &'static str) -> gpui::Div {
    let t = Theme::get();
    widgets::page_column().child(widgets::page_header(t, title, None)).child(widgets::page_subtitle(t, subtitle))
}

/// A select bound to one setting.
#[allow(clippy::too_many_arguments)]
fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    all: &'static [T],
    label: fn(T) -> &'static str,
    current: T,
    set: fn(&mut Settings, T),
    state: &SelectState,
    reach: fn(&mut Prefs) -> &mut SelectState,
    width: f32,
    disabled: bool,
    window: &Window,
    cx: &mut Context<Prefs>,
) -> gpui::Div {
    let select = widgets::select(id, Theme::get(), reach)
        .options(all.iter().map(|v| SelectOption::new(label(*v))), all.iter().position(|v| *v == current).unwrap_or(0))
        .width(width)
        .on_select(move |_, ix, _, cx| Settings::update(cx, |s| set(s, all[ix])))
        .render(state, window, cx);
    div().relative().flex_none().child(select).when(disabled, |el| el.child(div().absolute().inset_0().occlude()))
}

fn switch(id: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex_none().cursor_pointer().child(widgets::toggle_switch(Theme::get(), on, id))
}

fn text_action(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    widgets::text_action(Theme::get(), ActionTone::Filled, label).id(id)
}

fn toggle_row(
    first: bool,
    id: &'static str,
    title: &'static str,
    description: Option<&'static str>,
    on: bool,
    set: fn(&mut Settings, bool),
) -> gpui::Div {
    let t = Theme::get();
    widgets::card_row(t, first)
        .child(widgets::row_text(t, title, description.map(Into::into)))
        .child(switch(id, on).on_click(move |_, _, cx| Settings::update(cx, |s| set(s, !on))))
}

fn beep() {
    let _ = std::process::Command::new("canberra-gtk-play").args(["-i", "bell"]).spawn();
}

impl Render for Prefs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        motion::window_frame(window);
        let t = Theme::get();
        let body = match self.tab {
            Tab::General => self.general(window, cx),
            Tab::Capture => self.capture(window, cx),
            Tab::Privacy => self.privacy(cx),
            Tab::Shortcuts => self.shortcuts(cx),
            Tab::About => self.about(),
        };
        let content = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .flex()
            .text_size(px(13.))
            .child(self.sidebar(t, window, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    // Pages scroll to the window's top edge; the column carries the
                    // titlebar clearance inside the scroll.
                    .child(div().id("prefs-body").size_full().overflow_y_scroll().child(body))
                    .child(
                        chrome::drag_strip("prefs-titlebar")
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(Theme::TITLEBAR_HEIGHT))
                            .flex()
                            .justify_end()
                            .items_center()
                            .px(px(8.))
                            .child(ui::close_button()),
                    ),
            );
        chrome::frame(window, true, t.surface, content)
    }
}
