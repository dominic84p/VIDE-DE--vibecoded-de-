---
title: 'Multi-Monitor Setup'
description: 'Configuring and using multiple monitors'
---

# Multi-Monitor Setup

## Automatic Detection

Monitors are detected automatically via `wl_output` events. No config needed for basic usage.

## Output Configuration

### Via Settings App

Run `vide-settings` → Displays:
- Enable/disable outputs
- Set resolution/refresh rate
- Set position (drag to arrange)
- Set primary
- Set scale factor

### Via Config File

```ini
# ~/.config/vide-de/settings.conf
# Output settings are managed by compositor, not config file
# Use vide-settings or wlr-randr
```

### Via CLI (wlr-randr)

```bash
# List outputs
wlr-randr

# Configure
wlr-randr --output HDMI-A-1 --mode 1920x1080@60 --pos 0,0
wlr-randr --output DP-1 --mode 2560x1440@144 --pos 1920,0 --primary
```

## Workspace Behavior

### Per-Monitor Workspaces (Current)

Each monitor shows the **same workspace**. Switching workspace affects all monitors.

```
Monitor 1 (Primary)     Monitor 2
┌─────────────────┐    ┌─────────────────┐
│  Workspace 1    │    │  Workspace 1    │
│  [Win A] [Win B]│    │  [Win C]        │
└─────────────────┘    └─────────────────┘
```

### Future: Independent Workspaces

Planned: Each monitor gets independent workspace stack.

## Window Placement

### New Windows

Open on **current monitor** (where pointer is, or primary if unsure).

### Fullscreen

Fullscreens on **the monitor the window is on**.

### Maximize

Maximizes to **monitor geometry minus taskbar** (if taskbar on that monitor).

## Taskbar

### Current Behavior

Taskbar shows on **primary monitor only** (or all if configured).

```ini
# Future config
taskbar-position = bottom
taskbar-monitors = all  # or primary, or list
```

## Layer Shell

### Desktop (vide-desktop)

Spans **all monitors** as single background layer surface.

```rust
// desktop/main.rs
layer_shell.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
layer_shell.set_exclusive_zone(0);  // No exclusive zone
```

### Panels

External panels can reserve space per-monitor via `exclusive_zone`.

## XWayland

XWayland sees **one big X screen** spanning all monitors (Xinerama-style).

```bash
# Check with xrandr inside XWayland app
xrandr
# Shows combined resolution
```

## Gaming

### Fullscreen Games

Fullscreen on **monitor where window originated**. Use game settings to choose monitor.

### Borderless Windowed

Treat as maximized window — respects monitor boundaries.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Wrong primary | `wlr-randr --output DP-1 --primary` |
| Mirrored instead of extended | Check `wlr-randr` output, disable mirror |
| Resolution wrong | `wlr-randr --output HDMI-A-1 --mode 1920x1080@60` |
| Taskbar on wrong monitor | Set primary via `wlr-randr` |
| Fullscreen on wrong monitor | Move window first, then fullscreen |
| Scaling issues | `wlr-randr --output DP-1 --scale 2` |

## Logs

```bash
RUST_LOG=vibe_comp::output=debug ./target/release/vibe-comp --nested
# Shows output events, modes, positions
```

## Related

- [Configuration](/vide-de/configuration/settings)
- [Architecture](/vide-de/architecture/overview/)
- [Layer Shell](/vide-de/components/layers/)
- [XWayland](/vide-de/components/xwayland/)