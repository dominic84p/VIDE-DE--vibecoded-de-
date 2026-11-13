---
title: 'Screenshots & Snipping (screenshot.rs)'
description: 'PrtSc fullscreen, Win+Shift+S region snip, snip-and-sketchy integration'
---

# Screenshots & Snipping (`screenshot.rs`)

Handles fullscreen screenshots (PrtSc) and region snipping (Win+Shift+S). Markup/saving lives in separate `snip/` app.

## Screenshot Types

| Trigger | Action | Output |
|---------|--------|--------|
| `PrtSc` | Fullscreen capture | `~/Pictures/Screenshots/...png` |
| `Win+Shift+S` | Region snip (interactive) | Copied to clipboard + IPC to snip app |
| `Win+PrtSc` | Fullscreen + save | `~/Pictures/Screenshots/...png` |
| `Alt+PrtSc` | Active window | `~/Pictures/Screenshots/...png` |

## Fullscreen Screenshot

```rust
fn take_screenshot(&mut self, output: Option<Output>) {
    // 1. Get output geometry (or all outputs)
    // 2. Read pixels from GPU (via renderer)
    // 3. Encode PNG
    // 4. Save to ~/Pictures/Screenshots/
    // 5. Copy to clipboard (if clipboard manager running)
    // 6. Notify via IPC (for snip app)
}
```

## Region Snip (Interactive)

```rust
fn start_region_snip(&mut self) {
    // 1. Freeze screen (capture current frame as backdrop)
    // 2. Show dimming overlay
    // 3. Enter selection mode:
    //    - Click-drag = rectangular selection
    //    - ESC = cancel
    //    - Enter/Space = confirm
    // 4. On confirm:
    //    - Capture selected region
    //    - Send via IPC to snip-and-sketchy
    //    - Copy PNG to clipboard
}
```

## Snip Overlay Rendering

```rust
// In render.rs - elements()
if state.snip_active {
    // Dimming layer (semi-transparent black)
    elements.push(snip_dim_element());
    
    // Selection rectangle
    elements.push(snip_selection_element(state.snip_rect));
    
    // Crosshair cursor
    elements.push(snip_crosshair_element(state.pointer_pos));
    
    // Magnifier (optional, follows cursor)
    if state.snip_magnifier {
        elements.push(snip_magnifier_element(state.pointer_pos));
    }
}
```

## IPC Integration (`ipc.rs`)

```rust
// Compositor sends to snip app
enum IpcCommand {
    Snip { 
        rect: Rectangle, 
        image: Vec<u8>,  // PNG bytes
    },
    Screenshot { 
        image: Vec<u8>, 
        path: PathBuf 
    },
    CopyPng { path: PathBuf },
}

// snip app connects to $VIDE_IPC and receives
```

## Snip & Sketchy App (`snip/`)

Separate egui app for markup:

- Receives snip via IPC
- Tools: pen, highlighter, eraser, shapes, text, crop
- Save to file, copy to clipboard, share
- `Win+Shift+S` → snip → opens snip-and-sketchy automatically

## Configuration

```ini
[screenshot]
directory = "~/Pictures/Screenshots"
filename-format = "Screenshot_%Y-%m-%d_%H-%M-%S.png"
copy-to-clipboard = true
notify = true
snip-magnifier = true
snip-magnifier-scale = 2.0
```

## Related Files

- `ipc.rs` — IPC socket protocol
- `render.rs` — Snip overlay elements
- `input.rs` — Snip mode input handling
- `snip/` — Markup app (separate crate)
- `ui.rs` — Crosshair/magnifier rendering