---
title: 'Icons (icons.rs)'
description: 'Icon theme loading, fallback chains, caching'
---

# Icons (`icons.rs`)

Loads icons from freedesktop.org icon themes with fallback chains and caching.

## Theme Discovery

```rust
fn find_icon_themes() -> Vec<PathBuf> {
    let mut themes = Vec::new();
    
    // System themes
    themes.extend(glob("/usr/share/icons/*/index.theme"));
    themes.extend(glob("/usr/local/share/icons/*/index.theme"));
    
    // User themes
    if let Some(home) = dirs::home_dir() {
        themes.extend(glob(&home.join(".icons/*/index.theme")));
        themes.extend(glob(&home.join(".local/share/icons/*/index.theme")));
    }
    
    // Fallback: hicolor
    themes.push(PathBuf::from("/usr/share/icons/hicolor"));
    
    themes
}
```

## Icon Theme Parsing

```rust
struct IconTheme {
    name: String,
    comment: String,
    inherits: Vec<String>,  // Fallback chain
    directories: Vec<IconDirectory>,
}

struct IconDirectory {
    path: String,           // Relative to theme root
    size: u32,              // Nominal size
    scale: u32,             // Scale factor
    type_: DirectoryType,   // Fixed, Scalable, Threshold
    max_size: u32,
    min_size: u32,
    threshold: u32,
}
```

## Icon Lookup Algorithm

```rust
fn lookup_icon(theme: &IconTheme, name: &str, size: u32) -> Option<PathBuf> {
    // 1. Exact match in theme
    if let Some(path) = find_in_theme(theme, name, size) {
        return Some(path);
    }
    
    // 2. Scalable (SVG) in theme
    if let Some(path) = find_scalable(theme, name) {
        return Some(path);
    }
    
    // 3. Parent themes (Inherits chain)
    for parent_name in &theme.inherits {
        if let Some(parent) = load_theme(parent_name) {
            if let Some(path) = lookup_icon(&parent, name, size) {
                return Some(path);
            }
        }
    }
    
    // 4. hicolor fallback
    if theme.name != "hicolor" {
        return lookup_icon(&hicolor_theme(), name, size);
    }
    
    None
}
```

## Caching

```rust
struct IconCache {
    loaded: HashMap<String, IconData>,  // key = "name:size"
    surfaces: HashMap<String, MemoryRenderBuffer>,
}

impl IconCache {
    fn get(&mut self, name: &str, size: u32) -> Option<&MemoryRenderBuffer> {
        let key = format!("{}:{}", name, size);
        self.surfaces.get(&key).or_else(|| {
            let path = lookup_icon(&current_theme(), name, size)?;
            let data = load_image(&path)?;
            let buffer = upload_to_gpu(&data);
            self.surfaces.insert(key, buffer);
            self.surfaces.get(&key)
        })
    }
}
```

## Icon Data Structure

```rust
struct IconData {
    pixels: Vec<u8>,      // RGBA
    width: u32,
    height: u32,
    has_alpha: bool,
}
```

## SVG Support

Uses `resvg` for rasterizing SVG icons:

```rust
fn load_svg(path: &Path, size: u32) -> Result<IconData> {
    let tree = usvg::Tree::from_file(path)?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
    resvg::render(&tree, usvg::FitTo::Original, pixmap.as_mut())?;
    Ok(IconData {
        pixels: pixmap.take().to_vec(),
        width: size,
        height: size,
        has_alpha: true,
    })
}
```

## Configuration

```ini
[icons]
theme = "Adwaita"  # or "Papirus", "Tela", etc.
fallback-theme = "hicolor"
default-size = 24
```

## Related Files

- `apps.rs` — Icon resolution for desktop entries
- `ui.rs` — Icon rendering in canvas
- `sni.rs` — Tray icon loading
- `cursor.rs` — Cursor theme loading (similar pattern)