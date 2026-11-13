---
title: 'Settings Reference'
description: 'Complete settings.conf reference with all options'
---

# Settings Reference

Complete reference for `~/.config/vide-de/settings.conf`.

## Format

```ini
# Comments with #
key = value

# Sections ignored (flat namespace)
[section]
key = value
```

## All Options

### Taskbar

```ini
taskbar-height = 40              # int, 24-100
taskbar-position = bottom        # top|bottom|left|right
taskbar-bg = 0078D7FF            # ARGB hex (AABBGGRR)
taskbar-fg = FFFFFFFF            # ARGB hex
taskbar-opacity = 1.0            # float 0.0-1.0
taskbar-auto-hide = false        # bool
taskbar-show-clock = true        # bool
taskbar-clock-format = %H:%M     # strftime
taskbar-show-date = false        # bool
```

### Start Menu

```ini
start-menu-width = 500           # int
start-menu-height = 600          # int
start-menu-max-results = 10      # int
start-menu-show-recent = true    # bool
start-menu-recent-count = 5      # int
start-menu-pinned-apps = konsole.desktop,firefox.desktop,code.desktop,vide-settings.desktop
```

### Window Management

```ini
window-border-width = 1          # int
window-border-color = 0078D7FF   # ARGB hex (active)
window-border-color-inactive = 808080FF  # ARGB hex (inactive)
window-titlebar-height = 32      # int
window-titlebar-bg = 0078D7FF    # ARGB hex
window-titlebar-fg = FFFFFFFF    # ARGB hex
window-snap-distance = 20        # int (px)
window-animation-duration = 150  # int (ms)
```

### Desktops

```ini
desktops-count = 10              # int, 1-20
desktops-names = Main,Code,Web,Chat,Media,,,,,,  # comma-separated, 10 items
desktops-animation = 150         # int (ms)
```

### System Tray

```ini
systray-icon-size = 22           # int
systray-spacing = 4              # int
systray-show-hidden = false      # bool
```

### Screenshot

```ini
screenshot-directory = ~/Pictures/Screenshots
screenshot-filename-format = Screenshot_%Y-%m-%d_%H-%M-%S.png
screenshot-copy-to-clipboard = true
screenshot-notify = true
snip-magnifier = true
snip-magnifier-scale = 2.0
```

### Input

```ini
input-repeat-rate = 25           # int (Hz)
input-repeat-delay = 500         # int (ms)
input-natural-scroll = false     # bool
input-touchpad-tap = true        # bool
input-touchpad-drag = true       # bool
```

### Cursor

```ini
cursor-theme = Adwaita           # string (theme name)
cursor-size = 24                 # int
cursor-animation = true          # bool
```

### XWayland

```ini
xwayland-enabled = true          # bool
xwayland-lazy-start = true       # bool
```

### Layer Shell

```ini
layer-shell-bottom-panel = 0     # int (px reserved)
layer-shell-top-panel = 0        # int (px reserved)
```

### Icons

```ini
icons-theme = Adwaita            # string
icons-fallback-theme = hicolor   # string
icons-default-size = 24          # int
```

## Color Format

All colors are **ARGB hex** (8 digits): `AABBGGRR`

- `AA` = Alpha (00-FF)
- `BB` = Blue
- `GG` = Green
- `RR` = Red

Examples:
- `FF000000` = Opaque black
- `FFFFFFFF` = Opaque white
- `0078D7FF` = VIDE-DE blue (opaque)
- `80000000` = 50% transparent black

## Boolean Values

- `true` / `false`
- `1` / `0`
- `yes` / `no`
- `on` / `off`

## Lists

Comma-separated, no spaces:
```ini
start-menu-pinned-apps = app1.desktop,app2.desktop,app3.desktop
desktops-names = One,Two,Three,Four,Five,Six,Seven,Eight,Nine,Ten
```

## Live Reload

Changes apply within **1 second** — no restart needed.

Settings app (`vide-settings`) writes directly to file.

## CLI Access

```bash
# Get
vide-config get taskbar-height

# Set
vide-config set taskbar-height 48

# List all
vide-config list

# Validate
vide-config validate
```

## File Location

```
~/.config/vide-de/settings.conf
```

Override via `VIDE_CONFIG_DIR`:
```bash
export VIDE_CONFIG_DIR=/custom/path
# Config: /custom/path/settings.conf
```

## Default Values

Run `vide-config list` to see all defaults with descriptions.

## Related Files

- [Configuration](/configuration/settings.md) — Explanations
- [Config Library](/configuration/config-library.md) — Implementation
- [Keybindings](/reference/keybindings.md) — Shortcuts