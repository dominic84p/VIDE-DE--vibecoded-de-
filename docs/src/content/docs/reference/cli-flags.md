---
title: 'CLI Flags'
description: 'Command-line arguments for vibe-comp'
---

# CLI Flags (`vibe-comp`)

## Usage

```bash
vibe-comp [OPTIONS]
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--nested` | `-n` | Run nested (winit backend) — **default** |
| `--tty` | `-t` | Run on real hardware (TTY/DRM/KMS backend) |
| `--version` | `-V` | Print version and exit |
| `--help` | `-h` | Print help and exit |

## Examples

```bash
# Nested (default)
vibe-comp
vibe-comp --nested
vibe-comp -n

# TTY (real hardware)
vibe-comp --tty
vibe-comp -t

# Version
vibe-comp --version
vibe-comp -V
```

## Backend Selection

### Nested (`--nested`)

- Uses `winit` backend
- Runs as window inside current Wayland/X11 desktop
- No special permissions needed
- Good for development/testing

### TTY (`--tty`)

- Uses DRM/KMS + libinput + libseat
- Runs on real hardware (VT)
- Requires `seatd` running, user in `seat` group
- **Never run from inside a desktop session**

## Environment Variables

| Variable | Description |
|----------|-------------|
| `VIDE_IPC` | IPC socket path (default `~/.local/share/vide-de/ipc.sock`) |
| `VIDE_CONFIG_DIR` | Config directory (default `~/.config/vide-de`) |
| `VIDE_STARTUP` | App desktop ID to launch on startup |
| `VIBE_DEBUG_START` | `1` or `text` — open Start menu at launch |
| `VIBE_DEBUG_DESKTOPS` | `1` — open Task View at launch |
| `VIBE_DEBUG_SWITCHER` | `1` — step Alt+Tab after 5s |
| `VIBE_DEBUG_SHOT` | `1` — screenshot after 6s |
| `RUST_LOG` | Logging level (trace, debug, info, warn, error) |
| `XDG_RUNTIME_DIR` | Runtime directory (for nested testing) |
| `WAYLAND_DISPLAY` | Wayland socket (for nested testing) |

## Debug Flags Summary

```bash
# Full debug nested run
RUST_LOG=debug \
VIBE_DEBUG_START=text \
VIBE_DEBUG_SHOT=1 \
XDG_RUNTIME_DIR=/tmp/vt \
WAYLAND_DISPLAY=wayland-1 \
dbus-run-session -- \
timeout 15 \
./target/release/vibe-comp --nested 2> /tmp/vibe.log
```

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Clean exit |
| `1` | Initialization error |
| `2` | Backend error |
| `3` | Configuration error |
| `130` | Interrupted (SIGINT/SIGTERM) |

## Related Files

- [Installation](/getting-started/installation/) — Running instructions
- [Testing](/development/testing/) — Test procedures
- [Environment Variables](/reference/env-vars/) — Full env var list