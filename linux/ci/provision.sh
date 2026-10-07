#!/usr/bin/env bash
# Packages for building SlopShot and running it under a headless GNOME Wayland session
# (Ubuntu 24.04). Used by the Lima dev VM and the Linux release workflow.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config curl ca-certificates git clang \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev mesa-vulkan-drivers \
  libfontconfig-dev libfreetype-dev libx11-xcb-dev libxcb1-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libssl-dev libdbus-1-dev \
  gnome-shell mutter xdg-desktop-portal xdg-desktop-portal-gnome gnome-shell-extension-appindicator \
  ubuntu-session yaru-theme-gnome-shell \
  dbus-x11 dbus-user-session at-spi2-core gsettings-desktop-schemas \
  python3-gi python3-dbus python3-numpy gir1.2-glib-2.0 \
  pipewire wireplumber gstreamer1.0-pipewire gstreamer1.0-tools gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-ugly gstreamer1.0-plugins-bad gstreamer1.0-libav \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev tesseract-ocr tesseract-ocr-eng tesseract-ocr-vie \
  wl-clipboard dpkg-dev desktop-file-utils appstream imagemagick fonts-dejavu-core fonts-noto-core adwaita-icon-theme
if ! command -v cargo > /dev/null && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
fi
"$HOME/.cargo/bin/cargo" install cargo-deb --locked
# The E2E suite starts SlopShot in a systemd user scope, as GNOME does. A CI runner has no
# login session to bring the user manager up, so it is kept up regardless.
sudo loginctl enable-linger "$USER"
