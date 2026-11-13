---
title: 'Layer Shell (layers.rs)'
description: 'wlr-layer-shell support for desktop, panels, overlays'
---

# Layer Shell (`layers.rs`)

Implements `wlr-layer-shell` protocol so external programs can create desktop backgrounds, panels, overlays, and lock screens.

## Layer Types

```rust
enum Layer {
    Background,  // Wallpaper, desktop icons
    Bottom,      // Panels, docks
    Top,         // Always-on-top widgets
    Overlay,     // Lock screens, OSD, notifications
}
```

## LayerSurface State

```rust
struct LayerSurface {
    surface: WlSurface,
    layer: Layer,
    namespace: String,      // e.g., "vide-desktop", "vide-panel"
    anchor: AnchorEdges,    // Top, Bottom, Left, Right
    exclusive_zone: i32,    // Reserves space (panels)
    margin: Margins,
    size: Size,
    keyboard_interactivity: KeyboardInteractivity,
}
```

## Anchor & Exclusive Zone

```rust
// Panel at bottom, 40px high, full width
LayerSurface {
    layer: Layer::Bottom,
    anchor: AnchorEdges::BOTTOM | AnchorEdges::LEFT | AnchorEdges::RIGHT,
    exclusive_zone: 40,  // Other windows won't overlap this 40px
    margin: Margins::none(),
    size: Size { width: 0, height: 40 }, // 0 = fill anchor dimension
}
```

## Desktop Integration

`vide-desktop` runs as a separate SCTK client on `Layer::Background`:

```rust
// Started by compositor on startup
fn start_desktop_process(handle: &LoopHandle) {
    Command::new("vide-desktop")
        .env("WAYLAND_DISPLAY", wayland_display)
        .spawn()?;
}
```

Desktop features (wallpaper, icons) live in `desktop/` crate, NOT in compositor.

## Keyboard Interactivity

```rust
enum KeyboardInteractivity {
    None,           // Background, passive panels
    Exclusive,      // Lock screen, modal dialogs
    OnDemand,       // Panels that need focus (start menu)
}
```

## Z-Ordering (in render.rs)

```rust
// Layer shell surfaces rendered by layer:
elements.extend(layer_elements(Layer::Overlay));   // Topmost
elements.extend(layer_elements(Layer::Top));
elements.extend(layer_elements(Layer::Bottom));
elements.extend(layer_elements(Layer::Background)); // Bottommost
```

## Configuration

Layer surfaces can be configured via `settings.conf`:

```ini
[layer-shell]
# Reserve space for external panels
bottom-panel-height = 40
top-panel-height = 0
```

## Related Files

- `desktop/` — vide-desktop (wallpaper + icons as layer-shell client)
- `render.rs` — Layer element ordering
- `wm.rs` — Workspace layout respects exclusive zones
- `desktops.rs` — Task View as layer-shell overlay
- `state.rs` — LayerShellHandler implementation