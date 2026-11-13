---
title: 'Right-Click Menus (menu.rs)'
description: 'Context menus, window menus, keyboard navigation'
---

# Right-Click Menus (`menu.rs`)

Context menus for window titlebars, taskbar, desktop, and tray icons.

## Menu Structure

```rust
struct Menu {
    items: Vec<MenuItem>,
    position: Point,
    parent: Option<MenuId>,  // For submenus
}

enum MenuItem {
    Label { text: String, enabled: bool },
    Separator,
    Action { text: String, icon: Option<String>, action: MenuAction, enabled: bool },
    Submenu { text: String, icon: Option<String>, items: Vec<MenuItem> },
}

enum MenuAction {
    CloseWindow,
    MaximizeWindow,
    MinimizeWindow,
    MoveWorkspace(usize),
    PinApp,
    UnpinApp,
    LaunchApp(String),
    OpenSettings,
    // ...
}
```

## Menu Types

| Trigger | Menu | Items |
|---------|------|-------|
| Right-click titlebar | Window menu | Restore, Minimize, Maximize (greyed when they don't apply), Close; End process when the app is not responding |
| Right-click taskbar button | Jump list (Windoze 10 style: flush on the taskbar, an icon per row) | App name (launch another), Pin/Unpin from taskbar, Close window (Close all windows with several), End process (SIGKILL; X11 apps' real PID via XRes, so Flatpak apps too) |
| Right-click taskbar empty | Taskbar menu | Settings, Task Manager, Task View |
| Right-click desktop | Desktop menu (drawn by `vide-desktop`) | View ▸ (icon size, auto arrange), Sort by ▸, Refresh, New ▸ Folder, Open in PowerSlop, Display settings, Personalize. Submenus open on hover; it opens upward near the taskbar |
| Right-click tray icon | SNI menu | App-provided menu |
| Right-click volume/brightness | System tray menu | Mute, Open Mixer, etc. |

## Keyboard Navigation

```rust
fn handle_key_event(&mut self, key: Key) {
    match key {
        Key::Escape => self.close(),
        Key::Enter | Key::Space => self.activate_selected(),
        Key::ArrowDown => self.select_next(),
        Key::ArrowUp => self.select_prev(),
        Key::ArrowRight => self.enter_submenu(),
        Key::ArrowLeft => self.exit_submenu(),
        Key::Home => self.select_first(),
        Key::End => self.select_last(),
        Key::Character(c) => self.jump_to_letter(c),
        _ => {}
    }
}
```

## Positioning

```rust
fn calculate_position(&self, anchor: Point, direction: MenuDirection) -> Point {
    let monitor = self.output_at(anchor);
    let menu_size = self.measure();
    
    match direction {
        MenuDirection::Down => {
            let x = anchor.x.clamp(monitor.x, monitor.x + monitor.width - menu_size.width);
            let y = (anchor.y + 4).min(monitor.y + monitor.height - menu_size.height);
            Point { x, y }
        }
        MenuDirection::Up => { ... }
        MenuDirection::Right => { ... }
        MenuDirection::Left => { ... }
    }
}
```

## Submenu Delay

```rust
const SUBMENU_DELAY: Duration = Duration::from_millis(200);

fn on_hover_submenu(&mut self, index: usize) {
    self.submenu_timer = Some(Timer::after(SUBMENU_DELAY, move || {
        open_submenu(index);
    }));
}

fn on_leave_submenu(&mut self) {
    if let Some(timer) = self.submenu_timer.take() {
        timer.cancel();
    }
}
```

## Rendering (via ui.rs + canvas.rs)

```rust
fn render_menu(&self, canvas: &mut Canvas) {
    // Background
    canvas.rounded_rect(menu_rect, 8, BG_COLOR);
    canvas.rect(menu_rect, BORDER_COLOR);  // Border
    
    for (i, item) in self.items.iter().enumerate() {
        let item_rect = self.item_rect(i);
        
        if self.hovered == Some(i) {
            canvas.rounded_rect(item_rect, 4, HOVER_COLOR);
        }
        
        match item {
            MenuItem::Label { text, .. } => {
                canvas.text(item_rect.left_center(), text, ...);
            }
            MenuItem::Separator => {
                canvas.line(item_rect.top_left(), item_rect.top_right(), SEPARATOR_COLOR);
            }
            MenuItem::Action { text, icon, .. } => {
                if let Some(icon) = icon {
                    canvas.icon(item_rect.left_center(), icon, 16, FG_COLOR);
                }
                canvas.text(item_rect.left_center().x + 24, text, ...);
                // Shortcut hint on right
                if let Some(shortcut) = self.shortcut_for(item) {
                    canvas.text(item_rect.right_center(), shortcut, ...);
                }
            }
            MenuItem::Submenu { .. } => {
                // Chevron indicator
                canvas.icon(item_rect.right_center(), "go-next", 16, FG_COLOR);
            }
        }
    }
}
```

## Configuration

```ini
[menu]
animation-duration = 100
submenu-delay = 200
max-width = 300
icon-size = 16
```

## Related Files

- `ui.rs` — Menu rendering via canvas
- `input.rs` — Menu key handling
- `wm.rs` — Window menu actions
- `sni.rs` — Tray icon menus
- `tray.rs` — System tray menus