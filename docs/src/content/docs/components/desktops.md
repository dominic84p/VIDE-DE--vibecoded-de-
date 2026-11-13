---
title: 'Virtual Desktops (desktops.rs)'
description: 'Task View panel, 10 workspaces, workspace switching'
---

# Virtual Desktops (`desktops.rs`)

Manages 10 workspaces (virtual desktops) and the Task View panel (Win+Tab).

## Workspace Model

```rust
pub struct WorkspaceManager {
    current: usize,           // 0-9
    windows: [Vec<WindowId>; 10],  // Per-workspace window lists
    names: [String; 10],      // Optional names
}

impl WorkspaceManager {
    pub fn switch(&mut self, idx: usize) { ... }
    pub fn move_window(&mut self, win: WindowId, idx: usize) { ... }
    pub fn current_windows(&self) -> &[WindowId] { ... }
    pub fn get_name(&self, idx: usize) -> &str { ... }
    pub fn set_name(&mut self, idx: usize, name: String) { ... }
}
```

## Keybindings

| Shortcut | Action |
|----------|--------|
| `Super+1`..`Super+0` | Switch to workspace 1-10 |
| `Super+Shift+1`..`0` | Move window to workspace 1-10 |
| `Super+Tab` | Open Task View |
| `Super+Ctrl+Left/Right` | Prev/Next workspace |

## Task View Panel (Win+Tab)

```
┌─────────────────────────────────────────────────────────────┐
│  Desktop 1    Desktop 2    Desktop 3    + New Desktop      │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐                      │
│  │ Window1 │  │ Window2 │  │ Window3 │                      │
│  │ Window4 │  │         │  │         │                      │
│  └─────────┘  └─────────┘  └─────────┘                      │
├─────────────────────────────────────────────────────────────┤
│  Timeline (recent apps)                                     │
└─────────────────────────────────────────────────────────────┘
```

**Implemented as layer-shell overlay** (`Layer::Overlay`).

```rust
struct TaskView {
    visible: bool,
    selected_desktop: usize,
    hovered_window: Option<WindowId>,
    thumbnails: HashMap<WindowId, MemoryRenderBuffer>,
}
```

## Thumbnail Generation

```rust
fn generate_thumbnail(&self, window: &Window) -> MemoryRenderBuffer {
    // 1. Create offscreen canvas (256x144)
    // 2. Render window surface scaled down
    // 3. Add titlebar if SSD
    // 4. Cache by (window_id, geometry_hash)
}
```

## Workspace Switching Animation

```rust
fn animate_workspace_switch(&mut self, from: usize, to: usize) {
    // Slide animation: 150ms ease-out
    // Windows slide opposite direction
    // Taskbar indicator slides
}
```

## Configuration

```ini
[desktops]
count = 10
names = ["Main", "Code", "Web", "Chat", "Media", "", "", "", "", ""]
animation-duration = 150
```

## Related Files

- `wm.rs` — Window workspace assignment
- `input.rs` — Workspace keybindings
- `render.rs` — Workspace-aware element ordering
- `layers.rs` — Task View as layer-shell overlay
- `ui.rs` — Task View panel rendering