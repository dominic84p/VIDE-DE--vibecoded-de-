---
title: 'FAQ'
description: 'Frequently asked questions about VIDE-DE'
---

# FAQ

## General

### What is VIDE-DE?

A Wayland compositor + desktop shell written in Rust on **Smithay 0.7**. "Vibecoded" end-to-end by AI — no human-written code accepted.

### Why "vibecoded"?

All code, commits, PRs, and reviews are done by AI agents (20B+ parameters minimum). Humans only provide infrastructure and merge.

### Is it stable?

**Alpha.** Daily driver for some, but expect bugs. No guarantees.

### What's the license?

MIT — see [LICENSE](https://github.com/dominic84p/vide-de/blob/main/LICENSE).

## Installation

### How do I install?

```bash
cargo build --release --workspace
./target/release/vibe-comp --nested  # for testing
```

See [Installation Guide](/vide-de/getting-started/installation/).

### Does it work on NVIDIA?

Yes, with caveats. See [NVIDIA Guide](/vide-de/wiki/nvidia-guide/).

### Does it work on Hyprland/Sway/Wayfire/etc?

No — it **is** a compositor. You run it *instead of* your current compositor.

### Can I run it nested for testing?

Yes! `vibe-comp --nested` runs as a window inside your current desktop.

## Usage

### How do I open the Start menu?

Press `Super` (Super key).

### How do I take a screenshot?

- `PrtSc` — Fullscreen
- `Win+Shift+S` — Region snip (opens Snip & Sketchy)
- `Win+PrtSc` — Fullscreen save
- `Alt+PrtSc` — Active window

### How do I switch workspaces?

`Super+1` through `Super+0` for workspaces 1-10.

### How do I move a window to another workspace?

`Super+Shift+1` through `Super+Shift+0`.

### How do I pin an app to the taskbar?

Right-click taskbar button → "Pin to taskbar". Or drag from Start menu.

### How do I change settings?

Run `vide-settings` (GUI) or edit `~/.config/vide-de/settings.conf`.

### How do I launch a terminal?

`Super+Enter` launches `konsole` by default. Change via `VIDE_STARTUP` or Settings.

## Troubleshooting

### Black screen on NVIDIA

Add `nvidia-drm.modeset=1` to kernel cmdline. See [NVIDIA Guide](/vide-de/wiki/nvidia-guide/).

### Tray icons missing

Run with private D-Bus: `dbus-run-session -- vibe-comp --nested`

### XWayland apps slow/games don't work

Verify `linux-dmabuf` works. Check logs for dmabuf feedback.

### "Permission denied" on DRM

```bash
sudo usermod -aG video,seat $USER
# Log out and back in
```

### Flatpak apps stop working after test

```bash
systemctl --user restart xdg-document-portal
# Caused by portal remount - use private XDG_RUNTIME_DIR for testing
```

### Seat errors

```bash
sudo systemctl enable --now seatd
sudo usermod -aG seat $USER
# Log out and back in
```

## Development

### Can I contribute?

Only if you're an AI agent (20B+ params). See [Contributing](/vide-de/development/contributing/).

### How do I test my changes?

```bash
cargo build --release --workspace
VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log
```

### Why is debug build so slow?

CPU-painted UI with `opt-level = 1`. Use `cargo build --release`.

### How do I add a keybinding?

Edit `src/input.rs`, rebuild. Not yet configurable via settings.

## Architecture

### Why not use smithay's Space?

Deliberately custom WM in `wm.rs` — full control over stacking, layout, animations.

### Why CPU-painted UI?

Simplicity, no GPU shader complexity. Cached by key — fast enough in release.

### How does the desktop work?

Separate process (`vide-desktop`) as layer-shell client on background layer. Keeps compositor clean.

### What's the IPC socket for?

External control: `snip`, `screenshot`, `copy-png`. Used by Snip & Sketchy app.

## Related

- [Troubleshooting](/vide-de/wiki/troubleshooting/)
- [NVIDIA Guide](/vide-de/wiki/nvidia-guide/)
- [Architecture Overview](/vide-de/architecture/overview/)
- [Contributing](/vide-de/development/contributing/)