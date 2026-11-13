---
title: 'UI System (ui.rs + canvas.rs)'
description: 'Chrome painting on CPU, MemoryRenderBuffer caching'
---

# UI System (`ui.rs` + `canvas.rs`)

All chrome (taskbar, start menu, titlebars, menus, tray flyouts) is painted on the **CPU** into `MemoryRenderBuffer`s, cached by key.

## Architecture

```
ui.rs: High-level chrome components
    │
    ├── Taskbar
    ├── StartMenu
    ├── WindowTitlebar
    ├── RightClickMenu
    ├── TrayFlyout (volume/brightness)
    ├── TaskView (desktops panel)
    ├── AltTabSwitcher
    └── SnipOverlay
    │
    ▼
canvas.rs: Low-level 2D painter
    │
    ├── rect(), rounded_rect()
    ├── text(), text_with_shadow()
    ├── icon(), icon_at()
    ├── line(), path()
    ├── clip(), layer()
    └── MemoryRenderBuffer (GPU upload)
```

## Caching Strategy

```rust
struct UiCache {
    buffers: HashMap<UiKey, MemoryRenderBuffer>,
}

impl UiCache {
    fn get_or_create(&mut self, key: UiKey, builder: impl FnOnce(&mut Canvas)) -> &MemoryRenderBuffer {
        self.buffers.entry(key).or_insert_with(|| {
            let mut canvas = Canvas::new(key.size());
            builder(&mut canvas);
            canvas.finish()
        })
    }
}
```

**Key = hash of all visual properties.** Repaint only when key changes.

## UiKey Examples

```rust
enum UiKey {
    Taskbar { width: u32, height: u32, icons: Vec<IconKey>, clock: String, tray_icons: Vec<TrayIconKey> },
    StartMenu { width: u32, height: u32, search_text: String, results: Vec<AppResult>, pinned: Vec<AppId> },
    Titlebar { width: u32, title: String, buttons: TitlebarButtons, focused: bool },
    Menu { items: Vec<MenuItem>, width: u32 },
    TrayFlyout { kind: TrayKind, value: u8, muted: bool },
    // ...
}
```

## Canvas API

```rust
struct Canvas {
    buffer: Vec<u32>, // ARGB32
    width: u32,
    height: u32,
    clip_rect: Option<Rectangle>,
    transform: Transform,
}

impl Canvas {
    fn new(size: Size) -> Self { ... }
    fn finish(self) -> MemoryRenderBuffer { ... }
    
    // Primitives
    fn rect(&mut self, rect: Rectangle, color: Color) { ... }
    fn rounded_rect(&mut self, rect: Rectangle, radius: f32, color: Color) { ... }
    fn line(&mut self, from: Point, to: Point, width: f32, color: Color) { ... }
    
    // Text
    fn text(&mut self, pos: Point, text: &str, font: &Font, size: f32, color: Color) { ... }
    fn text_with_shadow(&mut self, pos: Point, text: &str, font: &Font, size: f32, color: Color, shadow: Color) { ... }
    
    // Icons (from icon theme)
    fn icon(&mut self, pos: Point, name: &str, size: f32, color: Color) { ... }
    fn icon_at(&mut self, rect: Rectangle, name: &str, color: Color) { ... }
    
    // Layers
    fn layer(&mut self, opacity: f32, f: impl FnOnce(&mut Canvas)) { ... }
    fn clip(&mut self, rect: Rectangle, f: impl FnOnce(&mut Canvas)) { ... }
}
```

## MemoryRenderBuffer

```rust
struct MemoryRenderBuffer {
    texture: GlTexture, // or wgpu::Texture
    size: Size,
    damage: Option<Rectangle>, // for partial updates
}

impl MemoryRenderBuffer {
    fn upload(&mut self, canvas: &Canvas) { ... }
    fn bind(&self) -> &GlTexture { ... }
}
```

## Performance Notes

- **Debug builds are slow** — CPU painting + no GPU acceleration
- `profile.dev` sets `opt-level = 1` for dev builds
- **Always use release builds** for real sessions: `cargo build --release`
- Cache hit rate > 95% in typical usage
- Only taskbar clock updates every second (small key change)

## Theme Integration

Colors come from `Config` (polling `settings.conf`):

```rust
fn taskbar_colors(config: &Config) -> (Color, Color) {
    let bg = config.taskbar_bg.unwrap_or(0xFF_0078D7); // VIDE-DE blue
    let fg = config.taskbar_fg.unwrap_or(0xFF_FFFFFF);
    (bg.into(), fg.into())
}
```

## Related Files

- `icons.rs` — Icon loading from theme
- `apps.rs` — App discovery for start menu
- `config/` — Settings for colors, fonts
- `render.rs` — Element ordering
- `sni.rs` — Tray icons in taskbar