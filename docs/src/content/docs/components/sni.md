---
title: 'StatusNotifierItem (sni.rs)'
description: 'App tray icons via D-Bus, StatusNotifierWatcher + Host'
---

# StatusNotifierItem (`sni.rs`)

VIDE-DE hosts the **StatusNotifierWatcher** and provides a **StatusNotifierHost** for app tray icons (SNI). Runs on a dedicated D-Bus thread communicating with main loop via channels.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Main Thread                            │
├─────────────────────────────────────────────────────────────┤
│  State.sni_sender ──────▶ Channel ──────▶ SniMessage       │
│  State.sni_receiver ◀─── Channel ◀─── SniMessage           │
└─────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┴─────────┐
                    ▼                   ▼
           ┌──────────────┐      ┌──────────────┐
           │  D-Bus Thread│      │  Render      │
           │  (zbus)      │      │  (ui.rs)     │
           └──────────────┘      └──────────────┘
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
   Watcher       Host         Item
   (register)   (provide)    (proxy)
```

## D-Bus Thread

```rust
// Spawned in State::new()
thread::spawn(move || {
    let connection = Connection::session()?;
    
    // 1. Register as StatusNotifierWatcher
    connection.request_name("org.kde.StatusNotifierWatcher")?;
    
    // 2. Provide StatusNotifierHost
    let host = StatusNotifierHost::new(&connection);
    
    // 3. Listen for Item registrations
    loop {
        // Handle D-Bus signals + main thread messages
        select! {
            msg = rx.recv() => handle_main_thread_msg(msg),
            signal = connection.receive() => handle_dbus_signal(signal),
        }
    }
});
```

## Key D-Bus Interfaces

| Interface | Purpose |
|-----------|---------|
| `org.kde.StatusNotifierWatcher` | Central registry, emits `StatusNotifierItemRegistered` |
| `org.kde.StatusNotifierHost` | Provides visual host for items |
| `org.kde.StatusNotifierItem` | Per-app tray icon (icon, menu, toolTip, status) |

## Message Types (Main ↔ D-Bus Thread)

```rust
enum SniMessage {
    // Main → D-Bus
    RegisterWatcher,
    RegisterHost,
    GetItems,
    RequestMenu(u32),        // Request menu for item
    ActivateItem(u32, i32),  // Click on item/menu
    
    // D-Bus → Main
    ItemsChanged(Vec<SniItem>),
    MenuReady(u32, Vec<MenuItem>),
    IconChanged(u32, IconData),
    ToolTipChanged(u32, String),
}
```

## SNI Item State

```rust
struct SniItem {
    id: u32,
    bus_name: String,
    object_path: String,
    icon_name: Option<String>,
    icon_data: Option<IconData>,  // Pixmap bytes
    title: String,
    status: ItemStatus,           // Passive, Active, NeedsAttention
    tool_tip: Option<ToolTip>,
    menu: Option<Vec<MenuItem>>,
}
```

## Rendering in Taskbar

```rust
// ui.rs - Taskbar rendering
fn render_taskbar_tray(&mut self, canvas: &mut Canvas, items: &[SniItem]) {
    for item in items {
        let icon = item.icon_data.as_ref()
            .or_else(|| load_icon_from_theme(&item.icon_name));
        
        canvas.icon_at(tray_rect, icon, Color::WHITE);
        
        // Tooltip on hover
        if hover && item.tool_tip.is_some() {
            canvas.rect(tooltip_rect, BG_COLOR);
            canvas.text(tooltip_rect.center(), &item.tool_tip, ...);
        }
    }
}
```

## Critical: No `ReplaceExisting`

```rust
// WRONG - steals bus name, kills host desktop's tray!
connection.request_name("org.kde.StatusNotifierWatcher", RequestNameFlags::REPLACE_EXISTING);

// CORRECT - fail if already registered
connection.request_name("org.kde.StatusNotifierWatcher", RequestNameFlags::empty())?;
```

## Configuration

```ini
[system-tray]
icon-size = 22
spacing = 4
show-hidden = false
```

## Related Files

- `tray.rs` — Volume/brightness flyouts (separate from SNI)
- `ui.rs` — Taskbar rendering of SNI items
- `state.rs` — Channel setup, SniMessage handling
- `ipc.rs` — IPC commands for external tray control