---
title: 'Adding Features'
description: 'Guide for adding new components, keybindings, settings'
---

# Adding Features

Guide for extending VIDE-DE with new components, keybindings, and settings.

## Adding a New Keybinding

### 1. Define Action in `input.rs`

```rust
pub enum Action {
    // ... existing
    NewFeature,  // Add here
}
```

### 2. Add Binding in `bind()`

```rust
fn bind(key: Key, mods: Modifiers) -> Option<Action> {
    match (mods, key) {
        // ...
        (Mod::Super | Mod::Shift, Key::KeyN) => Some(Action::NewFeature),
        _ => None,
    }
}
```

### 3. Handle in `handle_action()`

```rust
fn handle_action(&mut self, action: Action) {
    match action {
        // ...
        Action::NewFeature => self.do_new_feature(),
        _ => {}
    }
}
```

### 4. Add to Settings Schema (`config/`)

```rust
schema.add(SchemaEntry {
    key: "keybinding-new-feature",
    value_type: ValueType::String,  // "Super+Shift+N"
    default: "Super+Shift+N",
    description: "Trigger new feature",
    validator: Some(validate_keybinding),
});
```

## Adding a New Setting

### 1. Add to Schema (`config/src/lib.rs`)

```rust
schema.add(SchemaEntry {
    key: "my-feature-enabled",
    value_type: ValueType::Bool,
    default: "true",
    description: "Enable my new feature",
    validator: None,
});
```

### 2. Read in Component

```rust
// In your component
fn update_from_config(&mut self, config: &Config) {
    self.enabled = config.get_bool("my-feature-enabled");
}
```

### 3. Apply on Config Reload

```rust
// In State::apply_config()
fn apply_config(&mut self) {
    self.my_component.update_from_config(&self.config);
    self.needs_redraw = true;
}
```

### 4. Add to Settings GUI (`settings/src/main.rs`)

```rust
// Add UI control
ui.checkbox(&mut config.get_bool("my-feature-enabled"), "Enable My Feature");
```

## Adding a New Component

### 1. Create Module

```bash
# New file: src/my_feature.rs
```

### 2. Define Struct

```rust
// src/my_feature.rs
pub struct MyFeature {
    state: FeatureState,
    config: FeatureConfig,
}

impl MyFeature {
    pub fn new(config: &Config) -> Self { ... }
    pub fn update(&mut self, event: Event) { ... }
    pub fn render(&self, canvas: &mut Canvas) { ... }
}
```

### 3. Integrate in State

```rust
// src/state.rs
pub struct State {
    // ...
    pub my_feature: MyFeature,
}

impl State {
    fn new(...) -> Self {
        // ...
        my_feature: MyFeature::new(&config),
    }
}
```

### 4. Handle Events

```rust
// In input.rs or state.rs
fn handle_input(&mut self, event: InputEvent) {
    self.my_feature.update(event);
    // ...
}
```

### 5. Render

```rust
// In render.rs
fn elements(&self) -> Vec<RenderElement> {
    // ...
    if self.my_feature.visible() {
        elements.push(self.my_feature.element());
    }
    elements
}
```

## Adding a New Built-in App

### 1. Create Crate

```bash
cargo new --bin my-app
# Edit Cargo.toml: add to workspace
```

### 2. Implement as egui App

```rust
// my-app/src/main.rs
fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native("My App", options, Box::new(|cc| {
        Box::new(MyApp::new(cc))
    }));
}
```

### 3. Register in `apps.rs`

```rust
// src/apps.rs
pub fn install_builtin() {
    // ...
    let apps = vec![
        // ...
        BuiltinApp {
            id: "my-app",
            name: "My App",
            exec: "my-app",
            icon: "my-app-icon",
            categories: vec!["Utility"],
        },
    ];
    // ...
}
```

### 4. Add Desktop Icon

```rust
// In install_builtin()
fn write_desktop_file(app: BuiltinApp) {
    // Creates ~/.local/share/vide-de/applications/my-app.desktop
}
```

## Adding a New Layer Shell Client

### 1. Create Separate Crate (like `desktop/`)

```bash
cargo new --bin my-panel
# Use SCTK for Wayland client
```

### 2. Implement Layer Surface

```rust
// my-panel/src/main.rs
let layer = LayerShell::new(&compositor)
    .layer(Layer::Bottom)
    .anchor(Anchor::Bottom | Anchor::Left | Anchor::Right)
    .exclusive_zone(40)
    .namespace("my-panel");
```

### 3. Auto-start from Compositor

```rust
// main.rs
fn start_my_panel(handle: &LoopHandle) {
    Command::new("my-panel")
        .env("WAYLAND_DISPLAY", wayland_display)
        .spawn()?;
}
```

## Code Style

- **Comments explain WHY, not what**
- Match surrounding style
- Use `clippy` suggestions
- `cargo fmt` before commit

## Testing New Features

```bash
# 1. Build
cargo build --release --workspace

# 2. Test nested
VIBE_STARTUP=my-app timeout 12 ./target/release/vibe-comp --nested

# 3. Verify config works
vide-config set my-feature-enabled false
# Should apply within 1 second
```

## Related Files

- [Architecture](/vide-de/architecture/overview/) — Component overview
- [Configuration](/vide-de/configuration/settings) — Settings reference
- [Contributing](/vide-de/development/contributing/) — PR requirements