#!/usr/bin/env bash
# Installs Tidings for the current user (no root needed).
# Requirements: webkit2gtk-4.1, gtk3 and libxdo.
#   Arch / CachyOS:  sudo pacman -S --needed webkit2gtk-4.1 gtk3 xdotool
#   Ubuntu / Debian: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3
#   Fedora:          sudo dnf install webkit2gtk4.1 gtk3 libxdo
set -euo pipefail
cd "$(dirname "$0")"

PREFIX="${PREFIX:-$HOME/.local}"
if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$PREFIX/bin/tidings" "$PREFIX/share/applications/tidings.desktop"
  rm -f "$PREFIX/share/icons/hicolor/"{32x32,128x128,256x256}/apps/tidings.png
  echo "Tidings removed. Your data is still in ~/.local/share/org.tidings.reader"
  exit 0
fi

install -Dm755 tidings "$PREFIX/bin/tidings"
install -Dm644 tidings.desktop "$PREFIX/share/applications/tidings.desktop"
sed -i "s|^Exec=tidings|Exec=$PREFIX/bin/tidings|" "$PREFIX/share/applications/tidings.desktop"
install -Dm644 icons/32x32.png "$PREFIX/share/icons/hicolor/32x32/apps/tidings.png"
install -Dm644 icons/128x128.png "$PREFIX/share/icons/hicolor/128x128/apps/tidings.png"
install -Dm644 icons/128x128@2x.png "$PREFIX/share/icons/hicolor/256x256/apps/tidings.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" || true

if ldd "$PREFIX/bin/tidings" | grep -q "not found"; then
  echo "Installed, but some libraries are missing:"
  ldd "$PREFIX/bin/tidings" | grep "not found"
  echo "Install the packages listed at the top of this script."
  exit 1
fi
echo "Installed to $PREFIX/bin/tidings. Find Tidings in your app menu."
