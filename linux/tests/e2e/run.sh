#!/usr/bin/env bash
# Builds SlopShot and runs the E2E suite in a headless GNOME Wayland session.
# From macOS: ci/vm.sh tests/e2e/run.sh   ·   On Ubuntu 24.04 directly: tests/e2e/run.sh
# Screenshots and the app log land in .e2e-evidence/.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build ${CARGO_PROFILE:+--profile "$CARGO_PROFILE"}
target="${CARGO_TARGET_DIR:-target}/${CARGO_PROFILE:-debug}"
SLOPSHOT_BIN="$(cd "$target" && pwd)/slopshot"
export SLOPSHOT_BIN
export SLOPSHOT_EVIDENCE="$PWD/.e2e-evidence"
exec tests/e2e/session.sh python3 tests/e2e/e2e.py
