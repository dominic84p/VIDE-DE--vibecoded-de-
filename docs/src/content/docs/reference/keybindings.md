---
title: 'Keybindings Reference'
description: 'Complete keyboard shortcut list'
---

# Keybindings Reference

## Window Management

| Shortcut | Action |
|----------|--------|
| `Super+Q` | Close focused window |
| `Super+M` | Toggle maximize |
| `Super+F` | Toggle fullscreen |
| `Super+H` | Minimize window |
| `Super+Shift+Left` | Snap left |
| `Super+Shift+Right` | Snap right |
| `Super+Shift+Up` | Snap up |
| `Super+Shift+Down` | Snap down |
| `Super+Shift+M` | Toggle maximize (alternate) |

## Workspace (Virtual Desktops)

| Shortcut | Action |
|----------|--------|
| `Super+1` .. `Super+0` | Switch to workspace 1-10 |
| `Super+Shift+1` .. `Super+Shift+0` | Move window to workspace 1-10 |
| `Super+Ctrl+Left` | Previous workspace |
| `Super+Ctrl+Right` | Next workspace |
| `Super+Tab` | Open Task View |

## Start Menu & Search

| Shortcut | Action |
|----------|--------|
| `Super` | Toggle Start menu |
| `Super+R` | Open Run command |
| `Super+S` | Open search (in Start menu) |
| `Escape` | Close Start menu / cancel |
| `Enter` | Launch selected app |
| `Arrow Up/Down` | Navigate results |
| `Tab` | Switch between Pinned/All Apps |

## System

| Shortcut | Action |
|----------|--------|
| `Super+Enter` | Launch terminal (konsole) |
| `Super+E` | Launch File Exploder |
| `Super+Shift+Esc` | Launch Task Mangler |
| `Super+Shift+S` | Region snip (screenshot) |
| `PrtSc` | Fullscreen screenshot |
| `Win+PrtSc` | Fullscreen screenshot (save) |
| `Alt+PrtSc` | Active window screenshot |
| `Super+L` | Lock screen (if configured) |

## Focus & Navigation

| Shortcut | Action |
|----------|--------|
| `Alt+Tab` | Next window (switcher) |
| `Alt+Shift+Tab` | Previous window |
| `Super+Tab` | Task View |
| `Super+Shift+Tab` | Task View (reverse) |

## Media Keys

| Key | Action |
|-----|--------|
| `XF86AudioRaiseVolume` | Volume +5% |
| `XF86AudioLowerVolume` | Volume -5% |
| `XF86AudioMute` | Toggle mute |
| `XF86MonBrightnessUp` | Brightness +10% |
| `XF86MonBrightnessDown` | Brightness -10% |
| `XF86AudioPlay` | Play/Pause |
| `XF86AudioNext` | Next track |
| `XF86AudioPrev` | Previous track |

## Mouse Bindings

| Action | Binding |
|--------|---------|
| Move window | `Super+Left Drag` (titlebar or anywhere on SSD) |
| Resize window | `Super+Right Drag` (edges) |
| Show window menu | `Right Click` (titlebar or taskbar button) |
| Close window | `Middle Click` (taskbar button) |
| New window instance | `Middle Click` (pinned app) |

## Customization

Keybindings are **not yet user-configurable** via settings file. To change:

1. Edit `src/input.rs` bind() function
2. Rebuild: `cargo build --release --workspace`

Future: Keybinding configuration via `settings.conf` planned.

## Debug Shortcuts (Dev Only)

| Shortcut | Action |
|----------|--------|
| `VIBE_DEBUG_START=1` | Open Start menu at launch |
| `VIBE_DEBUG_START=text` | Open Start menu + type "text" |
| `VIBE_DEBUG_DESKTOPS=1` | Open Task View at launch |
| `VIBE_DEBUG_SWITCHER=1` | Step Alt+Tab after 5s |
| `VIBE_DEBUG_SHOT=1` | Screenshot after 6s |

Set via environment variables when launching.

## Related Files

- [Input Handling](/components/input-handling/) — Implementation details
- [Configuration](/configuration/settings.md) — Other settings
- [Installation](/getting-started/installation/) — Running with debug flags