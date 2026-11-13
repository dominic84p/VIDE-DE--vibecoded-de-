---
title: 'Settings'
description: 'VIDE-DE Settings app (vide-settings) and ~/.config/vide-de/settings.conf'
---

# Settings (`~/.config/vide-de/settings.conf`)

VIDE-DE stores its configuration in a flat `key=value` file at `~/.config/vide-de/settings.conf`, shared between `vide-settings` and the compositor. The compositor polls it **once per second** and applies changes live.

## Settings App (`vide-settings`)

Launch **Settings** from the Start menu or open any page directly with `--page <name>`:

```bash
vide-settings --page display
vide-settings --page privacy
vide-settings --page touchpad
vide-settings --page keyboard
vide-settings --page notifications
vide-settings --page gaming
```

### Recent Highlights

- **Privacy & App Permissions**: Manage per-app portal permissions (`flatpak permissions`) for camera, microphone, notifications (Allow / Block / Ask), location (reset), and screen sharing (Revoke, including VIDE's own screen-share list).
- **Display (Live TTY Switching)**: Changing `display_mode` or `display_scale` (100% or 200%) applies live on the TTY backend without logging out — resizing the logical output, re-fitting maximized/fullscreen windows, pulling stranded windows back on screen, and repainting cached chrome.
- **Notifications**: Toggle toast popups (`toasts`) on/off and set popup duration (`toast_seconds`). Notifications always land in the Action Center.
- **Touchpad**: Dedicated page for tap-to-click, natural scrolling, and disable-while-typing.
- **Keyboard**: Pick up to 4 XKB layouts & variants from `xkb`'s list (automatically adding `Alt+Shift` switching when >1), remap Caps Lock to Escape or Ctrl, and tune repeat delay and rate.
- **Colors** (Personalization): "Choose your default Windows mode", Light or Dark (`light_mode`), for the taskbar, Start, Action Center, flyouts and menus. Switches live.
- **Date & Time**: Toggle 24-hour taskbar clock (`clock_24h`) and pick region/city timezone via `timedatectl`.

## File Format

```ini
# Comments start with #
wallpaper=/home/user/.local/share/vide-de/wallpapers/bloom.png
display_scale=1.0
clock_24h=true
toasts=true
toast_seconds=5
```

See the [Settings Reference](/vide-de/reference/settings-reference/) for every `settings.conf` key.
