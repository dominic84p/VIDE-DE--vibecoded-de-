---
title: 'Window Manager (wm.rs)'
description: 'Window stacking, move/resize grabs, maximize/minimize/fullscreen'
---

# Window Manager (`wm.rs`)

Custom window manager — we don't use `smithay::desktop::Space`. Our own stacking, layout, and window state logic.

## Window Structure

```rust
pub struct Window {
    pub id: WindowId,
    pub kind: WinKind,           // Wayland or X11
    pub surface: WlSurface,
    pub geometry: Rectangle,     // Window geometry (includes SSD)
    pub pos: Point,              // Geometry origin (NOT surface origin)
    pub ssd: bool,               // True = we draw titlebar (SSD)
    pub title: String,
    pub app_id: Option<String>,
    pub maximized: bool,
    pub fullscreen: bool,
    pub minimized: bool,
    pub workspace: usize,
    pub last_floating_geo: Rectangle, // For restore from max/fs
}
```

## Key Conventions

### `pos` = Geometry Origin

```rust
// A window's `pos` is its geometry origin, NOT surface origin
// Use surface_loc() for drawing/hit-testing surfaces
fn surface_loc(&self) -> Point {
    if self.ssd {
        self.pos + TITLEBAR_HEIGHT // Titlebar offset
    } else {
        self.pos
    }
}
```

This keeps client-side shadows from shifting windows.

### Configure Through `Window::configure()`

```rust
// Ask clients for changes through this method
// Covers both xdg and X11 — don't call with_pending_state directly
impl Window {
    pub fn configure(&self, size: Size, flags: ResizeFlags) { ... }
}
```

### Respect Client Decoration Choices

```rust
// w.ssd is true only when WE draw the titlebar
// Apps that request client-side decorations (CSD) keep their own
if toplevel.requested_decorations() == DecorationMode::Client {
    window.ssd = false;
}
```

## Stacking Order

Windoze stored in `Vec<Window>` — **last = top** (front).

```rust
impl State {
    pub fn raise_window(&mut self, id: WindowId) {
        if let Some(idx) = self.windows.iter().position(|w| w.id == id) {
            let w = self.windows.remove(idx);
            self.windows.push(w);
            self.needs_redraw = true;
        }
    }
}
```

## Move/Resize Grabs

```rust
pub enum Grab {
    None,
    Move { start_pos: Point, window_pos: Point },
    Resize { edges: ResizeEdge, start_pos: Point, start_geo: Rectangle },
}

impl State {
    pub fn start_move(&mut self, window_id: WindowId, pointer_pos: Point) { ... }
    pub fn start_resize(&mut self, window_id: WindowId, edge: ResizeEdge, pointer_pos: Point) { ... }
    pub fn update_grab(&mut self, pointer_pos: Point) { ... }
    pub fn end_grab(&mut self) { ... }
}
```

## Window States

| State | Behavior |
|-------|----------|
| **Normal** | Floating, movable, resizable |
| **Maximized** | Fills workspace area (minus taskbar) |
| **Fullscreen** | Fills entire output, no decorations |
| **Minimized** | Hidden from layout, kept in workspace list |

```rust
impl Window {
    pub fn toggle_maximize(&mut self) { ... }
    pub fn toggle_fullscreen(&mut self, output: Output) { ... }
    pub fn minimize(&mut self) { ... }
    pub fn restore(&mut self) { ... }
}
```

## Snap Assist

Win+Arrow keys trigger snap:

```rust
pub fn snap_window(&mut self, window_id: WindowId, direction: SnapDirection) {
    let output_geo = self.current_output_geometry();
    match direction {
        SnapDirection::Left => { /* left half */ }
        SnapDirection::Right => { /* right half */ }
        SnapDirection::Up => { /* top half */ }
        SnapDirection::Down => { /* bottom half */ }
    }
}
```

## Hit Testing

```rust
impl Window {
    pub fn hit_test(&self, point: Point) -> HitTestResult {
        if self.ssd {
            // Check titlebar buttons
            if self.close_button_rect().contains(point) { return HitTestResult::Close; }
            if self.maximize_button_rect().contains(point) { return HitTestResult::Maximize; }
            if self.minimize_button_rect().contains(point) { return HitTestResult::Minimize; }
            if self.titlebar_rect().contains(point) { return HitTestResult::Titlebar; }
        }
        HitTestResult::Client
    }
}
```

## X11 Windows

X11 windows enter WM once **mapped AND associated with a `wl_surface`** (`manage_x11`).

```rust
fn manage_x11(&mut self, x11_surface: X11Surface) {
    // Create Window with WinKind::X11
    // Associate with Wayland surface from XWayland
    // Add to window stack
}
```

Override-redirect windows (menus, tooltips) live in `state.x11_overrides` and draw above everything.

## Popups & Focus

Wayland `xdg_popup` offsets are relative to their parent's **window geometry** (`w.pos`), not its surface origin (`surface_loc()`). Apps with client-side shadows (Chromium, Electron, GTK) have a shadow inset between the two, so drawing, hit-testing, and unconstraining all use:

```text
w.pos + popup_offset - popup.geometry().loc
```

When a pointer click lands inside a popup surface, `input.rs` **does not** re-focus the parent toplevel. Dropdowns and site-permission prompts in Chromium take keyboard focus themselves; handing focus back to the parent closes the popup before the click is delivered.

## Related Files

- `state.rs` — Window list, workspace management
- `input.rs` — Move/resize grab handling, keybinds
- `render.rs` — Window element ordering
- `focus.rs` — Keyboard focus for windows
- `xwayland.rs` — X11 window integration