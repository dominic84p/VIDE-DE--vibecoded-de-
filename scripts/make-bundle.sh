#!/usr/bin/env bash
# Build a prebuilt VIDE-DE for people who can't (or shouldn't) compile it: a folder with every
# binary, an installer you can double-click, and a tarball of it.
#
# Binaries built here on Arch need Arch's glibc, which Linux Mint and Ubuntu don't have yet, so
# the build runs in an Ubuntu 24.04 container (podman). The result runs on Ubuntu 24.04,
# Linux Mint 22 and anything newer.
#
# Usage: scripts/make-bundle.sh
# Output: target/bundle/vide-de/ plus target/bundle/vide-de-x86_64.tar.gz and .zip
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/target/bundle"
IMAGE=docker.io/library/ubuntu:24.04
BINS=(vibe-comp vide-desktop vide-settings vide-about vide-portal powerslop file-exploder task-mangler snip-and-sketchy window-shopping)

mkdir -p "$HOME/.cargo/registry"
echo "Building in $IMAGE (the first run downloads the image and Rust, a few minutes)..."
# A named volume keeps the container's Rust toolchain between runs; the crate registry is
# shared with the host so nothing is downloaded twice.
podman run --rm \
  -v "$ROOT:/src" \
  -v vide-bundle-cargo:/root/.cargo \
  -v vide-bundle-rustup:/root/.rustup \
  -v "$HOME/.cargo/registry:/root/.cargo/registry" \
  -e CARGO_TARGET_DIR=/src/target/ubuntu \
  -w /src "$IMAGE" bash -euc '
    export DEBIAN_FRONTEND=noninteractive PATH=/root/.cargo/bin:$PATH
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends build-essential pkg-config curl ca-certificates \
      libseat-dev libinput-dev libudev-dev libgbm-dev libdrm-dev libxkbcommon-dev libwayland-dev \
      libpipewire-0.3-dev libfontconfig-dev libegl-dev >/dev/null
    command -v cargo >/dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    cargo build --release --locked --workspace --features vibe-comp/tty
  '

STAGE="$OUT/vide-de"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share"
for b in "${BINS[@]}"; do
  install -m755 "$ROOT/target/ubuntu/release/$b" "$STAGE/bin/$b"
done
strip "$STAGE"/bin/* 2>/dev/null || true
for f in vide-session vide-de.desktop vide.portal VIDE-portals.conf org.freedesktop.impl.portal.desktop.vide.service; do
  cp "$ROOT/scripts/$f" "$STAGE/share/"
done
install -m755 "$ROOT/scripts/bundle/install.sh" "$ROOT/scripts/bundle/uninstall.sh" "$STAGE/"
install -m755 "$ROOT/scripts/bundle/Install VIDE-DE.desktop" "$STAGE/"
cp "$ROOT/scripts/bundle/README.txt" "$STAGE/"
git -C "$ROOT" log -1 --format='Built from %h (%cs): %s' > "$STAGE/VERSION"

tar -C "$OUT" -czf "$OUT/vide-de-x86_64.tar.gz" vide-de
# A zip too, for people who just double-click; bsdtar keeps the installer executable inside it.
(cd "$OUT" && rm -f vide-de-x86_64.zip && bsdtar -a -cf vide-de-x86_64.zip vide-de)
echo "Bundle: $STAGE"
echo "Archives: $OUT/vide-de-x86_64.tar.gz and .zip ($(du -h "$OUT/vide-de-x86_64.zip" | cut -f1))"
