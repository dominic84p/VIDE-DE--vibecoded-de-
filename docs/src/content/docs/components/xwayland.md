---
title: 'XWayland (xwayland.rs)'
description: 'X11 support via XWayland, GPU acceleration, relative pointer'
---

# XWayland (`xwayland.rs`)

X11 application support through XWayland. Full GPU acceleration via linux-dmabuf.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      XWayland                               │
├─────────────────────────────────────────────────────────────┤
│  X Server (Xwayland)                                        │
│       │                                                     │
│       ├── X11 Clients (apps)                                │
│       │                                                     │
│       └── Wayland Client (to our compositor)                │
│              │                                              │
│              ▼                                              │
│  Our Compositor: XWaylandSurface management                 │
└─────────────────────────────────────────────────────────────┘
```

## XWayland Surface Management

```rust
struct XWaylandSurface {
    surface: WlSurface,
    x11_window: x11rb::Window,
    geometry: Rectangle,
    override_redirect: bool,  // Menus, tooltips
    mapped: bool,
}
```

## Key Differences from Wayland Windows

| Aspect | Wayland | XWayland |
|--------|---------|----------|
| Client Data | `ClientState` | `XWaylandClientData` |
| Surface Role | `xdg_toplevel` | X11 window + `wl_surface` |
| Focus | `KeyboardFocus::Wayland` | `KeyboardFocus::X11` |
| Decorations | SSD/CSD negotiated | Usually CSD (app draws) |
| Override Redirect | N/A | Yes (menus, tooltips) |

## X11 Window Entry to WM

```rust
fn manage_x11(&mut self, x11_surface: XWaylandSurface) {
    // 1. Wait for BOTH: mapped AND associated with wl_surface
    if !x11_surface.mapped || x11_surface.surface.is_none() {
        return;
    }
    
    // 2. Create Window with WinKind::X11
    let window = Window::new(WinKind::X11, x11_surface);
    
    // 3. Add to window stack
    self.windows.push(window);
    self.needs_redraw = true;
}
```

## Override-Redirect Windows

```rust
// Menus, tooltips, dropdowns - no WM management
struct X11Override {
    surface: WlSurface,
    geometry: Rectangle,
    serial: u32,
}

// Stored separately, drawn ABOVE everything
// In render.rs:
elements.extend(override_redirect_elements());  // After all windows
```

## GPU Acceleration (Critical)

```rust
// In State::init_dmabuf() - MUST run in every backend
fn init_dmabuf(&mut self) {
    let dmabuf_feedback = DmabufFeedback::new(
        self.renderer.device(),
        self.renderer.format_modifiers(),
    );
    
    // Send to XWayland
    self.xwayland.as_mut().unwrap().set_dmabuf_feedback(dmabuf_feedback);
}
```

**Without linux-dmabuf (with render-node feedback), Xwayland can't use the GPU** — games drop to single-digit FPS.

## Relative Pointer + Pointer Constraints

Games need mouse-look (Minecraft, FPS):

```rust
// XWayland converts X11 pointer grabs to:
enum PointerProtocol {
    RelativePointer,      // Raw mouse delta
    PointerConstraints,   // Confine/lock cursor
    LockedPointer,        // Hidden + confined
}

impl State {
    fn handle_relative_pointer(&mut self, surface: WlSurface, dx: f64, dy: f64) {
        // Forward to game
    }
    
    fn handle_pointer_lock(&mut self, surface: WlSurface, lock: bool) {
        if lock {
            self.pointer_lock = Some(surface);
            self.hide_cursor();
        } else {
            self.pointer_lock = None;
            self.show_cursor();
        }
    }
}
```

**Locks follow pointer focus and released on Super / focus change** (`release_pointer`).

## X11 Keyboard Focus

```rust
// X11 focus needs X server told too
enum KeyboardFocus {
    Wayland(WlSurface),
    X11(x11rb::Window),   // Not WlSurface!
    None,
}

fn set_keyboard_focus(&mut self, focus: KeyboardFocus) {
    match focus {
        KeyboardFocus::X11(xwin) => {
            // Tell X server
            self.xwayland.connection.set_input_focus(xwin)?;
        }
        KeyboardFocus::Wayland(surface) => {
            // Normal Wayland focus
        }
    }
    self.keyboard_focus = focus;
}
```

## When Xwayland Dies

If the X server crashes or is killed, the compositor keeps running: its X11 windows are dropped and
a new Xwayland starts after a second (longer after repeated crashes; after more than 5 in a minute
it stops trying). The new server may get a new display number; apps started afterwards, and D-Bus
activated ones, get the new `DISPLAY`. `scripts/nested-tests/xwayland-restart.test` checks this.

## Configuration

```ini
[xwayland]
enabled = true
lazy-start = true  # Start on first X11 app
```

## Related Files

- `wm.rs` — X11 window management
- `focus.rs` — Keyboard focus enum
- `input.rs` — Relative pointer handling
- `state.rs` — XWayland initialization, dmabuf
- `render.rs` — X11 surface rendering