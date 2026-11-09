#!/bin/sh
# Builds Tidings natively on FreeBSD or OpenBSD and makes a portable tarball
# (the same layout as the Linux one: binary, icons, install.sh, .desktop file).
#
#   scripts/build-bsd.sh
#
# The Tauri CLI has no BSD binaries, so this calls cargo directly with the
# feature `tauri build` would turn on (tauri/custom-protocol), after building
# the UI with Vite. Tauri cannot make BSD packages; use the tarball.
#
# Options (environment variables):
#   JOBS=2      parallel compile jobs (lower = less memory)
#   LTO=false   turn off link-time optimization (faster, less memory)
#
# Output: release/<os>/tidings-<version>-<os>-<arch>.tar.gz and a .sha256 file
set -eu
cd "$(dirname "$0")/.."

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS" in
  freebsd | openbsd) ;;
  *) echo "Run this on FreeBSD or OpenBSD. Linux and macOS use 'npm run tauri build'." >&2; exit 1 ;;
esac
ARCH=$(uname -m)
[ "$ARCH" = amd64 ] && ARCH=x86_64

for tool in cargo node npm pkg-config; do
  command -v "$tool" >/dev/null || { echo "$tool is missing; see 'Building on FreeBSD and OpenBSD' in docs/README.md." >&2; exit 1; }
done
pkg-config --exists webkit2gtk-4.1 || { echo "webkit2gtk-4.1 is missing; see docs/README.md." >&2; exit 1; }

# OpenBSD limits how much memory one process may use by login class
# (1.5 GB for "staff"). Linking the app needs more, so raise it to the hard limit.
[ "$OS" = openbsd ] && ulimit -d "$(ulimit -Hd)"
[ -n "${JOBS:-}" ] && export CARGO_BUILD_JOBS="$JOBS"
[ -n "${LTO:-}" ] && export CARGO_PROFILE_RELEASE_LTO="$LTO"

echo "==> Installing JavaScript dependencies"
npm ci --no-audit --no-fund
if [ "$OS" = openbsd ]; then
  # Rolldown (Vite's bundler) has no native OpenBSD build. Its WebAssembly build
  # works, but npm skips it because it is marked cpu "wasm32", so add it by hand.
  # (lightningcss has no OpenBSD build either; vite.config.ts skips CSS minifying there.)
  ROLLDOWN=$(node -p "require('rolldown/package.json').version")
  npm install --no-save --force --no-audit --no-fund "@rolldown/binding-wasm32-wasi@$ROLLDOWN"
fi

echo "==> Building the UI"
npm run build

echo "==> Building the app ($OS $ARCH)"
(cd src-tauri && cargo build --release --features tauri/custom-protocol)

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
NAME="tidings-$VERSION-$OS-$ARCH"
OUT="release/$OS"

echo "==> Making $OUT/$NAME.tar.gz"
rm -rf "$OUT/$NAME" "$OUT/$NAME.tar.gz" "$OUT/$NAME.tar.gz.sha256"
mkdir -p "$OUT/$NAME/icons"
cp src-tauri/target/release/tidings scripts/linux/install.sh scripts/linux/tidings.desktop "$OUT/$NAME/"
cp src-tauri/icons/32x32.png src-tauri/icons/128x128.png src-tauri/icons/128x128@2x.png "$OUT/$NAME/icons/"
chmod 755 "$OUT/$NAME/tidings" "$OUT/$NAME/install.sh"
tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
rm -rf "$OUT/$NAME"
(
  cd "$OUT"
  if command -v sha256sum >/dev/null; then sha256sum "$NAME.tar.gz"; else sha256 -r "$NAME.tar.gz"; fi
) > "$OUT/$NAME.tar.gz.sha256"
cat "$OUT/$NAME.tar.gz.sha256"
echo "Done. Install with: tar xzf $OUT/$NAME.tar.gz && ./$NAME/install.sh"
