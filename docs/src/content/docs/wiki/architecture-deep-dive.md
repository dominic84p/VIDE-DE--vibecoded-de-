---
title: 'Architecture Deep Dive'
description: 'Detailed architecture of VIDE-DE compositor'
---

# Architecture Deep Dive

## Overview

VIDE-DE is a **Wayland compositor + desktop shell** built on **Smithay 0.7**. Smithay provides protocol plumbing and backends; everything user-visible is our own code.

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            vibe-comp (Main Process)                          │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │   main.rs   │  │  state.rs   │  │  input.rs   │  │  render.rs  │       │
│  │ Event Loop  │  │ Protocols   │  │ Keybinds    │  │ Elements    │       │
│  │ Backend Sel │  │ State Mgmt  │  │ Pointer/Key │  │ (Front→Back)│       │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘       │
│         │                │                │                │               │
│         ▼                ▼                ▼                ▼               │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                         Shared State                                │  │
│  │  Windows • Workspaces • Focus • Config • Pointer • Keyboard • SNI  │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│         │                │                │                │               │
│         ▼                ▼                ▼                ▼               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │   wm.rs     │  │   ui.rs     │  │  layers.rs  │  │  xwayland.rs│       │
│  │ Window Mgmt │  │ CPU Paint   │  │ Layer Shell │  │ X11 Support │       │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘       │
│         │                │                │                │               │
│         ▼                ▼                ▼                ▼               │
│  ┌─────────────────────────────────────────────────────────────────────┐  │
│  │                        Backends                                     │  │
│  │  ┌──────────────────┐              ┌──────────────────┐            │  │
│  │  │   Winit Backend  │              │    TTY Backend   │            │  │
│  │  │ (Nested/Dev)     │              │ (DRM/KMS/Real)   │            │  │
│  │  └──────────────────┘              └──────────────────┘            │  │
│  └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
       ┌────────────┐  ┌────────────┐  ┌────────────┐
       │ D-Bus Thread│  │ IPC Socket │  │ Layer Shell│
       │ (sni.rs)    │  │ (ipc.rs)   │  │ Clients    │
       └────────────┘  └────────────┘  └────────────┘
                                                     │
                                             ┌───────┴───────┐
                                             ▼               ▼
                                      ┌──────────┐    ┌────────────┐
                                      │Desktop   │    │  Panels/   │
                                      │(vide-desktop)│   │  Overlays  │
                                      └──────────┘    └────────────┘
```

## Event Loop (`main.rs`)

```rust
// calloop event loop
let mut event_loop = EventLoop::try_new()?;

// Sources inserted:
// 1. Wayland socket (via Smithay)
// 2. Render timer (~60fps)
// 3. IPC socket
// 4. Config poll timer (1s)
// 5. Signal handler (SIGINT/SIGTERM)
// 6. D-Bus connection (via channel from sni thread)

event_loop.run(None, &mut state, |state| {
    // Wayland dispatch
    state.backend.dispatch()?;
    
    // Handle IPC messages
    while let Ok(cmd) = state.ipc_receiver.try_recv() {
        handle_ipc(cmd);
    }
    
    // Handle SNI messages
    while let Ok(msg) = state.sni_receiver.try_recv() {
        handle_sni(msg);
    }
    
    // Config reload check
    if state.config.poll_changed() {
        state.apply_config();
    }
    
    // Render if needed
    if state.needs_redraw {
        state.needs_redraw = false;
        let elements = state.elements();
        state.backend.render(elements);
    }
})?;
```

## State Management (`state.rs`)

Central `State` struct holds everything:

```rust
pub struct State {
    // Backend abstraction
    pub backend: Backend,
    
    // Smithay protocol states
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub layer_shell_state: LayerShellState,
    pub output_state: OutputState,
    pub seat_state: SeatState<Self>,
    pub shm_state: ShmState,
    pub data_device_state: DataDeviceState,
    pub decoration_state: DecorationState,
    pub popup_state: PopupState,
    
    // Custom state
    pub windows: Vec<Window>,
    pub workspaces: WorkspaceManager,
    pub xwayland: Option<XWayland>,
    pub keyboard_focus: KeyboardFocus,
    pub pointer_state: PointerState,
    pub config: Config,
    pub needs_redraw: bool,
    
    // Cross-thread channels
    pub sni_sender: mpsc::Sender<SniMessage>,
    pub sni_receiver: mpsc::Receiver<SniMessage>,
    pub ipc_sender: mpsc::Sender<IpcCommand>,
    pub ipc_receiver: mpsc::Receiver<IpcCommand>,
}
```

## Window Manager (`wm.rs`)

Custom stacking — no `smithay::desktop::Space`:

```rust
pub struct Window {
    pub id: WindowId,
    pub kind: WinKind,           // Wayland | X11
    pub surface: WlSurface,
    pub geometry: Rectangle,     // Includes SSD
    pub pos: Point,              // Geometry origin (NOT surface origin!)
    pub ssd: bool,               // We draw titlebar?
    pub title: String,
    pub app_id: Option<String>,
    pub maximized: bool,
    pub fullscreen: bool,
    pub minimized: bool,
    pub workspace: usize,
    pub last_floating_geo: Rectangle,
}
```

**Key invariant:** `pos` = geometry origin. Use `surface_loc()` for drawing.

```rust
fn surface_loc(&self) -> Point {
    if self.ssd {
        self.pos + Point::new(0, TITLEBAR_HEIGHT)
    } else {
        self.pos
    }
}
```

## Rendering Pipeline (`render.rs`)

Front-to-back element list:

```rust
fn elements(&self) -> Vec<RenderElement> {
    let mut elements = Vec::new();
    
    // 1. Cursor (always top)
    elements.push(cursor_element());
    
    // 2. Alt+Tab thumbnails
    if self.alt_tab_visible {
        elements.extend(alt_tab_elements());
    }
    
    // 3. Start menu / Task View (overlay layer)
    if self.start_menu_open { elements.extend(start_menu_elements()); }
    if self.task_view_open { elements.extend(task_view_elements()); }
    
    // 4. Window titlebars & borders (SSD)
    for w in &self.windows {
        if w.ssd {
            elements.push(w.titlebar_element());
            elements.push(w.border_elements());
        }
    }
    
    // 5. Window surfaces (client content) — stacking order
    for w in &self.windows {
        elements.push(w.surface_element());
    }
    
    // 6. Layer shell: Overlay > Top > Bottom > Background
    for layer in [Overlay, Top, Bottom, Background] {
        elements.extend(layer_elements(layer));
    }
    
    // 7. Wallpaper (if no background layer)
    if !self.has_background_layer {
        elements.push(wallpaper_element());
    }
    
    // 8. Snip overlay
    if self.snip_active {
        elements.push(snip_dim_element());
        elements.push(snip_selection_element());
    }
    
    elements
}
```

## CPU-Painted UI (`ui.rs` + `canvas.rs`)

All chrome rendered on CPU, cached by key:

```rust
// Cache key = hash of all visual properties
struct UiKey {
    // Taskbar: width, height, icons, clock, tray items...
    // StartMenu: width, height, search text, results, pinned...
    // Titlebar: width, title, buttons, focused...
}

fn get_or_create(&mut self, key: UiKey, builder: FnOnce(&mut Canvas)) -> &MemoryRenderBuffer {
    self.cache.entry(key).or_insert_with(|| {
        let mut canvas = Canvas::new(key.size());
        builder(&mut canvas);
        canvas.finish()
    })
}
```

## Input Handling (`input.rs`)

```rust
enum Action {
    CloseWindow, ToggleMaximize, ToggleFullscreen, MinimizeWindow,
    SnapLeft, SnapRight, SnapUp, SnapDown,
    MoveWorkspace(usize), SendToWorkspace(usize),
    ToggleStartMenu, ToggleTaskView, ToggleRunCommand,
    Screenshot, RegionSnip,
    FocusNext, FocusPrev,
    // ...
}

fn handle_key(&mut self, key: Key, mods: Modifiers, pressed: bool) {
    if self.start_menu_open {
        handle_start_menu_keys(key);
        return;
    }
    if let Some(action) = bind(key, mods) {
        if pressed { handle_action(action); }
    }
}
```

## Layer Shell (`layers.rs`)

External programs create surfaces on layers:

```rust
enum Layer { Background, Bottom, Top, Overlay }

struct LayerSurface {
    surface: WlSurface,
    layer: Layer,
    namespace: String,        // "vide-desktop", "vide-panel"
    anchor: AnchorEdges,
    exclusive_zone: i32,      // Reserves space
    margin: Margins,
    keyboard_interactivity: KeyboardInteractivity,
}
```

## D-Bus Thread (`sni.rs`)

Separate thread for StatusNotifierItem:

```rust
// Main thread
let (tx, rx) = mpsc::channel();
thread::spawn(move || {
    let conn = Connection::session()?;
    conn.request_name("org.kde.StatusNotifierWatcher")?; // NO ReplaceExisting!
    
    loop {
        select! {
            msg = rx.recv() => handle_main_msg(msg),
            signal = conn.receive() => handle_dbus_signal(signal),
        }
    }
});
```

## IPC Socket (`ipc.rs`)

Unix socket for external control:

```rust
enum IpcCommand {
    Snip { rect: Option<Rectangle> },
    Screenshot { output: Option<String> },
    CopyPng { path: PathBuf },
    GetVersion, GetWindows, SetWorkspace { idx: usize },
    Launch { desktop_id: String },
}
```

## Configuration (`config/`)

Single file, polled every second:

```rust
// In main loop
if state.config.poll_changed() {
    state.apply_config();
    state.needs_redraw = true;
}
```

## Built-in Apps (Separate Crates)

| App | Crate | Purpose |
|-----|-------|---------|
| Settings | `settings/` | Mouse, display, wallpaper, gaming |
| PowerSlop | `powerslop/` | Terminal (blue, PS> prompt, tabs) |
| File Exploder | `exploder/` | File manager (Win+E) |
| Task Mangler | `mangler/` | Process monitor (Ctrl+Shift+Esc) |
| Snip & Sketchy | `snip/` | Screenshot markup (Win+Shift+S) |
| About VIDE-DE | `about/` | System info |

Registered as `.desktop` entries via `apps::install_builtin()`.

## Related

- [Architecture Overview](/vide-de/architecture/overview/)
- [Main Event Loop](/vide-de/architecture/main-event-loop/)
- [Compositor State](/vide-de/architecture/state/)
- [Window Manager](/vide-de/components/window-manager/)
- [Rendering](/vide-de/components/rendering/)