#!/usr/bin/env bash
# Fails if the version is not the same in every file (and in the tag, if one is given).
#   scripts/check-version.sh [v0.1.0]
set -euo pipefail
cd "$(dirname "$0")/.."
pkg=$(node -p "require('./package.json').version")
conf=$(node -p "require('./src-tauri/tauri.conf.json').version")
cargo=$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)
lock=$(awk '/^name = "tidings"$/{getline; gsub(/version = |"/,""); print; exit}' src-tauri/Cargo.lock)
echo "package.json=$pkg tauri.conf.json=$conf Cargo.toml=$cargo Cargo.lock=$lock"
[ "$pkg" = "$conf" ] && [ "$pkg" = "$cargo" ] && [ "$pkg" = "$lock" ] || { echo "Versions differ." >&2; exit 1; }
if [ -n "${1:-}" ] && [ "${1#v}" != "$pkg" ]; then
  echo "Tag $1 does not match version $pkg." >&2; exit 1
fi
grep -q "^## \[$pkg\]" CHANGELOG.md || { echo "CHANGELOG.md has no section for $pkg." >&2; exit 1; }
echo "OK: $pkg"
