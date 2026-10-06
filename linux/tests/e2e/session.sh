#!/usr/bin/env bash
# Runs "$@" inside a throwaway headless GNOME Shell Wayland session (Ubuntu 24.04):
# its own D-Bus session bus, runtime dir and virtual monitor, with the AppIndicator
# extension on and xdg-desktop-portal(-gnome) D-Bus activated, as on a real desktop.
#
#   tests/e2e/session.sh python3 tests/e2e/e2e.py
set -euo pipefail

if [ -z "${SLOPSHOT_E2E_INNER:-}" ]; then
  export SLOPSHOT_E2E_INNER=1
  run_dir="$(mktemp -d /tmp/slopshot-e2e-run.XXXXXX)"
  chmod 700 "$run_dir"
  export XDG_RUNTIME_DIR="$run_dir"
  # A fresh home: no settings, dconf database or GNOME shortcuts carried between runs.
  export HOME="$run_dir/home"
  unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME
  mkdir -p "$HOME"
  # xdg-document-portal leaves a FUSE mount in the runtime dir.
  trap 'fusermount3 -uz "$run_dir/doc" 2>/dev/null; rm -rf "$run_dir"' EXIT
  dbus-run-session -- "$0" "$@"
  exit
fi

export XDG_CURRENT_DESKTOP=ubuntu:GNOME XDG_SESSION_TYPE=wayland XDG_SESSION_DESKTOP=ubuntu
export GNOME_SHELL_SESSION_MODE=ubuntu
unset DISPLAY WAYLAND_DISPLAY
monitor="${SLOPSHOT_E2E_MONITOR:-1280x800}"

gsettings set org.gnome.shell enabled-extensions "['ubuntu-appindicators@ubuntu.com']"
gsettings set org.gnome.shell disable-user-extensions false
# No first-run tour, no lock screen or blanking in the middle of a run.
gsettings set org.gnome.shell welcome-dialog-last-shown-version '999'
gsettings set org.gnome.desktop.session idle-delay 0
gsettings set org.gnome.desktop.screensaver lock-enabled false

log="${SLOPSHOT_E2E_LOG:-/tmp/slopshot-e2e-shell.log}"
# Screen casts and recorded audio go through PipeWire, which systemd --user starts on a desktop.
pipewire > "$log.pipewire" 2>&1 &
pw_pids=($!)
wireplumber >> "$log.pipewire" 2>&1 &
pw_pids+=($!)
pipewire-pulse >> "$log.pipewire" 2>&1 &
pw_pids+=($!)
for _ in $(seq 50); do
  [ -S "$XDG_RUNTIME_DIR/pipewire-0" ] && break
  sleep 0.1
done
gnome-shell --headless --wayland --no-x11 --virtual-monitor "$monitor" > "$log" 2>&1 &
shell_pid=$!
stop_session() {
  kill "$shell_pid" "${pw_pids[@]}" 2> /dev/null
  for _ in $(seq 30); do
    kill -0 "$shell_pid" 2> /dev/null || break
    sleep 0.1
  done
  kill -9 "$shell_pid" 2> /dev/null
  wait "$shell_pid" 2> /dev/null || true
}
trap stop_session EXIT

for _ in $(seq 100); do
  [ -S "$XDG_RUNTIME_DIR/wayland-0" ] && timeout 2 gdbus call --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell --method org.freedesktop.DBus.Properties.Get \
    org.gnome.Shell ShellVersion > /dev/null 2>&1 && break
  sleep 0.2
done
[ -S "$XDG_RUNTIME_DIR/wayland-0" ] || {
  echo "gnome-shell did not start, see $log" >&2
  exit 1
}
export WAYLAND_DISPLAY=wayland-0
# Services the bus activates from now on (portals, the tray watcher) need these, as
# gnome-session would have set them.
dbus-update-activation-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE \
  XDG_SESSION_DESKTOP XDG_RUNTIME_DIR GNOME_SHELL_SESSION_MODE
"$@"
