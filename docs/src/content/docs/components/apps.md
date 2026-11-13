---
title: 'App Discovery (apps.rs)'
description: 'Desktop entry parsing, built-in app registration, icon resolution'
---

# App Discovery (`apps.rs`)

Parses `.desktop` files, registers built-in apps, resolves icons and launch commands.

## Desktop Entry Parsing

```rust
pub fn parse_desktop_file(path: &Path) -> Result<AppEntry, ParseError> {
    let content = fs::read_to_string(path)?;
    let mut entry = AppEntry::default();
    
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "Name" => entry.name = value.to_string(),
                "GenericName" => entry.generic_name = Some(value.to_string()),
                "Comment" => entry.comment = Some(value.to_string()),
                "Exec" => entry.exec = parse_exec(value),
                "Icon" => entry.icon = Some(value.to_string()),
                "Terminal" => entry.terminal = value == "true",
                "StartupNotify" => entry.startup_notify = value == "true",
                "NoDisplay" => entry.no_display = value == "true",
                "Categories" => entry.categories = value.split(';').map(|s| s.to_string()).collect(),
                _ => {}
            }
        }
    }
    Ok(entry)
}
```

## Exec Line Parsing

```rust
fn parse_exec(exec: &str) -> String {
    // Remove field codes: %f %F %u %U %i %c %k
    let cleaned = exec
        .replace("%f", "")
        .replace("%F", "")
        .replace("%u", "")
        .replace("%U", "")
        .replace("%i", "")
        .replace("%c", "")
        .replace("%k", "");
    
    // Take first token as command
    cleaned.split_whitespace().next().unwrap_or("").to_string()
}
```

## Built-in App Registration

```rust
pub fn install_builtin() {
    let apps = vec![
        BuiltinApp {
            id: "vide-settings",
            name: "Settings",
            exec: "vide-settings",
            icon: "preferences-system",
            categories: vec!["Settings"],
        },
        BuiltinApp {
            id: "powerslop",
            name: "PowerSlop",
            exec: "powerslop",
            icon: "utilities-terminal",
            categories: vec!["System", "TerminalEmulator"],
        },
        BuiltinApp {
            id: "file-exploder",
            name: "File Exploder",
            exec: "file-exploder",
            icon: "system-file-manager",
            categories: vec!["System", "FileManager"],
        },
        BuiltinApp {
            id: "task-mangler",
            name: "Task Mangler",
            exec: "task-mangler",
            icon: "utilities-system-monitor",
            categories: vec!["System", "Monitor"],
        },
        BuiltinApp {
            id: "snip-and-sketchy",
            name: "Snip & Sketchy",
            exec: "snip-and-sketchy",
            icon: "accessories-screenshot",
            categories: vec!["Graphics", "Photography"],
        },
        BuiltinApp {
            id: "vide-about",
            name: "About VIDE-DE",
            exec: "vide-about",
            icon: "vide-de",
            categories: vec!["Settings"],
        },
    ];
    
    for app in apps {
        write_desktop_file(app)?;
    }
}
```

## Desktop File Installation

```rust
fn write_desktop_file(app: BuiltinApp) -> Result<()> {
    let dir = dirs::data_local_dir()
        .join("vide-de/applications");
    fs::create_dir_all(&dir)?;
    
    let content = format!(
        r#"[Desktop Entry]
Type=Application
Name={}
Exec={}
Icon={}
Categories={}
StartupNotify=true
"#,
        app.name, app.exec, app.icon, app.categories.join(";")
    );
    
    fs::write(dir.join(format!("{}.desktop", app.id)), content)?;
    Ok(())
}
```

## App Categories (for Start Menu)

```
Accessories
Development
Education
Game
Graphics
Network
Office
Science
Settings
System
Utility
Video
Audio
```

## Icon Resolution

```rust
pub fn resolve_icon(name: &str, size: u32) -> Option<PathBuf> {
    // 1. Check theme directories
    for theme in ["/usr/share/icons", "/usr/local/share/icons", "~/.icons", "~/.local/share/icons"] {
        for ext in [".png", ".svg", ".xpm"] {
            let path = theme.join(format!("{}/apps/{}{}", size, name, ext));
            if path.exists() { return Some(path); }
        }
    }
    // 2. Fallback to hicolor
    // 3. Return None if not found
}
```

## Related Files

- `icons.rs` — Icon theme loading
- `search.rs` — Search index building
- `ui.rs` — Start menu rendering
- `config/` — Pinned apps configuration