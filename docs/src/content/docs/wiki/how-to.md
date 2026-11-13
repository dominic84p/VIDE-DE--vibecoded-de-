---
title: 'How-To Guides'
description: 'Step-by-step guides for common tasks'
---

# How-To Guides

## Change Theme

```bash
# Dark theme
vide-config set theme dark

# Light theme
vide-config set theme light

# System (follows GTK)
vide-config set theme system
```

Or use Settings app: `vide-settings` → Appearance.

## Add Custom Keybinding

Not yet configurable via settings. Edit `src/input.rs`:

```rust
// In bind() function
(Mod::Super | Mod::Shift, Key::KeyN) => Some(Action::MyAction),
```

Then rebuild: `cargo build --release --workspace`.

## Pin App to Taskbar

1. Open Start menu (`Super`)
2. Find app (search or browse)
3. Right-click → "Pin to taskbar"

Or drag from Start menu to taskbar.

## Change Wallpaper

1. Right-click desktop → "Change Wallpaper"
2. Select image
3. Or: `vide-settings` → Desktop → Wallpaper

Wallpapers stored in `~/.local/share/vide-de/wallpapers/`.

## Add Custom Wallpaper

```bash
cp my-wallpaper.jpg ~/.local/share/vide-de/wallpapers/
```

Then select in Settings or right-click desktop.

## Take Screenshot

| Shortcut | Result |
|----------|--------|
| `PrtSc` | Fullscreen → `~/Pictures/Screenshots/` |
| `Win+Shift+S` | Region snip → Opens Snip & Sketchy |
| `Win+PrtSc` | Fullscreen save |
| `Alt+PrtSc` | Active window |

## Record Screen

Not built-in. Use `wf-recorder` or `obs-studio` (both work on Wayland).

## Launch App on Startup

```bash
# One-time
VIDE_STARTUP=firefox ./target/release/vibe-comp --nested

# Permanent (add to shell profile)
echo 'export VIDE_STARTUP=firefox' >> ~/.bashrc
```

## Switch Workspace

`Super+1` through `Super+0` for workspaces 1-10.

## Move Window to Workspace

`Super+Shift+1` through `Super+Shift+0`.

## Open Task View

`Super+Tab` — shows all workspaces and windows.

## Use Alt+Tab Switcher

Hold `Alt`, press `Tab` repeatedly, release to switch.

## Configure Taskbar Position

```bash
# Top
vide-config set taskbar-position top

# Bottom (default)
vide-config set taskbar-position bottom

# Left
vide-config set taskbar-position left

# Right
vide-config set taskbar-position right
```

## Auto-Hide Taskbar

```bash
vide-config set taskbar-auto-hide true
```

## Change Taskbar Color

```bash
# VIDE-DE blue (default)
vide-config set taskbar-bg 0078D7FF

# Dark transparent
vide-config set taskbar-bg 80000000

# Custom (ARGB hex: AABBGGRR)
vide-config set taskbar-bg FF123456
```

## Enable/Disable Animations

```bash
vide-config set window-animation-duration 0  # Disable
vide-config set window-animation-duration 150  # Enable (ms)
```

## Configure Snip Magnifier

```bash
vide-config set snip-magnifier true
vide-config set snip-magnifier-scale 3.0
```

## Change Cursor Theme

```bash
vide-config set cursor-theme Papirus
vide-config set cursor-size 28
```

## Reset All Settings

```bash
vide-config reset
```

## Run Nested for Testing

```bash
# Private D-Bus + private runtime dir (recommended)
XDG_RUNTIME_DIR=/tmp/vt WAYLAND_DISPLAY=wayland-1 \
dbus-run-session -- \
env VIBE_STARTUP=konsole VIBE_DEBUG_SHOT=1 \
timeout 15 ./target/release/vibe-comp --nested
```

## Debug Start Menu

```bash
VIBE_DEBUG_START=1 ./target/release/vibe-comp --nested
# Opens Start menu at launch

VIBE_DEBUG_START=text ./target/release/vibe-comp --nested
# Opens Start menu + types "text" in search
```

## Capture Compositor Screenshot

```bash
VIBE_DEBUG_SHOT=1 timeout 10 ./target/release/vibe-comp --nested
# Saves to ~/Pictures/Screenshots/ after 6s
```

## Test TTY Mode Safely

1. Open new terminal
2. `sudo systemctl enable --now seatd`
3. `sudo usermod -aG seat,video $USER`
4. Log out and back in
5. `Ctrl+Alt+F4` (switch to VT)
6. Log in
7. `dbus-run-session -- ./target/release/vibe-comp --tty`
8. `Ctrl+Alt+Backspace` to exit

## Fix Flatpak After Testing

```bash
systemctl --user restart xdg-document-portal
```

## Related

- [FAQ](/wiki/faq/)
- [Troubleshooting](/wiki/troubleshooting/)
- [Keybindings](/reference/keybindings/)
- [Settings Reference](/reference/settings-reference/)