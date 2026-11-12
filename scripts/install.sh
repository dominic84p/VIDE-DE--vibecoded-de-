#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BINS=(
  vibe-comp
  vide-desktop
  vide-settings
  vide-about
  vide-portal
  powerslop
  file-exploder
  task-mangler
  snip-and-sketchy
)

# Build as the normal user before elevating so target/ stays user-owned.
if [ "$(id -u)" -ne 0 ]; then
  echo "Building VIDE-DE release binaries (with TTY backend)..."
  cargo build --release --workspace --features vibe-comp/tty
  echo "Installing system-wide (sudo required)..."
  exec sudo "$0" --install-only
fi

if [ "${1:-}" != "--install-only" ]; then
  for b in "${BINS[@]}"; do
    if [ ! -x "$ROOT/target/release/$b" ]; then
      echo "Missing $ROOT/target/release/$b. Run ./scripts/install.sh as your normal user first." >&2
      exit 1
    fi
  done
fi

for b in "${BINS[@]}"; do
  install -Dm755 "$ROOT/target/release/$b" "/usr/local/bin/$b"
done

install -Dm755 "$ROOT/scripts/vide-session" /usr/local/bin/vide-session
install -Dm644 "$ROOT/scripts/vide-de.desktop" /usr/share/wayland-sessions/vide-de.desktop
install -Dm644 "$ROOT/scripts/vide.portal" /usr/share/xdg-desktop-portal/portals/vide.portal
install -Dm644 "$ROOT/scripts/VIDE-portals.conf" /usr/share/xdg-desktop-portal/VIDE-portals.conf
install -Dm644 "$ROOT/scripts/org.freedesktop.impl.portal.desktop.vide.service" /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.vide.service

# Hook up KWallet PAM auto-unlock for SDDM if not already configured.
if [ -x "$ROOT/scripts/setup-keyring-pam.sh" ]; then
  "$ROOT/scripts/setup-keyring-pam.sh"
fi

echo "Installed VIDE-DE to /usr/local/bin and /usr/share/wayland-sessions/vide-de.desktop."
echo "Log out to SDDM and select 'VIDE-DE' from the session menu."
