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
- **Animations**: Ease of Access → Display → *Show animations* (`animations`). Windows zoom open (130 ms) and shrink toward the taskbar when minimized (110 ms); Start rises, the Action Center slides in from the right, and flyouts, Task View and menus fade in (70–150 ms). Closing is instant, and input never waits for an animation. Turn it off and everything appears at once.
- **Display (Live TTY Switching)**: Changing `display_mode` or `display_scale` (100%, 125%, 150%, 175%, 200%, 250% or 300%) applies live on the TTY backend without logging out — resizing the logical output, re-fitting maximized/fullscreen windows, pulling stranded windows back on screen, and repainting cached chrome. The scale also applies nested. Apps that support `wp_fractional_scale` (GTK 4, Qt 6, egui, Chromium/Electron, Firefox) draw sharp at in-between scales; X11 and older apps are stretched. Our own chrome is painted at the scale rounded up and scaled down, and screenshots capture every physical pixel.
- **Notifications**: Toggle toast popups (`toasts`) on/off, set popup duration (`toast_seconds`), and pick the notification sound (`notification_sound`, with a Play button). Our sounds are synthesised by `scripts/make-sounds.py`. Notifications always land in the Action Center.
- **Touchpad**: Dedicated page for tap-to-click, natural scrolling, and disable-while-typing.
- **Keyboard**: Pick up to 4 XKB layouts & variants from `xkb`'s list (automatically adding `Alt+Shift` switching when >1), remap Caps Lock to Escape or Ctrl, and tune repeat delay and rate.
- **Colors** (Personalization): "Choose your default Windoze mode", Light or Dark (`light_mode`), for the taskbar, Start, Action Center, flyouts and menus. Switches live.
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
