#!/usr/bin/env bash
# Installs the given .deb with the exact command the README gives users, checks what landed
# and that apt pulled in the dependencies, then purges it again.
#
#   tests/e2e/install.sh <file.deb>
set -euo pipefail
deb="$(realpath "${1:?usage: $0 <file.deb>}")"
# shellcheck disable=SC2329 # run by the EXIT trap
cleanup() { sudo apt-get purge -y -qq slopshot > /dev/null 2>&1 || true; }
trap cleanup EXIT

sudo apt-get install -y -qq "${deb}" > /dev/null

fail=0
check() {
  local name="$1"
  shift
  if "$@"; then
    echo "[install] PASS ${name}"
  else
    echo "[install] FAIL ${name}"
    fail=1
  fi
}
# shellcheck disable=SC2329 # run through check
quiet() { "$@" > /dev/null 2>&1; }
want="$(dpkg-deb -f "${deb}" Version)"
check "apt installs version ${want}" test "$(dpkg-query -W -f="\${Version}" slopshot)" = "${want}"
check "slopshot is on PATH and runs" test "$(slopshot --version)" = "slopshot ${want%-*}"
check "desktop entry is valid" desktop-file-validate /usr/share/applications/com.thanglb.slopshot.desktop
check "metainfo is valid" quiet appstreamcli validate --no-net --pedantic /usr/share/metainfo/com.thanglb.slopshot.metainfo.xml
check "icon is installed" test -f /usr/share/icons/hicolor/256x256/apps/com.thanglb.slopshot.png
check "dependencies pull the screenshot portal" quiet dpkg -s xdg-desktop-portal
echo "[install] $(dpkg-deb -f "${deb}" Package) ${want} $(dpkg-deb -f "${deb}" Architecture), $(du -h "${deb}" | cut -f1); Depends: $(dpkg-deb -f "${deb}" Depends)"
exit "${fail}"
