//! Global hotkeys. GNOME 46 (Ubuntu 24.04) has no GlobalShortcuts portal, and Wayland
//! gives apps no way to grab keys themselves, so SlopShot registers GNOME custom
//! shortcuts that run `slopshot <command>`; the running instance picks it up over IPC.
//! They show up, and can be rebound, in Settings → Keyboard → Custom Shortcuts.

use std::process::Command;

use anyhow::{Context as _, bail};

pub struct Shortcut {
    pub id: &'static str,
    pub name: &'static str,
    pub arg: &'static str,
    /// Mirrors ⌘⇧1…6 on macOS; Shift+Super+digit is taken by the Ubuntu dock.
    pub binding: &'static str,
}

pub const SHORTCUTS: [Shortcut; 6] = [
    Shortcut { id: "area", name: "SlopShot: Capture Area", arg: "area", binding: "<Control><Alt>1" },
    Shortcut { id: "full", name: "SlopShot: Capture Fullscreen", arg: "full", binding: "<Control><Alt>2" },
    Shortcut { id: "scroll", name: "SlopShot: Capture Scrolling Area", arg: "scroll", binding: "<Control><Alt>3" },
    Shortcut { id: "text", name: "SlopShot: Capture Text (OCR)", arg: "text", binding: "<Control><Alt>4" },
    Shortcut { id: "color", name: "SlopShot: Pick Color", arg: "color", binding: "<Control><Alt>5" },
    Shortcut { id: "record", name: "SlopShot: Record Area", arg: "record", binding: "<Control><Alt>6" },
];

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const LIST_KEY: &str = "custom-keybindings";
const ENTRY_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

fn entry_path(id: &str) -> String {
    format!("/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/slopshot-{id}/")
}

pub fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.split(':').any(|part| part.eq_ignore_ascii_case("GNOME")))
        .unwrap_or(false)
}

fn gsettings(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("gsettings").args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("gsettings {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn gvariant_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Parses gsettings' printing of an `as`: `@as []` or `['/a/', '/b/']`.
fn parse_string_array(text: &str) -> Vec<String> {
    text.split('\'').skip(1).step_by(2).map(str::to_owned).collect()
}

fn command_for(arg: &str) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "slopshot".into());
    // GNOME splits the command with g_shell_parse_argv.
    if exe.contains([' ', '\'', '"']) {
        format!("'{}' {arg}", exe.replace('\'', "'\\''"))
    } else {
        format!("{exe} {arg}")
    }
}

/// Registers the shortcuts in `ids`, leaving the others as the user has them.
pub fn install(ids: &[&str]) -> anyhow::Result<()> {
    let mut list = parse_string_array(&gsettings(&["get", SCHEMA, LIST_KEY])?);
    for shortcut in SHORTCUTS.iter().filter(|s| ids.contains(&s.id)) {
        let path = entry_path(shortcut.id);
        let entry = format!("{ENTRY_SCHEMA}:{path}");
        gsettings(&["set", &entry, "name", &gvariant_str(shortcut.name)])?;
        gsettings(&["set", &entry, "command", &gvariant_str(&command_for(shortcut.arg))])?;
        gsettings(&["set", &entry, "binding", &gvariant_str(shortcut.binding)])?;
        if !list.contains(&path) {
            list.push(path);
        }
    }
    let value = format!("[{}]", list.iter().map(|p| gvariant_str(p)).collect::<Vec<_>>().join(", "));
    gsettings(&["set", SCHEMA, LIST_KEY, &value])?;
    Ok(())
}

/// The binding currently registered for `id`, in GTK accelerator syntax, if any.
pub fn current_binding(id: &str) -> Option<String> {
    let path = entry_path(id);
    let list = parse_string_array(&gsettings(&["get", SCHEMA, LIST_KEY]).ok()?);
    if !list.contains(&path) {
        return None;
    }
    let raw = gsettings(&["get", &format!("{ENTRY_SCHEMA}:{path}"), "binding"]).ok()?;
    parse_string_array(&raw).into_iter().next().filter(|b| !b.is_empty())
}

/// `<Control><Alt>1` → `Ctrl+Alt+1`.
pub fn display_binding(binding: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = binding;
    while let Some(start) = rest.strip_prefix('<') {
        let Some(end) = start.find('>') else { break };
        parts.push(match &start[..end] {
            "Control" | "Primary" | "Ctrl" => "Ctrl".to_owned(),
            "Super" => "Super".to_owned(),
            other => other.to_owned(),
        });
        rest = &start[end + 1..];
    }
    parts.push(rest.to_uppercase());
    parts.join("+")
}

/// Registers `binding` for one action, adding its entry if it was removed.
pub fn set_binding(id: &str, binding: &str) -> anyhow::Result<()> {
    let Some(shortcut) = SHORTCUTS.iter().find(|s| s.id == id) else { bail!("unknown shortcut {id}") };
    let path = entry_path(id);
    let entry = format!("{ENTRY_SCHEMA}:{path}");
    gsettings(&["set", &entry, "name", &gvariant_str(shortcut.name)])?;
    gsettings(&["set", &entry, "command", &gvariant_str(&command_for(shortcut.arg))])?;
    gsettings(&["set", &entry, "binding", &gvariant_str(binding)])?;
    let mut list = parse_string_array(&gsettings(&["get", SCHEMA, LIST_KEY])?);
    if !list.contains(&path) {
        list.push(path);
        let value = format!("[{}]", list.iter().map(|p| gvariant_str(p)).collect::<Vec<_>>().join(", "));
        gsettings(&["set", SCHEMA, LIST_KEY, &value])?;
    }
    Ok(())
}

pub fn reset(id: &str) -> anyhow::Result<()> {
    let Some(shortcut) = SHORTCUTS.iter().find(|s| s.id == id) else { bail!("unknown shortcut {id}") };
    set_binding(id, shortcut.binding)
}

/// A GPUI keystroke as a GTK accelerator; `None` without a modifier, which macOS
/// rejects too.
pub fn accelerator(keystroke: &gpui::Keystroke) -> Option<String> {
    let m = &keystroke.modifiers;
    if !(m.control || m.alt || m.shift || m.platform) {
        return None;
    }
    let mut out = String::new();
    for (on, name) in [(m.control, "<Control>"), (m.alt, "<Alt>"), (m.shift, "<Shift>"), (m.platform, "<Super>")] {
        if on {
            out.push_str(name);
        }
    }
    let key = keystroke.key.as_str();
    let gtk = match key {
        "enter" => "Return".to_owned(),
        "space" => "space".to_owned(),
        "tab" => "Tab".to_owned(),
        "backspace" => "BackSpace".to_owned(),
        "delete" => "Delete".to_owned(),
        "left" => "Left".to_owned(),
        "right" => "Right".to_owned(),
        "up" => "Up".to_owned(),
        "down" => "Down".to_owned(),
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "pageup" => "Page_Up".to_owned(),
        "pagedown" => "Page_Down".to_owned(),
        k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok() => k.to_uppercase(),
        k => k.to_owned(),
    };
    out.push_str(&gtk);
    Some(out)
}
