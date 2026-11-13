---
title: 'Troubleshooting'
description: 'Common issues and solutions'
---

# Troubleshooting

## Build Issues

### `smithay` version conflicts

```bash
cargo update -p smithay
cargo clean
cargo build --release --workspace
```

### Missing system dependencies

```bash
# Arch
sudo pacman -S wayland libxkbcommon mesa libinput seatd libdrm xorg-xwayland

# Fedora
sudo dnf install wayland-devel libxkbcommon-devel mesa-libgbm-devel libinput-devel seatd-devel libdrm-devel xorg-x11-server-Xwayland

# Ubuntu/Debian
sudo apt install libwayland-dev libxkbcommon-dev libgbm-dev libinput-dev seatd libdrm-dev xwayland
```

### `resvg` / `fontdue` build fails

```bash
# Install build deps
sudo pacman -S clang lld  # or gcc
cargo clean
cargo build --release
```

## Runtime Issues

### Black Screen / No Output

| Cause | Fix |
|-------|-----|
| NVIDIA no modeset | Kernel: `nvidia-drm.modeset=1` |
| Wrong DRM device | `VIDE_TTY_DEVICE=/dev/dri/card1` |
| No seatd | `sudo systemctl enable --now seatd` |
| User not in video/seat | `sudo usermod -aG video,seat $USER` |

### Nested Mode Issues

| Issue | Fix |
|-------|-----|
| Tray icons missing | Use `dbus-run-session --` |
| Portal conflicts (Flatpak breaks) | Use private `XDG_RUNTIME_DIR=/tmp/vt` |
| Wayland display conflict | Use `WAYLAND_DISPLAY=wayland-1` |
| Host desktop captures keys | Click into nested window first |

### TTY Mode Issues

| Issue | Fix |
|-------|-----|
| "Permission denied" DRM | Add user to `video` and `seat` groups |
| Input not working | Ensure `seatd` running, user in `seat` |
| VT switch doesn't work | `Ctrl+Alt+F1`-`F7` to switch |
| GPU hang | Check `dmesg`, try `nvidia-drm.modeset=1` |

### XWayland Issues

| Issue | Fix |
|-------|-----|
| X11 apps won't start | Ensure `xorg-xwayland` installed |
| Games slow (no GPU) | Verify dmabuf in logs: `RUST_LOG=vibe_comp::xwayland=debug` |
| X11 windows not managed | Check `manage_x11` in logs |
| Pointer grab escapes | Ensure relative-pointer + pointer-constraints protocols |

### Rendering Issues

| Issue | Fix |
|-------|-----|
| Flickering frames | NVIDIA: ensure dmabuf sync, `FrameFlags::empty()` |
| Stale/duplicated frames | NVIDIA: linux-drm-syncobj required |
| Black rectangles | dmabuf feedback missing — check `init_dmabuf()` |
| Titlebars wrong size | Check `window-titlebar-height` setting |
| CSD apps have double titlebar | App requests CSD — we respect it (`w.ssd = false`) |

### Input Issues

| Issue | Fix |
|-------|-----|
| Keys not working | Check `RUST_LOG=vibe_comp::input=debug` |
| Media keys not working | Ensure `libinput` has keyboard access |
| Touchpad not working | Check `libinput` device, `input-touchpad-tap` setting |
| Alt+Tab not working | Check `KeyboardFocus` state in logs |

### Config Issues

| Issue | Fix |
|-------|-----|
| Settings not applying | Wait 1s (poll interval), check `vide-config validate` |
| Config file corrupt | `vide-config reset` |
| Colors wrong | Use ARGB hex: `AABBGGRR` (not RRGGBB) |

## Logs

### Enable Debug Logging

```bash
# All modules
RUST_LOG=debug ./target/release/vibe-comp --nested

# Specific modules
RUST_LOG=vibe_comp::wm=debug,vibe_comp::input=debug,vibe_comp::render=trace ./target/release/vibe-comp --nested

# TTY mode
RUST_LOG=debug dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log
```

### Log Locations

| Mode | Location |
|------|----------|
| Nested | stderr (terminal) |
| TTY | `~/vibe.log` (redirect stderr) |
| Screenshots | `~/Pictures/Screenshots/` |

### Key Log Patterns

```bash
# Errors
grep -i error /tmp/vibe.log

# Panics
grep -i panic /tmp/vibe.log

# XWayland dmabuf
grep -i dmabuf /tmp/vibe.log

# Seat/input
grep -i "seat\|input" /tmp/vibe.log

# Config
grep -i config /tmp/vibe.log
```

## NVIDIA-Specific

See [NVIDIA Guide](/vide-de/wiki/nvidia-guide/) for detailed NVIDIA troubleshooting.

## Performance

### High CPU Usage

- Debug build? Use `--release`
- UI cache thrashing? Check `needs_redraw` spam in logs
- Too many windows? Close unused

### Low FPS

- NVIDIA: check dmabuf sync
- XWayland games: ensure relative-pointer works
- Release build? `cargo build --release`

### Memory Leaks

```bash
heaptrack ./target/release/vibe-comp --nested
# Check heaptrack GUI for growth
```

## Getting Help

1. Check [FAQ](/vide-de/wiki/faq/)
2. Search [GitHub Issues](https://github.com/dominic84p/vide-de/issues)
3. Open new issue with:
   - `cargo build --release --workspace` output
   - `RUST_LOG=debug` logs
   - System info: `uname -a`, `nvidia-smi` (if NVIDIA)
   - Steps to reproduce

## Related

- [FAQ](/vide-de/wiki/faq/)
- [NVIDIA Guide](/vide-de/wiki/nvidia-guide/)
- [Testing](/vide-de/development/testing/)
- [Installation](/vide-de/getting-started/installation/)