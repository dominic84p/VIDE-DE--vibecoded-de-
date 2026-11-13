---
title: 'Input Handling (input.rs)'
description: 'Keybinds, pointer/keyboard routing, start menu typing'
---

# Input Handling (`input.rs`)

All keybindings, pointer routing, and keyboard focus logic.

## Keybindings

Defined as `Action` enum with `bind()` mapping keys to actions:

```rust
pub enum Action {
    // Window management
    CloseWindow,
    ToggleMaximize,
    ToggleFullscreen,
    MinimizeWindow,
    SnapLeft, SnapRight, SnapUp, SnapDown,
    MoveWorkspace(usize),
    SendToWorkspace(usize),
    
    // Desktop
    ToggleStartMenu,
    ToggleTaskView,
    ToggleRunCommand,
    Screenshot,
    RegionSnip,
    
    // Focus
    FocusNext, FocusPrev,
    FocusWorkspaceNext, FocusWorkspacePrev,
}
```

## Binding Map

```rust
fn bind(key: Key, mods: Modifiers) -> Option<Action> {
    match (mods, key) {
        (Mod::Super, Key::Q) => Some(Action::CloseWindow),
        (Mod::Super, Key::M) => Some(Action::ToggleMaximize),
        (Mod::Super, Key::F) => Some(Action::ToggleFullscreen),
        (Mod::Super, Key::H) => Some(Action::MinimizeWindow),
        (Mod::Super | Mod::Shift, Key::Left) => Some(Action::SnapLeft),
        (Mod::Super | Mod::Shift, Key::Right) => Some(Action::SnapRight),
        // ... more bindings
    }
}
```

## Pointer Routing

```rust
fn route_pointer_event(&mut self, event: PointerEvent) {
    // 1. Check chrome (taskbar, start menu, titlebars)
    if self.chrome_hit_test(event.pos).is_some() {
        return self.handle_chrome_event(event);
    }
    
    // 2. Check window titlebars (SSD)
    if let Some((window, hit)) = self.titlebar_hit_test(event.pos) {
        return self.handle_titlebar_event(window, hit, event);
    }
    
    // 3. Route to client surface
    self.route_to_client(event);
}
```

## Start Menu Typing

```rust
fn handle_key_event(&mut self, key: Key, state: KeyState) {
    if self.start_menu_open {
        match key {
            Key::Escape => self.close_start_menu(),
            Key::Enter => self.launch_selected(),
            Key::Backspace => self.search_backspace(),
            Key::Character(c) => self.search_type(c),
            Key::ArrowDown => self.search_next(),
            Key::ArrowUp => self.search_prev(),
            _ => {}
        }
        return;
    }
    // ... normal keybind handling
}
```

## Modifier Handling

```rust
struct Modifiers {
    super_: bool,
    shift: bool,
    ctrl: bool,
    alt: bool,
}

impl Modifiers {
    fn matches(&self, required: Modifiers) -> bool { ... }
}
```

## Related Files

- `wm.rs` — Window operations triggered by keybinds
- `focus.rs` — Keyboard focus management
- `menu.rs` — Right-click menu key handling
- `desktops.rs` — Task View key handling
- `config/` — User-configurable keybinds (future)