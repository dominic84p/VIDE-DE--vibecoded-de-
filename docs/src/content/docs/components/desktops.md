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

## Task View (Win+Tab)

Full screen above the taskbar, like Windows 10's:

```
┌─────────────────────────────────────────────────────────────┐
│        [Desktop 1]  [Desktop 2]  [ + New desktop ]          │
│                                                             │
│   ▣ PowerSlop            ▣ Settings          ▣ About        │
│   ┌──────────────┐       ┌──────────────┐    ┌──────────┐   │
│   │ live window  │       │ live window  │    │  live    │   │
│   └──────────────┘       └──────────────┘    └──────────┘   │
└─────────────────────────────────────────────────────────────┘
```

- **Windows**: every window on the current desktop as a live thumbnail (drawn by `render.rs` with the
  same helper as Alt+Tab), its icon and title above it. Click one to switch to it; hover shows a
  close button.
- **Desktops strip**: desktops that have windows, the current one, and ones added with **New desktop**
  (which adds a desktop to the strip; click it to go there). There are 10 desktops at most.
- Clicking empty space, pressing Esc, or the Task View button again closes it.
- The background is a tint over the desktop, dark or light with the Windows mode.

## Related Files

- `wm.rs` — Window workspace assignment
- `input.rs` — Workspace keybindings
- `render.rs` — Workspace-aware element ordering, Task View's live thumbnails
- `layers.rs` — Task View as layer-shell overlay
- `ui.rs` — Task View panel rendering