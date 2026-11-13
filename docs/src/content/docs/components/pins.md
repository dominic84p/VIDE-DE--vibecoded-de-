---
title: 'Pinned Apps (pins.rs)'
description: 'Taskbar pinning, persistent app shortcuts, drag-to-reorder'
---

# Pinned Apps (`pins.rs`)

Taskbar pinning — persistent app shortcuts that stay on the taskbar.

## Pin Model

```rust
struct PinManager {
    pinned: Vec<PinEntry>,  // Ordered: left to right on taskbar
}

struct PinEntry {
    app_id: String,         // Desktop file ID (e.g., "firefox.desktop")
    position: usize,        // Index in pinned list
    window_id: Option<WindowId>,  // Currently running window (if any)
}
```

## Pin Operations

```rust
impl PinManager {
    fn pin(&mut self, app_id: String) {
        if !self.pinned.iter().any(|p| p.app_id == app_id) {
            self.pinned.push(PinEntry { app_id, position: self.pinned.len(), window_id: None });
            self.save();
        }
    }
    
    fn unpin(&mut self, app_id: &str) {
        self.pinned.retain(|p| p.app_id != app_id);
        self.reindex();
        self.save();
    }
    
    fn reorder(&mut self, from: usize, to: usize) {
        let entry = self.pinned.remove(from);
        self.pinned.insert(to, entry);
        self.reindex();
        self.save();
    }
    
    fn set_window(&mut self, app_id: &str, window_id: Option<WindowId>) {
        if let Some(pin) = self.pinned.iter_mut().find(|p| p.app_id == app_id) {
            pin.window_id = window_id;
        }
    }
}
```

## Taskbar Rendering

```rust
// In ui.rs - taskbar rendering
fn render_taskbar_pins(&mut self, canvas: &mut Canvas, pins: &PinManager) {
    for (i, pin) in pins.pinned.iter().enumerate() {
        let rect = pin_rect(i);
        
        // Background (highlight if running)
        if pin.window_id.is_some() {
            canvas.rounded_rect(rect, 6, RUNNING_PIN_BG);
            // Running indicator dot
            canvas.circle(rect.bottom_center() + (0, -4), 3, ACCENT_COLOR);
        } else {
            canvas.rounded_rect(rect, 6, PIN_BG);
        }
        
        // Icon
        let icon = resolve_icon(&pin.app_id, 24);
        canvas.icon_at(rect.center(), icon, ...);
        
        // Hover tooltip
        if hover {
            show_tooltip(pin.app_id);
        }
    }
}
```

## Click Behavior

```rust
fn on_pin_click(&mut self, pin: &PinEntry, button: MouseButton) {
    match button {
        MouseButton::Left => {
            if let Some(win) = pin.window_id {
                // Focus existing window
                self.focus_window(win);
            } else {
                // Launch app
                launch_app(&pin.app_id);
            }
        }
        MouseButton::Right => {
            // Show context menu
            show_pin_menu(pin);
        }
        MouseButton::Middle => {
            // New instance (even if running)
            launch_app(&pin.app_id);
        }
    }
}
```

## Pin Menu

```
┌─────────────────────┐
│  Unpin from taskbar │
├─────────────────────┤
│  New window         │  (if running)
│  Close window       │  (if running)
├─────────────────────┤
│  Open Settings      │
└─────────────────────┘
```

## Drag to Reorder

```rust
fn handle_drag(&mut self, pointer_pos: Point) {
    if self.drag_state.is_dragging_pin() {
        let new_index = self.pin_index_at(pointer_pos);
        if new_index != self.drag_state.pin_index {
            self.pins.reorder(self.drag_state.pin_index, new_index);
            self.drag_state.pin_index = new_index;
        }
    }
}
```

## Persistence

Saved to `~/.config/vide-de/pins.conf`:

```ini
[pins]
apps = ["konsole.desktop", "firefox.desktop", "code.desktop", "vide-settings.desktop"]
```

## Configuration

```ini
[taskbar]
max-pins = 20
pin-size = 40
pin-spacing = 4
show-running-indicator = true
```

## Related Files

- `apps.rs` — App ID resolution
- `icons.rs` — Icon loading for pins
- `ui.rs` — Taskbar pin rendering
- `input.rs` — Pin click/drag handling
- `config/` — Pins persistence