---
title: 'Compositor State (state.rs)'
description: 'Compositor state and Wayland protocol handlers'
---

# Compositor State (`state.rs`)

Central state struct holding all Wayland protocol handlers, window list, input state, and configuration.

## Structure

```rust
pub struct State {
    // Backend
    pub backend: Backend,
    
    // Wayland globals
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
    
    // D-Bus channels (SNI)
    pub sni_sender: mpsc::Sender<SniMessage>,
    
    // IPC
    pub ipc_sender: mpsc::Sender<IpcCommand>,
}
```

## Key Responsibilities

### Wayland Protocol Handlers

Implements all required Smithay handler traits:

| Trait | Purpose |
|-------|---------|
| `CompositorHandler` | Surface creation/destruction |
| `XdgShellHandler` | Toplevel/popup window management |
| `LayerShellHandler` | Desktop, panels, overlays |
| `SeatHandler` | Input device management |
| `DataDeviceHandler` | Copy/paste, drag-and-drop |
| `DecorationHandler` | Server-side decorations (SSD) |
| `PopupHandler` | Popup positioning/constraints |

### Window Management

```rust
impl State {
    pub fn new_window(&mut self, kind: WinKind, surface: WlSurface) -> Window { ... }
    pub fn remove_window(&mut self, id: WindowId) { ... }
    pub fn get_window(&self, id: WindowId) -> Option<&Window> { ... }
    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Window> { ... }
}
```

### Workspace Management

```rust
pub struct WorkspaceManager {
    current: usize,
    windows: [Vec<WindowId>; 10],
}

impl WorkspaceManager {
    pub fn switch(&mut self, idx: usize) { ... }
    pub fn move_window(&mut self, win: WindowId, idx: usize) { ... }
    pub fn current_windows(&self) -> &[WindowId] { ... }
}
```

### Configuration Reload

```rust
impl State {
    pub fn reload_config_if_changed(&mut self) {
        if self.config.poll_changed() {
            self.apply_config();
            self.needs_redraw = true;
        }
    }
}
```

## Important Patterns

### Surface Creation Hook

```rust
fn new_surface(&mut self, surface: WlSurface) {
    // CRITICAL: NVIDIA sync - block on acquire point
    if cfg!(feature = "nvidia") {
        surface.with_pending_state(|state| {
            state.acquire_point = Some(AcquirePoint::default());
        });
    }
}
```

### Redraw Flag

```rust
// ALWAYS set when something visible changes
self.needs_redraw = true;
// Backends only render when this is true — no busy loop
```

### Client Configuration

```rust
// Ask clients for changes through Window::configure()
window.configure(size, ResizeFlags::empty());
// Covers both xdg and X11 — don't call with_pending_state directly
```

## Related Files

- `wm.rs` — Window struct and management
- `input.rs` — Input state and keybind handling
- `focus.rs` — Keyboard focus enum
- `config/` — Shared settings crate
- `xwayland.rs` — X11 integration