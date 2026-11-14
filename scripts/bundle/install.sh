#!/usr/bin/env bash
# Install the prebuilt VIDE-DE in this folder. Made for Linux Mint 22 / Ubuntu 24.04 and newer.
# Double-click "Install VIDE-DE" next to this file, or run ./install.sh in a terminal.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$(id -u)" -ne 0 ]; then
  echo "VIDE-DE installer"
  echo "This needs your password to install into /usr/local and add the login session."
  exec sudo -- "$0" "$@"
fi

# Libraries the binaries link, plus what a session needs: Xwayland for X11 apps, portals for
# file pickers and screen sharing, Noto fonts for our UI, a polkit agent to mount drives.
if command -v apt-get >/dev/null; then
  echo "Installing what VIDE-DE needs from your distro..."
  apt-get install -y --no-install-recommends \
    libseat1 libinput10 libgbm1 libxkbcommon0 libudev1 xwayland \
    xdg-desktop-portal xdg-desktop-portal-gtk pipewire wireplumber \
    fonts-noto-core fonts-noto-color-emoji policykit-1-gnome udisks2 \
    || echo "Some packages didn't install; VIDE-DE may still work. Carrying on."
  # Ubuntu 24.04 renamed this one (t64); older releases use the plain name.
  apt-get install -y libpipewire-0.3-0t64 2>/dev/null || apt-get install -y libpipewire-0.3-0 || true
fi

echo "Copying VIDE-DE..."
for b in "$HERE"/bin/*; do
  install -Dm755 "$b" "/usr/local/bin/$(basename "$b")"
done
install -Dm755 "$HERE/share/vide-session" /usr/local/bin/vide-session
install -Dm644 "$HERE/share/vide-de.desktop" /usr/share/wayland-sessions/vide-de.desktop
install -Dm644 "$HERE/share/vide.portal" /usr/share/xdg-desktop-portal/portals/vide.portal
install -Dm644 "$HERE/share/VIDE-portals.conf" /usr/share/xdg-desktop-portal/vide-portals.conf
install -Dm644 "$HERE/share/VIDE-portals.conf" /usr/share/xdg-desktop-portal/vide-de-portals.conf
install -Dm644 "$HERE/share/org.freedesktop.impl.portal.desktop.vide.service" \
  /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.vide.service

echo
echo "Done. VIDE-DE is installed."
echo "Log out, click the session icon on the login screen, pick VIDE-DE, and log in."
echo "If anything goes wrong, the log is ~/vibe.log. To remove it: ./uninstall.sh"
