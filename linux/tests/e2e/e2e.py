#!/usr/bin/env python3
"""End-to-end test of SlopShot against a real headless GNOME Shell Wayland session.

Input goes through Mutter's RemoteDesktop API (the path GNOME Remote Desktop uses), and
reference screenshots through the same xdg-desktop-portal SlopShot captures with.

    tests/e2e/run.sh            # builds, then runs this inside tests/e2e/session.sh
"""

import glob
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

import dbus
import numpy as np
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)
BUS = dbus.SessionBus()
BIN = os.environ["SLOPSHOT_BIN"]
EVIDENCE = pathlib.Path(os.environ.get("SLOPSHOT_EVIDENCE", "/tmp/slopshot-evidence"))
HOME = pathlib.Path(os.environ["HOME"])
SETTINGS = HOME / ".config" / "slopshot" / "settings.json"
TEMP_DIR = pathlib.Path(f"/tmp/slopshot-{os.getuid()}")

BTN_LEFT = 272
KEY = {"ctrl": 0xFFE3, "s": 0x73, "f": 0x66, "escape": 0xFF1B, "return": 0xFF0D, "tab": 0xFF09, "right": 0xFF53,
       "left": 0xFF51, "shift": 0xFFE1, "space": 0x20, "delete": 0xFFFF, "backspace": 0xFF08}
NOTIFY_LOG = EVIDENCE / "notifications.jsonl"

failures = []


def log(msg):
    print(f"[e2e] {msg}", flush=True)


def check(cond, what):
    log(("PASS " if cond else "FAIL ") + what)
    if not cond:
        failures.append(what)
    return cond


def wait_for(pred, timeout=10.0, step=0.1):
    end = time.time() + timeout
    while time.time() < end:
        value = pred()
        if value:
            return value
        time.sleep(step)
    return None


def load_png(path):
    """Decodes with ImageMagick to raw RGBA; the VM has no Pillow."""
    out = subprocess.run(["convert", str(path), "-depth", "8", "rgba:-"], capture_output=True, check=True).stdout
    w, h = map(int, subprocess.run(["identify", "-format", "%w %h", str(path)], capture_output=True,
                                   check=True, text=True).stdout.split())
    return np.frombuffer(out, np.uint8).reshape(h, w, 4)


def png_from_bytes(data, name):
    path = EVIDENCE / name
    path.write_bytes(data)
    return load_png(path)


# ── Portal screenshot (reference + evidence) ────────────────────────────────

def portal_screenshot(name):
    portal = BUS.get_object("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop")
    token = f"e2e{int(time.time() * 1000) % 10_000_000}"
    sender = BUS.get_unique_name()[1:].replace(".", "_")
    request_path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    loop = GLib.MainLoop()
    result = {}

    def on_response(code, results):
        result["code"], result["uri"] = int(code), str(results.get("uri", ""))
        loop.quit()

    BUS.add_signal_receiver(on_response, "Response", "org.freedesktop.portal.Request", path=request_path)
    portal.Screenshot("", {"handle_token": token, "interactive": False},
                      dbus_interface="org.freedesktop.portal.Screenshot")
    GLib.timeout_add_seconds(10, loop.quit)
    loop.run()
    assert result.get("code") == 0, f"portal screenshot failed: {result}"
    src = pathlib.Path(result["uri"].removeprefix("file://"))
    dst = EVIDENCE / f"{name}.png"
    shutil.move(src, dst)
    return load_png(dst)


# ── Input through Mutter RemoteDesktop ──────────────────────────────────────

class Input:
    def __init__(self):
        rd = BUS.get_object("org.gnome.Mutter.RemoteDesktop", "/org/gnome/Mutter/RemoteDesktop")
        path = rd.CreateSession(dbus_interface="org.gnome.Mutter.RemoteDesktop")
        self.s = dbus.Interface(BUS.get_object("org.gnome.Mutter.RemoteDesktop", path),
                                "org.gnome.Mutter.RemoteDesktop.Session")
        self.s.Start()
        self.x, self.y = 0.0, 0.0
        self.home()

    def home(self):
        # Relative motion clamps at the screen edge, which gives a known origin.
        self.s.NotifyPointerMotionRelative(-10000.0, -10000.0)
        self.x, self.y = 0.0, 0.0
        time.sleep(0.05)

    def move(self, x, y, steps=1):
        for i in range(1, steps + 1):
            tx = self.x + (x - self.x) / (steps - i + 1)
            ty = self.y + (y - self.y) / (steps - i + 1)
            self.s.NotifyPointerMotionRelative(tx - self.x, ty - self.y)
            self.x, self.y = tx, ty
            time.sleep(0.01)
        time.sleep(0.05)

    def button(self, pressed):
        self.s.NotifyPointerButton(BTN_LEFT, pressed)
        time.sleep(0.05)

    def click(self, x, y):
        # A pointer that enters a window and presses in the same instant goes unnoticed
        # by gpui; a hand always moves in over a few events.
        self.move(x, y, steps=5)
        time.sleep(0.1)
        self.button(True)
        self.button(False)

    def drag(self, x0, y0, x1, y1):
        self.move(x0, y0)
        self.button(True)
        self.move(x1, y1, steps=20)
        self.button(False)

    def wheel(self, clicks):
        """Vertical wheel clicks at the pointer, positive scrolls down."""
        self.s.NotifyPointerAxisDiscrete(dbus.UInt32(0), dbus.Int32(clicks))
        time.sleep(0.05)

    def keys(self, *names):
        for n in names:
            self.s.NotifyKeyboardKeysym(KEY.get(n, ord(n[0])), True)
        for n in reversed(names):
            self.s.NotifyKeyboardKeysym(KEY.get(n, ord(n[0])), False)
        time.sleep(0.1)


# ── Helpers ─────────────────────────────────────────────────────────────────

def slopshot(*args):
    subprocess.run([BIN, *args], check=True, timeout=10)


def desktop_dir():
    out = subprocess.run(["xdg-user-dir", "DESKTOP"], capture_output=True, text=True)
    path = pathlib.Path(out.stdout.strip()) if out.returncode == 0 and out.stdout.strip() else HOME / "Desktop"
    return HOME / "Desktop" if path == HOME else path


def files(folder):
    return sorted(glob.glob(str(folder / "*.png")), key=os.path.getmtime)


def wait_new_file(folder, before, timeout=10):
    """A new, fully written PNG in `folder`."""
    def done():
        for f in files(folder):
            if f not in before and os.path.getsize(f) > 0:
                time.sleep(0.2)
                return f
        return None
    return wait_for(done, timeout)


def notifications():
    if not NOTIFY_LOG.exists():
        return []
    return [json.loads(line) for line in NOTIFY_LOG.read_text().splitlines()]


def screenshot_permission():
    store = BUS.get_object("org.freedesktop.impl.portal.PermissionStore",
                           "/org/freedesktop/impl/portal/PermissionStore")
    try:
        return [str(p) for p in store.GetPermission("screenshot", "screenshot", "",
                                                    dbus_interface="org.freedesktop.impl.portal.PermissionStore")]
    except dbus.DBusException:
        return []


def wait_overlay_opened(before, timeout=10):
    """Waits for the app to log a newly opened overlay. Polling with portal screenshots
    instead would collide with the app's own capture: GNOME runs one screenshot per sender,
    and every portal request comes from the same xdg-desktop-portal-gnome."""
    opened = lambda: overlays_opened() > before
    if not wait_for(opened, timeout):
        return False
    time.sleep(0.7)  # mapped and first frame presented
    return True


def overlays_opened():
    return (EVIDENCE / "app.log").read_text().count("overlay opened (")


def overlay_left(name):
    """Whether a dimming overlay is still up: points that no other SlopShot window covers
    (the card sits bottom left) must show the wallpaper as is. Returns the differences."""
    shot = portal_screenshot(name)[:, :, :3].astype(int)
    wall = load_png(HOME / "wallpaper.png")[:, :, :3].astype(int)
    spots = [(400, 15), (785, 1265)]
    return [(spot, shot[spot].tolist(), wall[spot].tolist()) for spot in spots
            if np.abs(shot[spot] - wall[spot]).max() > 12]


def clipboard(mime):
    out = subprocess.run(["wl-paste", "--no-newline", "--type", mime], capture_output=True, timeout=10)
    return out.stdout if out.returncode == 0 else None


def clipboard_types():
    out = subprocess.run(["wl-paste", "--list-types"], capture_output=True, text=True, timeout=10)
    return out.stdout.split()


def find_subimage(haystack, needle):
    """Top-left of `needle` in `haystack` (exact RGB match), or None."""
    hh, hw = haystack.shape[:2]
    nh, nw = needle.shape[:2]
    first = needle[0, :, :3]
    for y in range(hh - nh + 1):
        row = haystack[y, :, :3]
        for x in range(hw - nw + 1):
            if row[x, 0] == first[0, 0] and np.array_equal(row[x:x + nw], first) \
                    and np.array_equal(haystack[y:y + nh, x:x + nw, :3], needle[:, :, :3]):
                return x, y
    return None


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    for old in EVIDENCE.glob("*"):
        old.unlink()

    # A wallpaper whose every pixel differs, so a crop that is off by one pixel fails.
    wallpaper = HOME / "wallpaper.png"
    subprocess.run(["convert", "-size", "1280x800", "xc:", "-sparse-color", "Bilinear",
                    "0,0 #ff2020 1279,0 #f0e000 0,799 #2040ff 1279,799 #20d060",
                    "-fill", "white", "-pointsize", "48", "-annotate", "+420+560", "SlopShot E2E",
                    str(wallpaper)], check=True)
    for key in ("picture-uri", "picture-uri-dark"):
        subprocess.run(["gsettings", "set", "org.gnome.desktop.background", key, f"file://{wallpaper}"], check=True)
    subprocess.run(["gsettings", "set", "org.gnome.desktop.background", "picture-options", "stretched"], check=True)
    time.sleep(1)

    notifyd = subprocess.Popen([sys.executable, str(pathlib.Path(__file__).with_name("notifyd.py")), str(NOTIFY_LOG)],
                               stdout=subprocess.PIPE, text=True)
    assert notifyd.stdout.readline().strip() == "ready"

    app = App()
    inp = Input()
    only = os.environ.get("SLOPSHOT_E2E_ONLY", "").split(",") if os.environ.get("SLOPSHOT_E2E_ONLY") else None
    try:
        if only:
            allow_screenshots()
            time.sleep(2)
        else:
            app = run(app, inp)
        for name, feature in FEATURES.items():
            if only is None or name in only:
                app = feature(app, inp) or app
    finally:
        app.stop()
        notifyd.terminate()

    log(f"evidence in {EVIDENCE}")
    if failures:
        log(f"{len(failures)} check(s) failed:")
        for f in failures:
            log(f"  - {f}")
        sys.exit(1)
    log("all checks passed")


class App:
    """The running SlopShot instance; its log accumulates across restarts."""

    def __init__(self):
        self.log = open(EVIDENCE / "app.log", "a")
        self.proc = subprocess.Popen([BIN], stdout=self.log, stderr=subprocess.STDOUT,
                                     env={**os.environ, "RUST_LOG": "slopshot=debug,warn"})

    def stop(self):
        if self.proc.poll() is None:
            subprocess.run([BIN, "quit"], timeout=10)
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.log.close()


def settings():
    return json.loads(SETTINGS.read_text()) if SETTINGS.exists() else {}


def same_pixels(a, b):
    return a.shape == b.shape and int(np.abs(a[:, :, :3].astype(int) - b[:, :, :3].astype(int)).max()) == 0


def red_near(image, x, y, r=6):
    """The reddest pixel around (x, y): annotations are a few pixels wide."""
    around = image[y - r:y + r + 1, x - r:x + r + 1, :3].reshape(-1, 3).astype(int)
    return around[np.argmax(around[:, 0] - around[:, 1] - around[:, 2])]


def is_red(p):
    return p[0] > 200 and p[1] < 110 and p[2] < 110


def clipboard_image(name):
    types = clipboard_types()
    data = clipboard("image/png")
    return types, (png_from_bytes(data, name) if data else None)


def run(app, inp):
    watcher = BUS.get_object("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher")
    items = wait_for(lambda: list(watcher.Get("org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems",
                                               dbus_interface="org.freedesktop.DBus.Properties")), 10)
    check(bool(items), f"tray icon registered with the StatusNotifier host ({items})")

    def bindings():
        out = subprocess.run(["gsettings", "get", "org.gnome.settings-daemon.plugins.media-keys",
                              "custom-keybindings"], capture_output=True, text=True).stdout
        return out if "slopshot-area" in out and "slopshot-color" in out else None
    check(wait_for(bindings, 5) is not None, "first run registered GNOME custom shortcuts")
    area_binding = subprocess.run(
        ["gsettings", "get",
         "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:"
         "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/slopshot-area/", "command"],
        capture_output=True, text=True).stdout.strip()
    check(area_binding.endswith(" area'"), f"area shortcut runs `slopshot area` ({area_binding})")

    # Like macOS asking for Screen Recording at launch, the app triggers GNOME's
    # "Allow … to take screenshots?" prompt right away. Enter presses its default, Deny.
    time.sleep(2)
    inp.keys("return")
    check(wait_for(lambda: screenshot_permission() == ["no"], 5) is not None,
          f"Deny in the launch prompt is stored ({screenshot_permission()})")
    slopshot("area")
    # No screenshot here: the stored Deny covers this script too (same empty app ID).
    time.sleep(1.5)
    # The alert's default button is Ask Again: it forgets the Deny and retries.
    inp.keys("return")
    check(wait_for(lambda: screenshot_permission() == [], 5) is not None, "Ask Again deletes the stored Deny")
    time.sleep(2)
    inp.keys("tab")
    inp.keys("return")  # Tab moves to Allow
    check(wait_for(lambda: screenshot_permission() == ["yes"], 5) is not None,
          f"Allow in the prompt is stored ({screenshot_permission()})")
    check(wait_overlay_opened(0), "the allowed retry opens the overlay")
    portal_screenshot("00-overlay-after-allow")
    inp.keys("escape")
    time.sleep(0.5)
    left = overlay_left("00-after-escape")
    check(not left, f"Escape closes the overlay ({left})")

    time.sleep(1)
    reference = portal_screenshot("01-desktop")
    check(reference.shape[:2] == (800, 1280), f"portal screenshot is 1280x800 ({reference.shape})")
    desktop = desktop_dir()
    want_area = reference[100:400, 100:500]

    # ── Capture Area, inline editor (the default): draw, then Return copies ──
    temp_before = files(TEMP_DIR)
    opened = overlays_opened()
    slopshot("area")
    check(wait_overlay_opened(opened), "`slopshot area` opens the overlay")
    inp.move(300, 250)
    portal_screenshot("02-area-overlay")
    inp.drag(100, 100, 500, 400)
    time.sleep(0.5)
    portal_screenshot("03-inline-editor")
    # The editor's last tool, Arrow on first run, draws straight away.
    inp.drag(150, 350, 450, 150)
    time.sleep(0.3)
    portal_screenshot("04-inline-arrow")
    inp.keys("return")
    temp = wait_new_file(TEMP_DIR, temp_before)
    check(temp is not None, f"the capture is kept as a temp PNG in {TEMP_DIR} ({temp})")
    check(files(desktop) == [], "nothing is written to the save folder until Save")
    time.sleep(1)
    types, pasted = clipboard_image("05-inline-clipboard.png")
    check("image/png" in types and "text/uri-list" in types, f"clipboard offers the image and its file ({types})")
    if check(pasted is not None, "inline Copy put an image on the clipboard"):
        check(pasted.shape[:2] == (300, 400), f"inline capture is 400x300 ({pasted.shape[1]}x{pasted.shape[0]})")
        if pasted.shape[:2] == (300, 400):
            check(same_pixels(pasted[:40, :40], want_area[:40, :40]), "unannotated pixels match the screen exactly")
            mid = red_near(pasted, 200, 150)
            check(is_red(mid), f"the red arrow is in the copied image ({mid})")
        if temp:
            check(same_pixels(load_png(temp), pasted), "the temp file equals the clipboard image")
    shot = portal_screenshot("06-card")
    check(not overlay_left("06-overlay-check"), "Copy closes the overlay")
    check(settings().get("editor_tool") in (None, "arrow"), f"the last tool is remembered ({settings().get('editor_tool')})")

    # ── Inline Save (Ctrl+S) writes to the save folder ──
    opened = overlays_opened()
    slopshot("area")
    check(wait_overlay_opened(opened), "a second capture opens the overlay")
    inp.drag(100, 100, 500, 400)
    time.sleep(0.5)
    inp.keys("ctrl", "s")
    saved = wait_new_file(desktop, [])
    if check(saved is not None, f"Ctrl+S saves into the default folder {desktop} ({saved})"):
        check(os.path.basename(saved).startswith("SlopShot "), f"macOS file naming ({os.path.basename(saved)})")
        check(same_pixels(load_png(saved), want_area), "the saved capture matches the screen exactly")

    # ── Escape cancels an inline session without output ──
    temp_before = files(TEMP_DIR)
    opened = overlays_opened()
    slopshot("area")
    check(wait_overlay_opened(opened), "the overlay opens again")
    inp.drag(100, 100, 500, 400)
    time.sleep(0.4)
    inp.keys("escape")
    time.sleep(0.6)
    check(files(TEMP_DIR) == temp_before, "Escape in the inline editor captures nothing")
    check(not overlay_left("07-after-escape"), "Escape closes the overlay")

    # ── Pick Color: → switches format and persists it; a click copies ──
    opened = overlays_opened()
    slopshot("color")
    check(wait_overlay_opened(opened), "`slopshot color` opens the overlay")
    inp.move(640, 300)
    time.sleep(0.3)
    portal_screenshot("08-color-loupe")
    inp.keys("right")
    time.sleep(0.3)
    portal_screenshot("09-color-format")
    inp.click(640, 300)
    time.sleep(0.5)
    r, g, b = reference[300, 640, :3]
    want = f"#{r:02x}{g:02x}{b:02x}"
    got = (clipboard("text/plain;charset=utf-8") or clipboard("text/plain") or b"").decode()
    check(got == want, f"colour picker copied {want} in the lowercase hex format (got {got!r})")
    check(settings().get("color_format") == "hexLower", f"the format choice persists ({settings().get('color_format')})")
    check(not overlay_left("10-after-pick"), "the overlay closes after picking a colour")

    # ── Capture Fullscreen ──
    temp_before = files(TEMP_DIR)
    slopshot("full")
    temp = wait_new_file(TEMP_DIR, temp_before)
    if check(temp is not None, "fullscreen capture made a temp file"):
        full = load_png(temp)
        check(full.shape[:2] == (800, 1280), f"fullscreen capture is 1280x800 ({full.shape[1]}x{full.shape[0]})")
    time.sleep(1)
    portal_screenshot("11-fullscreen-card")

    # ── Adjust step: inline editing off ──
    app.stop()
    data = settings()
    data["edit_after_select"] = False
    SETTINGS.write_text(json.dumps(data))
    app = App()
    time.sleep(2)
    opened = overlays_opened()
    slopshot("area")
    check(wait_overlay_opened(opened), "the overlay opens with inline editing off")
    inp.drag(100, 100, 500, 400)
    time.sleep(0.5)
    portal_screenshot("12-adjust")
    temp_before = files(TEMP_DIR)
    inp.keys("return")
    temp = wait_new_file(TEMP_DIR, temp_before)
    time.sleep(0.5)
    types, pasted = clipboard_image("13-adjust-clipboard.png")
    if check(pasted is not None, "Return in the adjust step captures and copies"):
        check(same_pixels(pasted, want_area), "the adjusted capture matches the screen exactly")

    opened = overlays_opened()
    slopshot("area")
    check(wait_overlay_opened(opened), "the overlay opens for F")
    inp.move(640, 400)
    inp.keys("f")
    time.sleep(0.4)
    portal_screenshot("14-full-display-selection")
    inp.keys("return")
    time.sleep(1.5)
    types, pasted = clipboard_image("15-f-clipboard.png")
    check(pasted is not None and pasted.shape[:2] == (800, 1280), "F then Return captures the whole display")

    # ── Editor: Edit Last, draw, Return = Done copies and closes ──
    opened = overlays_opened()
    temp_before = files(TEMP_DIR)
    slopshot("area")
    check(wait_overlay_opened(opened), "the overlay opens before editing")
    inp.drag(100, 100, 500, 400)
    time.sleep(0.4)
    inp.keys("return")
    check(wait_new_file(TEMP_DIR, temp_before) is not None, "the area capture for the editor is taken")
    slopshot("edit-last")
    time.sleep(1.5)
    shot = portal_screenshot("16-editor")
    # Like macOS, the editor scales the image to fit its canvas (upscaling small ones);
    # the image is the colourful block inside the gray canvas.
    rgb = shot[140:690, 170:1110, :3].astype(int)
    colourful = (rgb.max(axis=2) - rgb.min(axis=2)) > 40
    ys, xs = np.nonzero(colourful)
    if check(len(xs) > 1000, "the editor shows the capture"):
        x0, x1, y0, y1 = xs.min() + 170, xs.max() + 170, ys.min() + 140, ys.max() + 140
        ratio = (x1 - x0 + 1) / (y1 - y0 + 1)
        check(abs(ratio - 4 / 3) < 0.02, f"fitted with its aspect ratio ({x1 - x0 + 1}x{y1 - y0 + 1})")
        cx_, cy_ = (x0 + x1) // 2, (y0 + y1) // 2
        inp.drag(cx_ - 150, cy_ + 100, cx_ + 150, cy_ - 100)
        time.sleep(0.3)
        portal_screenshot("17-editor-arrow")
        inp.keys("return")
        time.sleep(1.5)
        types, pasted = clipboard_image("18-editor-clipboard.png")
        if check(pasted is not None and pasted.shape[:2] == (300, 400), "Done copies the edited image at full size"):
            mid = red_near(pasted, 200, 150)
            check(is_red(mid), f"the edited copy has the red arrow ({mid})")

    # ── Settings window ──
    slopshot("settings")
    time.sleep(1.5)
    portal_screenshot("19-settings")
    slopshot("about")
    time.sleep(1)
    portal_screenshot("20-settings-about")
    inp.click(918, 192)  # close, or it covers what the feature tests capture
    time.sleep(0.5)

    # Like macOS, SlopShot never posts a notification.
    check(notifications() == [], f"no notifications were posted ({notifications()})")

    return app


def allow_screenshots():
    """Grants what the launch prompt asks, for runs that skip the prompt tests."""
    store = BUS.get_object("org.freedesktop.impl.portal.PermissionStore",
                           "/org/freedesktop/impl/portal/PermissionStore")
    store.SetPermission("screenshot", True, "screenshot", "", ["yes"],
                        dbus_interface="org.freedesktop.impl.portal.PermissionStore")


def select_area(inp, command, rect=(100, 100, 500, 400)):
    opened = overlays_opened()
    slopshot(command)
    if not check(wait_overlay_opened(opened), f"`slopshot {command}` opens the overlay"):
        return False
    inp.drag(*rect)
    time.sleep(0.4)
    return True


def video_frame(video, name, at_ms=500):
    """A decoded frame of `video` as RGBA, through GStreamer like the app."""
    out = EVIDENCE / name
    subprocess.run(["gst-launch-1.0", "-q", "filesrc", f"location={video}", "!", "decodebin", "!",
                    "videoconvert", "!", "pngenc", "snapshot=true", "!", "filesink", f"location={out}"],
                   check=True, timeout=30, capture_output=True)
    return load_png(out)


def history_items():
    path = HOME / ".local" / "share" / "slopshot" / "history.json"
    return json.loads(path.read_text()) if path.exists() else []


def record(app, inp):
    reference = portal_screenshot("30-record-reference")
    before = set(glob.glob(str(TEMP_DIR / "*.mp4")))
    if not select_area(inp, "record"):
        return
    portal_screenshot("31-record-selection")
    inp.click(418, 424)  # Start Recording
    time.sleep(2)
    portal_screenshot("32-screencast-dialog")
    # GNOME's screen-share dialog: the only monitor is preselected, "Remember" is on.
    inp.click(925, 188)  # Share
    started = wait_for(lambda: "recording started" in (EVIDENCE / "app.log").read_text(), 10)
    check(started is not None, "the ScreenCast portal starts a recording")
    time.sleep(3)
    slopshot("record")
    video = wait_for(lambda: next((f for f in glob.glob(str(TEMP_DIR / "*.mp4")) if f not in before), None), 15)
    if not check(video is not None, f"stopping writes an MP4 to {TEMP_DIR} ({video})"):
        return
    time.sleep(1)
    frame = video_frame(video, "33-record-frame.png")
    check(frame.shape[:2] == (300, 400), f"the video is the selected 400x300 ({frame.shape[1]}x{frame.shape[0]})")
    if frame.shape[:2] == (300, 400):
        diff = np.abs(frame[20:280, 20:380, :3].astype(int) - reference[120:380, 120:480, :3].astype(int)).mean()
        check(diff < 12, f"the video shows the selected area (mean difference {diff:.1f})")
    if os.environ.get("DISPLAY"):
        # Nothing of SlopShot has focus when a recording stops; only the X11 helper can copy.
        uris = (clipboard("text/uri-list") or b"").decode()
        check(pathlib.Path(video).as_uri() in uris, f"the clip's file is on the clipboard ({uris.strip()!r})")
    items = history_items()
    check(bool(items) and items[0].get("kind") == "video", f"history lists the recording first ({items[:1]})")
    time.sleep(1)
    portal_screenshot("34-record-card")


def scrolling(app, inp):
    # A page with distinct rows all the way down; the window shows its top 800 px.
    page = HOME / "page.png"
    subprocess.run(["convert", "-size", "1280x2400", "gradient:#f6d365-#2b5876", "-fill", "black",
                    "-pointsize", "28"] + sum((["-annotate", f"+{60 + (i % 5) * 220}+{60 + i * 47}",
                                                f"Line {i:02d} of the scrolled page"] for i in range(50)), [])
                   + [str(page)], check=True)
    viewer = subprocess.Popen([sys.executable, str(pathlib.Path(__file__).with_name("page.py")), str(page)],
                              stdout=subprocess.PIPE, text=True)
    try:
        assert viewer.stdout.readline().strip() == "ready"
        time.sleep(1.5)
        top = portal_screenshot("40-scroll-page")
        want = load_png(page)
        # GTK renders the gradient a unit off here and there.
        near = lambda a, b: int(np.abs(a[:, :, :3].astype(int) - b[:, :, :3].astype(int)).max()) <= 2
        check(near(top[100:400, 200:600], want[100:400, 200:600]), "the page shows its top, unscaled")
        if not select_area(inp, "scroll", (200, 100, 600, 400)):
            return
        inp.click(418, 424)  # Start Scrolling Capture
        time.sleep(2)
        inp.click(893, 323)  # Allow Remote Interaction, for auto-scroll
        time.sleep(0.3)
        portal_screenshot("41-remote-desktop-dialog")
        inp.click(925, 188)  # Share
        started = wait_for(lambda: "scrolling capture started" in (EVIDENCE / "app.log").read_text(), 10)
        if not check(started is not None, "the RemoteDesktop portal starts the scrolling capture"):
            return
        time.sleep(1)
        inp.move(400, 250)
        for _ in range(12):
            inp.wheel(1)
            time.sleep(0.35)
        portal_screenshot("42-scrolled")
        before = files(TEMP_DIR)
        slopshot("scroll")  # without the X11 helper, running it again is Done
        temp = wait_new_file(TEMP_DIR, before, 15)
        if not check(temp is not None, "Done writes the stitched capture"):
            return
        tall = load_png(temp)
        log(f"stitched image is {tall.shape[1]}x{tall.shape[0]}")
        check(tall.shape[1] == 400 and tall.shape[0] > 600, f"the capture is taller than the area ({tall.shape[1]}x{tall.shape[0]})")
        n = tall.shape[0]
        shutil.copy(temp, EVIDENCE / "43-scroll-stitched.png")
        diff = np.abs(tall[:, :, :3].astype(int) - want[100:100 + n, 200:600, :3].astype(int))
        check(int(diff.max()) <= 2, f"it equals the page rows it covers, seams included (max difference {int(diff.max())})")
        items = history_items()
        check(bool(items) and "(scrolling)" in (items[0].get("subtitle") or ""), f"history notes a scrolling capture ({items[:1]})")
    finally:
        viewer.terminate()
        viewer.wait(5)
        time.sleep(1.5)  # the full-screen window's exit transition


def ocr_text(image_path):
    out = subprocess.run(["tesseract", str(image_path), "-", "--psm", "11"], capture_output=True, text=True, timeout=60)
    return " ".join(out.stdout.split())


def capture_text(app, inp):
    # The wallpaper's white "SlopShot E2E" sits around (420..700, 515..570).
    if not select_area(inp, "text", (380, 490, 760, 590)):
        return
    portal_screenshot("49-text-selection")
    inp.click(720, 614)  # Read Text, right-aligned under the area
    text = lambda: (clipboard("text/plain;charset=utf-8") or clipboard("text/plain") or b"").decode()
    time.sleep(3)
    portal_screenshot("50-ocr-window")
    check("SlopShot E2E" in ocr_text(EVIDENCE / "50-ocr-window.png"), "the Text Recognition window shows the text")
    if os.environ.get("DISPLAY"):
        check(wait_for(lambda: "SlopShot E2E" in text(), 5) is not None, f"the X11 helper copies the text ({text()!r})")
    else:
        # Only a focused Wayland window may set the clipboard, and GNOME doesn't focus one
        # opened without an activation token; Ubuntu's Xwayland lets the helper copy.
        log("skipped: copying from the unfocused window needs the X11 helper")
    items = history_items()
    check(bool(items) and items[0].get("kind") == "text" and "SlopShot E2E" in (items[0].get("text") or ""),
          f"history keeps the recognized text ({items[:1]})")


def history(app, inp):
    slopshot("history")
    time.sleep(1.5)
    portal_screenshot("60-history")
    seen = ocr_text(EVIDENCE / "60-history.png")
    check("Recent captures" in seen, f"`slopshot history` opens Capture History ({seen[:120]!r})")
    kinds = [i.get("kind") for i in history_items()]
    log(f"history kinds: {kinds}")


FEATURES = {"record": record, "scroll": scrolling, "text": capture_text, "history": history}


if __name__ == "__main__":
    main()
