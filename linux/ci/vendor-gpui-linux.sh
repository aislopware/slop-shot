#!/usr/bin/env bash
# Re-vendors gpui_linux from aislopware/gpui-fast at the rev pinned in Cargo.toml and
# reapplies vendor/gpui_linux.patch (see vendor/gpui_linux/PATCHES.md). Cargo can't patch
# one crate of a git workspace, so the crate is copied with its workspace manifest resolved.
#
#   ci/vendor-gpui-linux.sh
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
git_url=https://github.com/aislopware/gpui-fast.git
rev="$(grep -m1 -o "gpui-fast.git\", rev = \"[0-9a-f]*" "$here/Cargo.toml" | grep -o '[0-9a-f]*$')"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git -C "$work" init -q src
git -C "$work/src" fetch -q --depth 1 "$git_url" "$rev"
git -C "$work/src" checkout -q FETCH_HEAD
crate="$work/gpui_linux"
cp -R "$work/src/crates/gpui_linux" "$crate"
rm "$crate/LICENSE-APACHE"
cp "$work/src/LICENSE-APACHE" "$crate/"

uv run -q --no-project --with tomli-w python3 -I - "$crate" "$work/src" "$git_url" "$rev" << 'EOF'
import sys, tomllib, tomli_w, pathlib
crate, root, git, rev = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4]
ws = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
m = tomllib.loads((crate / "Cargo.toml").read_text())
for k, v in list(m["package"].items()):
    if isinstance(v, dict) and v.get("workspace"):
        m["package"][k] = ws["package"][k]
m["lints"] = ws["lints"]

def resolve(name, spec):
    if not (isinstance(spec, dict) and spec.get("workspace")):
        return spec
    base = ws["dependencies"][name]
    base = {"version": base} if isinstance(base, str) else dict(base)
    if "path" in base:
        base = {"git": git, "rev": rev} | {k: v for k, v in base.items() if k != "path"}
    local = {k: v for k, v in spec.items() if k != "workspace"}
    features = base.get("features", []) + local.pop("features", [])
    base |= local
    if features:
        base["features"] = features
    return base

for target in m.get("target", {}).values():
    for section in ("dependencies", "dev-dependencies"):
        if section in target:
            target[section] = {n: resolve(n, s) for n, s in target[section].items()}
(crate / "Cargo.toml").write_text(tomli_w.dumps(m))
EOF

(cd "$crate" && git apply "$here/vendor/gpui_linux.patch")
cp "$here/vendor/gpui_linux/PATCHES.md" "$crate/"
rm -rf "$here/vendor/gpui_linux"
mv "$crate" "$here/vendor/gpui_linux"
echo "vendored gpui_linux at $rev"
