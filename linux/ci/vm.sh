#!/usr/bin/env bash
# Run a command in the Lima dev VM (see ci/provision.sh) from the crate directory.
# The target dir stays inside the VM: building over the host mount is several times slower.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cmd="$(printf '%q ' "$@")"
exec limactl shell --workdir "$here" "${LIMA_INSTANCE:-slopshot}" -- \
  bash -lc "export CARGO_TARGET_DIR=\$HOME/target PATH=\$HOME/.cargo/bin:\$PATH; $cmd"
