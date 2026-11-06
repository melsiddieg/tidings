#!/usr/bin/env bash
# Runs inside the build container. /src is the project (read-only), /out receives packages.
set -euo pipefail

TARGET=x86_64-unknown-linux-gnu
export CARGO_TARGET_DIR=/cache/target

# On an x86_64 host there is no cross compiler; use the normal one.
if ! command -v x86_64-linux-gnu-gcc >/dev/null; then
  unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER CC_x86_64_unknown_linux_gnu \
        CXX_x86_64_unknown_linux_gnu AR_x86_64_unknown_linux_gnu
fi

echo "==> Copying sources"
mkdir -p /work
tar -C /src --exclude=./node_modules --exclude=./dist --exclude=./release \
    --exclude=./src-tauri/target --exclude=./src-tauri/gen -cf - . | tar -C /work -xf -
cd /work

# LOW_MEMORY=1: build the very large GTK/WebKit binding crates with less optimization
# and smaller code units. They are thin wrappers, so speed is not affected in practice,
# but the compiler needs far less memory (useful in small container VMs).
if [ "${LOW_MEMORY:-0}" = "1" ]; then
  echo "==> Low-memory build settings"
  mkdir -p src-tauri/.cargo
  : > src-tauri/.cargo/config.toml
  for crate in gtk webkit2gtk javascriptcore-rs soup3 tao wry muda tauri tauri-runtime-wry; do
    printf '[profile.release.package.%s]\nopt-level = 1\ncodegen-units = 16\n\n' "$crate" >> src-tauri/.cargo/config.toml
  done
fi

echo "==> Installing JavaScript dependencies"
npm ci --no-audit --no-fund

echo "==> Building ($TARGET, bundles: ${BUNDLES})"
npx tauri build --target "$TARGET" --bundles "$BUNDLES"

BUNDLE_DIR="$CARGO_TARGET_DIR/$TARGET/release/bundle"
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"

echo "==> Making a portable tarball"
rm -f /out/*.tar.gz
PORTABLE="tidings-${VERSION}-linux-x86_64"
rm -rf "/tmp/$PORTABLE" && mkdir -p "/tmp/$PORTABLE/icons"
cp "$CARGO_TARGET_DIR/$TARGET/release/tidings" "/tmp/$PORTABLE/tidings"
cp src-tauri/icons/32x32.png src-tauri/icons/128x128.png src-tauri/icons/128x128@2x.png "/tmp/$PORTABLE/icons/"
cp scripts/linux/install.sh scripts/linux/tidings.desktop "/tmp/$PORTABLE/"
chmod +x "/tmp/$PORTABLE/install.sh" "/tmp/$PORTABLE/tidings"
tar -C /tmp -czf "/out/$PORTABLE.tar.gz" "$PORTABLE"

echo "==> Collecting packages"
rm -f /out/*.deb /out/*.rpm /out/*.AppImage /out/SHA256SUMS
find "$BUNDLE_DIR" -maxdepth 2 -type f \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) \
    -exec cp -v {} /out/ \;
(cd /out && sha256sum ./* > SHA256SUMS)
ls -la /out
