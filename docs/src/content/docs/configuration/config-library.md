---
title: 'Config Library (vide-config)'
description: 'Shared settings crate, polling, type-safe access'
---

# Config Library (`vide-config`)

Shared crate (`config/`) used by compositor, Settings app, and built-in apps.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      vide-config                            │
├─────────────────────────────────────────────────────────────┤
│  Config struct: type-safe accessors                         │
│  FileWatcher: polls file every 1s                           │
│  Schema: validation, defaults, documentation                │
└─────────────────────────────────────────────────────────────┘
         ▲                    ▲                    ▲
         │                    │                    │
   ┌─────┴─────┐        ┌─────┴─────┐        ┌─────┴─────┐
   │ Compositor│        │  Settings │        │  Apps     │
   │  (read)   │        │  (write)  │        │  (read)   │
   └───────────┘        └───────────┘        └───────────┘
```

## Config Struct

```rust
pub struct Config {
    path: PathBuf,
    data: HashMap<String, String>,
    schema: Schema,
    last_modified: SystemTime,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> { ... }
    
    // Type-safe getters
    pub fn get_bool(&self, key: &str) -> bool { ... }
    pub fn get_int(&self, key: &str) -> i64 { ... }
    pub fn get_float(&self, key: &str) -> f64 { ... }
    pub fn get_string(&self, key: &str) -> String { ... }
    pub fn get_color(&self, key: &str) -> Color { ... }
    pub fn get_list(&self, key: &str) -> Vec<String> { ... }
    pub fn get_enum<T: FromStr>(&self, key: &str) -> T { ... }
    
    // Change detection
    pub fn poll_changed(&mut self) -> bool { ... }
    
    // Write (used by Settings app)
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> { ... }
    pub fn save(&self) -> Result<()> { ... }
}
```

## Schema Definition

```rust
pub struct Schema {
    entries: HashMap<String, SchemaEntry>,
}

pub struct SchemaEntry {
    key: String,
    value_type: ValueType,
    default: String,
    description: String,
    validator: Option<fn(&str) -> Result<()>>,
}

pub enum ValueType {
    Bool,
    Int { min: Option<i64>, max: Option<i64> },
    Float { min: Option<f64>, max: Option<f64> },
    String,
    Color,      // ARGB hex
    Path,
    List { separator: char },  // comma-separated
    Enum { variants: Vec<String> },
}
```

## Example Schema

```rust
pub fn default_schema() -> Schema {
    let mut schema = Schema::new();
    
    schema.add(SchemaEntry {
        key: "taskbar-height",
        value_type: ValueType::Int { min: Some(24), max: Some(100) },
        default: "40",
        description: "Taskbar height in pixels",
        validator: None,
    });
    
    schema.add(SchemaEntry {
        key: "theme",
        value_type: ValueType::Enum { 
            variants: vec!["light", "dark", "system".into()] 
        },
        default: "system",
        description: "UI theme",
        validator: None,
    });
    
    // ... more entries
    
    schema
}
```

## Polling for Changes

```rust
impl Config {
    pub fn poll_changed(&mut self) -> bool {
        let metadata = fs::metadata(&self.path)?;
        let modified = metadata.modified()?;
        
        if modified > self.last_modified {
            self.last_modified = modified;
            self.reload()?;
            return true;
        }
        false
    }
}
```

**Called once per second** from compositor main loop.

## Validation

```rust
impl Config {
    fn validate(&self) -> Result<()> {
        for (key, entry) in &self.schema.entries {
            let value = self.data.get(key).unwrap_or(&entry.default);
            
            if let Some(validator) = entry.validator {
                validator(value)?;
            }
            
            // Type validation
            match entry.value_type {
                ValueType::Int { min, max } => {
                    let v: i64 = value.parse()?;
                    if let Some(min) = min { if v < min { return Err(...) } }
                    if let Some(max) = max { if v > max { return Err(...) } }
                }
                ValueType::Enum { ref variants } => {
                    if !variants.contains(value) { return Err(...) }
                }
                // ...
            }
        }
        Ok(())
    }
}
```

## CLI Tool (`vide-config`)

```bash
# Get value
vide-config get taskbar-height
# → 40

# Set value
vide-config set taskbar-height 48
# → Saved to ~/.config/vide-de/settings.conf

# List all with descriptions
vide-config list
# → taskbar-height = 40 (Taskbar height in pixels)
# → theme = dark (UI theme)

# Validate config
vide-config validate
# → OK

# Reset to defaults
vide-config reset
```

## Integration in Compositor

```rust
// In State::new()
let config = Config::load(config_path)?;
let schema = default_schema();
config.schema = schema;
config.validate()?;

// In main loop (every second)
if state.config.poll_changed() {
    state.apply_config();
    state.needs_redraw = true;
}
```

## Integration in Settings App

```rust
// Settings GUI writes via vide-config
fn on_setting_changed(key: &str, value: &str) {
    config.set(key, value)?;
    config.save()?;  // Triggers compositor reload within 1s
}
```

## File Location

```
~/.config/vide-de/settings.conf
```

Can be overridden via `VIDE_CONFIG_DIR` environment variable.

## Related Files

- `settings/` — Settings GUI app
- `state.rs` — Config polling and application
- `ui.rs` — Color/theme usage
- `configuration/settings.md` — All settings reference