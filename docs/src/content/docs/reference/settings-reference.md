---
title: 'Settings Reference'
description: 'Complete ~/.config/vide-de/settings.conf reference'
---

# Settings Reference

Complete reference for `~/.config/vide-de/settings.conf` as defined in `config/src/lib.rs`. The compositor polls this file once per second and applies updates live.

## Display & Appearance

| Key | Type | Default | Description |
|---|---|---|---|
| `wallpaper` | path | `~/.local/share/vide-de/wallpapers/bloom.png` | Desktop wallpaper image (drawn by `vide-desktop`) |
| `display_mode` | string | `""` | Monitor mode as `WIDTHxHEIGHT@HZ` (e.g. `2560x1440@144`); empty = monitor's preferred mode. Applied live in TTY mode |
| `display_scale` | float | `1.0` | UI scale, fractional allowed (`1.0` = 100%, `1.5` = 150%; 1.0–3.0). Applied live |
| `night_light` | bool | `false` | Warm the screen to cut blue light |
| `night_light_strength` | float | `0.5` | Night light warmth (`0.0` to `1.0`) |
| `direct_scanout` | bool | `false` | Ultra Performance Mode: bypass compositing for fullscreen apps (off by default for NVIDIA stability) |

## Taskbar, Start Menu & Notifications

| Key | Type | Default | Description |
|---|---|---|---|
| `hide_kde_apps` | bool | `false` | Hide KDE's own apps from the Start menu list and search |
| `toasts` | bool | `true` | Show a popup for new notifications (notifications always land in Action Center) |
| `toast_seconds` | int | `5` | How long a notification popup stays (seconds), unless the app requests its own timeout |
| `clock_24h` | bool | `false` | Display the taskbar clock in 24-hour time (`14:30` instead of `2:30 PM`) |
| `light_mode` | bool | `false` | Windoze 10's "default Windoze mode": light taskbar, Start, Action Center, flyouts and menus instead of dark. Applies live |
| `notification_sound` | string | `ding` | Sound for a new notification: `ding`, `chime`, `pop`, `bell`, `glass` or `none`. Quiet in Focus assist; an app's own `sound-file` hint wins, `suppress-sound` silences it |

## Mouse & Touchpad

| Key | Type | Default | Description |
|---|---|---|---|
| `mouse_flat` | bool | `true` (or KDE setting) | Flat mouse profile (no acceleration); `false` = libinput adaptive curve |
| `mouse_speed` | float | `0.0` (or KDE setting) | Pointer speed from `-1.0` (slowest) to `1.0` (fastest) |
| `tap_to_click` | bool | `true` | Touchpad tap-to-click |
| `natural_scroll` | bool | `false` | Reverse (natural) scrolling direction |
| `disable_while_typing` | bool | `true` | Disable touchpad while typing |

## Keyboard

| Key | Type | Default | Description |
|---|---|---|---|
| `xkb_layout` | string | `""` | XKB keyboard layout(s), comma-separated (e.g. `us`, `us,de`); empty = system default |
| `xkb_variant` | string | `""` | XKB keyboard variant(s); empty = default |
| `xkb_options` | string | `""` | XKB options (e.g. `caps:escape,grp:alt_shift_toggle`); empty = none |
| `key_repeat_delay` | int | `300` | Key repeat delay in milliseconds |
| `key_repeat_rate` | int | `30` | Key repeat rate in Hz |

## Power / Idle

| Key | Type | Default | Description |
|---|---|---|---|
| `idle_lock_minutes` | int | `0` | Idle minutes before lock (`0` = never) |
