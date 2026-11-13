---
title: 'Overview'
description: 'A Wayland desktop environment with its own compositor, written in Rust and vibecoded end to end.'
---

**VIDE-DE** (`vibecoded-de`) is a Wayland compositor and Windows 10-inspired desktop shell built on **Smithay 0.7**, written entirely in Rust through AI-assisted "vibecoding".

:::note[AI-Only Project]
All code, commits, PRs, and reviews are AI-written. See [Contributing Rules](/vide-de/development/contributing/) before submitting changes.
:::

## Features at a Glance

- **Dual Backend Architecture** — Runs nested inside another desktop (`winit`) or on real hardware (`--tty` with DRM/KMS, libinput, and libseat). Both backends share the same rendering pipeline.
- **Windows 10-Style Desktop** — Bottom taskbar with Start button, search box, window buttons, system tray (PipeWire volume, brightness, SNI app icons), Action Center, 10 virtual desktops, Task View, and edge/corner window snapping.
- **CPU-Painted UI** — All compositor chrome (taskbar, start menu, titlebars) is painted on the CPU into cached `MemoryRenderBuffer`s via `canvas.rs` and composited on the GPU with damage tracking.
- **XWayland + Gaming** — GPU-accelerated X11 apps via `linux-dmabuf` with explicit sync (`linux-drm-syncobj`), relative pointer + pointer constraints for games, and optional direct scanout.
- **Six Built-in Apps** — `Settings`, `PowerSlop` (terminal), `File Exploder`, `Task Mangler`, `Snip & Sketchy`, and `About VIDE-DE`, automatically registered as `.desktop` entries.
- **Health & Freeze Diagnostics** — 5s `xdg` client ping watchdog (`(Not responding)` titlebar), 3s main-loop stall watchdog, 1.5s TTY page-flip watchdog, and protocol error disconnect logging.
- **Layer Shell Desktop** — Separate `vide-desktop` process on `wlr-layer-shell` handling wallpapers, `~/Desktop` icons, Recycle Bin, and desktop right-click menus.

## Quick Links

| Section | Description |
|---|---|
| [Installation](/vide-de/getting-started/installation/) | Build and run VIDE-DE |
| [Architecture Overview](/vide-de/architecture/overview/) | How the compositor works |
| [Window Manager](/vide-de/components/window-manager/) | Stacking, snapping, popups, and focus |
| [Health & Diagnostics](/vide-de/components/health/) | Ping & main-loop watchdogs |
| [Settings & Config](/vide-de/configuration/settings/) | `settings.conf`, Privacy, Touchpad, Keyboard, Clock |
| [Testing](/vide-de/development/testing/) | `scripts/nested-test.sh` & input injection |
| [Keybindings](/vide-de/reference/keybindings/) | Complete keyboard shortcut reference |
| [FAQ & Wiki](/vide-de/wiki/faq/) | Common questions and troubleshooting |

## Quick Start

```bash
# Build (nested mode)
cargo build --release --workspace

# Build with TTY hardware backend
cargo build --release --workspace --features vibe-comp/tty

# Run nested (inside current desktop)
cargo run --release -- --nested

# Run on real hardware (from a spare TTY)
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log
```
