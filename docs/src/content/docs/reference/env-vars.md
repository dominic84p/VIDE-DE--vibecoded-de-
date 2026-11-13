---
title: 'Environment Variables'
description: 'All environment variables used by VIDE-DE'
---

# Environment Variables

## Compositor (`vibe-comp`)

| Variable | Default | Description |
|----------|---------|-------------|
| `VIDE_IPC` | `~/.local/share/vide-de/ipc.sock` | IPC socket path |
| `VIDE_CONFIG_DIR` | `~/.config/vide-de` | Configuration directory |
| `VIDE_STARTUP` | — | Desktop ID to launch on startup |
| `VIBE_DEBUG_START` | — | `1` = open Start menu, `text` = open + type "text" |
| `VIBE_DEBUG_DESKTOPS` | — | `1` = open Task View at launch |
| `VIBE_DEBUG_SWITCHER` | — | `1` = step Alt+Tab after 5s |
| `VIBE_DEBUG_SHOT` | — | `1` = screenshot after 6s |
| `RUST_LOG` | `info` | Logging level (trace/debug/info/warn/error) |
| `XDG_RUNTIME_DIR` | `/run/user/$UID` | Runtime directory (Wayland socket, D-Bus) |
| `WAYLAND_DISPLAY` | `wayland-0` | Wayland socket name |
| `DBUS_SESSION_BUS_ADDRESS` | — | D-Bus session bus (auto if unset) |

## Nested Testing (Important)

```bash
# Private XDG_RUNTIME_DIR prevents portal conflicts
XDG_RUNTIME_DIR=/tmp/vt \
WAYLAND_DISPLAY=wayland-1 \
dbus-run-session -- \
env VIBE_STARTUP=konsole \
./target/release/vibe-comp --nested
```

## TTY Backend

| Variable | Description |
|----------|-------------|
| `VIDE_TTY_DEVICE` | DRM device (e.g., `/dev/dri/card0`) |
| `VIDE_TTY_SEAT` | Seat name (default `seat0`) |

## Built-in Apps

All apps respect:

| Variable | Description |
|----------|-------------|
| `VIDE_CONFIG_DIR` | Config directory for settings |
| `RUST_LOG` | Logging level |

### Settings App (`vide-settings`)

| Variable | Description |
|----------|-------------|
| `VIDE_CONFIG_DIR` | Config directory |

### PowerSlop (`powerslop`)

| Variable | Description |
|----------|-------------|
| `POWERSLOP_CONFIG` | Config file path |

### File Exploder (`file-exploder`)

| Variable | Description |
|----------|-------------|
| `FILE_EXPLODER_START_PATH` | Initial directory |

### Task Mangler (`task-mangler`)

| Variable | Description |
|----------|-------------|
| `TASK_MANGLER_REFRESH_MS` | Refresh interval (default 1000) |

### Snip & Sketchy (`snip-and-sketchy`)

| Variable | Description |
|----------|-------------|
| `SNIP_IPC_SOCKET` | IPC socket (default `$VIDE_IPC`) |

### About VIDE-DE (`vide-about`)

No special variables.

## XWayland

| Variable | Description |
|----------|-------------|
| `XDG_RUNTIME_DIR` | Required for XWayland socket |
| `DISPLAY` | Set by XWayland (e.g., `:1`) |

## Seat Management (TTY)

| Variable | Description |
|----------|-------------|
| `SEATD_SOCK` | seatd socket (default `/run/seatd.sock`) |

## Logging

```bash
# Module-specific
RUST_LOG=vibe_comp::wm=debug,vibe_comp::input=debug,vibe_comp::render=trace

# All debug
RUST_LOG=debug

# With timestamps
RUST_LOG=debug cargo run --release -- --nested 2>&1 | ts '[%Y-%m-%d %H:%M:%S]'
```

## Configuration Override

```bash
# Custom config location
export VIDE_CONFIG_DIR=/etc/vide-de
# Settings: /etc/vide-de/settings.conf
# Pins: /etc/vide-de/pins.conf
```

## Summary for Testing

### Nested (Development)
```bash
XDG_RUNTIME_DIR=/tmp/vt \
WAYLAND_DISPLAY=wayland-1 \
dbus-run-session -- \
env VIBE_STARTUP=konsole VIBE_DEBUG_SHOT=1 \
timeout 15 \
./target/release/vibe-comp --nested 2> /tmp/vibe.log
```

### TTY (Real Hardware)
```bash
# From VT (Ctrl+Alt+F4)
dbus-run-session -- \
./target/release/vibe-comp --tty 2> ~/vibe.log
```

## Related Files

- [CLI Flags](/reference/cli-flags/) — Command-line arguments
- [Installation](/getting-started/installation/) — Running instructions
- [Testing](/development/testing/) — Test procedures