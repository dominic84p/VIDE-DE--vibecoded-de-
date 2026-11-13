---
title: 'Installation'
description: 'Build and run VIDE-DE on your system'
---

# Installation Guide

## System Requirements

### Base Dependencies (Arch Linux package names)

```bash
# Wayland + graphics
sudo pacman -S wayland libxkbcommon mesa libinput

# TTY backend (for real hardware)
sudo pacman -S seatd libdrm

# X11 apps support
sudo pacman -S xorg-xwayland
```

Other distributions: install equivalents (`wayland`, `libxkbcommon`, `mesa`, `libinput`, `seatd`, `libdrm`, `xorg-xwayland`).

### Seat Daemon (TTY mode)

The TTY backend requires `seatd` running:

```bash
sudo systemctl enable --now seatd
# Add your user to the seat group
sudo usermod -aG seat $USER
# Log out and back in
```

## Building

### Nested Mode Only (default)

```bash
cargo build --release --workspace
```

### Nested + TTY Backend

```bash
cargo build --release --workspace --features vibe-comp/tty
```

## Running

### Nested Mode (Development)

Runs as a window inside your current Wayland/X11 desktop.

```bash
cargo run --release -- --nested
```

**Useful debug flags:**

| Env Var | Effect |
|---------|--------|
| `VIBE_DEBUG_START=1` | Open Start menu at launch |
| `VIBE_DEBUG_START=text` | Open Start menu + type "text" in search |
| `VIBE_DEBUG_DESKTOPS=1` | Open Task View at launch |
| `VIBE_DEBUG_SWITCHER=1` | Step Alt+Tab after 5s |
| `VIBE_DEBUG_SHOT=1` | Screenshot after 6s (saves to ~/Pictures/Screenshots) |

**Autostart apps:**

```bash
VIBE_STARTUP="konsole" cargo run --release -- --nested
```

**Private D-Bus (avoids tray collision with host desktop):**

```bash
dbus-run-session -- env VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log
```

### TTY Mode (Real Hardware)

:::caution
Never run `--tty` from inside a running desktop session. It competes for the GPU and seat.
:::

1. Switch to a free VT: `Ctrl+Alt+F4`
2. Log in as your normal user
3. Run:

```bash
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log
```

With autostart:

```bash
VIBE_STARTUP="konsole" dbus-run-session -- ./target/release/vibe-comp --tty
```

**Emergency exit:** `Ctrl+Alt+Backspace` always quits.

## Configuration

Settings file: `~/.config/vide-de/settings.conf`

Written by the Settings app, polled by compositor once/second. Live reload works automatically.

## Troubleshooting

| Issue | Fix |
|-------|-----|
| "Permission denied" on DRM | Add user to `video` and `seat` groups |
| Black screen on NVIDIA | Ensure `nvidia-drm.modeset=1` in kernel cmdline |
| XWayland apps slow | Verify `linux-dmabuf` works (check logs for dmabuf feedback) |
| Tray icons missing | Run with private D-Bus: `dbus-run-session --` |
| Seat errors | Ensure `seatd` is running and user in `seat` group |

## Logs

- Nested: stderr output
- TTY: `~/vibe.log` (redirect stderr)
- Debug logging: `RUST_LOG=trace cargo run --release -- --nested`

## Next Steps

- [Architecture Overview](/vide-de/architecture/overview/) — Understand how it works
- [Keybindings](/vide-de/reference/keybindings/) — Learn the shortcuts
- [Configuration](/vide-de/configuration/settings/) — Customize your setup