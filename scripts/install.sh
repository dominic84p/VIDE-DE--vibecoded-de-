#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# What gets built and installed:
#   ./scripts/install.sh        everything (first install, or after compositor changes)
#   ./scripts/install.sh apps   just the apps: quick, and no logout needed, since each
#                               app starts fresh from /usr/local/bin when you open it
#   ./scripts/install.sh comp   just the compositor (takes effect at next login)
APPS=(
  vide-desktop
  vide-settings
  vide-about
  vide-portal
  powerslop
  file-exploder
  task-mangler
  snip-and-sketchy
  window-shopping
)
COMP=(vibe-comp)

MODE=all
INSTALL_ONLY=0
for arg in "$@"; do
  case "$arg" in
    all | apps | comp) MODE="$arg" ;;
    --install-only) INSTALL_ONLY=1 ;;
    *)
      echo "usage: $0 [all|apps|comp]" >&2
      exit 1
      ;;
  esac
done

case "$MODE" in
  all) BINS=("${COMP[@]}" "${APPS[@]}") ;;
  apps) BINS=("${APPS[@]}") ;;
  comp) BINS=("${COMP[@]}") ;;
esac

build() {
  case "$MODE" in
    all) cargo build --release --workspace --features vibe-comp/tty ;;
    apps) cargo build --release $(printf -- '-p %s ' "${APPS[@]}") ;;
    comp) cargo build --release -p vibe-comp --features tty ;;
  esac
}

# Build as the normal user before elevating so target/ stays user-owned.
if [ "$(id -u)" -ne 0 ]; then
  echo "Building VIDE-DE release binaries ($MODE)..."
  build
  echo "Installing system-wide (sudo required)..."
  exec sudo "$0" --install-only "$MODE"
fi

if [ "$INSTALL_ONLY" -ne 1 ]; then
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

if [ "$MODE" = apps ]; then
  echo "Updated the apps in /usr/local/bin. Open them again to get the new versions; no logout needed."
  echo "vide-desktop and vide-portal keep running until restarted (log out, or restart them yourself)."
  exit 0
fi
if [ "$MODE" = comp ]; then
  echo "Updated the compositor. Log out and back in to run it."
  exit 0
fi

install -Dm755 "$ROOT/scripts/vide-session" /usr/local/bin/vide-session
install -Dm644 "$ROOT/scripts/vide-de.desktop" /usr/share/wayland-sessions/vide-de.desktop
install -Dm644 "$ROOT/scripts/vide.portal" /usr/share/xdg-desktop-portal/portals/vide.portal
rm -f /usr/share/xdg-desktop-portal/VIDE-portals.conf
install -Dm644 "$ROOT/scripts/VIDE-portals.conf" /usr/share/xdg-desktop-portal/vide-portals.conf
install -Dm644 "$ROOT/scripts/VIDE-portals.conf" /usr/share/xdg-desktop-portal/vide-de-portals.conf
install -Dm644 "$ROOT/scripts/org.freedesktop.impl.portal.desktop.vide.service" /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.vide.service

# Hook up KWallet PAM auto-unlock for SDDM if not already configured.
if [ -x "$ROOT/scripts/setup-keyring-pam.sh" ]; then
  "$ROOT/scripts/setup-keyring-pam.sh"
fi

echo "Installed VIDE-DE to /usr/local/bin and /usr/share/wayland-sessions/vide-de.desktop."
echo "Log out to SDDM and select 'VIDE-DE' from the session menu."
