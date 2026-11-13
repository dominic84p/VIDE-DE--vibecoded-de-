---
title: 'Building & Testing'
description: 'Build instructions, test procedures, debug flags'
---

# Building & Testing

## Prerequisites

```bash
# Rust (latest stable)
rustup update stable

# System dependencies (Arch Linux)
sudo pacman -S wayland libxkbcommon mesa libinput seatd libdrm xorg-xwayland

# For TTY backend
sudo systemctl enable --now seatd
sudo usermod -aG seat $USER
# Log out and back in
```

## Build Commands

### Nested Mode Only (Default)

```bash
cargo build --release --workspace
```

### Nested + TTY Backend

```bash
cargo build --release --workspace --features vibe-comp/tty
```

### Debug Build (for debugging)

```bash
cargo build --workspace
# Note: Debug builds are slow due to CPU-painted UI (opt-level = 1)
```

## Running Tests

### Nested Mode (Development)

```bash
# Basic run
cargo run --release -- --nested

# With autostart app
VIBE_STARTUP=konsole cargo run --release -- --nested

# With debug flags
VIBE_DEBUG_START=1 cargo run --release -- --nested
VIBE_DEBUG_START=text cargo run --release -- --nested
VIBE_DEBUG_DESKTOPS=1 cargo run --release -- --nested
VIBE_DEBUG_SWITCHER=1 cargo run --release -- --nested
VIBE_DEBUG_SHOT=1 cargo run --release -- --nested

# Private D-Bus (avoids tray collision)
dbus-run-session -- env VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log

# Private XDG_RUNTIME_DIR (avoids portal conflicts)
XDG_RUNTIME_DIR=/tmp/vt WAYLAND_DISPLAY=wayland-1 dbus-run-session -- env VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested
```

### TTY Mode (Real Hardware)

**⚠️ Never from inside a desktop session.** Switch to free VT (Ctrl+Alt+F4).

```bash
# Basic
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log

# With autostart
VIBE_STARTUP=konsole dbus-run-session -- ./target/release/vibe-comp --tty

# Emergency exit: Ctrl+Alt+Backspace
```

## Debug Flags Reference

| Env Var | Values | Effect |
|---------|--------|--------|
| `VIBE_DEBUG_START` | `1`, `text` | Open Start menu at launch |
| `VIBE_DEBUG_DESKTOPS` | `1` | Open Task View at launch |
| `VIBE_DEBUG_SWITCHER` | `1` | Step Alt+Tab after 5s |
| `VIBE_DEBUG_SHOT` | `1` | Screenshot after 6s |
| `VIBE_STARTUP` | app desktop ID | Launch app on startup |
| `RUST_LOG` | trace,debug,info,warn,error | Logging level |

## Logging

```bash
# Full trace
RUST_LOG=trace cargo run --release -- --nested

# Specific modules
RUST_LOG=vibe_comp::wm=debug,vibe_comp::input=debug cargo run --release -- --nested

# With pretty output
RUST_LOG=info cargo run --release -- --nested 2>&1 | grep -v "^$"
```

## Common Issues

| Issue | Fix |
|-------|-----|
| "Permission denied" on DRM | `sudo usermod -aG video,seat $USER` then relogin |
| Black screen on NVIDIA | Kernel: `nvidia-drm.modeset=1` |
| XWayland apps slow | Check dmabuf in logs: `RUST_LOG=debug` |
| Tray icons missing | Use `dbus-run-session --` |
| Seat errors | Ensure `seatd` running, user in `seat` group |
| Portal issues (Flatpak broken) | Use private `XDG_RUNTIME_DIR` |

## CI/CD

GitHub Actions runs:
1. `cargo check --workspace`
2. `cargo build --release --workspace`
3. `cargo build --release --workspace --features vibe-comp/tty`
4. `cargo test --workspace`

## Profiling

```bash
# CPU profiling
cargo build --release --workspace
perf record ./target/release/vibe-comp --nested
perf report

# Memory
heaptrack ./target/release/vibe-comp --nested
```

## Related Files

- [Installation](/getting-started/installation/) — System requirements
- [Architecture](/architecture/overview/) — Component overview
- [Testing](/development/testing/) — Detailed test procedures