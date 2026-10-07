//! Settings window (SettingsWindowController.swift): General, Capture, Privacy,
//! Shortcuts and About tabs; every change applies and persists immediately.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Hsla,
    KeyDownEvent, PathPromptOptions, SharedString, Subscription, Window,
    div, img, prelude::*, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::searchable_list::SearchableVec;
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, Icon, IndexPath, Sizable as _};
use gpui_kit_assets::IconName;

use crate::settings::{ColorFormat, ImageFormat, PreviewSide, Settings, config_dir, display_path};
use crate::{capture, shortcuts};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    General,
    Capture,
    Privacy,
    Shortcuts,
    About,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::General, Tab::Capture, Tab::Privacy, Tab::Shortcuts, Tab::About];

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
    let options = crate::chrome::window_options("Settings", (640., 400.), None, cx);
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

struct Prefs {
    tab: Tab,
    focus: FocusHandle,
    image_format: Entity<Choice>,
    preview_side: Entity<Choice>,
    color_format: Entity<Choice>,
    /// The shortcut whose recorder is listening.
    recording: Option<&'static str>,
    shortcut_error: Option<SharedString>,
    permission: Option<bool>,
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
    fn new(tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
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

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent: Hsla = gpui::rgb(0x0A84FF).into();
        div()
            .flex()
            .justify_center()
            .gap_1()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(Tab::ALL.into_iter().map(|tab| {
                let on = self.tab == tab;
                let fg = if on { accent } else { cx.theme().muted_foreground };
                div()
                    .id(tab.title())
                    .w(px(72.))
                    .py_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .rounded(px(6.))
                    .when(on, |d| d.bg(cx.theme().secondary))
                    .hover(|d| d.bg(cx.theme().secondary_hover))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        p.tab = tab;
                        p.recording = None;
                        cx.notify();
                    }))
                    .child(Icon::new(tab.icon()).size(px(20.)).text_color(fg))
                    .child(div().text_size(px(11.)).text_color(fg).child(tab.title()))
            }))
    }

    fn general(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = Settings::get(cx).clone();
        let muted = cx.theme().muted_foreground;
        div()
            .flex()
            .flex_col()
            .child(section(
                "Startup",
                vec![
                    row(cx).child(
                        Switch::new("autostart").checked(autostart_path().exists()).label("Launch SlopShot at login").on_click(
                            |checked, _, cx| {
                                if let Err(err) = set_autostart(*checked) {
                                    log::error!("autostart: {err}");
                                }
                                cx.refresh_windows();
                            },
                        ),
                    ),
                    row(cx).child(Switch::new("tray").checked(s.show_tray_icon).label("Show icon in the top bar").on_click(
                        |checked, _, cx| {
                            let on = *checked;
                            Settings::update(cx, |s| s.show_tray_icon = on);
                            crate::tray::sync(cx);
                        },
                    )),
                ],
                (!s.show_tray_icon).then_some("Hotkeys keep working. To get back here, open SlopShot again from the app grid."),
                cx,
            ))
            .child(section(
                "Save location",
                vec![row(cx)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(Icon::new(IconName::Folder).size(px(16.)).text_color(muted))
                            .child(div().truncate().child(display_path(&s.save_folder))),
                    )
                    .child(Button::new("folder").small().label("Choose…").on_click(move |_, _, cx| {
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
                    }))],
                Some("Captures are kept temporarily until you click Save on the preview — then they're written to the folder above."),
                cx,
            ))
            .child(section(
                "Image format",
                vec![row(cx).child("Format").child(Select::new(&self.image_format).small().w(px(160.)))],
                None,
                cx,
            ))
            .into_any_element()
    }

    fn capture(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = Settings::get(cx).clone();
        div()
            .flex()
            .flex_col()
            .child(section(
                "Selection",
                vec![row(cx).child(toggle("snap", "Snap to window & item edges", s.snap, |s, v| s.snap = v))],
                Some("Hover to outline the item under the cursor, then click to grab it. Hold Alt while dragging for a free selection."),
                cx,
            ))
            .child(section(
                "After capture",
                vec![
                    row(cx).child(
                        toggle("inline", "Annotate right after selecting an area", s.edit_after_select, |s, v| {
                            s.edit_after_select = v
                        })
                        .tooltip("Drawing tools appear on the selection; Ctrl+C copies, Ctrl+S saves, esc cancels."),
                    ),
                    row(cx).child(toggle("copy", "Also copy to clipboard", s.copy_to_clipboard, |s, v| s.copy_to_clipboard = v)),
                    row(cx).child(toggle("sound", "Play a sound", s.play_sound, |s, v| s.play_sound = v)),
                    row(cx).child(toggle("thumb", "Show preview thumbnail", s.show_thumbnail, |s, v| s.show_thumbnail = v)),
                    row(cx)
                        .child(div().when(!s.show_thumbnail, |d| d.opacity(0.5)).child("Preview position"))
                        .child(Select::new(&self.preview_side).small().w(px(160.)).disabled(!s.show_thumbnail)),
                ],
                Some("Right-handed? Put the preview on the right, closer to where your mouse already is."),
                cx,
            ))
            .child(section(
                "Recording",
                vec![
                    row(cx).child(toggle("sys-audio", "Record system audio", s.record_system_audio, |s, v| s.record_system_audio = v)),
                    row(cx).child(toggle("mic", "Record microphone", s.record_microphone, |s, v| s.record_microphone = v)),
                ],
                Some("System audio is what your computer plays (apps, videos, calls). The microphone is the default input in Sound settings."),
                cx,
            ))
            .child(section(
                "Color picker",
                vec![row(cx).child("Copy color as").child(Select::new(&self.color_format).small().w(px(180.)))],
                Some("Press ← → while picking to switch format without leaving the screen."),
                cx,
            ))
            .into_any_element()
    }

    fn privacy(&self, cx: &mut Context<Self>) -> AnyElement {
        let (icon, text, color): (IconName, &str, Hsla) = match self.permission {
            Some(true) => (IconName::CircleCheck, "Granted", gpui::rgb(0x30D158).into()),
            Some(false) => (IconName::TriangleAlert, "Not granted", gpui::rgb(0xFF9F0A).into()),
            None => (IconName::CircleQuestionMark, "Not asked yet", cx.theme().muted_foreground),
        };
        let muted = cx.theme().muted_foreground;
        let s = Settings::get(cx).clone();
        div()
            .flex()
            .flex_col()
            .child(section(
                "Permissions",
                vec![row(cx)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child("Screenshots")
                            .child(div().text_xs().text_color(muted).child("Every capture. Without it GNOME blocks SlopShot from reading the screen.")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .child(Icon::new(icon).size(px(14.)).text_color(color))
                            .child(div().text_sm().text_color(color).child(text))
                            .when(self.permission != Some(true), |d| {
                                let refused = self.permission == Some(false);
                                let label = if refused { "Ask Again" } else { "Ask Now" };
                                d.child(Button::new("ask").small().label(label).on_click(move |_, _, cx| capture::ask_permission(refused, None, cx)))
                            }),
                    )],
                None,
                cx,
            ))
            .child(section(
                "Sensitive data",
                vec![row(cx).child(toggle("redact", "Scan captures for sensitive data", s.redact_scan_on_open, |s, v| s.redact_scan_on_open = v))],
                Some("The editor counts emails, phone numbers, card numbers with their CVV, expiry and cardholder name, API tokens, labelled passwords and ID numbers it can see, and offers to cover them. Nothing is covered until you press Redact."),
                cx,
            ))
            .child(section(
                "Search",
                vec![row(cx).child(toggle("index", "Index text inside screenshots", s.index_capture_text, |s, v| s.index_capture_text = v))],
                Some("Reads each screenshot in the background so History can be searched by what's written in the image."),
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_3()
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(IconName::Lock).size(px(12.)).text_color(muted))
                    .child("Both run on-device with Tesseract. No image, and no text read out of one, ever leaves this computer."),
            )
            .into_any_element()
    }

    fn shortcuts(&self, cx: &mut Context<Self>) -> AnyElement {
        let accent: Hsla = gpui::rgb(0x0A84FF).into();
        let rows = shortcuts::SHORTCUTS
            .iter()
            .map(|sc| {
                let listening = self.recording == Some(sc.id);
                let binding = if listening {
                    "Type shortcut…".to_owned()
                } else {
                    shortcuts::current_binding(sc.id).map(|b| shortcuts::display_binding(&b)).unwrap_or_else(|| "—".into())
                };
                let id = sc.id;
                row(cx)
                    .child(sc.name.trim_start_matches("SlopShot: ").to_owned())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .id(SharedString::from(format!("rec-{id}")))
                                    .w(px(110.))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.))
                                    .border_1()
                                    .text_size(px(12.))
                                    .map(|d| {
                                        if listening {
                                            d.bg(accent.opacity(0.18)).border_color(accent).text_color(accent)
                                        } else {
                                            d.bg(cx.theme().secondary).border_color(cx.theme().border)
                                        }
                                    })
                                    .on_click(cx.listener(move |p, _, window, cx| {
                                        p.recording = Some(id);
                                        window.focus(&p.focus, cx);
                                        cx.notify();
                                    }))
                                    .child(binding),
                            )
                            .child(
                                Button::new(SharedString::from(format!("reset-{id}")))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Undo2)
                                    .tooltip("Reset to default")
                                    .on_click(cx.listener(move |p, _, _, cx| {
                                        p.shortcut_error = shortcuts::reset(id).err().map(|e| e.to_string().into());
                                        p.recording = None;
                                        cx.notify();
                                    })),
                            ),
                    )
            })
            .collect();
        let caption = if shortcuts::is_gnome() {
            "Click a shortcut, then press the new key combo (needs at least one of Ctrl, Alt, Shift, Super). Press Esc to cancel."
        } else {
            "Bind these commands in your desktop's keyboard settings: slopshot area, slopshot full, slopshot color."
        };
        div()
            .flex()
            .flex_col()
            .child(section("Global shortcuts", rows, Some(caption), cx))
            .children(self.shortcut_error.clone().map(|e| div().pt_2().text_sm().text_color(cx.theme().danger).child(e)))
            .into_any_element()
    }

    fn about(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .child(img("app-icon.png").size(px(84.)))
            .child(div().pt_2().text_xl().font_weight(gpui::FontWeight::BOLD).child("SlopShot"))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("Version {}", env!("CARGO_PKG_VERSION"))))
            .child(div().pt_1().text_sm().child("A CleanShot-style capture tool, built native on Linux."))
            .into_any_element()
    }
}

/// Title and a close button, the only window control macOS shows here.
fn toggle(id: &'static str, label: &'static str, checked: bool, set: fn(&mut Settings, bool)) -> Switch {
    Switch::new(id).checked(checked).label(label).on_click(move |checked, _, cx| {
        let v = *checked;
        Settings::update(cx, |s| set(s, v))
    })
}

fn beep() {
    let _ = std::process::Command::new("canberra-gtk-play").args(["-i", "bell"]).spawn();
}

fn row(cx: &App) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .px_3()
        .min_h(px(36.))
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border.opacity(0.5))
}

/// A macOS grouped-form section: header, rounded box of rows, caption.
fn section(title: &'static str, rows: Vec<gpui::Div>, caption: Option<&'static str>, cx: &App) -> gpui::Div {
    let n = rows.len();
    div()
        .flex()
        .flex_col()
        .pb_3()
        .child(div().px_1().pb_1().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(title))
        .child(
            div()
                .rounded(px(8.))
                .bg(cx.theme().secondary.opacity(0.5))
                .border_1()
                .border_color(cx.theme().border.opacity(0.6))
                .children(rows.into_iter().enumerate().map(move |(i, r)| if i + 1 == n { r.border_b_0() } else { r })),
        )
        .children(caption.map(|c| div().px_1().pt_1().text_xs().text_color(cx.theme().muted_foreground).child(c)))
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
                .flex_col()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .text_sm()
                .child(crate::chrome::title_bar("Settings", cx))
                .child(self.tab_bar(cx))
                .child(div().id("prefs-body").flex_1().min_h_0().overflow_y_scroll().px_6().py_4().child(body)),
        )
    }
}
