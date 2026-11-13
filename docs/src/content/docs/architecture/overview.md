---
title: 'Architecture Overview'
description: 'High-level architecture of the VIDE-DE compositor and desktop shell'
---

# Architecture Overview

VIDE-DE is a **Wayland compositor + desktop shell** built on **Smithay 0.7**. Smithay provides protocol plumbing and backends; everything user-visible (window management, input routing, taskbar, start menu, titlebars, rendering order) is our own code.

## High-Level Components

```
┌─────────────────────────────────────────────────────────────────┐
│                        vibe-comp (Compositor)                    │
├─────────────────────────────────────────────────────────────────┤
│  main.rs                    Event loop, backend selection       │
│  state.rs                   Compositor state, protocol handlers │
│  wm.rs                      Window manager (stacking, layout)   │
│  input.rs                   Keybinds, pointer/keyboard routing  │
│  render.rs                  Frame element list (front to back)  │
│  ui.rs + canvas.rs          Chrome painting (CPU, cached)       │
│  xwayland.rs                X11 support via XWayland            │
│  focus.rs                   Keyboard focus management           │
│  layers.rs                  wlr-layer-shell (desktop, panels)   │
│  apps.rs, icons.rs          App discovery, icon loading         │
│  sni.rs                     StatusNotifierItem (tray icons)     │
│  tray.rs                    Volume/brightness flyouts           │
│  desktops.rs                Task View panel (virtual desktops)  │
│  search.rs                  Start menu search logic             │
│  screenshot.rs              Screenshots and region snipping     │
│  menu.rs, pins.rs           Right-click menus, pinned apps      │
│  health.rs                  Ping & main-loop freeze watchdogs   │
│  config/                    Shared settings (vide-config)       │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌──────────┐   ┌────────────┐   ┌────────────┐
        │  TTY     │   │  Winit     │   │  Layer     │
        │  Backend │   │  Backend   │   │  Shell     │
        │ (DRM/KMS)│   │ (Nested)   │   │ Clients    │
        └──────────┘   └────────────┘   └────────────┘
                                                         │
                          ┌──────────────────────────────┘
                          ▼
                   ┌────────────────┐
                   │ vide-desktop   │  ← Separate process (SCTK client)
                   │ (wallpaper,    │
                   │  desktop icons)│
                   └────────────────┘
```

## Key Design Decisions

### 1. Custom Window Manager (`wm.rs`)

We don't use `smithay::desktop::Space` or any layout engine. Our WM in `wm.rs` is deliberately our own:

- Windows are a simple stack (`Vec`, last = top)
- Floating only: move, resize, maximize, minimize, fullscreen, snap
- `WinKind::Wayland` or `WinKind::X11`
- Workspace support (10 workspaces)

### 2. CPU-Painted UI (`ui.rs` + `canvas.rs`)

All chrome (taskbar, start menu, titlebars, wallpaper) is painted on the CPU into `MemoryRenderBuffer`s:

- Cached by a key (repainted only when key changes)
- `canvas.rs` = tiny 2D painter (rects, text, icons, rounded corners)
- Debug builds are slow (`opt-level = 1` in dev profile); use release builds

### 3. Element List Order (Front to Back)

`render.rs::elements()` builds the frame:

1. Cursor
2. Alt+Tab thumbnails
3. Start menu / Task View panel
4. Window titlebars & borders
5. Window surfaces (client content)
6. Layer shell surfaces (overlay, top, bottom, background)
7. Wallpaper (if no background layer)
8. Snip overlay (dimming + selection outline)

### 4. Dual Backend Architecture

- `winit.rs`: Nested mode — runs as a window inside another Wayland/X11 desktop
- `tty.rs`: Real hardware — DRM/KMS, libinput, libseat
- Both just call `render::elements()` and present the result

### 5. Separate Desktop Process (`vide-desktop`)

The desktop (wallpaper + icons) is its own program (`desktop/`) running as an SCTK client on the wlr-layer-shell background layer.

- Keeps desktop features out of the compositor
- Compositor starts it automatically
- Wallpapers installed to `~/.local/share/vide-de/wallpapers`

### 6. XWayland Integration

- Full GPU acceleration via linux-dmabuf
- Games work properly (relative pointer + pointer constraints)
- X11 windows managed in `xwayland.rs` + `wm.rs`

## Data Flow

```
Input Events (libinput/Wayland)
        │
        ▼
input.rs: keybinds, pointer routing
        │
        ├──▶ wm.rs: move/resize grabs, window state changes
        ├──▶ ui.rs: hover/click on chrome (taskbar, start, titlebars)
        ├──▶ sni.rs: tray icon clicks/menus (via D-Bus channel)
        └──▶ layers.rs: layer shell surface interactions
        │
        ▼
state.rs: Wayland protocol handlers (compositor, xdg-shell, etc.)
        │
        ▼
render.rs: build element list for frame
        │
        ▼
Backend (winit/tty): submit to GPU / scanout
```

## Concurrency Model

- **Main thread**: Event loop (calloop), Wayland dispatch, rendering
- **D-Bus thread** (`sni.rs`): StatusNotifierItem watcher + host, talks to main via channels
- **XWayland**: Runs on main thread, X11 connection managed by Smithay
- **IPC socket** (`ipc.rs`): Control socket for snip, screenshot, copy-png

## Configuration System

`config/` crate (`vide-config`):

- Single key=value file: `~/.config/vide-de/settings.conf`
- Written by Settings app, polled by compositor once/second
- Live reload: `state.reload_config_if_changed()` called every second

## Built-in Apps (Separate Crates)

| App | Crate | Description |
|-----|-------|-------------|
| Settings | `settings/` | Mouse, display, wallpaper, gaming, start menu |
| PowerSlop | `powerslop/` | Terminal (blue, banner, PS> prompt, tabs) |
| File Exploder | `exploder/` | File manager (Win+E) |
| Task Mangler | `mangler/` | Process/performance monitor (Ctrl+Shift+Esc) |
| Snip & Sketchy | `snip/` | Screenshot markup (Win+Shift+S) |
| About VIDE-DE | `about/` | System info via fastfetch |

All registered as `.desktop` entries on startup via `apps::install_builtin()`.

## Security Notes

- No privileged code: Runs as regular user
- Seat management: Uses libseat (via seatd) for TTY mode
- D-Bus: Tray watcher runs on separate thread, communicates via channels
- XWayland: Started by compositor, no `ReplaceExisting` on bus name (avoids killing host desktop's tray)

## Further Reading

- [Main Event Loop](/vide-de/architecture/main-event-loop/) — `main.rs` deep dive
- [Compositor State](/vide-de/architecture/state/) — `state.rs` protocol handlers
- [Window Manager](/vide-de/components/window-manager/) — `wm.rs` stacking and layout
- [Input Handling](/vide-de/components/input-handling/) — `input.rs` keybinds and routing
- [Rendering](/vide-de/components/rendering/) — `render.rs` frame building
- [UI System](/vide-de/components/ui/) — `ui.rs` + `canvas.rs` chrome painting