#!/bin/sh
# Installs Tidings for the current user (no root needed). Plain POSIX sh, so the
# same script works on Linux, FreeBSD and OpenBSD.
# Requirements: webkit2gtk-4.1, gtk3 and libxdo.
#   Arch / CachyOS:  sudo pacman -S --needed webkit2gtk-4.1 gtk3 xdotool
#   Ubuntu / Debian: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3
#   Fedora:          sudo dnf install webkit2gtk4.1 gtk3 libxdo
#   FreeBSD:         sudo pkg install webkit2-gtk_41 gtk3 xdotool gstreamer1-plugins-good gstreamer1-plugins-ugly gstreamer1-libav
#   OpenBSD:         doas pkg_add webkitgtk41 gtk+3 xdotool gst-plugins-good gst-plugins-ugly gst-libav
set -eu
cd "$(dirname "$0")"

PREFIX="${PREFIX:-$HOME/.local}"
ICONS="$PREFIX/share/icons/hicolor"
if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$PREFIX/bin/tidings" "$PREFIX/share/applications/tidings.desktop"
  for size in 32x32 128x128 256x256; do
    rm -f "$ICONS/$size/apps/tidings.png"
  done
  echo "Tidings removed. Your data is still in ~/.local/share/org.tidings.reader"
  exit 0
fi

mkdir -p "$PREFIX/bin" "$PREFIX/share/applications" \
  "$ICONS/32x32/apps" "$ICONS/128x128/apps" "$ICONS/256x256/apps"
install -m 755 tidings "$PREFIX/bin/tidings"
sed "s|^Exec=tidings|Exec=$PREFIX/bin/tidings|" tidings.desktop > "$PREFIX/share/applications/tidings.desktop"
chmod 644 "$PREFIX/share/applications/tidings.desktop"
install -m 644 icons/32x32.png "$ICONS/32x32/apps/tidings.png"
install -m 644 icons/128x128.png "$ICONS/128x128/apps/tidings.png"
install -m 644 icons/128x128@2x.png "$ICONS/256x256/apps/tidings.png"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$ICONS" 2>/dev/null || true

if ldd "$PREFIX/bin/tidings" | grep -q "not found"; then
  echo "Installed, but some libraries are missing:"
  ldd "$PREFIX/bin/tidings" | grep "not found"
  echo "Install the packages listed at the top of this script."
  exit 1
fi
echo "Installed to $PREFIX/bin/tidings. Find Tidings in your app menu."
