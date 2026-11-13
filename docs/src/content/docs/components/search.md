---
title: 'Start Menu Search (search.rs)'
description: 'App launcher search, indexing, fuzzy matching'
---

# Start Menu Search (`search.rs`)

App discovery, indexing, and fuzzy matching for the Start menu.

## App Discovery

```rust
pub fn discover_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    
    // 1. System .desktop files
    for dir in ["/usr/share/applications", "/usr/local/share/applications"] {
        apps.extend(scan_desktop_dir(dir));
    }
    
    // 2. User .desktop files
    if let Some(home) = dirs::home_dir() {
        apps.extend(scan_desktop_dir(&home.join(".local/share/applications")));
    }
    
    // 3. Built-in apps (registered via apps::install_builtin)
    apps.extend(builtin_apps());
    
    apps
}
```

## AppEntry Structure

```rust
pub struct AppEntry {
    pub id: String,           // desktop file ID (e.g., "org.kde.kate")
    pub name: String,         // Display name
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub exec: String,         // Exec= line
    pub icon: Option<String>, // Icon name
    pub categories: Vec<String>,
    pub terminal: bool,
    pub startup_notify: bool,
    pub no_display: bool,     // Hidden from menu
}
```

## Search Index

```rust
struct SearchIndex {
    apps: Vec<AppEntry>,
    name_index: Fst<Map<()>>,      // Finite state transducer
    keyword_index: Fst<Map<()>>,   // Categories, keywords
}

impl SearchIndex {
    fn query(&self, text: &str, limit: usize) -> Vec<SearchResult> {
        // 1. Exact prefix match on name
        // 2. Fuzzy match on name (Levenshtein ≤ 2)
        // 3. Match on generic_name, comment
        // 4. Match on categories/keywords
        // 5. Sort by: exact > prefix > fuzzy > keyword > recency
    }
}
```

## Search Results Ranking

```rust
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct SearchResult {
    score: i32,           // Higher = better
    recency: u64,         // Last launched timestamp
    app: AppEntry,
}

// Score components:
// - Exact name match: +1000
// - Prefix match: +500
// - Fuzzy match: +100 - distance*10
// - Generic name: +200
// - Comment/keywords: +100
// - Category match: +50
// - Recency bonus: +0..50
```

## Start Menu UI Flow

```
User presses Super
       │
       ▼
┌──────────────────┐
│  Start Menu      │
│  ┌────────────┐  │
│  │ Search Box │  │ ◄── Focused, type to search
│  └────────────┘  │
│  ┌────────────┐  │
│  │ Pinned     │  │ ◄── Fixed apps (configurable)
│  │ Apps       │  │
│  ├────────────┤  │
│  │ All Apps   │  │ ◄── Categorized grid
│  └────────────┘  │
└──────────────────┘
```

## Key Handling

```rust
fn handle_search_input(&mut self, key: Key) {
    match key {
        Key::Character(c) => self.query.push(c),
        Key::Backspace => { self.query.pop(); }
        Key::Enter => self.launch_selected(),
        Key::Escape => self.close(),
        Key::ArrowDown => self.select_next(),
        Key::ArrowUp => self.select_prev(),
        Key::Tab => self.toggle_category_view(),
        _ => {}
    }
    self.rebuild_results();
}
```

## Built-in Apps (Auto-registered)

| App | Desktop ID | Category |
|-----|------------|----------|
| Settings | `vide-settings.desktop` | Settings |
| PowerSlop | `powerslop.desktop` | System |
| File Exploder | `file-exploder.desktop` | System |
| Task Mangler | `task-mangler.desktop` | System |
| Snip & Sketchy | `snip-and-sketchy.desktop` | Graphics |
| About VIDE-DE | `vide-about.desktop` | Settings |

Registered via `apps::install_builtin()` on compositor startup.

## Configuration

```ini
[start-menu]
max-results = 10
show-recent = true
recent-count = 5
pinned-apps = ["konsole", "firefox", "code", "vide-settings"]
```

## Related Files

- `apps.rs` — App discovery and .desktop parsing
- `icons.rs` — Icon loading for search results
- `ui.rs` — Start menu rendering
- `input.rs` — Search key handling
- `config/` — Pinned apps, preferences