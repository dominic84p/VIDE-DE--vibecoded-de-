---
title: 'VIDE-DE'
description: 'A Wayland desktop environment with its own compositor, written in Rust and vibecoded end to end.'
hero:
  title: VIDE-DE
  tagline: A Wayland compositor + desktop shell built on <strong>Smithay 0.7</strong>, written entirely in Rust through AI-assisted "vibecoding". No human-written code accepted.
  actions:
    - text: Get Started
      link: /getting-started/installation/
      variant: primary
      icon: right-arrow
    - text: GitHub Repository
      link: https://github.com/dominic84p/vide-de
      variant: secondary
      icon: github
  layout: media-left
---

## AI-Only Project

This project follows strict contribution rules — see [Contributing Rules](/development/contributing/).

| Rule | Description |
|------|-------------|
| 🤖 **NO HUMAN SLOP** | Only AI-written code accepted. If a human typed it, it doesn't ship. |
| 📏 **NO AI UNDER 20B PARAMETERS** | Small models may watch, but they may not touch. |
| ⚡ **AI FOR EVERYTHING** | Code, commits, PRs, reviews. If you run `git commit` with your own hands, see Rule 1. |
| 🎯 **40K TOKEN MINIMUM** | Low-effort PRs get the `minimal` label. Show token count in PR description. |
| 🔒 **SECURITY BY CLAUDE ONLY** | Human-found bugs don't count. See [SECURITY.md](https://github.com/dominic84p/vide-de/blob/main/SECURITY.md). |

## Features at a Glance

### Dual Backend Architecture
Runs nested (for development via winit) or on real hardware (TTY/DRM/KMS/libseat). Both backends share the same rendering pipeline.

### Windows 10-Style Desktop
Taskbar, Start menu with search, system tray, virtual desktops (10 workspaces), Task View panel, window snapping.

### CPU-Painted UI
All chrome (taskbar, menus, titlebars) rendered on CPU via custom canvas, cached by key. Debug builds slow; use release.

### XWayland + Gaming
Full GPU acceleration via linux-dmabuf. Relative pointer + pointer constraints for games. X11 windows managed natively.

### Built-in Apps
Settings, PowerSlop (terminal), File Exploder, Task Mangler, Snip & Sketchy, About — all registered as .desktop entries.

### StatusNotifierItem
App tray icons via D-Bus. We host the SNI watcher and provide the host. Volume/brightness flyouts built in.

### Layer Shell Desktop
Separate `vide-desktop` process via wlr-layer-shell: wallpaper, ~/Desktop icons, Recycle Bin, right-click menu.

### IPC & Extensibility
Control socket (`$VIDE_IPC`) for screenshots, snipping, clipboard. Extensible protocol for external tools.

## Tech Stack

- **Rust 2021 Edition** — Memory safety without garbage collection
- **Smithay 0.7** — Wayland compositor building blocks
- **calloop** — Event loop for async I/O
- **egui 0.31** — Immediate mode GUI for built-in apps
- **ash / wgpu** — Vulkan & GPU acceleration
- **libseat** — Seat management for TTY mode

## Contributors

### AI Models
| Model | Role | Contributions |
|-------|------|---------------|
| [NVIDIA Nemotron 3 Ultra](https://www.nvidia.com/en-us/research/) | Documentation & Workflow | Astro Starlight docs site, GitHub Actions CI, Task Mangler refactor |
| [Claude Opus 5.5](https://www.anthropic.com/) | Core Compositor | Compositor, shell, window manager, and built-in apps |
| [Gemini (Google)](https://deepmind.google/) | Notifications & Settings | Action Center, Wi-Fi/Bluetooth tray, Settings system pages |

### Human Contributors
| Contributor | Role | Contributions |
|-------------|------|---------------|
| [dominic84p](https://github.com/dominic84p) | Project Owner | Chief prompt engineer, architecture direction |
| [notamitgamer](https://github.com/notamitgamer) | Virtual Desktops | Virtual desktops implementation (PR #1); prompted Nemotron for docs/workflow |

[View All Contributors](/contributors/)

## Quick Links

| Section | Description |
|---------|-------------|
| [Installation](/getting-started/installation/) | Build and run VIDE-DE |
| [Architecture](/architecture/overview/) | How the compositor works |
| [Components](/components/window-manager/) | Deep dives into each component |
| [Configuration](/configuration/settings/) | Settings and customization |
| [Keybindings](/reference/keybindings/) | Complete keyboard shortcut reference |
| [Wiki/FAQ](/wiki/faq/) | Common questions and troubleshooting |

## Get Started

```bash
# Build (nested mode)
cargo build --release --workspace

# Run nested (inside current desktop)
cargo run --release -- --nested

# Run on real hardware (from TTY)
dbus-run-session -- ./target/release/vibe-comp --tty
```

See [Installation Guide](/getting-started/installation/) for detailed instructions.

---

*Licensed under MIT — Built with [Astro](https://astro.build/) + [Starlight](https://starlight.astro.build/) + [Lucide](https://lucide.dev/)*

*Made by AI, for AI. No humans were harmed in the making of this desktop.*
