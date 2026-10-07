use anyhow::Context as _;
use ashpd::desktop::screenshot::Screenshot;
use ashpd::zbus;
use fancy_regex::Regex;
use image::RgbaImage;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub enum Failure {
    /// The portal said no and GNOME has a Deny stored for SlopShot: the user pressed Deny
    /// in "Allow … to Take Screenshots?", now or on an earlier capture.
    Refused,
    /// The portal said no with nothing stored: that prompt was closed without an answer,
    /// or went unanswered for the 25 s the portal waits on it.
    Unanswered,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(err: anyhow::Error) -> Self {
        Failure::Other(err)
    }
}

/// Set by the first capture the portal lets through, after which the stored answer no
/// longer needs checking: should `portal_app_id` ever disagree with the portal's, the check
/// would otherwise keep asking for a permission that is already granted.
static PORTAL_ALLOWED: AtomicBool = AtomicBool::new(false);

/// `screenshot`, failing at once while GNOME has no answer stored rather than after the
/// 25 s the portal waits on a prompt GNOME Shell won't show a tray app (see `ask_permission`).
pub async fn screenshot_if_asked() -> Result<RgbaImage, Failure> {
    if !PORTAL_ALLOWED.load(Ordering::Relaxed) && stored_permission().await?.is_none() {
        return Err(Failure::Unanswered);
    }
    screenshot().await
}

/// Captures every monitor through xdg-desktop-portal, the only path that works on
/// Wayland. The portal writes a PNG and hands back its URI; we read it and delete it
/// because the user never asked for that file.
pub async fn screenshot() -> Result<RgbaImage, Failure> {
    let request = Screenshot::request()
        .interactive(false)
        .modal(false)
        .send()
        .await
        .context("screenshot portal request")?;
    let response = match request.response() {
        Ok(response) => response,
        // A stored Deny, a dismissed prompt and a timed-out one all come back as the same
        // response code, so the permission store tells them apart.
        Err(ashpd::Error::Response(err)) => {
            log::debug!("screenshot portal said no: {err}");
            return Err(match stored_permission().await {
                Ok(Some(false)) => Failure::Refused,
                Ok(None) => Failure::Unanswered,
                Ok(Some(true)) => anyhow::anyhow!("the screenshot portal failed although SlopShot is allowed to take screenshots").into(),
                Err(err) => err.context("screenshot portal refused; reading the stored permission").into(),
            });
        }
        Err(err) => return Err(anyhow::Error::from(err).context("screenshot portal").into()),
    };
    PORTAL_ALLOWED.store(true, Ordering::Relaxed);
    let uri = response.uri().as_str();
    let path = url::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .with_context(|| format!("portal returned a non-file URI: {uri}"))?;
    let image = image::open(&path)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgba8();
    if let Err(err) = std::fs::remove_file(&path) {
        log::warn!("removing portal screenshot {}: {err}", path.display());
    }
    Ok(image)
}

/// The app ID xdg-desktop-portal files SlopShot's permissions under. A host app gets
/// one from the systemd unit the desktop launched it in (`app-gnome-com.thanglb.slopshot@1234.service`
/// from the app grid or autostart), and "" when started any other way, e.g. from a
/// terminal: two separate entries, each with its own answer.
fn portal_app_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let cgroup = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
        user_unit(&cgroup).map(app_id_from_unit).unwrap_or_default()
    })
}

/// What `sd_pid_get_user_unit` returns: the first non-slice unit under the user manager.
fn user_unit(cgroup: &str) -> Option<&str> {
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    let (_, below_manager) = path.split_once(".service/")?;
    below_manager.split('/').find(|unit| !unit.ends_with(".slice"))
}

/// xdg-desktop-portal 1.18's `_xdp_parse_app_id_from_unit_name`, regexes verbatim.
fn app_id_from_unit(unit: &str) -> String {
    static PATTERNS: OnceLock<[Regex; 2]> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            Regex::new(r"^app-(?:[[:alnum:]]+\-)?(.+?)(?:\-[[:alnum:]]*)(?:\.scope|\.slice)$").unwrap(),
            Regex::new(r"^app-(?:[[:alnum:]]+\-)?(.+?)(?:@[[:alnum:]]*|\-autostart)?\.service$").unwrap(),
        ]
    });
    patterns
        .iter()
        .find_map(|re| re.captures(unit).ok().flatten())
        .and_then(|c| unescape_unit(c.get(1)?.as_str()))
        .unwrap_or_default()
}

/// Undoes systemd's `\x2d`-style escaping of characters a unit name can't hold.
fn unescape_unit(escaped: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(escaped.len());
    let mut rest = escaped.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'\\' && tail.first() == Some(&b'x') {
            let hex = std::str::from_utf8(tail.get(1..3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[3..];
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// GNOME's stored answer to the screenshot prompt for this process: `None` until it was asked.
async fn stored_permission() -> anyhow::Result<Option<bool>> {
    let store = permission_store().await?;
    let stored: Result<Vec<String>, _> = store.call("GetPermission", &("screenshot", "screenshot", portal_app_id())).await;
    Ok(stored.ok().and_then(|p| p.first().map(|answer| answer == "yes")))
}

/// Deletes a stored "Deny" so the portal asks again on the next capture.
pub async fn forget_refusal() -> anyhow::Result<()> {
    if stored_permission().await? == Some(false) {
        permission_store()
            .await?
            .call::<_, _, ()>("DeletePermission", &("screenshot", "screenshot", portal_app_id()))
            .await
            .with_context(|| format!("deleting the stored screenshot refusal for {:?}", portal_app_id()))?;
    }
    Ok(())
}

async fn permission_store() -> anyhow::Result<zbus::Proxy<'static>> {
    let connection = zbus::Connection::session().await?;
    Ok(zbus::Proxy::new(
        &connection,
        "org.freedesktop.impl.portal.PermissionStore",
        "/org/freedesktop/impl/portal/PermissionStore",
        "org.freedesktop.impl.portal.PermissionStore",
    )
    .await?)
}

pub async fn permission_granted() -> Option<bool> {
    stored_permission().await.ok().flatten()
}

/// Like the macOS app asking for Screen Recording at launch: asks for the screenshot
/// permission now rather than on the first hotkey.
pub fn request_permission_early(cx: &mut gpui::App) {
    cx.spawn(async move |cx| {
        if stored_permission().await.is_ok_and(|p| p.is_none()) {
            cx.update(|cx| ask_permission(false, None, cx));
        }
    })
    .detach();
}

/// Gets GNOME's "Allow SlopShot to take screenshots?" prompt on screen, then runs `retry`
/// if the answer was Allow. GNOME Shell only shows that prompt for the app that has focus
/// ("Only the focused app is allowed to show a system access dialog") and a tray app has
/// none, so the request goes out from behind this alert of ours; a request without it
/// waits 25 s for a prompt that never appears.
pub fn ask_permission(refused: bool, retry: Option<crate::ipc::Command>, cx: &mut gpui::App) {
    let alert = if refused {
        // GNOME stores a Deny forever and never asks again, so without this a single
        // mis-click leaves SlopShot unable to capture with no explanation.
        crate::alert::Alert {
            title: "SlopShot needs permission to take screenshots".into(),
            message: "GNOME is blocking screen capture for SlopShot because the screenshot prompt was answered with Deny, and it remembers that answer.\n\nChoose Ask Again, then Allow in the prompt that follows.".into(),
            buttons: vec!["Ask Again", "Later"],
        }
    } else {
        crate::alert::Alert {
            title: "SlopShot needs permission to take screenshots".into(),
            message: "GNOME asks once whether SlopShot may read the screen, which every capture needs.\n\nChoose Continue, then Allow in the prompt that follows.".into(),
            buttons: vec!["Continue", "Later"],
        }
    };
    crate::alert::show_holding(
        alert,
        move |choice, cx| {
            if choice != 0 {
                return None;
            }
            Some(cx.spawn(async move |_| {
                let asked = async {
                    if refused {
                        forget_refusal().await?;
                    }
                    // The picture is of this alert; only the answer matters.
                    match screenshot().await {
                        Ok(_) => Ok(true),
                        Err(Failure::Other(err)) => Err(err),
                        Err(Failure::Refused | Failure::Unanswered) => Ok(false),
                    }
                };
                let result: anyhow::Result<bool> = asked.await;
                let then: crate::alert::Then = Box::new(move |cx| match result {
                    Ok(true) => retry.into_iter().for_each(|command| crate::run(command, cx)),
                    Ok(false) => {}
                    Err(err) => crate::alert::error("Capture failed", &format!("{err:#}"), cx),
                });
                then
            }))
        },
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_matches_the_portal() {
        // xdg-desktop-portal 1.18.4 tests/test-xdp-utils.c, plus what GNOME names SlopShot's units.
        for (unit, id) in [
            ("app-not-a-well-formed-unit-name", ""),
            (r"app-gnome-org.gnome.Evolution\x2dalarm\x2dnotify-2437.scope", "org.gnome.Evolution-alarm-notify"),
            ("app-gnome-org.gnome.Epiphany-182352.scope", "org.gnome.Epiphany"),
            (r"app-glib-spice\x2dvdagent-1839.scope", "spice-vdagent"),
            ("app-KDE-org.kde.okular@12345.service", "org.kde.okular"),
            ("app-firefox.service", "firefox"),
            ("app-org.kde.amarok.service", "org.kde.amarok"),
            ("app-gnome-org.gnome.SettingsDaemon.DiskUtilityNotify-autostart.service", "org.gnome.SettingsDaemon.DiskUtilityNotify"),
            ("app-gnome-org.gnome.Terminal-92502.slice", "org.gnome.Terminal"),
            ("app-com.obsproject.Studio-d70acc38b5154a3a8b4a60accc4b15f4.scope", "com.obsproject.Studio"),
            ("app-firefox-jcfppqx.scope", "firefox"),
            ("app-gnome-firefox.service", "firefox"),
            ("app-gnome-com.thanglb.slopshot-4242.scope", "com.thanglb.slopshot"),
            ("app-gnome-com.thanglb.slopshot@autostart.service", "com.thanglb.slopshot"),
        ] {
            assert_eq!(app_id_from_unit(unit), id, "{unit}");
        }
    }

    #[test]
    fn user_unit_skips_slices() {
        let launched = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-app\\x2dgnome\\x2dcom.thanglb.slopshot.slice/app-gnome-com.thanglb.slopshot@4242.service\n";
        assert_eq!(user_unit(launched), Some("app-gnome-com.thanglb.slopshot@4242.service"));
        let terminal = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.gnome.Terminal.slice/vte-spawn-3b6e.scope\n";
        assert_eq!(user_unit(terminal).map(app_id_from_unit), Some(String::new()));
        assert_eq!(user_unit("0::/system.slice/ssh.service\n"), None);
    }
}
