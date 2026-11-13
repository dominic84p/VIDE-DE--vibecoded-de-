---
title: 'Cursors (cursor.rs)'
description: 'XCursor theme loading, animated cursors, hotspot handling'
---

# Cursors (`cursor.rs`)

Loads XCursor themes, handles animated cursors, hotspots, and cursor state machine.

## XCursor Theme Loading

```rust
pub fn load_cursor_theme(name: &str, size: u32) -> Result<CursorTheme> {
    let theme_dir = find_theme_dir(name)?;
    
    let mut cursors = HashMap::new();
    
    // Load cursor files (e.g., "left_ptr", "pointer", "hand2")
    for entry in fs::read_dir(theme_dir.join("cursors"))? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".cursor") || !name.contains('.') {
            let images = load_xcursor_file(&entry.path())?;
            cursors.insert(name, images);
        }
    }
    
    Ok(CursorTheme { cursors, size })
}
```

## XCursor File Format

```rust
fn load_xcursor_file(path: &Path) -> Result<Vec<CursorFrame>> {
    let data = fs::read(path)?;
    let mut cursor = Cursor::new(data);
    
    // Header
    let magic = cursor.read_u32::<NativeEndian>()?;  // 0x72637278 ("xcur")
    let header_size = cursor.read_u32::<NativeEndian>()?;
    let ntype = cursor.read_u32::<NativeEndian>()?;  // 0xfffd0002 = cursor
    let subtype = cursor.read_u32::<NativeEndian>()?;
    let version = cursor.read_u32::<NativeEndian>()?;
    
    // TOC entries
    let mut frames = Vec::new();
    for _ in 0..ntype {
        let type_ = cursor.read_u32::<NativeEndian>()?;
        let subtype = cursor.read_u32::<NativeEndian>()?;
        let pos = cursor.read_u32::<NativeEndian>()?;
        
        // Seek to frame data
        cursor.seek(SeekFrom::Start(pos as u64))?;
        
        let width = cursor.read_u32::<NativeEndian>()?;
        let height = cursor.read_u32::<NativeEndian>()?;
        let xhot = cursor.read_u32::<NativeEndian>()?;
        let yhot = cursor.read_u32::<NativeEndian>()?;
        let delay = cursor.read_u32::<NativeEndian>()?;  // ms for animated
        
        let pixels = cursor.read_bytes((width * height * 4) as usize)?;
        
        frames.push(CursorFrame {
            width, height, xhot, yhot, delay, pixels,
        });
    }
    
    Ok(frames)
}
```

## Cursor State Machine

```rust
enum CursorState {
    Default,
    Resize(ResizeEdge),
    Move,
    Text,
    Pointer,
    NotAllowed,
    Wait,
    Crosshair,
    Grabbing,
}

impl State {
    fn update_cursor(&mut self) {
        let new_state = match self.current_grab {
            Some(Grab::Move { .. }) => CursorState::Move,
            Some(Grab::Resize { edge, .. }) => CursorState::Resize(edge),
            None => {
                // Check hover target
                if self.hover_titlebar { CursorState::Default }
                else if self.hover_resize_edge { CursorState::Resize(...) }
                else if self.hover_text_input { CursorState::Text }
                else if self.hover_link { CursorState::Pointer }
                else { CursorState::Default }
            }
        };
        
        if new_state != self.cursor_state {
            self.cursor_state = new_state;
            self.set_cursor_image(new_state);
        }
    }
}
```

## Animated Cursors

```rust
struct AnimatedCursor {
    frames: Vec<CursorFrame>,
    current_frame: usize,
    last_update: Instant,
}

impl AnimatedCursor {
    fn tick(&mut self) -> &CursorFrame {
        let frame = &self.frames[self.current_frame];
        if self.last_update.elapsed() >= Duration::from_millis(frame.delay as u64) {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
            self.last_update = Instant::now();
        }
        &self.frames[self.current_frame]
    }
}
```

## Hardware Cursors (TTY backend)

```rust
// In tty.rs - uses DRM hardware cursors when available
fn set_hardware_cursor(&mut self, cursor: &CursorFrame) {
    if self.drm.has_cursor_plane() {
        self.drm.set_cursor(
            cursor.width, cursor.height,
            cursor.xhot, cursor.yhot,
            &cursor.pixels
        );
    } else {
        // Fallback: software cursor (composited)
        self.software_cursor = Some(cursor.clone());
    }
}
```

**NVIDIA Note:** Hardware cursors unreliable on NVIDIA. TTY backend forces software cursor via `FrameFlags::empty()`.

## Configuration

```ini
[cursor]
theme = "Adwaita"
size = 24
animation = true
```

## Fallback Chain

1. User theme (`~/.icons/Theme/cursors`)
2. System theme (`/usr/share/icons/Theme/cursors`)
3. `default` theme
4. `hicolor` theme
5. Built-in minimal cursor (last resort)

## Related Files

- `icons.rs` — Similar theme loading pattern
- `tty.rs` — Hardware cursor setup
- `input.rs` — Cursor state updates
- `render.rs` — Software cursor rendering (first element)