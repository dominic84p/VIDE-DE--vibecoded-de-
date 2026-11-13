---
title: 'Built-in Apps'
description: 'VIDE-DE built-in applications'
---

# Built-in Apps

VIDE-DE includes several built-in applications, registered as `.desktop` entries on startup via `apps::install_builtin()`.

## Overview

| App | Command | Desktop ID | Category | Shortcut |
|-----|---------|------------|----------|----------|
| Settings | `vide-settings` | `vide-settings.desktop` | Settings | — |
| PowerSlop | `powerslop` | `powerslop.desktop` | Terminal | `Super+Enter` |
| File Exploder | `file-exploder` | `file-exploder.desktop` | Files | `Super+E` |
| Task Mangler | `task-mangler` | `task-mangler.desktop` | System | `Ctrl+Shift+Esc` |
| Snip & Sketchy | `snip-and-sketchy` | `snip-and-sketchy.desktop` | Graphics | `Win+Shift+S` |
| About VIDE-DE | `vide-about` | `vide-about.desktop` | Settings | — |

All apps are **separate crates** in the workspace, built with **egui** (except compositor).

---

## Settings (`vide-settings`)

**Crate:** `settings/`

GUI for all compositor settings.

### Features

- **Mouse:** Sensitivity, acceleration, natural scroll, tap-to-click
- **Display:** Resolution, refresh rate, position, scale, primary
- **Wallpaper:** Select from `~/.local/share/vide-de/wallpapers/`
- **Gaming:** XWayland toggle, VRR, HDR (future)
- **Start Menu:** Pinned apps, recent count, search behavior
- **Taskbar:** Position, height, colors, auto-hide, clock format
- **Keyboard:** Repeat rate, delay
- **Theme:** Light/Dark/System, accent color

### Screenshot

```
┌─────────────────────────────────────┐
│  Settings                    [×]    │
├─────────────────────────────────────┤
│ ▸ Mouse          ▸ Display          │
│ ▸ Wallpaper      ▸ Gaming           │
│ ▸ Start Menu     ▸ Taskbar          │
│ ▸ Keyboard       ▸ Theme            │
├─────────────────────────────────────┤
│  [Apply]  [Reset]  [Close]          │
└─────────────────────────────────────┘
```

### Config

Writes to `~/.config/vide-de/settings.conf`. Compositor reloads within 1 second.

---

## PowerSlop (`powerslop`)

**Crate:** `powerslop/`

Terminal emulator styled like Windoze Terminal / PowerSlop.

### Features

- **Blue theme** (#0078D7 background)
- **Banner** on startup
- **PS> prompt** (PowerSlop style)
- **Tabs** (Ctrl+Shift+T)
- **Split panes** (Ctrl+Shift+D)
- **Configurable** font, size, opacity
- **QuickEdit**: right-click copies the selection (and clears it) or pastes when nothing is selected; `Ctrl+C` copies a selection or sends ^C; `Ctrl+Shift+C` copies; `Ctrl+V` / `Ctrl+Shift+V` paste; `Ctrl+X` goes to the app (nano's Exit)
- **Mouse wheel** scrolls history; apps that track the mouse (vim, htop, mc) get real wheel events, and `Shift+wheel` still scrolls history
- **Uses egui 0.31** (egui_term version)

### Screenshot

```
┌────────────────────────────────────────────────┐
│ PowerSlop  [+] [□] [×]    Tab1  Tab2  [+]     │
├────────────────────────────────────────────────┤
│                                                │
│  Windoze PowerSlop                            │
│  Copyright (C) Microslop Corporation.         │
│                                                │
│  PS C:\Users\user> _                           │
│                                                │
└────────────────────────────────────────────────┘
```

### Config

`~/.config/powerslop/config.toml` (future)

---

## File Exploder (`file-exploder`)

**Crate:** `exploder/`

File manager styled like Windoze File Explorer.

### Features

- **Toolbar:** Back, Forward, Up, Refresh, Address bar
- **Sidebar:** Quick access, This PC, Network
- **List/Details/Grid views**
- **Preview pane**
- **Copy/Move/Delete/Rename**
- **This PC lists every drive:** each disk partition with a filesystem, mounted or not
  (read from `/proc/mounts` and udev, like `lsblk`). Opening an unmounted drive mounts it
  through udisks2 (`udisksctl`); the polkit agent asks for a password if the disk needs one.
  Right-click a mounted drive for **Eject** (removable, also powers it off) or **Unmount**.
  Partitions udisks hides (EFI, recovery) are hidden here too.
- **Drag & drop**
- **Context menus**

### Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+N` | New window |
| `Ctrl+W` | Close tab/window |
| `Ctrl+L` | Focus address bar |
| `Alt+Left` | Back |
| `Alt+Right` | Forward |
| `Alt+Up` | Parent folder |
| `F2` | Rename |
| `Delete` | Delete |

### Screenshot

```
┌────────────────────────────────────────────────┐
│ File Exploder  [◀] [▶] [↑] [⟳]  [Address...]   │
├───────────────┬────────────────────────────────┤
│ Quick Access  │ Name        │ Type    │ Size   │
│ This PC       ├─────────────┼─────────┼────────┤
│   Desktop     │ Documents   │ Folder  │        │
│   Downloads   │ Pictures    │ Folder  │        │
│   Documents   │ Music       │ Folder  │        │
│ Network       │ Videos      │ Folder  │        │
│               │ file.txt    │ Text    │ 1.2 KB │
└───────────────┴────────────────────────────────┘
```

---

## Task Mangler (`task-mangler`)

**Crate:** `mangler/`

Process/performance monitor styled like Windoze Task Manager.

### Features

- **Processes tab:** Name, PID, CPU%, Memory, Disk, Network, GPU
- **Performance tab:** CPU, Memory, Disk, Network, GPU graphs
- **Startup tab:** Enabled/disabled startup apps
- **Users tab:** Session info
- **Details tab:** Full process list
- **Services tab:** Systemd services
- **Kill process** (right-click → End task)
- **Search/filter**

### Screenshot

```
┌────────────────────────────────────────────────┐
│ Task Mangler  [Processes] [Performance] [Startup]│
├────────────────────────────────────────────────┤
│  [Search...]                     [End Task]    │
├──────────────┬─────┬──────┬────────┬───────────┤
│ Name         │ PID │ CPU% │ Memory │ Status    │
├──────────────┼─────┼──────┼────────┼───────────┤
│ firefox      │ 1234│ 12.5%│ 1.2 GB │ Running   │
│ code         │ 5678│ 5.2% │ 800 MB │ Running   │
│ vibe-comp    │ 999 │ 2.1% │ 150 MB │ Running   │
└──────────────┴─────┴──────┴────────┴───────────┘
```

---

## Snip & Sketchy (`snip-and-sketchy`)

**Crate:** `snip/`

Screenshot markup tool. **Compositor only captures** (via IPC); markup/save/copy lives here.

### Features

- Receives snip via IPC (`$VIDE_IPC`)
- **Tools:** Pen, Highlighter, Eraser, Shapes, Text, Crop
- **Colors:** Preset + custom
- **Stroke width** slider
- **Undo/Redo** (Ctrl+Z / Ctrl+Y)
- **Save** to file
- **Copy** to clipboard
- **Share** via xdg-share (future)

### Workflow

```
Win+Shift+S
    │
    ▼
Compositor: Region selection overlay
    │
    ▼
User selects region
    │
    ▼
Compositor: Captures region → PNG bytes
    │
    ▼
IPC: Send Snip { rect, image } to $VIDE_IPC
    │
    ▼
Snip & Sketchy: Receives → Opens with image
    │
    ▼
User marks up → Save/Copy/Share
```

### Screenshot

```
┌────────────────────────────────────────────────┐
│ Snip & Sketchy [Pen] [Mark] [Erase] [O] [□] [T] [Crop] │
├────────────────────────────────────────────────┤
│                                                │
│   [Screenshot with annotations]               │
│                                                │
├────────────────────────────────────────────────┤
│  [Save]  [Copy]  [Share]  [Undo]  [Redo]      │
└────────────────────────────────────────────────┘
```

---

## About VIDE-DE (`vide-about`)

**Crate:** `about/`

System information viewer using `fastfetch` backend.

### Features

- **OS info:** Distro, kernel, uptime
- **Hardware:** CPU, GPU, Memory, Disk
- **Compositor:** Version, Smithay version, uptime
- **Wayland:** Protocol versions, seat
- **Copy** to clipboard button

### Screenshot

```
┌────────────────────────────────────────────────┐
│ About VIDE-DE                          [×]     │
├────────────────────────────────────────────────┤
│                                                │
│        VIDE-DE v0.1.0                         │
│        Smithay 0.7                            │
│                                                │
│  OS: Arch Linux x86_64                        │
│  Kernel: 6.10.2-arch1-1                       │
│  Uptime: 2h 34m                               │
│                                                │
│  CPU: AMD Ryzen 7 5800X (16) @ 4.7GHz         │
│  GPU: NVIDIA RTX 3080                         │
│  Memory: 16.2 GiB / 31.4 GiB                  │
│                                                │
│  [Copy to Clipboard]  [Close]                  │
└────────────────────────────────────────────────┘
```

---

## Installation

All apps built with compositor:

```bash
cargo build --release --workspace
# Binaries in target/release/
```

Installed to `~/.local/bin/` or system via package.

## Desktop Entries

Created by `apps::install_builtin()` on compositor startup:

```
~/.local/share/vide-de/applications/
├── vide-settings.desktop
├── powerslop.desktop
├── file-exploder.desktop
├── task-mangler.desktop
├── snip-and-sketchy.desktop
└── vide-about.desktop
```

## Adding New Built-in Apps

1. Create crate in workspace
2. Implement as egui app
3. Add to `apps::install_builtin()` in `src/apps.rs`
4. Rebuild

## Related

- [App Discovery](/vide-de/components/apps/)
- [Start Menu Search](/vide-de/components/search/)
- [Pinned Apps](/vide-de/components/pins/)
- [IPC Socket](/vide-de/components/ipc/)
- [Screenshots](/vide-de/components/screenshot/)