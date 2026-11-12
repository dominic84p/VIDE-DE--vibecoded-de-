<p align="center">
  <img src="assets/logo-512.png" alt="vibecoded-de logo" width="200">
</p>

# vibecoded-de

A Wayland desktop environment with its own compositor, written in Rust and
vibecoded end to end. No human-written code (see [CONTRIBUTING.md](CONTRIBUTING.md)).

It's built on [Smithay](https://github.com/Smithay/smithay), a library of
compositor building blocks. The window manager, input handling, rendering,
taskbar, start menu and titlebars are all our own code.

## What it does

- **Runs on real hardware** from a TTY (DRM/KMS, libinput, libseat), or
  **nested** as a window inside another desktop for development
- **Windows 10-style desktop**: bottom taskbar with a start button, search box,
  a button per window and a clock; light titlebars with minimize, maximize and close
- **Start menu** with every installed app (from `.desktop` files) and their
  real icons; type to search
- **Floating windows**: move, resize from any edge, maximize, minimize,
  fullscreen, double-click titlebar to maximize, drag a maximized window to restore
- **Works with real apps**:
  - Qt/KDE, GTK, Firefox, Chromium and Electron
  - popup menus
  - apps that draw their own titlebars
  - clipboard and middle-click paste
  - your cursor and icon themes
- **X11 apps** through Xwayland
- **Smooth**: damage-tracked rendering, paced to vsync

## Keybinds

The modifier is **Super** on a TTY and **Alt** when nested (the host desktop
eats Super).

| Keys | Does |
|---|---|
| Super tap / Mod+Space | Start menu |
| Mod+Enter | Terminal (konsole) |
| Mod+Q | Close window |
| Mod+Tab | Cycle windows |
| Mod+Up | Maximize / restore |
| Mod+Down | Minimize (or restore if maximized) |
| Mod+F | Fullscreen |
| Mod+drag | Move a window from anywhere |
| Mod+Shift+E | Quit |
| Ctrl+Alt+F1–F12 | Switch VT |
| **Ctrl+Alt+Backspace** | **Quit (always works)** |

## Building

Needs Rust (stable). On Arch:

```bash
sudo pacman -S wayland libxkbcommon mesa libinput seatd libdrm xorg-xwayland
```

```bash
cargo build --release --features tty
```

Leave out `--features tty` if you only want the nested mode (then you don't need `seatd`).

## Running

**Nested**, as a window inside your current desktop:

```bash
cargo run --release -- --nested
```

**On a real TTY**: switch to a free VT (e.g. Ctrl+Alt+F4), log in as your
normal user (not root), then:

```bash
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log
```

Set `VIBE_STARTUP="konsole"` (or any command) to launch something once the
session is up.

## Status

Early but usable. Known gaps:

- Single monitor only
- No window snapping or virtual desktops yet
- Clipboard doesn't sync between X11 and Wayland apps yet
- Apps that allow only one running copy (Discord, Claude, Firefox) hand off
  to an existing copy if one is running in another session

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Then have an AI read it.

## Security

See [SECURITY.md](SECURITY.md).
