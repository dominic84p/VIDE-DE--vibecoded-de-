---
title: 'Settings'
description: 'Configuration file format, live reload, all available options'
---

# Settings (`~/.config/vide-de/settings.conf`)

Single key=value file shared by compositor and Settings app. Polled once/second for live reload.

## File Format

```ini
# Comments start with #
# Section headers [section] are ignored (flat namespace)

# Example
taskbar-height = 40
taskbar-position = bottom
theme = dark
```

## All Settings

### Taskbar

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `taskbar-height` | int | 40 | Taskbar height in pixels |
| `taskbar-position` | enum | bottom | `top`, `bottom`, `left`, `right` |
| `taskbar-bg` | hex | 0078D7 | Background color (ARGB) |
| `taskbar-fg` | hex | FFFFFFFF | Text/icon color (ARGB) |
| `taskbar-opacity` | float | 1.0 | 0.0-1.0 |
| `taskbar-auto-hide` | bool | false | Auto-hide when not hovered |
| `taskbar-show-clock` | bool | true | Show clock |
| `taskbar-clock-format` | str | %H:%M | strftime format |
| `taskbar-show-date` | bool | false | Show date in clock |

### Start Menu

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `start-menu-width` | int | 500 | Width in pixels |
| `start-menu-height` | int | 600 | Height in pixels |
| `start-menu-max-results` | int | 10 | Max search results |
| `start-menu-show-recent` | bool | true | Show recent apps |
| `start-menu-recent-count` | int | 5 | Number of recent apps |
| `start-menu-pinned-apps` | list | [...] | Pinned app desktop IDs |

### Window Management

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `window-border-width` | int | 1 | Border width (SSD) |
| `window-border-color` | hex | 0078D7 | Active border color |
| `window-border-color-inactive` | hex | 808080 | Inactive border color |
| `window-titlebar-height` | int | 32 | Titlebar height (SSD) |
| `window-titlebar-bg` | hex | 0078D7 | Titlebar background |
| `window-titlebar-fg` | hex | FFFFFFFF | Titlebar text color |
| `window-snap-distance` | int | 20 | Snap trigger distance |
| `window-animation-duration` | int | 150 | Animation ms |

### Desktops (Workspaces)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `desktops-count` | int | 10 | Number of workspaces |
| `desktops-names` | list | ["1".."10"] | Workspace names |
| `desktops-animation` | int | 150 | Switch animation ms |

### System Tray

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `systray-icon-size` | int | 22 | Icon size |
| `systray-spacing` | int | 4 | Icon spacing |
| `systray-show-hidden` | bool | false | Show hidden icons |

### Screenshot

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `screenshot-directory` | path | ~/Pictures/Screenshots | Save location |
| `screenshot-filename-format` | str | Screenshot_%Y-%m-%d_%H-%M-%S.png | strftime format |
| `screenshot-copy-to-clipboard` | bool | true | Copy to clipboard |
| `screenshot-notify` | bool | true | Show notification |
| `snip-magnifier` | bool | true | Show magnifier on snip |
| `snip-magnifier-scale` | float | 2.0 | Magnifier zoom |

### Input

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `input-repeat-rate` | int | 25 | Key repeat rate (Hz) |
| `input-repeat-delay` | int | 500 | Initial repeat delay (ms) |
| `input-natural-scroll` | bool | false | Natural scroll direction |
| `input-touchpad-tap` | bool | true | Tap to click |
| `input-touchpad-drag` | bool | true | Tap and drag |

### Cursor

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `cursor-theme` | str | Adwaita | XCursor theme name |
| `cursor-size` | int | 24 | Cursor size |
| `cursor-animation` | bool | true | Animated cursors |

### XWayland

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `xwayland-enabled` | bool | true | Enable XWayland |
| `xwayland-lazy-start` | bool | true | Start on first X11 app |

### Layer Shell

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `layer-shell-bottom-panel` | int | 0 | Reserve space for bottom panel |
| `layer-shell-top-panel` | int | 0 | Reserve space for top panel |

## Live Reload

```rust
// In State - called every second
fn reload_config_if_changed(&mut self) {
    if self.config.poll_changed() {
        self.apply_config();
        self.needs_redraw = true;
    }
}
```

Settings app writes to file → compositor picks up within 1 second → applies immediately.

## Configuration via Settings App

Run `vide-settings` (GUI) or use CLI:

```bash
# Set a value
vide-config set taskbar-height 48

# Get a value
vide-config get taskbar-height

# List all
vide-config list
```

## Environment Variable Override

```bash
# Override config directory
export VIDE_CONFIG_DIR=/custom/path

# Config file: $VIDE_CONFIG_DIR/settings.conf
# Default: ~/.config/vide-de/settings.conf
```

## Related Files

- `config/` — vide-config crate
- `settings/` — Settings GUI app
- `ui.rs` — Color/theme application
- `wm.rs` — Window decoration settings
- `input.rs` — Input settings