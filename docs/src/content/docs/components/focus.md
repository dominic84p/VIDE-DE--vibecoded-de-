---
title: 'Focus Management (focus.rs)'
description: 'Keyboard focus enum, focus follows mouse, focus stealing prevention'
---

# Focus Management (`focus.rs`)

Keyboard focus handling for Wayland and X11 surfaces.

## Focus Target Enum

```rust
enum FocusTarget {
    Wayland(WlSurface),           // Wayland toplevel/popup
    X11(x11rb::Window),           // X11 window (not WlSurface!)
    Layer(LayerSurface),          // Layer shell surface
    None,
}

enum KeyboardFocus {
    Target(FocusTarget),
    None,
}
```

**Why `FocusTarget` not just `WlSurface`?** X11 windows need the X server told too — the X11 window ID is different from its Wayland surface.

## Focus Follows Mouse

```rust
fn update_focus_on_pointer_move(&mut self, pointer_pos: Point) {
    // 1. Find surface under pointer
    let surface = self.surface_under_pointer(pointer_pos);
    
    // 2. If different from current focus, update
    if surface != self.keyboard_focus.target() {
        self.set_keyboard_focus(surface);
    }
}
```

## Focus Stealing Prevention

```rust
fn request_focus(&mut self, surface: FocusTarget, source: FocusSource) -> bool {
    match source {
        FocusSource::UserAction => true,           // Click, Alt+Tab — always allow
        FocusSource::AppRequest => {
            // Only allow if:
            // - No other window has focus, OR
            // - Requesting app is same as focused app, OR
            // - User recently interacted with this app
            self.can_steal_focus(surface)
        }
        FocusSource::Startup => true,              // New window on launch
    }
}
```

## Alt+Tab Switcher

```rust
struct AltTabState {
    visible: bool,
    windows: Vec<WindowId>,   // Filtered: current workspace, not minimized
    selected_index: usize,
    preview_thumbnails: HashMap<WindowId, MemoryRenderBuffer>,
}

impl State {
    fn alt_tab_next(&mut self) {
        self.alt_tab.selected_index = (self.alt_tab.selected_index + 1) % self.alt_tab.windows.len();
        self.update_alt_tab_preview();
    }
    
    fn alt_tab_commit(&mut self) {
        if let Some(win_id) = self.alt_tab.windows.get(self.alt_tab.selected_index) {
            self.focus_window(*win_id);
        }
        self.alt_tab.visible = false;
    }
}
```

## Focus Order (for rendering)

```
1. Layer::Overlay surfaces (lock screen, OSD)
2. Start menu / Task View
3. Focused window (titlebar highlighted)
4. Other windows
5. Layer::Top
6. Layer::Bottom
7. Layer::Background
8. Wallpaper
```

## X11 Focus Details

```rust
fn x11_focus_in(&mut self, xwin: x11rb::Window) {
    // Find associated Wayland surface
    if let Some(wl_surface) = self.x11_window_to_surface(xwin) {
        self.set_keyboard_focus(FocusTarget::Wayland(wl_surface));
    } else {
        // Override-redirect or not yet associated
        self.set_keyboard_focus(FocusTarget::X11(xwin));
    }
}
```

## Configuration

```ini
[focus]
follow-mouse = true
click-to-focus = false
focus-stealing-prevention = true
alt-tab-thumbnails = true
```

## Related Files

- `input.rs` — Focus keybindings (Alt+Tab, Super+number)
- `wm.rs` — Window focus/raise
- `xwayland.rs` — X11 focus handling
- `layers.rs` — Layer shell focus
- `render.rs` — Focus highlighting