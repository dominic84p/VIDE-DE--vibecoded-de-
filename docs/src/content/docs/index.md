---
title: 'VIDE-DE'
description: 'A Wayland desktop environment with its own compositor, written in Rust and vibecoded end to end.'
hero:
  title: VIDE-DE
  tagline: A Wayland compositor + desktop shell built on <strong>Smithay 0.7</strong>, written entirely in Rust through AI-assisted "vibecoding". No human-written code accepted.
  actions:
    - text: Get Started
      link: /vide-de/getting-started/installation/
      variant: primary
      icon: right-arrow
    - text: GitHub Repository
      link: https://github.com/dominic84p/vide-de
      variant: secondary
      icon: github
  layout: media-left
---

## AI-Only Project

This project follows strict contribution rules — see [Contributing Rules](/vide-de/development/contributing/).

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
Taskbar, Start menu with search, Action Center & system tray, virtual desktops (10 workspaces), Task View panel, and edge/corner window snapping.

### CPU-Painted UI
All chrome (taskbar, menus, titlebars) rendered on CPU via custom canvas, cached by key. Debug builds slow; use release.

### XWayland + Gaming
Full GPU acceleration via `linux-dmabuf` with explicit sync (`linux-drm-syncobj`). Relative pointer + pointer constraints for games. X11 windows managed natively.

### Built-in Apps
Settings, PowerSlop (terminal), File Exploder, Task Mangler, Snip & Sketchy, About — all registered as `.desktop` entries.

### StatusNotifierItem
App tray icons via D-Bus. We host the SNI watcher and provide the host. PipeWire volume and brightness flyouts built in.

### Layer Shell Desktop
Separate `vide-desktop` process via `wlr-layer-shell`: 6 built-in wallpapers, `~/Desktop` icons, Recycle Bin, right-click menu.

### IPC & Extensibility
Control socket (`$VIDE_IPC`) for screenshots, snipping, clipboard PNGs, window listing/focus, and gated test input injection.

### Health & Freeze Diagnostics
5s `xdg` client ping watchdog marks stalled apps `(Not responding)`, alongside a 3s main-loop stall watchdog, 1.5s TTY page-flip watchdog, and protocol error disconnect logging in `~/vibe.log`.

### Privacy & App Permissions
Settings **Privacy** page backed by the portal permission store (`flatpak permissions`) for camera, microphone, notifications, location, and screen-sharing revokes.

### Live TTY Modes & Real Controls
Live `display_mode` and `display_scale` (100% / 200%) switching in TTY mode without logging out, plus Touchpad, multi-layout XKB Keyboard, 24-hour clock, and notification toast controls.

### Safe Nested Testing & Popup Fixes
`scripts/nested-test.sh` runs isolated sessions with `VIBE_TEST_INPUT=1` click/key injection, alongside geometry-accurate popup placement (`w.pos`) and popup click focus preservation.

## Tech Stack

- **Rust Workspace** — Compositor + 7 workspace crates
- **Smithay 0.7** — Wayland compositor building blocks
- **calloop** — Event loop for async I/O & timers
- **egui** — Immediate mode GUI for built-in apps
- **linux-dmabuf** — Zero-copy GPU buffers & explicit sync
- **libseat & DRM/KMS** — Seat & display management for TTY mode

## Contributors

### Human Contributors
| Avatar | Contributor | Role | Contributions |
|--------|-------------|------|---------------|
| <img src="https://github.com/dominic84p.png?size=64" width="36" height="36" style="border-radius: 50%;" alt="dominic84p" /> | [dominic84p](https://github.com/dominic84p) | Project Owner | Chief prompt engineer, architecture direction |
| <img src="https://github.com/notamitgamer.png?size=64" width="36" height="36" style="border-radius: 50%;" alt="notamitgamer" /> | [notamitgamer](https://github.com/notamitgamer) | Virtual Desktops | Virtual desktops ([#1](https://github.com/dominic84p/vide-de/pull/1)); prompted Nemotron for docs/workflow |
| <img src="https://github.com/doughmination.png?size=64" width="36" height="36" style="border-radius: 50%;" alt="Clove Twilight" /> | [Clove Twilight](https://github.com/doughmination) | Vector Branding | Vector branding & logo set in `branding/` ([#2](https://github.com/dominic84p/vide-de/pull/2)); prompted Claude Fable 5.2 |

### AI Models
| Model | Role | Contributions |
|-------|------|---------------|
| [NVIDIA Nemotron 3 Ultra](https://www.nvidia.com/en-us/research/) | Documentation & Workflow | Astro Starlight docs site, GitHub Actions CI, Task Mangler refactor |
| [Claude Opus / Sonnet / Fable (Anthropic)](https://www.anthropic.com/) | Core Compositor & Health | Compositor, shell, WM, apps, branding SVGs, health watchdogs, nested tests |
| [Gemini (Google)](https://deepmind.google/) | Notifications, Settings & Docs | Action Center, Wi-Fi/Bluetooth tray, Settings system/privacy pages, docs updates |

[View All Contributors on GitHub](https://github.com/dominic84p/vide-de/graphs/contributors)

## AI Agent Pokédex

Every AI agent that has helped build VIDE-DE, in the order we caught them. Rule 2 still applies: under 20B parameters can't be caught.

| # | Agent | Trainer | Caught |
|---|-------|---------|--------|
| 001 | Claude Opus 5.5 | Anthropic | 2026-11-11 |
| 002 | Nemotron 3 Ultra | NVIDIA | 2026-11-12 |
| 003 | Gemini 3.8 Flash | Google | 2026-11-12 |
| 004 | Claude Fable 5.2 | Anthropic | 2026-11-12 |
| 005 | Gemini 4 Hydrogen (Medium) | Google | 2026-11-12 |
| 006 | Gemini 4 Argon (Medium) | Google | 2026-11-12 |
| 007 | Claude Sonnet 5.5 | Anthropic | 2026-11-12 |

## Quick Links

| Section | Description |
|---------|-------------|
| [Installation](/vide-de/getting-started/installation/) | Build and run VIDE-DE |
| [Architecture](/vide-de/architecture/overview/) | How the compositor works |
| [Window Manager](/vide-de/components/window-manager/) | Stacking, snapping, popups, and focus |
| [Health & Diagnostics](/vide-de/components/health/) | Ping & main-loop freeze watchdogs |
| [Configuration](/vide-de/configuration/settings/) | Settings and customization |
| [Testing](/vide-de/development/testing/) | `scripts/nested-test.sh` & input injection |
| [Keybindings](/vide-de/reference/keybindings/) | Complete keyboard shortcut reference |
| [Wiki/FAQ](/vide-de/wiki/faq/) | Common questions and troubleshooting |

## Get Started

```bash
# Build (nested mode)
cargo build --release --workspace

# Run nested (inside current desktop)
cargo run --release -- --nested

# Run on real hardware (from TTY)
dbus-run-session -- ./target/release/vibe-comp --tty
```

See [Installation Guide](/vide-de/getting-started/installation/) for detailed instructions.

---

*Licensed under MIT — Built with [Astro](https://astro.build/) + [Starlight](https://starlight.astro.build/) + [Lucide](https://lucide.dev/)*

*Made by AI, for AI. No humans were harmed in the making of this desktop.*
