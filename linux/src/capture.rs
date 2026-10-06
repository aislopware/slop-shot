use anyhow::Context as _;
use ashpd::desktop::screenshot::Screenshot;
use ashpd::zbus;
use image::RgbaImage;

pub enum Failure {
    /// The portal said no: the user pressed Deny in GNOME's "Allow … to Take
    /// Screenshots?" prompt, now or on an earlier capture (the answer is stored).
    Refused,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(err: anyhow::Error) -> Self {
        Failure::Other(err)
    }
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
        Err(ashpd::Error::Response(_)) => return Err(Failure::Refused),
        Err(err) => return Err(anyhow::Error::from(err).context("screenshot portal").into()),
    };
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

/// Deletes a stored "Deny" so the portal asks again on the next capture. A host app is
/// recorded under its systemd scope's app ID, or under "" when launched outside one.
pub async fn forget_refusal() -> anyhow::Result<()> {
    let store = permission_store().await?;
    for app in ["", crate::APP_ID] {
        let stored: Result<Vec<String>, _> = store.call("GetPermission", &("screenshot", "screenshot", app)).await;
        if stored.is_ok_and(|p| p == ["no"]) {
            store
                .call::<_, _, ()>("DeletePermission", &("screenshot", "screenshot", app))
                .await
                .with_context(|| format!("deleting the stored screenshot refusal for {app:?}"))?;
        }
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

/// GNOME's stored answer to the screenshot prompt: `None` until it was asked.
pub async fn permission_granted() -> Option<bool> {
    let store = permission_store().await.ok()?;
    for app in ["", crate::APP_ID] {
        let stored: Result<Vec<String>, _> = store.call("GetPermission", &("screenshot", "screenshot", app)).await;
        if let Ok(p) = stored
            && let Some(answer) = p.first()
        {
            return Some(answer == "yes");
        }
    }
    None
}

/// Whether GNOME has stored any answer to "Allow SlopShot to take screenshots?".
async fn permission_decided() -> anyhow::Result<bool> {
    let store = permission_store().await?;
    for app in ["", crate::APP_ID] {
        let stored: Result<Vec<String>, _> = store.call("GetPermission", &("screenshot", "screenshot", app)).await;
        if stored.is_ok_and(|p| !p.is_empty()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Like the macOS app asking for Screen Recording at launch: brings up GNOME's
/// permission prompt now rather than on the first hotkey.
pub fn request_permission_early(cx: &mut gpui::App) {
    cx.spawn(async move |_| {
        if permission_decided().await.unwrap_or(true) {
            return;
        }
        if let Err(Failure::Other(err)) = screenshot().await {
            log::warn!("asking for the screenshot permission: {err:#}");
        }
    })
    .detach();
}

/// GNOME stores a Deny forever and never asks again, so without this a single
/// mis-click leaves SlopShot unable to capture with no explanation.
pub fn explain_refusal(retry: crate::ipc::Command, cx: &mut gpui::App) {
    crate::alert::show(
        crate::alert::Alert {
            title: "SlopShot needs permission to take screenshots".into(),
            message: "GNOME is blocking screen capture for SlopShot because the screenshot prompt was answered with Deny, and it remembers that answer.\n\nChoose Ask Again, then Allow in the prompt that follows.".into(),
            buttons: vec!["Ask Again", "Later"],
        },
        move |choice, cx| {
            if choice != 0 {
                return;
            }
            cx.spawn(async move |cx| {
                let result = forget_refusal().await;
                cx.update(|cx| match result {
                    Ok(()) => crate::run(retry, cx),
                    Err(err) => crate::alert::error("Capture failed", &format!("{err:#}"), cx),
                });
            })
            .detach();
        },
        cx,
    );
}
