---
title: 'Rendering (render.rs)'
description: 'Frame element list building, front-to-back ordering'
---

# Rendering (`render.rs`)

Builds the front-to-back element list for each frame. Both backends (winit/tty) call `elements()` and present the result.

## Element Order (Front to Back)

```rust
pub fn elements(&self) -> Vec<RenderElement> {
    let mut elements = Vec::new();
    
    // 1. Cursor (always on top)
    elements.push(cursor_element());
    
    // 2. Alt+Tab thumbnails
    if self.alt_tab_visible {
        elements.extend(alt_tab_elements());
    }
    
    // 3. Start menu / Task View panel
    if self.start_menu_open {
        elements.extend(start_menu_elements());
    }
    if self.task_view_open {
        elements.extend(task_view_elements());
    }
    
    // 4. Window titlebars & borders (SSD)
    for window in &self.windows {
        if window.ssd {
            elements.push(window.titlebar_element());
            elements.push(window.border_elements());
        }
    }
    
    // 5. Window surfaces (client content)
    for window in &self.windows {
        elements.push(window.surface_element());
    }
    
    // 6. Layer shell surfaces
    // Order: overlay > top > bottom > background
    elements.extend(layer_shell_elements(Layer::Overlay));
    elements.extend(layer_shell_elements(Layer::Top));
    elements.extend(layer_shell_elements(Layer::Bottom));
    elements.extend(layer_shell_elements(Layer::Background));
    
    // 7. Wallpaper (if no background layer)
    if !self.has_background_layer {
        elements.push(wallpaper_element());
    }
    
    // 8. Snip overlay (dimming + selection outline)
    if self.snip_active {
        elements.push(snip_dim_element());
        elements.push(snip_selection_element());
    }
    
    elements
}
```

## Key Principles

1. **Cursor first** — Always visible, never obscured
2. **Chrome above windows** — Start menu, titlebars render above client surfaces
3. **Client surfaces in stacking order** — `windows` Vec: last = top
4. **Layer shell by layer** — Overlay > Top > Bottom > Background
5. **Wallpaper last** — Behind everything
6. **Snip on top** — Dimming + selection rectangle

## Redraw Flag

```rust
// In State
pub needs_redraw: bool = false;

// Backends check this before rendering
if state.needs_redraw {
    state.needs_redraw = false;
    let elements = state.elements();
    backend.render(elements);
}
```

**Set `needs_redraw = true` whenever anything visible changes.** No busy loop.

## Damage Tracking (Optimization)

```rust
// Track damaged regions to avoid full redraw
pub fn damage_tracker(&self) -> DamageTracker { ... }
```

## Related Files

- `wm.rs` — Window stacking order
- `ui.rs` + `canvas.rs` — Chrome element creation
- `layers.rs` — Layer shell element ordering
- `winit.rs` / `tty.rs` — Backend presentation
- `screenshot.rs` — Snip overlay elements