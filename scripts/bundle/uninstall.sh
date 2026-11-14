#!/usr/bin/env bash
# Remove a VIDE-DE installed by install.sh. Your settings in ~/.config/vide-de stay.
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  exec sudo -- "$0" "$@"
fi

for b in vibe-comp vide-desktop vide-settings vide-about vide-portal powerslop file-exploder \
  task-mangler snip-and-sketchy window-shopping vide-session; do
  rm -f "/usr/local/bin/$b"
done
rm -f /usr/share/wayland-sessions/vide-de.desktop \
  /usr/share/xdg-desktop-portal/portals/vide.portal \
  /usr/share/xdg-desktop-portal/vide-portals.conf \
  /usr/share/xdg-desktop-portal/vide-de-portals.conf \
  /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.vide.service
echo "VIDE-DE removed. Your settings are still in ~/.config/vide-de if you come back."
