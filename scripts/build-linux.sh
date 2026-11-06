#!/usr/bin/env bash
# Builds Linux x86_64 packages (.deb, .rpm, AppImage and a portable tarball)
# inside a container, using Podman or Docker. Works on macOS and Linux hosts.
#
#   scripts/build-linux.sh
#
# Options (environment variables):
#   BUNDLES=deb,rpm,appimage   which Tauri bundles to make
#   MEMORY=1200m               memory limit for the build container
#   JOBS=2                     parallel compile jobs (lower = less memory)
#   LTO=false                  turn off link-time optimization (less memory)
#   LOW_MEMORY=1               lighter settings for the big GTK/WebKit crates
#
# Output: release/linux/
set -euo pipefail
cd "$(dirname "$0")/.."

ENGINE="${ENGINE:-$(command -v podman || command -v docker || true)}"
if [ -z "$ENGINE" ]; then
  echo "Podman or Docker is required." >&2
  exit 1
fi

mkdir -p release/linux
"$ENGINE" build -t tidings-linux-build -f scripts/linux/Containerfile scripts/linux

args=(--rm
  -v "$PWD:/src:ro"
  -v "$PWD/release/linux:/out"
  -v tidings-cargo-registry:/usr/local/cargo/registry
  -v tidings-build-cache:/cache
  -e "BUNDLES=${BUNDLES:-deb,rpm,appimage}")
[ -n "${MEMORY:-}" ] && args+=(--memory "$MEMORY" --memory-swap "$MEMORY")
[ -n "${JOBS:-}" ] && args+=(-e "CARGO_BUILD_JOBS=$JOBS")
[ -n "${LTO:-}" ] && args+=(-e "CARGO_PROFILE_RELEASE_LTO=$LTO")
[ -n "${LOW_MEMORY:-}" ] && args+=(-e "LOW_MEMORY=$LOW_MEMORY")

"$ENGINE" run "${args[@]}" tidings-linux-build bash /src/scripts/linux/build-in-container.sh
echo
echo "Packages are in release/linux/"
